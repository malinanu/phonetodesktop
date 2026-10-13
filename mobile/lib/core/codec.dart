import 'dart:convert';
import 'dart:typed_data';

/// base64url without padding, the encoding every key, nonce, signature and fingerprint uses on the wire.
String b64u(List<int> bytes) => base64Url.encode(bytes).replaceAll('=', '');

/// Inverse of [b64u]. Throws [FormatException] on anything that is not valid base64url.
Uint8List unb64u(String s) {
  final pad = (4 - s.length % 4) % 4;
  return Uint8List.fromList(base64Url.decode(s + '=' * pad));
}
