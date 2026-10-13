// The account client and controller against a fake service (no network).
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:phoneremote/app/account.dart';
import 'package:phoneremote/core/account_api.dart';
import 'package:phoneremote/core/codec.dart';
import 'package:phoneremote/core/identity.dart';

String certWithExpiry(int exp) => '${b64u(utf8.encode(jsonEncode({'exp': exp, 'dev': 'd'})))}.sig';

class FakeService {
  FakeService({this.clock = 1000000});
  int clock;
  final requests = <String>[];
  final bodies = <String, Map<String, dynamic>>{};
  bool down = false;
  bool refreshRejected = false;
  int refreshCount = 0;
  String certToIssue = '';
  List<Map<String, dynamic>> devices = [];

  Future<http.Response> handle(http.Request r) async {
    if (down) throw http.ClientException('offline');
    final key = '${r.method} ${r.url.path}';
    requests.add(key);
    if (r.body.isNotEmpty) bodies[key] = jsonDecode(r.body) as Map<String, dynamic>;
    http.Response json(Object o, [int code = 200]) => http.Response(jsonEncode(o), code, headers: {'content-type': 'application/json'});
    Map<String, dynamic> tokens(int n) => {
      'access_token': 'access-$n',
      'refresh_token': 'refresh-$n',
      'user': {'id': 'acct-1', 'email': 'me@mail.dev'},
    };

    switch (key) {
      case 'POST /account/v1/auth/email':
        return json({}, 202);
      case 'POST /account/v1/auth/verify':
        return bodies[key]!['code'] == '123456' ? json(tokens(0)) : json({'detail': 'That code is not valid. Ask for a new one.'}, 401);
      case 'POST /account/v1/auth/refresh':
        if (refreshRejected) return json({'detail': 'Sign in again.'}, 401);
        return json(tokens(++refreshCount));
      case 'POST /account/v1/devices':
        return json({'device': {}, 'cert': certToIssue, 'account_id': 'acct-1', 'keys': {}}, 201);
      case 'GET /account/v1/devices':
        return json({'devices': devices});
      case 'POST /account/v1/auth/logout':
        return http.Response('', 204);
      case 'DELETE /account/v1/me':
        return http.Response('', 204);
    }
    if (r.method == 'POST' && r.url.path.endsWith('/cert')) return json({'cert': certToIssue, 'keys': {}});
    return json({'detail': 'nope'}, 404);
  }
}

