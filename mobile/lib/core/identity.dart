import 'dart:math';
import 'dart:typed_data';

import 'package:cryptography/cryptography.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'codec.dart';

/// This phone's identity: a random device id and an Ed25519 key pair. The PC stores only the public key.
class DeviceIdentity {
  DeviceIdentity._(this.deviceId, this._seed, this.publicKey, this._keyPair);

  final String deviceId;
  final Uint8List _seed;
  final Uint8List publicKey;
  final SimpleKeyPair _keyPair;

  static final _algorithm = Ed25519();

  static Future<DeviceIdentity> fromSeed(String deviceId, Uint8List seed) async {
    final pair = await _algorithm.newKeyPairFromSeed(seed);
    final pub = await pair.extractPublicKey();
    return DeviceIdentity._(deviceId, seed, Uint8List.fromList(pub.bytes), pair);
  }

  static Future<DeviceIdentity> generate() {
    final rng = Random.secure();
    Uint8List bytes(int n) => Uint8List.fromList(List<int>.generate(n, (_) => rng.nextInt(256)));
    return fromSeed(b64u(bytes(12)), bytes(32));
  }

  String get publicKeyB64 => b64u(publicKey);

  Future<Uint8List> sign(List<int> message) async {
    final sig = await _algorithm.sign(message, keyPair: _keyPair);
    return Uint8List.fromList(sig.bytes);
  }

  /// For [IdentityStore] implementations.
  String get seedB64 => b64u(_seed);
}

/// Where the identity lives between app launches.
abstract class IdentityStore {
  Future<DeviceIdentity?> load();
  Future<void> save(DeviceIdentity identity);
}

class MemoryIdentityStore implements IdentityStore {
  DeviceIdentity? _identity;
  @override
  Future<DeviceIdentity?> load() async => _identity;
  @override
  Future<void> save(DeviceIdentity identity) async => _identity = identity;
}

/// Keeps the private key in the OS keystore (Android Keystore-backed storage / iOS Keychain).
class SecureIdentityStore implements IdentityStore {
  SecureIdentityStore([FlutterSecureStorage? storage]) : _storage = storage ?? const FlutterSecureStorage();
  final FlutterSecureStorage _storage;

  @override
  Future<DeviceIdentity?> load() async {
    final id = await _storage.read(key: 'identity.id');
    final seed = await _storage.read(key: 'identity.seed');
    if (id == null || seed == null) return null;
    try {
      return await DeviceIdentity.fromSeed(id, unb64u(seed));
    } on FormatException {
      return null; // damaged: the caller makes a new identity and the phone pairs again
    }
  }

  @override
  Future<void> save(DeviceIdentity identity) async {
    await _storage.write(key: 'identity.id', value: identity.deviceId);
    await _storage.write(key: 'identity.seed', value: identity.seedB64);
  }
}

/// The saved identity, or a new one (created and saved on first launch).
Future<DeviceIdentity> loadOrCreateIdentity(IdentityStore store) async {
  final existing = await store.load();
  if (existing != null) return existing;
  final created = await DeviceIdentity.generate();
  await store.save(created);
  return created;
}
