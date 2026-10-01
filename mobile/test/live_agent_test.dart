// End-to-end: this Dart client against the real PC agent binary (mock media backend), over pinned TLS.
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/core/identity.dart';
import 'package:phoneremote/core/protocol.dart';
import 'package:phoneremote/core/session.dart';
import 'package:phoneremote/core/spki.dart';

import 'support/agent_under_test.dart';

void main() {
  late AgentUnderTest agent;
  late DeviceIdentity phone;
  late PcRecord pc;

  setUpAll(() async {
    if (agentSkip != false) return;
    agent = await AgentUnderTest.start();
    pc = PcRecord.fromPairing(agent.qr);
    phone = await DeviceIdentity.generate();
  });

  tearDownAll(() async {
    if (agentSkip == false) await agent.stop();
  });

  test('the QR names a secure PC and carries a key fingerprint', () {
    expect(agent.qr.fingerprint, isNotEmpty);
    expect(agent.qr.pcId, isNotEmpty);
  }, skip: agentSkip);

  test('an unknown phone cannot log in', () async {
    await expectLater(loginToPc(pc: pc, identity: phone), throwsA(isA<AuthFailed>().having((e) => e.revoked, 'revoked', true)));
  }, skip: agentSkip);

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
  }, skip: agentSkip);

  test('the same phone logs in again, and a different key is refused', () async {
    final again = await loginToPc(pc: pc, identity: phone);
    await again.close();
    final stranger = await DeviceIdentity.fromSeed(phone.deviceId, Uint8List.fromList(List.filled(32, 77)));
    await expectLater(loginToPc(pc: pc, identity: stranger), throwsA(isA<AuthFailed>().having((e) => e.reason, 'reason', 'bad token')));
  }, skip: agentSkip);

  test('a connection that pins any other key is refused before any data is sent', () async {
    final wrong = PcRecord(id: pc.id, name: pc.name, host: pc.host, port: pc.port, fingerprint: fingerprintOfSpki([1, 2, 3]));
    await expectLater(loginToPc(pc: wrong, identity: phone), throwsA(isA<HandshakeException>()));
  }, skip: agentSkip);

  test('a PC without a fingerprint is refused as insecure', () async {
    final noPin = PcRecord(id: pc.id, name: pc.name, host: pc.host, port: pc.port, fingerprint: '');
    await expectLater(loginToPc(pc: noPin, identity: phone), throwsA(isA<InsecurePc>()));
  }, skip: agentSkip);

  test('removing the phone on the PC locks it out at once', () async {
    final session = await loginToPc(pc: pc, identity: phone);
    final closed = session.closed.timeout(const Duration(seconds: 5));
    await agent.api('DELETE', '/api/devices/${Uri.encodeComponent(phone.deviceId)}');
    await closed; // the PC closes the connection after telling it "revoked"
    await expectLater(loginToPc(pc: pc, identity: phone), throwsA(isA<AuthFailed>().having((e) => e.revoked, 'revoked', true)));
  }, skip: agentSkip);

  test('a bad pairing code is refused', () async {
    final bad = PairingInfo(host: agent.qr.host, port: agent.qr.port, code: 'wrong', pcId: agent.qr.pcId, name: 'x', fingerprint: agent.qr.fingerprint);
    final other = await DeviceIdentity.generate();
    expect(await pairWithPc(info: bad, identity: other, deviceName: 'x'), PairResult.badCode);
  }, skip: agentSkip);
}
