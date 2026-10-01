import 'dart:async';

import 'agent_link.dart';
import 'codec.dart';
import 'identity.dart';
import 'protocol.dart';

/// A PC this phone has paired with.
class PcRecord {
  const PcRecord({required this.id, required this.name, required this.host, required this.port, required this.fingerprint});

  final String id, name, host, fingerprint;
  final int port;

  PcRecord copyWith({String? name, String? host, int? port}) =>
      PcRecord(id: id, name: name ?? this.name, host: host ?? this.host, port: port ?? this.port, fingerprint: fingerprint);

  Map<String, dynamic> toJson() => {'id': id, 'name': name, 'host': host, 'port': port, 'fp': fingerprint};

  factory PcRecord.fromJson(Map<String, dynamic> j) => PcRecord(
        id: j['id'] as String,
        name: j['name'] as String? ?? '',
        host: j['host'] as String,
        port: (j['port'] as num).toInt(),
        fingerprint: j['fp'] as String? ?? '',
      );

  factory PcRecord.fromPairing(PairingInfo info) =>
      PcRecord(id: info.pcId, name: info.name, host: info.host, port: info.port, fingerprint: info.fingerprint);
}

/// The PC refused the login. [reason] is the agent's code: `revoked` (forget this PC and pair again),
/// `bad token` (signature rejected), `locked` (too many failures, wait a minute) or `v2_required`.
class AuthFailed implements Exception {
  AuthFailed(this.reason);
  final String reason;
  bool get revoked => reason == 'revoked';
  @override
  String toString() => 'Login refused: $reason';
}

/// The PC has no secure connection (no key fingerprint in its QR), so there is nothing to pin.
class InsecurePc implements Exception {
  @override
  String toString() => 'This PC does not offer a secure connection. Update Phone Remote on the PC.';
}

enum PairResult { approved, denied, expired, badCode, badKey }

/// First contact: show the PC a pairing request (from the QR) and wait for its owner to answer.
Future<PairResult> pairWithPc({
  required PairingInfo info,
  required DeviceIdentity identity,
  required String deviceName,
  String platform = 'android',
  void Function()? onPending,
  SocketOpener opener = openPinnedSocket,
  Duration approvalTimeout = const Duration(seconds: 135),
}) async {
  if (info.fingerprint.isEmpty) throw InsecurePc();
  final link = AgentLink(await opener(info.host, info.port, info.fingerprint));
  try {
    link.send({'t': 'pair', 'code': info.code, 'device': identity.deviceId, 'name': deviceName, 'pk': identity.publicKeyB64, 'platform': platform});
    final deadline = DateTime.now().add(approvalTimeout);
    while (true) {
      final left = deadline.difference(DateTime.now());
      if (left <= Duration.zero) return PairResult.expired;
      final m = await link.waitFor((m) => m['t'] == 'pair' || (m['t'] == 'auth' && m['ok'] == false), timeout: left);
      if (m['t'] == 'auth') return PairResult.denied;
      switch (m['status']) {
        case 'pending':
          onPending?.call();
        case 'approved':
          return PairResult.approved;
        case 'bad_code':
          return PairResult.badCode;
        case 'bad_key':
          return PairResult.badKey;
        case 'expired':
          return PairResult.expired;
        default:
          return PairResult.denied;
      }
    }
  } on TimeoutException {
    return PairResult.expired;
  } finally {
    await link.close();
  }
}

/// A logged-in connection to one PC.
class AgentSession {
  AgentSession._(this.link, this.inputAllowed, this.lastState);

  final AgentLink link;

  /// May this phone move the mouse and type? The owner decides per phone, in the PC's dashboard.
  final bool inputAllowed;

  /// The state the PC sent right after login (more arrive on [states]).
  AgentState? lastState;

  Stream<AgentState> get states => link.messages.where((m) => m['t'] == 'state').map((m) => lastState = AgentState.fromJson(m));

  /// Acknowledgements and errors for commands (`{"t":"ack","ok":false,"err":...}`), and `auth` revocations.
  Stream<Map<String, dynamic>> get notices => link.messages.where((m) => m['t'] == 'ack' || m['t'] == 'auth');

  Future<void> get closed => link.done;

  void command(String c, [Map<String, dynamic> args = const {}]) => link.send({'t': 'cmd', 'c': c, ...args});

  void playPause() => command('play_pause');
  void next() => command('next');
  void prev() => command('prev');
  void seekRel(int seconds) => command('seek_rel', {'d': seconds});
  void seekAbs(int posMs) => command('seek_abs', {'pos_ms': posMs});
  void volume(int steps) => command('volume', {'d': steps});
  void volumeSet(int level) => command('volume_set', {'level': level.clamp(0, 100)});
  void mute() => command('mute');
  void select(String playerId) => command('select', {'id': playerId});
  void mouseMove(int dx, int dy) => command('mouse_move', {'dx': dx, 'dy': dy});
  void mouseButton(String button, String action) => command('mouse_button', {'button': button, 'action': action});
  void scroll(int dx, int dy) => command('scroll', {'dx': dx, 'dy': dy});
  void text(String s) => command('text', {'s': s});
  void key(String name, [List<String> mods = const []]) => command('key', {'name': name, 'mods': mods});
  void ping() => link.send({'t': 'ping'});

  Future<void> close() => link.close();
}

/// Log in with the device key: ask for a challenge, sign it, wait for the verdict.
Future<AgentSession> loginToPc({required PcRecord pc, required DeviceIdentity identity, SocketOpener opener = openPinnedSocket}) async {
  if (pc.fingerprint.isEmpty) throw InsecurePc();
  final link = AgentLink(await opener(pc.host, pc.port, pc.fingerprint));
  try {
    final challenge = link.waitFor((m) => m['t'] == 'challenge');
    link.send({'t': 'challenge', 'device': identity.deviceId});
    final nonce = unb64u((await challenge)['nonce'] as String);
    final signature = await identity.sign(authMessage(pc.id, identity.deviceId, nonce));
    final verdict = link.waitFor((m) => m['t'] == 'auth');
    link.send({'t': 'auth_sig', 'device': identity.deviceId, 'sig': b64u(signature)});
    final result = await verdict;
    if (result['ok'] != true) throw AuthFailed(result['err']?.toString() ?? 'refused');
    final buffered = link.startStreaming();
    final first = buffered.where((m) => m['t'] == 'state').map(AgentState.fromJson).fold<AgentState?>(null, (_, s) => s);
    return AgentSession._(link, result['input'] == true, first);
  } catch (_) {
    await link.close();
    rethrow;
  }
}