void main() {
  const url = 'https://svc.example/account';

  late FakeService svc;
  late MemoryAccountStore store;
  late MemoryIdentityStore ids;
  late AccountController account;

  setUp(() {
    svc = FakeService()..certToIssue = certWithExpiry(1000000 + 30 * 86400);
    store = MemoryAccountStore();
    ids = MemoryIdentityStore();
    account = AccountController(
      store: store,
      identityStore: ids,
      deviceName: 'Test phone',
      platform: 'android',
      apiFor: (base) => AccountApi(base, client: MockClient(svc.handle)),
      clock: () => svc.clock,
    );
  });

  group('addresses', () {
    test('are cleaned and unsafe ones refused', () {
      expect(AccountApi.clean('https://files.example.com/account/'), 'https://files.example.com/account');
      expect(AccountApi.clean(' files.example.com '), 'https://files.example.com');
      expect(AccountApi.clean('http://127.0.0.1:8000'), 'http://127.0.0.1:8000');
      for (final bad in ['', 'http://example.com', 'ftp://example.com', 'https://', 'https://a b.com', 'https://u@example.com', 'https://example.com?x=1']) {
        expect(AccountApi.clean(bad), isNull, reason: bad);
      }
    });
  });

  test('a wrong code says so and signs nobody in', () async {
    expect(await account.requestCode(url, 'me@mail.dev'), isTrue);
    expect(account.waitingForCodeAt, 'me@mail.dev');
    expect(await account.signIn('000000'), isFalse);
    expect(account.error, contains('not valid'));
    expect(account.signedIn, isFalse);
    expect(store.saved, isNull);
  });

  test('the right code signs in and enrols this phone with its existing key, never the private key', () async {
    await account.requestCode(url, 'me@mail.dev');
    expect(await account.signIn('123456'), isTrue);
    expect(account.signedIn, isTrue);
    expect(store.saved!.refresh, 'refresh-0');
    final sent = svc.bodies['POST /account/v1/devices']!;
    final id = await loadOrCreateIdentity(ids);
    expect(sent['id'], id.deviceId);
    expect(sent['pubkey'], id.publicKeyB64);
    expect((sent['kind'], sent['platform'], sent['name']), ('phone', 'android', 'Test phone'));
    expect(jsonEncode(sent), isNot(contains(id.seedB64)));
  });

  test('a bad address or email is refused before anything is sent', () async {
    expect(await account.requestCode('http://example.com', 'me@mail.dev'), isFalse);
    expect(await account.requestCode(url, 'not an email'), isFalse);
    expect(svc.requests, isEmpty);
  });

  group('the certificate', () {
    Future<void> signedIn() async {
      await account.requestCode(url, 'me@mail.dev');
      await account.signIn('123456');
      svc.requests.clear();
    }

    test('is used as it is while it has more than a week left', () async {
      await signedIn();
      expect(await account.freshCert(), store.saved!.cert);
      expect(svc.requests, isEmpty, reason: 'no network needed');
    });

    test('is renewed when under a week is left, and the rotated refresh token is saved', () async {
      await signedIn();
      svc.clock += 24 * 86400; // 6 days left
      svc.certToIssue = certWithExpiry(svc.clock + 30 * 86400);
      expect(await account.freshCert(), svc.certToIssue);
      expect(store.saved!.cert, svc.certToIssue);
      expect(store.saved!.refresh, 'refresh-1');
      expect(svc.requests, ['POST /account/v1/auth/refresh', startsWith('POST /account/v1/devices/')]);
    });

    test('offline, the saved one is used even when it is old', () async {
      await signedIn();
      final old = store.saved!.cert;
      svc.clock += 40 * 86400;
      svc.down = true;
      expect(await account.freshCert(), old);
      expect(account.signedIn, isTrue);
    });

    test('two logins at once renew only once', () async {
      await signedIn();
      svc.clock += 25 * 86400;
      svc.certToIssue = certWithExpiry(svc.clock + 30 * 86400);
      final both = await Future.wait([account.freshCert(), account.freshCert()]);
      expect(both, [svc.certToIssue, svc.certToIssue]);
      expect(svc.refreshCount, 1);
    });

    test('a sign-in that ended is flagged, and the saved certificate keeps being offered', () async {
      await signedIn();
      final saved = store.saved!.cert;
      svc.clock += 25 * 86400;
      svc.refreshRejected = true;
      expect(await account.freshCert(), saved);
      expect(account.session!.expired, isTrue);
      expect(store.saved!.expired, isTrue);
    });

    test('there is none without an account', () async {
      expect(await account.freshCert(), isNull);
    });

    test('expiry is read from the payload and garbage is ignored', () {
      expect(certExpiry(certWithExpiry(42)), 42);
      for (final bad in ['', '.', 'not base64!.x', 'e30.x']) {
        expect(certExpiry(bad), isNull, reason: bad);
      }
    });
  });

  test('only computers that can be found and checked on the local network are offered', () async {
    await account.requestCode(url, 'me@mail.dev');
    await account.signIn('123456');
    svc.devices = [
      {'id': 'pc-a', 'name': 'Desk', 'platform': 'windows', 'kind': 'pc', 'revoked': false, 'pc_id': 'deskid123456', 'fingerprint': 'F' * 43},
      {'id': 'pc-b', 'name': 'Removed', 'platform': 'linux', 'kind': 'pc', 'revoked': true, 'pc_id': 'oldid1234567', 'fingerprint': 'G' * 43},
      {'id': 'pc-c', 'name': 'Old version', 'platform': 'mac', 'kind': 'pc', 'revoked': false},
      {'id': 'ph-a', 'name': 'Pixel', 'platform': 'android', 'kind': 'phone', 'revoked': false},
    ];
    final list = await account.computers();
    expect([for (final d in list!) d.name], ['Desk']);
  });

  test('signing out forgets everything on the phone even when offline, and tells the service when it can', () async {
    await account.requestCode(url, 'me@mail.dev');
    await account.signIn('123456');
    svc.down = true;
    await account.signOut();
    expect(account.signedIn, isFalse);
    expect(store.saved, isNull);
  });

  test('deleting the account removes it from the service and the phone; a failure keeps the sign-in', () async {
    await account.requestCode(url, 'me@mail.dev');
    await account.signIn('123456');
    svc.down = true;
    expect(await account.deleteAccount(), isFalse);
    expect(account.signedIn, isTrue);
    expect(account.error, isNotNull);
    svc.down = false;
    expect(await account.deleteAccount(), isTrue);
    expect(account.signedIn, isFalse);
    expect(svc.requests, contains('DELETE /account/v1/me'));
  });
}
