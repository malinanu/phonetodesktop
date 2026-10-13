// The whole account chain with real parts: the Python account service, the Rust PC agent and this Dart client.
// A phone that never paired with the PC (no QR) logs in with its account certificate, keeps working with the
// account service switched off, and is cut off after the owner removes it from the account.
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:phoneremote/app/account.dart';
import 'package:phoneremote/core/account_api.dart';
import 'package:phoneremote/core/identity.dart';
import 'package:phoneremote/core/session.dart';

import 'support/account_harness.dart';
import 'support/agent_harness.dart';

const email = 'owner@mail.dev';

void main() {
  final Object? skip = agentSkip == false ? accountSkip : agentSkip;
  late AccountServiceUnderTest service;
  late AgentUnderTest agent;

  setUpAll(() async {
    if (skip != null) return;
    service = await AccountServiceUnderTest.start();
    agent = await AgentUnderTest.start();
  });
  tearDownAll(() async {
    if (skip != null) return;
    await agent.stop();
    try {
      await service.stop();
    } catch (_) {}
  });

  test(
    'sign in, a computer joins, the phone logs in with its certificate, offline, then is removed',
    () async {
      // --- the phone signs in and enrols
      final ids = MemoryIdentityStore();
      final store = MemoryAccountStore();
      final account = AccountController(store: store, identityStore: ids, deviceName: 'Test phone', platform: 'android');
      expect(await account.requestCode(service.url, email), isTrue);
      expect(await account.signIn(await service.codeFor(email)), isTrue, reason: account.error);
      final phone = await loadOrCreateIdentity(ids);

      // --- the owner makes a code for the computer (a signed-in action) and the computer joins with it
      // The owner signs in on another device (a second session: refresh tokens are single-use, so the phone's must not be borrowed).
      Future<Tokens> ownerSession() async {
        final api = AccountApi(service.url);
        await api.requestCode(email);
        return api.verify(email, await service.codeFor(email));
      }

      final tokens = await ownerSession();
      final link = await http.post(Uri.parse('${service.url}/v1/links'), headers: {'authorization': 'Bearer ${tokens.access}'});
      expect(link.statusCode, 201);
      final code = (jsonDecode(link.body) as Map)['code'] as String;
      await agent.api('POST', '/api/account/join', body: {'url': service.url, 'code': code});
      final overview = await agent.api('GET', '/api/overview') as Map<String, dynamic>;
      expect((overview['account'] as Map)['url'], service.url);
      expect(overview['devices'], isEmpty, reason: 'this phone never paired with the computer');

      // --- the phone finds the computer in the account and pins the key the account holds for it
      final computers = (await account.computers()) ?? fail('could not list computers: ${account.error}');
      expect(computers, hasLength(1));
      expect(computers.single.pcId, agent.qr.pcId);
      expect(computers.single.fingerprint, agent.qr.fingerprint, reason: 'the account holds the same key the QR carries');
      final pc = PcRecord(
        id: computers.single.pcId!,
        name: computers.single.name,
        host: '127.0.0.1',
        port: agent.port,
        fingerprint: computers.single.fingerprint!,
        viaAccount: true,
      );

      // --- without the certificate the computer refuses; with it, the phone is in
      await expectLater(loginToPc(pc: pc, identity: phone), throwsA(isA<AuthFailed>()));
      final cert = await account.freshCert();
      final session = await loginToPc(pc: pc, identity: phone, cert: cert);
      expect(session.inputAllowed, isTrue);
      session.playPause();
      expect(session.lastState ?? await session.states.first.timeout(const Duration(seconds: 5)), isNotNull);

      // --- the account service goes away: the same login still works (the certificate is checked offline)
      await service.stop(cleanup: false);
      final offline = await loginToPc(pc: pc, identity: phone, cert: cert);
      await offline.close();
      await agent.api('POST', '/api/account/refresh', expectStatus: 502); // an unreachable service is reported, not fatal
      final again = await loginToPc(pc: pc, identity: phone, cert: cert);
      await again.close();

      // --- the owner removes the phone; once the computer learns of it, the live connection ends and login is refused
      service = await AccountServiceUnderTest.start(again: service);
      final owner = await ownerSession();
      await http.delete(Uri.parse('${service.url}/v1/devices/${phone.deviceId}'), headers: {'authorization': 'Bearer ${owner.access}'});
      await agent.api('POST', '/api/account/refresh');
      await session.closed.timeout(const Duration(seconds: 10), onTimeout: () => fail('the removed phone stayed connected'));
      await expectLater(loginToPc(pc: pc, identity: phone, cert: cert), throwsA(isA<AuthFailed>().having((e) => e.revoked, 'revoked', true)));

      // --- a phone paired with a QR code is unaffected by account changes
      final other = await DeviceIdentity.generate();
      final approved = agent.approveNextRequest();
      expect(await pairWithPc(info: agent.qr, identity: other, deviceName: 'QR phone'), PairResult.approved);
      await approved;
      final qrSession = await loginToPc(pc: pc, identity: other);
      await qrSession.close();

      // --- leaving the account stops accepting its phones
      await agent.api('POST', '/api/account/leave');
      expect(((await agent.api('GET', '/api/overview')) as Map)['account'], isNull);
    },
    skip: skip,
    timeout: const Timeout(Duration(minutes: 2)),
  );
}
