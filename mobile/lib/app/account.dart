import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../core/account_api.dart';
import '../core/identity.dart';

/// What the phone keeps about the account it signed in to. Sensitive (the refresh token), so it lives in the
/// OS keystore like the device key.
class AccountSession {
  const AccountSession({required this.url, required this.email, required this.refresh, required this.cert, required this.accountId, this.expired = false});
  final String url, email, refresh, cert, accountId;

  /// The service no longer accepts the saved sign-in (it was signed out elsewhere or timed out): sign in again.
  final bool expired;

  AccountSession copyWith({String? refresh, String? cert, bool? expired}) =>
      AccountSession(url: url, email: email, refresh: refresh ?? this.refresh, cert: cert ?? this.cert, accountId: accountId, expired: expired ?? this.expired);

  Map<String, dynamic> toJson() => {'url': url, 'email': email, 'refresh': refresh, 'cert': cert, 'acct': accountId, 'expired': expired};

  factory AccountSession.fromJson(Map<String, dynamic> j) => AccountSession(
    url: j['url'] as String,
    email: j['email'] as String,
    refresh: j['refresh'] as String,
    cert: j['cert'] as String,
    accountId: j['acct'] as String,
    expired: j['expired'] == true,
  );
}

abstract class AccountStore {
  Future<AccountSession?> load();
  Future<void> save(AccountSession? session);
}

class MemoryAccountStore implements AccountStore {
  AccountSession? saved;
  @override
  Future<AccountSession?> load() async => saved;
  @override
  Future<void> save(AccountSession? session) async => saved = session;
}

class SecureAccountStore implements AccountStore {
  SecureAccountStore([FlutterSecureStorage? storage]) : _storage = storage ?? const FlutterSecureStorage();
  final FlutterSecureStorage _storage;
  static const _key = 'account.session';

  @override
  Future<AccountSession?> load() async {
    try {
      final raw = await _storage.read(key: _key);
      return raw == null ? null : AccountSession.fromJson(jsonDecode(raw) as Map<String, dynamic>);
    } catch (_) {
      return null; // damaged: sign in again
    }
  }

  @override
  Future<void> save(AccountSession? session) async {
    if (session == null) {
      await _storage.delete(key: _key);
    } else {
      await _storage.write(key: _key, value: jsonEncode(session.toJson()));
    }
  }
}

/// The optional account: sign in with an email code, enrol this phone's key, keep its certificate fresh, find the
/// account's computers. Controlling a PC never waits for any of this.
class AccountController extends ChangeNotifier {
  AccountController({
    required this.store,
    required this.identityStore,
    required this.deviceName,
    required this.platform,
    this.apiFor = AccountApi.new,
    int Function()? clock,
    this.renewBefore = const Duration(days: 7),
  }) : _clock = clock ?? (() => DateTime.now().millisecondsSinceEpoch ~/ 1000);

  final AccountStore store;
  final IdentityStore identityStore;
  final String deviceName, platform;
  final AccountApi Function(String baseUrl) apiFor;
  final Duration renewBefore;
  final int Function() _clock;

  AccountSession? _session;
  String? _pendingUrl, _pendingEmail;
  bool _busy = false;
  String? _error;
  Future<String?>? _renewing;

  AccountSession? get session => _session;
  bool get signedIn => _session != null;
  bool get busy => _busy;
  String? get error => _error;

  /// A code was sent to this address and is waiting to be typed in.
  String? get waitingForCodeAt => _pendingEmail;

  Future<void> load() async {
    _session = await store.load();
    notifyListeners();
  }

  Future<bool> _guard(Future<void> Function() body) async {
    _busy = true;
    _error = null;
    notifyListeners();
    try {
      await body();
      return true;
    } on AccountError catch (e) {
      _error = e.message;
      return false;
    } finally {
      _busy = false;
      notifyListeners();
    }
  }

  /// Step 1: ask the service to email a code.
  Future<bool> requestCode(String serviceUrl, String email) => _guard(() async {
    final url = AccountApi.clean(serviceUrl);
    if (url == null) throw AccountError('That address does not look right. It must start with https://');
    if (!email.contains('@') || email.trim().length < 5) throw AccountError('Type your email address.');
    final api = apiFor(url);
    try {
      await api.requestCode(email);
    } finally {
      api.close();
    }
    _pendingUrl = url;
    _pendingEmail = email.trim();
  });

