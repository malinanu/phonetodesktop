import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../app/input_sink.dart';

/// What the phone's Bluetooth remote is doing right now.
class HidState {
  const HidState({
    this.ready = false,
    this.connected = false,
    this.message = 'Not started',
    this.device,
    this.descriptorVersion = 0,
  });
  final bool ready;
  final bool connected;
  final String message;
  final String? device;
  final int descriptorVersion;

  factory HidState.fromMap(Map<Object?, Object?> m) => HidState(
    ready: m['ready'] == true,
    connected: m['connected'] == true,
    message: m['message'] is String ? m['message'] as String : '',
    device: m['device'] as String?,
    descriptorVersion: m['descriptorVersion'] is int
        ? m['descriptorVersion'] as int
        : 0,
  );
}

class BondedPc {
  const BondedPc({
    required this.name,
    required this.address,
    required this.isComputer,
  });
  final String name;
  final String address;
  final bool isComputer;
}

/// The native side. Real on Android; a fake in tests.
abstract class HidPlatform {
  Stream<HidState> get states;
  Future<bool> start();
  Future<void> stop();
  Future<List<BondedPc>> bonded();
  Future<bool> connect(String address);
  Future<void> discoverable();
  Future<void> media(int usage);
  Future<bool> key(String name, List<String> mods);
  Future<int> text(String s);
  Future<void> move(int dx, int dy);
  Future<void> button(String button, String action);
  Future<void> scroll(int dx, int dy);
}

class ChannelHidPlatform implements HidPlatform {
  static const _methods = MethodChannel('app.phoneremote/hid');
  static const _events = EventChannel('app.phoneremote/hid/events');

  @override
  Stream<HidState> get states => _events.receiveBroadcastStream().map(
    (e) => HidState.fromMap(e as Map<Object?, Object?>),
  );

  @override
  Future<bool> start() async =>
      (await _methods.invokeMethod<bool>('start')) ?? false;
  @override
  Future<void> stop() => _methods.invokeMethod('stop');
  @override
  Future<List<BondedPc>> bonded() async {
    final list =
        await _methods.invokeListMethod<Map<Object?, Object?>>('bonded') ??
        const [];
    return [
      for (final m in list)
        BondedPc(
          name: '${m['name']}',
          address: '${m['address']}',
          isComputer: m['computer'] == true,
        ),
    ];
  }

  @override
  Future<bool> connect(String address) async =>
      (await _methods.invokeMethod<bool>('connect', {'address': address})) ??
      false;
  @override
  Future<void> discoverable() => _methods.invokeMethod('discoverable');
  @override
  Future<void> media(int usage) =>
      _methods.invokeMethod('media', {'usage': usage});
  @override
  Future<bool> key(String name, List<String> mods) async =>
      (await _methods.invokeMethod<bool>('key', {
        'name': name,
        'mods': mods,
      })) ??
      false;
  @override
  Future<int> text(String s) async =>
      (await _methods.invokeMethod<int>('text', {'text': s})) ?? 0;
  @override
  Future<void> move(int dx, int dy) =>
      _methods.invokeMethod('move', {'dx': dx, 'dy': dy});
  @override
  Future<void> button(String button, String action) =>
      _methods.invokeMethod('button', {'button': button, 'action': action});
  @override
  Future<void> scroll(int dx, int dy) =>
      _methods.invokeMethod('scroll', {'dx': dx, 'dy': dy});
}

/// USB HID consumer-control usages for the media keys.
class MediaKey {
  static const playPause = 0xCD;
  static const next = 0xB5;
  static const prev = 0xB6;
  static const volUp = 0xE9;
  static const volDown = 0xEA;
  static const mute = 0xE2;
}

/// The Bluetooth mode: the phone pretends to be a keyboard, mouse and media remote the PC already knows how to use.
/// Nothing needs to be installed on the PC, and no Wi-Fi is needed.
class BluetoothController extends ChangeNotifier implements InputSink {
  BluetoothController(this._hid);
  final HidPlatform _hid;

  HidState state = const HidState();
  List<BondedPc> pcs = const [];
  String?
  notice; // a one-off explanation, e.g. a character that cannot be typed
  bool _started = false;
  StreamSubscription<HidState>? _sub;

  bool get started => _started;

  /// Starts the Bluetooth profile. Returns false when the user did not allow Nearby devices.
  Future<bool> start() async {
    _sub ??= _hid.states.listen((s) {
      state = s;
      notifyListeners();
    });
    final ok = await _hid.start();
    _started = ok;
    if (ok) await refreshPcs();
    notifyListeners();
    return ok;
  }

  Future<void> stop() async {
    await _hid.stop();
    _started = false;
    state = const HidState();
    notifyListeners();
  }

  Future<void> refreshPcs() async {
    pcs = [...await _hid.bonded()]
      ..sort(
        (a, b) => a.isComputer == b.isComputer
            ? a.name.compareTo(b.name)
            : (a.isComputer ? -1 : 1),
      );
    notifyListeners();
  }

  Future<void> connectTo(BondedPc pc) => _hid.connect(pc.address);
  Future<void> makeDiscoverable() => _hid.discoverable();

  // ---- media ----
  void media(int usage) => unawaited(_hid.media(usage));
  void playPause() => media(MediaKey.playPause);
  void next() => media(MediaKey.next);
  void prev() => media(MediaKey.prev);
  void mute() => media(MediaKey.mute);
  void volume(int steps) {
    for (var i = 0; i < steps.abs(); i++) {
      media(steps > 0 ? MediaKey.volUp : MediaKey.volDown);
    }
  }

  // ---- InputSink ----
  @override
  bool get inputConnected => state.connected;
  @override
  bool get inputAllowed => true;
  @override
  void mouseMove(int dx, int dy) => unawaited(_hid.move(dx, dy));
  @override
  void mouseButton(String button, String action) =>
      unawaited(_hid.button(button, action));
  @override
  void scroll(int dx, int dy) => unawaited(_hid.scroll(dx, dy));
  @override
  void pressKey(String name, [List<String> mods = const []]) =>
      unawaited(_hid.key(name, mods));
  @override
  void typeText(String s) {
    _hid.text(s).then((skipped) {
      if (skipped > 0) {
        notice = 'Some characters can only be typed over Wi-Fi: Bluetooth types US-layout keys.';
        notifyListeners();
      }
    });
  }

  void clearNotice() {
    notice = null;
    notifyListeners();
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }
}
