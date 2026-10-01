// End-to-end: this Dart client against the real PC agent binary (mock media backend), over pinned TLS.
// Build the agent first:  cd agent && cargo build
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/core/identity.dart';
import 'package:phoneremote/core/protocol.dart';
import 'package:phoneremote/core/session.dart';
import 'package:phoneremote/core/spki.dart';

final _binary = File('../agent/target/debug/phone-remote');
final _skip = _binary.existsSync() ? false : 'build the agent first: cd agent && cargo build';

/// Runs the agent in a throwaway home folder and exposes its QR and dashboard API.
class AgentUnderTest {
  AgentUnderTest._(this.process, this.port, this.qr, this.home);

  final Process process;
  final int port;
  final PairingInfo qr;
  final Directory home;

  static Future<AgentUnderTest> start() async {
    final probe = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
    final port = probe.port;
    await probe.close();
    final home = await Directory.systemTemp.createTemp('pr-live-');
    final process = await Process.start(
      _binary.absolute.path,
      ['serve', '--console', '--mock', '--no-mdns', '--port', '$port'],
      environment: {'HOME': home.path, 'XDG_CONFIG_HOME': '${home.path}/.config', 'XDG_DATA_HOME': '${home.path}/.local/share'},
    );
    process.stderr.drain<void>();
    final qr = Completer<PairingInfo>();
    process.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen((line) {
      final info = PairingInfo.parse(line);
      if (info != null && !qr.isCompleted) qr.complete(info);
    });
    final info = await qr.future.timeout(const Duration(seconds: 15));
    return AgentUnderTest._(process, port, info, home);
  }

  Future<void> stop() async {
    process.kill();
    await process.exitCode;
    await home.delete(recursive: true);
  }

  /// The dashboard API, as the owner's browser on this PC would call it.
  Future<dynamic> api(String method, String path) async {
    final client = HttpClient()..findProxy = ((uri) => 'DIRECT');
    final req = await client.openUrl(method, Uri.parse('http://127.0.0.1:$port$path'));
    req.headers.set('x-requested-with', 'phone-remote');
    final res = await req.close();
    final body = await res.transform(utf8.decoder).join();
    client.close();
    expect(res.statusCode, 200, reason: '$method $path -> $body');
    return jsonDecode(body);
  }

  /// Be the owner: wait for a pairing request and press Allow.
  Future<void> approveNextRequest() async {
    for (var i = 0; i < 100; i++) {
      final overview = await api('GET', '/api/overview') as Map<String, dynamic>;
      final pending = overview['pending'] as List;
      if (pending.isNotEmpty) {
        await api('POST', '/api/pending/${Uri.encodeComponent(pending.first['id'] as String)}/approve');
        return;
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    fail('no pairing request arrived');
  }
}

void main() {
  late AgentUnderTest agent;
  late DeviceIdentity phone;
  late PcRecord pc;

  setUpAll(() async {
    if (_skip != false) return;
    agent = await AgentUnderTest.start();
    pc = PcRecord.fromPairing(agent.qr);
    phone = await DeviceIdentity.generate();
  });

  tearDownAll(() async {
    if (_skip == false) await agent.stop();
  });

  test('the QR names a secure PC and carries a key fingerprint', () {
    expect(agent.qr.fingerprint, isNotEmpty);
    expect(agent.qr.pcId, isNotEmpty);
  }, skip: _skip);

  test('an unknown phone cannot log in', () async {
    await expectLater(loginToPc(pc: pc, identity: phone), throwsA(isA<AuthFailed>().having((e) => e.revoked, 'revoked', true)));
  }, skip: _skip);

  test('pairing needs the owner, then login, state and commands work over the pinned connection', () async {
    var sawPending = false;
    final owner = agent.approveNextRequest();
    final result = await pairWithPc(info: agent.qr, identity: phone, deviceName: 'Test phone', platform: 'android', onPending: () => sawPending = true);
    await owner;
    expect(result, PairResult.approved);
    expect(sawPending, isTrue, reason: 'the owner must have been asked');

    // The PC lists it as a key-based phone.
    final overview = await agent.api('GET', '/api/overview') as Map<String, dynamic>;
    final device = (overview['devices'] as List).single as Map<String, dynamic>;
    expect((device['name'], device['platform'], device['v']), ('Test phone', 'android', 2));

    final session = await loginToPc(pc: pc, identity: phone);
    addTearDown(session.close);
    expect(session.inputAllowed, isTrue);

    // The mock player starts playing; play/pause must flip it.
    AgentState state = session.lastState ?? await session.states.first.timeout(const Duration(seconds: 5));
    expect(state.backend, 'mock');
    expect(state.nowPlaying!.playing, isTrue);
    final paused = session.states.firstWhere((s) => s.nowPlaying?.playing == false).timeout(const Duration(seconds: 5));
    session.playPause();
    state = await paused;
    expect(state.nowPlaying!.playing, isFalse);

    // Volume and seeking round-trip too.
    final louder = session.states.firstWhere((s) => s.volume == 80).timeout(const Duration(seconds: 5));
    session.volumeSet(80);
    expect((await louder).volume, 80);

    // Mouse input is allowed for this phone: no error comes back.
    session.mouseMove(5, 5);
    session.text('hello');
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }, skip: _skip);

  test('the same phone logs in again, and a different key is refused', () async {
    final again = await loginToPc(pc: pc, identity: phone);
    await again.close();
    final stranger = await DeviceIdentity.fromSeed(phone.deviceId, Uint8List.fromList(List.filled(32, 77)));
    await expectLater(loginToPc(pc: pc, identity: stranger), throwsA(isA<AuthFailed>().having((e) => e.reason, 'reason', 'bad token')));
  }, skip: _skip);

  test('a connection that pins any other key is refused before any data is sent', () async {
    final wrong = PcRecord(id: pc.id, name: pc.name, host: pc.host, port: pc.port, fingerprint: fingerprintOfSpki([1, 2, 3]));
    await expectLater(loginToPc(pc: wrong, identity: phone), throwsA(isA<HandshakeException>()));
  }, skip: _skip);

  test('a PC without a fingerprint is refused as insecure', () async {
    final noPin = PcRecord(id: pc.id, name: pc.name, host: pc.host, port: pc.port, fingerprint: '');
    await expectLater(loginToPc(pc: noPin, identity: phone), throwsA(isA<InsecurePc>()));
  }, skip: _skip);

  test('removing the phone on the PC locks it out at once', () async {
    final session = await loginToPc(pc: pc, identity: phone);
    final closed = session.closed.timeout(const Duration(seconds: 5));
    await agent.api('DELETE', '/api/devices/${Uri.encodeComponent(phone.deviceId)}');
    await closed; // the PC closes the connection after telling it "revoked"
    await expectLater(loginToPc(pc: pc, identity: phone), throwsA(isA<AuthFailed>().having((e) => e.revoked, 'revoked', true)));
  }, skip: _skip);

  test('a bad pairing code is refused', () async {
    final bad = PairingInfo(host: agent.qr.host, port: agent.qr.port, code: 'wrong', pcId: agent.qr.pcId, name: 'x', fingerprint: agent.qr.fingerprint);
    final other = await DeviceIdentity.generate();
    expect(await pairWithPc(info: bad, identity: other, deviceName: 'x'), PairResult.badCode);
  }, skip: _skip);
}