  void cancelCode() {
    _pendingUrl = _pendingEmail = null;
    _error = null;
    notifyListeners();
  }

  /// Step 2: the code from the email. Signs in and enrols this phone with its existing device key.
  Future<bool> signIn(String code) => _guard(() async {
    final url = _pendingUrl, email = _pendingEmail;
    if (url == null || email == null) throw AccountError('Ask for a code first.');
    final api = apiFor(url);
    try {
      final tokens = await api.verify(email, code);
      final id = await loadOrCreateIdentity(identityStore);
      final enrolled = await api.enroll(tokens.access, id: id.deviceId, name: deviceName, platform: platform, pubkey: id.publicKeyB64);
      _session = AccountSession(url: url, email: tokens.email, refresh: tokens.refresh, cert: enrolled.cert, accountId: enrolled.accountId);
      await store.save(_session);
      _pendingUrl = _pendingEmail = null;
    } finally {
      api.close();
    }
  });

  /// A fresh access token (the saved refresh token is single-use, so the new one is saved at once).
  Future<String> _access(AccountApi api) async {
    final s = _session!;
    try {
      final t = await api.refresh(s.refresh);
      _session = s.copyWith(refresh: t.refresh, expired: false);
      await store.save(_session);
      return t.access;
    } on AccountError catch (e) {
      if (e.unauthorized) {
        _session = s.copyWith(expired: true);
        await store.save(_session);
        notifyListeners();
        throw AccountError('Your sign-in ended. Sign in again.', status: 401);
      }
      rethrow;
    }
  }

  /// The certificate to show a PC: the saved one, renewed first when it has under a week left. Never throws and
  /// never waits for long: offline, the saved certificate is used (a PC accepts it for a grace period after expiry).
  Future<String?> freshCert() {
    final s = _session;
    if (s == null) return Future.value(null);
    final exp = certExpiry(s.cert);
    final left = exp == null ? Duration.zero : Duration(seconds: exp - _clock());
    if (left > renewBefore || s.expired) return Future.value(s.cert);
    return _renewing ??= _renew().whenComplete(() => _renewing = null);
  }

  Future<String?> _renew() async {
    final s = _session!;
    final api = apiFor(s.url);
    try {
      final access = await _access(api);
      final id = await loadOrCreateIdentity(identityStore);
      final cert = await api.renewCert(access, id.deviceId);
      _session = _session!.copyWith(cert: cert);
      await store.save(_session);
      return cert;
    } catch (_) {
      return _session?.cert ?? s.cert;
    } finally {
      api.close();
    }
  }

  /// The computers on the account that have told the service how to find them on the local network.
  Future<List<AccountDevice>?> computers() async {
    List<AccountDevice>? out;
    await _guard(() async {
      if (_session == null) return;
      final api = apiFor(_session!.url);
      try {
        final list = await api.devices(await _access(api));
        out = [
          for (final d in list)
            if (d.kind == 'pc' && !d.revoked && d.pcId != null && d.fingerprint != null) d,
        ];
      } finally {
        api.close();
      }
    });
    return out;
  }

  /// Sign out of this phone. The device stays listed in the account until removed there.
  Future<void> signOut() async {
    final s = _session;
    _session = null;
    await store.save(null);
    notifyListeners();
    if (s == null) return;
    final api = apiFor(s.url);
    try {
      await api.logout(s.refresh);
    } catch (_) {
      // offline: the saved token is gone from this phone anyway
    } finally {
      api.close();
    }
  }

  /// Delete the whole account (all devices, the email). Cannot be undone.
  Future<bool> deleteAccount() async {
    final s = _session;
    if (s == null) return false;
    final ok = await _guard(() async {
      final api = apiFor(s.url);
      try {
        await api.deleteAccount(await _access(api));
      } finally {
        api.close();
      }
    });
    if (ok) {
      _session = null;
      await store.save(null);
      notifyListeners();
    }
    return ok;
  }
}
