import 'dart:typed_data';

import 'package:crypto/crypto.dart';

import 'codec.dart';

/// Reads the SubjectPublicKeyInfo out of an X.509 certificate (DER), without a full certificate parser.
/// Same walk as `spki_from_cert` in the PC agent (agent/src/tls.rs).
Uint8List? spkiFromCert(Uint8List cert) {
  final c = _element(cert, 0); // Certificate ::= SEQUENCE
  if (c == null || c.tag != 0x30 || c.header + c.length > cert.length) return null;
  final tbs = _element(cert, c.header); // tbsCertificate ::= SEQUENCE
  if (tbs == null || tbs.tag != 0x30 || c.header + tbs.header + tbs.length > cert.length) return null;
  var offset = c.header + tbs.header;
  final end = offset + tbs.length;
  var e = _element(cert, offset);
  if (e == null) return null;
  if (e.tag == 0xA0) offset += e.header + e.length; // optional version [0]
  // serial, signature, issuer, validity, subject
  for (var i = 0; i < 5; i++) {
    e = _element(cert, offset);
    if (e == null || offset + e.header + e.length > end) return null;
    offset += e.header + e.length;
  }
  e = _element(cert, offset); // subjectPublicKeyInfo ::= SEQUENCE
  if (e == null || e.tag != 0x30 || offset + e.header + e.length > end) return null;
  return Uint8List.sublistView(cert, offset, offset + e.header + e.length);
}

/// What the QR carries as `fp`: base64url SHA-256 of the SubjectPublicKeyInfo.
String fingerprintOfSpki(List<int> spki) => b64u(sha256.convert(spki).bytes);

/// True when this certificate's public key is the one the QR named.
bool certMatchesPin(Uint8List certDer, String fingerprint) {
  final spki = spkiFromCert(certDer);
  return spki != null && fingerprintOfSpki(spki) == fingerprint;
}

class _Element {
  _Element(this.tag, this.header, this.length);
  final int tag, header, length;
}

_Element? _element(Uint8List b, int offset) {
  if (offset + 2 > b.length) return null;
  final tag = b[offset];
  final l0 = b[offset + 1];
  if (l0 < 0x80) return _Element(tag, 2, l0);
  final n = l0 & 0x7f;
  if (n == 0 || n > 4 || offset + 2 + n > b.length) return null;
  var len = 0;
  for (var i = 0; i < n; i++) {
    len = (len << 8) | b[offset + 2 + i];
  }
  return _Element(tag, 2 + n, len);
}
