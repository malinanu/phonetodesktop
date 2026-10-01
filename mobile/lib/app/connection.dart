import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../core/agent_link.dart';
import '../core/identity.dart';
import '../core/protocol.dart';
import '../core/session.dart';
import 'discovery.dart';
import 'pc_store.dart';

enum LinkStatus {
  /// No PC is paired yet.
  idle,
  connecting,
  connected,

  /// The PC is not reachable right now (asleep, other network, restarting). Retrying.
  offline,

  /// The PC removed this phone. Pair again to use it.
  revoked,

  /// The PC presented a different key than the one this phone paired with (it was reinstalled, or someone is
  /// pretending to be it). Pair again only if you expect that.
  keyChanged,

  /// The PC offers no secure connection (an old version). Update Phone Remote on the PC.
  insecure,
}

/// Keeps one PC connected: logs in with the device key, follows the PC's state, reconnects with backoff, and finds
/// the PC again if its address changed.
class ConnectionController extends ChangeNotifier {
  ConnectionController({
    required this.pcStore,
    required this.identityStore,
    this.opener = openPinnedSocket,
    this.discovery,
    this.retryDelays = const [Duration(seconds: 1), Duration(seconds: 2), Duration(seconds: 4), Duration(seconds: 8), Duration(seconds: 15)],
  });

  final PcStore pcStore;
  final IdentityStore identityStore;
  final SocketOpener opener;
  final Discovery? discovery;
  final List<Duration> retryDelays;

  DeviceIdentity? _identity;
  List<PcRecord> _pcs = [];
  String? _activeId;
  AgentSession? _session;
  AgentState? _state;
  DateTime _stateAt = DateTime.now();
  LinkStatus _status = LinkStatus.idle;
  int _generation = 0;
  bool _disposed = false;

  DeviceIdentity get identity => _identity!;
  List<PcRecord> get pcs => List.unmodifiable(_pcs);
  LinkStatus get status => _status;
  AgentState? get state => _state;
  bool get inputAllowed => _session?.inputAllowed ?? false;
  PcRecord? get active {
    for (final p in _pcs) {
      if (p.id == _activeId) return p;
    }
    return null;
  }

  /// A new stream of acknowledgements/errors for commands of the current connection (empty when offline).
  Stream<Map<String, dynamic>> get notices => _session?.notices ?? const Stream.empty();

  Future<void> load() async {
    _identity = await loadOrCreateIdentity(identityStore);
    _pcs = await pcStore.load();
    _activeId = await pcStore.activeId();
    if (active == null && _pcs.isNotEmpty) _activeId = _pcs.first.id;
    _start();
  }

  /// After a successful pairing: remember the PC and open it.
  Future<void> addPairedPc(PcRecord pc) async {
    _pcs = [..._pcs.where((p) => p.id != pc.id), pc];
    _activeId = pc.id;
    await pcStore.save(_pcs);
    await pcStore.setActive(pc.id);
    _start();
  }

  Future<void> select(String id) async {
    if (_pcs.every((p) => p.id != id)) return;
    _activeId = id;
    await pcStore.setActive(id);
    _start();
  }

  Future<void> forget(String id) async {
    _pcs = _pcs.where((p) => p.id != id).toList();
    await pcStore.save(_pcs);
    if (_activeId == id) {
      _activeId = _pcs.isEmpty ? null : _pcs.first.id;
      await pcStore.setActive(_activeId);
      _start();
    } else {
      _notify();
    }
  }

  /// Try now instead of waiting for the next retry.
  void reconnectNow() => _start();

  // ---- commands (ignored while offline) ----------------------------------------------------------------------

  AgentSession? get session => _status == LinkStatus.connected ? _session : null;

  /// Where the progress bar is right now: the PC's last report plus the time since, while it is playing.
  int estimatedPosMs(PlayerInfo p) {
    if (!p.playing) return p.posMs;
    final pos = p.posMs + DateTime.now().difference(_stateAt).inMilliseconds;
    return p.durMs > 0 && pos > p.durMs ? p.durMs : pos;
  }

