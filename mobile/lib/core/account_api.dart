import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;

import 'codec.dart';

/// The account service refused, or could not be reached. [message] is fit to show to a person.
class AccountError implements Exception {
  AccountError(this.message, {this.status = 0});
  final String message;

  /// HTTP status, or 0 when the service could not be reached.
  final int status;

  bool get unauthorized => status == 401;
  bool get unreachable => status == 0;
  @override
  String toString() => message;
}

class Tokens {
  const Tokens({required this.access, required this.refresh, required this.userId, required this.email});
  final String access, refresh, userId, email;
}

/// A device in the account, as the service lists it.
class AccountDevice {
  const AccountDevice({required this.id, required this.name, required this.platform, required this.kind, required this.revoked, this.pcId, this.fingerprint});
  final String id, name, platform, kind;
  final bool revoked;

  /// For a computer: the id it advertises on the local network, and the fingerprint of its TLS key (what phones pin).
  final String? pcId, fingerprint;

  factory AccountDevice.fromJson(Map<String, dynamic> j) => AccountDevice(
    id: j['id'] as String,
    name: j['name'] as String? ?? '',
    platform: j['platform'] as String? ?? '',
    kind: j['kind'] as String? ?? 'phone',
    revoked: j['revoked'] == true,
    pcId: j['pc_id'] as String?,
    fingerprint: j['fingerprint'] as String?,
  );
}

class Enrolled {
  const Enrolled(this.cert, this.accountId);
  final String cert, accountId;
}

/// Calls to the optional account service (docs/ACCOUNTS.md). Nothing here is needed to control a PC; it only
/// signs in, enrols this phone and keeps its device certificate fresh.
class AccountApi {
  AccountApi(String baseUrl, {http.Client? client, this.timeout = const Duration(seconds: 15)})
    : base = baseUrl.endsWith('/') ? baseUrl.substring(0, baseUrl.length - 1) : baseUrl,
      _client = client ?? http.Client();

  final String base;
  final Duration timeout;
  final http.Client _client;

  /// The cleaned-up service address, or null if it is not usable (https only; plain http for this machine only).
  static String? clean(String input) {
    var t = input.trim();
    if (t.isEmpty || t.length > 200 || t.contains(RegExp(r'[\s?#@\\]'))) return null;
    if (!t.contains('://')) t = 'https://$t';
    final u = Uri.tryParse(t);
    if (u == null || u.host.isEmpty) return null;
    final local = u.host == 'localhost' || u.host == '127.0.0.1';
    if (u.scheme != 'https' && !(u.scheme == 'http' && local)) return null;
    final path = u.path.endsWith('/') ? u.path.substring(0, u.path.length - 1) : u.path;
    return '${u.scheme}://${u.host}${u.hasPort ? ':${u.port}' : ''}$path';
  }

  Future<dynamic> _call(String method, String path, {Object? body, String? bearer}) async {
    final req = http.Request(method, Uri.parse('$base$path'));
    if (body != null) {
      req.headers['content-type'] = 'application/json';
      req.body = jsonEncode(body);
    }
    if (bearer != null) req.headers['authorization'] = 'Bearer $bearer';
    http.Response res;
    try {
      res = await http.Response.fromStream(await _client.send(req).timeout(timeout));
    } on TimeoutException {
      throw AccountError('The account service did not answer.');
    } catch (_) {
      throw AccountError('Could not reach the account service. Check your internet connection.');
    }
    if (res.statusCode >= 200 && res.statusCode < 300) {
      return res.body.isEmpty ? null : jsonDecode(res.body);
    }
    var detail = 'The account service answered ${res.statusCode}.';
    try {
      final d = (jsonDecode(res.body) as Map)['detail'];
      if (d is String) detail = d;
      if (res.statusCode == 422) detail = 'Please check what you typed.';
    } catch (_) {}
    throw AccountError(detail, status: res.statusCode);
  }

  Tokens _tokens(dynamic j) => Tokens(
    access: j['access_token'] as String,
    refresh: j['refresh_token'] as String,
    userId: (j['user'] as Map)['id'] as String,
    email: (j['user'] as Map)['email'] as String,
  );

  Future<void> requestCode(String email) => _call('POST', '/v1/auth/email', body: {'email': email.trim()});

  Future<Tokens> verify(String email, String code) async => _tokens(await _call('POST', '/v1/auth/verify', body: {'email': email.trim(), 'code': code.trim()}));

  Future<Tokens> refresh(String refreshToken) async => _tokens(await _call('POST', '/v1/auth/refresh', body: {'refresh_token': refreshToken}));

  Future<void> logout(String refreshToken) => _call('POST', '/v1/auth/logout', body: {'refresh_token': refreshToken});

  Future<void> deleteAccount(String access) => _call('DELETE', '/v1/me', bearer: access);

  /// Enrol this phone (its public key only; the private key never leaves the phone) and get its certificate.
  Future<Enrolled> enroll(String access, {required String id, required String name, required String platform, required String pubkey}) async {
    final j = await _call('POST', '/v1/devices', bearer: access, body: {'id': id, 'name': name, 'platform': platform, 'kind': 'phone', 'pubkey': pubkey});
    return Enrolled(j['cert'] as String, j['account_id'] as String);
  }

  Future<String> renewCert(String access, String deviceId) async => (await _call('POST', '/v1/devices/$deviceId/cert', bearer: access))['cert'] as String;

  Future<List<AccountDevice>> devices(String access) async => [
    for (final d in (await _call('GET', '/v1/devices', bearer: access))['devices'] as List) AccountDevice.fromJson(d as Map<String, dynamic>),
  ];

  void close() => _client.close();
}

/// When a certificate stops being valid (unix seconds), read from its payload without checking the signature:
/// only the PC checks it; the phone just wants to know when to ask for a new one. Null if unreadable.
int? certExpiry(String cert) {
  try {
    final body = jsonDecode(utf8.decode(unb64u(cert.split('.').first))) as Map;
    return (body['exp'] as num).toInt();
  } catch (_) {
    return null;
  }
}
