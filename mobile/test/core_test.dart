import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/core/codec.dart';
import 'package:phoneremote/core/identity.dart';
import 'package:phoneremote/core/protocol.dart';
import 'package:phoneremote/core/spki.dart';

String hex(List<int> b) => b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();

void main() {
  group('base64url', () {
    test('round trips with and without padding needs', () {
      for (final n in [0, 1, 2, 3, 4, 31, 32, 33]) {
        final bytes = Uint8List.fromList(List.generate(n, (i) => (i * 37 + 11) % 256));
        final text = b64u(bytes);
        expect(text.contains('='), isFalse);
        expect(text.contains('+') || text.contains('/'), isFalse);
        expect(unb64u(text), bytes);
      }
    });
    test('rejects invalid input', () {
      expect(() => unb64u('not base64 !!'), throwsFormatException);
    });
  });

  group('certificate pin (ground truth from the Rust agent and openssl)', () {
    // tls-cert.der was written by the PC agent; the fingerprint was computed independently with openssl.
    final cert = File('test/fixtures/agent_cert.der').readAsBytesSync();
    final fp = File('test/fixtures/agent_cert.fp').readAsStringSync().trim();

    test('the fingerprint of the real certificate matches openssl', () {
      final spki = spkiFromCert(cert);
      expect(spki, isNotNull);
      expect(fingerprintOfSpki(spki!), fp);
      expect(certMatchesPin(cert, fp), isTrue);
    });
    test('a different fingerprint does not match', () {
      expect(certMatchesPin(cert, fingerprintOfSpki(Uint8List(8))), isFalse);
      expect(certMatchesPin(cert, ''), isFalse);
    });
    test('damaged or truncated certificates are rejected, never crash', () {
      expect(spkiFromCert(Uint8List(0)), isNull);
      expect(spkiFromCert(Uint8List.fromList([0x30, 0x03, 1, 2, 3])), isNull);
      for (var cut = 1; cut < cert.length; cut += 17) {
        expect(() => spkiFromCert(Uint8List.sublistView(cert, 0, cut)), returnsNormally);
      }
      expect(certMatchesPin(Uint8List.fromList(List.filled(300, 0x30)), fp), isFalse);
    });
  });

  group('login message', () {
    test('has exactly the bytes the Rust agent signs', () {
      final expected = File('test/fixtures/auth_message.hex').readAsStringSync().trim();
      expect(hex(authMessage('pc-test', 'dev1', List.filled(32, 9))), expected);
    });
    test('is bound to the PC, the device and the nonce', () {
      final base = authMessage('pc', 'dev', [1, 2, 3]);
      expect(authMessage('other', 'dev', [1, 2, 3]), isNot(base));
      expect(authMessage('pc', 'other', [1, 2, 3]), isNot(base));
      expect(authMessage('pc', 'dev', [1, 2, 4]), isNot(base));
    });
  });

  group('device identity', () {
    test('is deterministic from its seed and signs verifiably', () async {
      final a = await DeviceIdentity.fromSeed('d', Uint8List.fromList(List.filled(32, 5)));
      final b = await DeviceIdentity.fromSeed('d', Uint8List.fromList(List.filled(32, 5)));
      expect(a.publicKey, b.publicKey);
      expect(a.publicKey.length, 32);
      final msg = authMessage('pc', 'd', List.filled(32, 1));
      expect(await a.sign(msg), await b.sign(msg));
      expect((await a.sign(msg)).length, 64);
    });
    test('a new identity is created once and then reused', () async {
      final store = MemoryIdentityStore();
      final first = await loadOrCreateIdentity(store);
      final second = await loadOrCreateIdentity(store);
      expect(second.deviceId, first.deviceId);
      expect(second.publicKey, first.publicKey);
      expect(first.deviceId.length, greaterThan(8));
      final other = await loadOrCreateIdentity(MemoryIdentityStore());
      expect(other.deviceId, isNot(first.deviceId));
    });
  });

  group('pairing QR', () {
    const good = 'http://192.168.1.20:8765/#k=CODE123&id=pc-1&n=My%20PC&fp=nEP5t69iUf2D04WbRyyyJ-7GWG4gTzzw6Ii0PzOWJ1c';
    test('is parsed', () {
      final p = PairingInfo.parse(good)!;
      expect((p.host, p.port, p.code, p.pcId, p.name), ('192.168.1.20', 8765, 'CODE123', 'pc-1', 'My PC'));
      expect(p.fingerprint, 'nEP5t69iUf2D04WbRyyyJ-7GWG4gTzzw6Ii0PzOWJ1c');
    });
    test('an older QR without a fingerprint still parses (the app then refuses it as insecure)', () {
      final p = PairingInfo.parse('http://10.0.0.5:8765/#k=C&id=p')!;
      expect(p.fingerprint, '');
      expect(p.name, '10.0.0.5');
    });
    test('anything else is rejected', () {
      for (final bad in [null, '', 'hello', 'https://192.168.1.2:8765/#k=C&id=p', 'http://192.168.1.2/#k=C&id=p', 'http://192.168.1.2:8765/', 'http://192.168.1.2:8765/#id=p', 'http://192.168.1.2:8765/#k=C', 'javascript:alert(1)']) {
        expect(PairingInfo.parse(bad), isNull, reason: '$bad');
      }
    });
  });

  group('agent state', () {
    test('is parsed, with missing fields tolerated', () {
      final s = AgentState.fromJson({
        't': 'state',
        'host': 'pc',
        'backend': 'mock',
        'version': '1',
        'current': 'b',
        'players': [
          {'id': 'a', 'app': 'A', 'title': 'x', 'artist': '', 'playing': false, 'pos_ms': 1000, 'dur_ms': 5000, 'can_seek': true},
          {'id': 'b', 'app': 'B', 'playing': true},
        ],
        'volume': 40,
        'muted': false,
      });
      expect(s.nowPlaying!.id, 'b');
      expect(s.players.first.posMs, 1000);
      expect(s.volume, 40);
      expect(AgentState.fromJson({}).nowPlaying, isNull);
    });
  });
}