  // Commands. Ignored while the PC is not connected, so the screens never have to check.
  void playPause() => session?.playPause();
  void next() => session?.next();
  void prev() => session?.prev();
  void seekRel(int seconds) => session?.seekRel(seconds);
  void seekAbs(int posMs) => session?.seekAbs(posMs);
  void volume(int steps) => session?.volume(steps);
  void volumeSet(int level) => session?.volumeSet(level);
  void mute() => session?.mute();
  void selectPlayer(String id) => session?.select(id);
  void mouseMove(int dx, int dy) => session?.mouseMove(dx, dy);
  void mouseButton(String button, String action) => session?.mouseButton(button, action);
  void scroll(int dx, int dy) => session?.scroll(dx, dy);
  void typeText(String s) => session?.text(s);
  void pressKey(String name, [List<String> mods = const []]) => session?.key(name, mods);

  // ---- the connection loop -----------------------------------------------------------------------------------

  void _start() {
    _generation++;
    final old = _session;
    _session = null;
    _state = null;
    old?.close();
    final pc = active;
    if (pc == null) {
      _status = LinkStatus.idle;
      _notify();
      return;
    }
    _status = LinkStatus.connecting;
    _notify();
    unawaited(_run(_generation, pc));
  }

  bool _current(int gen) => gen == _generation && !_disposed;

  Future<void> _run(int gen, PcRecord pc) async {
    var failures = 0;
    while (_current(gen)) {
      try {
        _setStatus(gen, LinkStatus.connecting);
        final session = await loginToPc(pc: pc, identity: identity, opener: opener);
        if (!_current(gen)) {
          await session.close();
          return;
        }
        _session = session;
        _state = session.lastState;
        _stateAt = DateTime.now();
        failures = 0;
        _setStatus(gen, LinkStatus.connected);
        final sub = session.states.listen((s) {
          if (!_current(gen)) return;
          _state = s;
          _stateAt = DateTime.now();
          _notify();
        });
        await session.closed;
        await sub.cancel();
        if (!_current(gen)) return;
        _session = null;
        _setStatus(gen, LinkStatus.offline);
      } on AuthFailed catch (e) {
        if (e.revoked || e.reason == 'bad token') return _setStatus(gen, LinkStatus.revoked);
        if (e.reason == 'v2_required') return _setStatus(gen, LinkStatus.insecure);
        failures++;
        _setStatus(gen, LinkStatus.offline);
        if (e.reason == 'locked') await Future<void>.delayed(const Duration(seconds: 60));
      } on InsecurePc {
        return _setStatus(gen, LinkStatus.insecure);
      } on HandshakeException catch (e) {
        // A certificate that does not match the pinned key is final; any other TLS hiccup is just a bad moment.
        if (e.toString().contains('CERTIFICATE_VERIFY_FAILED')) return _setStatus(gen, LinkStatus.keyChanged);
        failures++;
        _setStatus(gen, LinkStatus.offline);
      } catch (_) {
        failures++;
        _setStatus(gen, LinkStatus.offline);
      }
      if (!_current(gen)) return;
      if (failures >= 2) pc = await _rediscover(pc);
      await Future<void>.delayed(retryDelays[failures.clamp(0, retryDelays.length - 1)]);
    }
  }

  /// The PC may have a new address: look for it by its identity and remember the new one.
  Future<PcRecord> _rediscover(PcRecord pc) async {
    final d = discovery;
    if (d == null) return pc;
    try {
      for (final s in await d.scan()) {
        if (s.id == pc.id && (s.host != pc.host || s.port != pc.port)) {
          final moved = pc.copyWith(host: s.host, port: s.port);
          _pcs = [for (final p in _pcs) p.id == pc.id ? moved : p];
          await pcStore.save(_pcs);
          _notify();
          return moved;
        }
      }
    } catch (_) {}
    return pc;
  }

  void _setStatus(int gen, LinkStatus s) {
    if (!_current(gen) || _status == s) return;
    _status = s;
    _notify();
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _generation++;
    _session?.close();
    super.dispose();
  }
}
