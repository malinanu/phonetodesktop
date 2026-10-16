import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../core/hid.dart';
import '../core/identity.dart';
import 'connection.dart';
import 'settings.dart';

/// What the notification and lock-screen controls show. Null means: show nothing.
@immutable
class MediaSnapshot {
  const MediaSnapshot({
    required this.mode,
    required this.pcName,
    required this.title,
    required this.artist,
    required this.app,
    required this.playing,
    required this.posMs,
    required this.durMs,
    required this.canSeek,
    required this.connected,
  });

  /// `wifi` (the PC agent, with song details) or `bluetooth` (media keys only, no details).
  final String mode;
  final String pcName, title, artist, app;
  final bool playing, canSeek, connected;
  final int posMs, durMs;

  Map<String, Object> toMap() => {
    'mode': mode,
    'pc': pcName,
    'title': title,
    'artist': artist,
    'app': app,
    'playing': playing,
    'posMs': posMs,
    'durMs': durMs,
    'canSeek': canSeek,
    'connected': connected,
  };

  /// Same picture on screen (the position moves by itself while playing, so it only counts when it jumps).
  bool sameAs(MediaSnapshot o) =>
      mode == o.mode &&
      pcName == o.pcName &&
      title == o.title &&
      artist == o.artist &&
      app == o.app &&
      playing == o.playing &&
      durMs == o.durMs &&
      canSeek == o.canSeek &&
      connected == o.connected &&
      (posMs - o.posMs).abs() < 2500;
}

/// The phone's own side of the controls: an Android media notification (also on the lock screen), and on iPhone a
/// Live Activity, a Control Center button and, if the user turns it on, the lock-screen player.
abstract class MediaPlatform {
  /// Show or refresh the controls.
  Future<void> show(MediaSnapshot s, {required Map<String, Object> options});
  Future<void> hide();

  /// What the user tapped: `play_pause`, `next`, `prev`, `seek_to` (with a position in ms), `seek_fwd`, `seek_back`.
  Stream<(String, int?)> get commands;

  /// Android 13+: ask to show notifications. True when allowed.
  Future<bool> requestPermission();

  /// iPhone: the login details the Live Activity and Control Center buttons use to reach the PC while the app is
  /// suspended. Kept by the system in the keychain, readable only by Lanyard on this device. Null removes them.
  Future<void> setCredentials(Map<String, Object>? credentials);
}

class ChannelMediaPlatform implements MediaPlatform {
  ChannelMediaPlatform() {
    _channel.setMethodCallHandler((call) async {
      if (call.method != 'command') return null;
      final args = call.arguments;
      if (args is Map) {
        final c = args['c'];
        final v = args['v'];
        if (c is String) _commands.add((c, v is int ? v : (v is num ? v.toInt() : null)));
      }
      return null;
    });
  }

  static const _channel = MethodChannel('app.phoneremote/media');
  final _commands = StreamController<(String, int?)>.broadcast();

  @override
  Stream<(String, int?)> get commands => _commands.stream;

  @override
  Future<void> show(MediaSnapshot s, {required Map<String, Object> options}) async {
    try {
      await _channel.invokeMethod('show', {...s.toMap(), ...options});
    } on PlatformException catch (e) {
      debugPrint('media controls: ${e.message}');
    } on MissingPluginException {
      // a platform without the native side (tests, desktop)
    }
  }

  @override
  Future<void> hide() async {
    try {
      await _channel.invokeMethod('hide');
    } on PlatformException catch (_) {
    } on MissingPluginException catch (_) {}
  }

  @override
  Future<bool> requestPermission() async {
    try {
      return await _channel.invokeMethod<bool>('requestPermission') ?? false;
    } on PlatformException catch (_) {
      return false;
    } on MissingPluginException catch (_) {
      return false;
    }
  }

  @override
  Future<void> setCredentials(Map<String, Object>? credentials) async {
    try {
      await _channel.invokeMethod('setCredentials', credentials);
    } on PlatformException catch (_) {
    } on MissingPluginException catch (_) {}
  }
}

/// Keeps the notification / lock-screen controls in step with the connection, and runs what is tapped there.
///
/// Wi-Fi (the PC agent) wins over Bluetooth: it knows the song and can seek. The controls stay up while the app is
/// (re)connecting to a paired PC, so the system keeps the app running and the connection comes back by itself.
class MediaControls {
  MediaControls({required this.link, required this.settings, required this.platform, this.bluetooth, bool? isIOS}) : _isIOS = isIOS ?? Platform.isIOS {
    link.addListener(_refresh);
    bluetooth?.addListener(_refresh);
    settings.addListener(_onSettings);
    _sub = platform.commands.listen((c) => run(c.$1, c.$2));
    _lastEnabled = settings.lockControls;
    _refresh();
  }

  final ConnectionController link;
  final BluetoothController? bluetooth;
  final AppSettings settings;
  final MediaPlatform platform;
  final bool _isIOS;
  late final StreamSubscription<(String, int?)> _sub;
  MediaSnapshot? _shown;
  Map<String, Object>? _shownOptions;
  String? _credsFor;
  bool _lastEnabled = true;
  bool _askedPermission = false;
  bool _disposed = false;

  MediaSnapshot? get shown => _shown;

  /// The picture for the current connection, or null for none.
  MediaSnapshot? snapshot() {
    if (!settings.lockControls) return null;
    final pc = link.active;
    final s = link.status;
    if (pc != null && (s == LinkStatus.connected || s == LinkStatus.connecting || s == LinkStatus.offline)) {
      final connected = s == LinkStatus.connected;
      final p = connected ? link.state?.nowPlaying : null;
      return MediaSnapshot(
        mode: 'wifi',
        pcName: pc.name,
        title: p == null || p.title.isEmpty ? (connected ? 'Nothing playing' : 'Connecting to ${pc.name}…') : p.title,
        artist: p == null ? '' : p.artist,
        app: p?.app ?? '',
        playing: p?.playing ?? false,
        posMs: p == null ? 0 : link.estimatedPosMs(p),
        durMs: p?.durMs ?? 0,
        canSeek: p?.canSeek ?? false,
        connected: connected,
      );
    }
    final bt = bluetooth;
    if (bt != null && bt.state.connected) {
      final name = bt.state.device ?? 'your computer';
      return MediaSnapshot(
        mode: 'bluetooth',
        pcName: name,
        title: 'Bluetooth remote',
        artist: name,
        app: '',
        playing: false,
        posMs: 0,
        durMs: 0,
        canSeek: false,
        connected: true,
      );
    }
    return null;
  }

  Map<String, Object> _options() => {'iosNowPlaying': settings.iosNowPlaying};

  void _onSettings() {
    // Turned back on: ask again (the user may have said no the first time).
    if (settings.lockControls && !_lastEnabled) _askedPermission = false;
    _lastEnabled = settings.lockControls;
    _refresh();
  }

  void _refresh() {
    if (_disposed) return;
    final next = snapshot();
    final options = _options();
    if (next == null) {
      if (_shown != null) {
        _shown = null;
        unawaited(platform.hide());
      }
    } else if (_shown == null || !_shown!.sameAs(next) || !mapEquals(_shownOptions, options)) {
      // Asked the first time there is something to show, not at app start.
      if (!_askedPermission) {
        _askedPermission = true;
        unawaited(platform.requestPermission());
      }
      _shown = next;
      _shownOptions = options;
      unawaited(platform.show(next, options: options));
    }
    if (_isIOS) unawaited(_syncCredentials(next));
  }

  /// iPhone only: hand the native buttons what they need to log in to the active PC on their own.
  Future<void> _syncCredentials(MediaSnapshot? s) async {
    final pc = link.active;
    final key = s != null && s.mode == 'wifi' && pc != null ? '${pc.id}@${pc.host}:${pc.port}' : null;
    if (key == _credsFor) return;
    if (key == null || pc == null) {
      _credsFor = null;
      await platform.setCredentials(null);
      return;
    }
    final DeviceIdentity id;
    try {
      id = link.identity;
    } catch (_) {
      return; // not loaded yet: tried again on the next change
    }
    _credsFor = key;
    final cert = await link.certProvider?.call();
    await platform.setCredentials({
      'pcId': pc.id,
      'pcName': pc.name,
      'host': pc.host,
      'port': pc.port,
      'fp': pc.fingerprint,
      'deviceId': id.deviceId,
      'seed': id.seedB64,
      'cert': ?cert,
    });
  }

  /// Runs a tap from the notification or lock screen.
  void run(String command, [int? value]) {
    final wifi = link.status == LinkStatus.connected;
    final bt = bluetooth;
    final viaBt = !wifi && bt != null && bt.state.connected;
    switch (command) {
      case 'play_pause':
        wifi ? link.playPause() : (viaBt ? bt.playPause() : null);
      case 'next':
        wifi ? link.next() : (viaBt ? bt.next() : null);
      case 'prev':
        wifi ? link.prev() : (viaBt ? bt.prev() : null);
      case 'seek_to':
        if (wifi && value != null) link.seekAbs(value);
      case 'seek_fwd':
        if (wifi) link.seekRel(settings.skip);
      case 'seek_back':
        if (wifi) link.seekRel(-settings.skip);
      case 'vol_up':
        wifi ? link.volume(settings.volStep) : (viaBt ? bt.volume(1) : null);
      case 'vol_down':
        wifi ? link.volume(-settings.volStep) : (viaBt ? bt.volume(-1) : null);
    }
  }

  void dispose() {
    _disposed = true;
    link.removeListener(_refresh);
    bluetooth?.removeListener(_refresh);
    settings.removeListener(_onSettings);
    unawaited(_sub.cancel());
    unawaited(platform.hide());
  }
}
