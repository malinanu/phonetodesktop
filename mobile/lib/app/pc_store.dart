import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

import '../core/session.dart';

/// The PCs this phone is paired with, and which one is open. (The phone's private key is NOT here: it lives in
/// the OS keystore, see core/identity.dart.)
abstract class PcStore {
  Future<List<PcRecord>> load();
  Future<void> save(List<PcRecord> pcs);
  Future<String?> activeId();
  Future<void> setActive(String? id);
}

class MemoryPcStore implements PcStore {
  List<PcRecord> _pcs = [];
  String? _active;
  @override
  Future<List<PcRecord>> load() async => List.of(_pcs);
  @override
  Future<void> save(List<PcRecord> pcs) async => _pcs = List.of(pcs);
  @override
  Future<String?> activeId() async => _active;
  @override
  Future<void> setActive(String? id) async => _active = id;
}

class PrefsPcStore implements PcStore {
  static const _pcs = 'pr.pcs';
  static const _active = 'pr.activePc';

  @override
  Future<List<PcRecord>> load() async {
    final prefs = await SharedPreferences.getInstance();
    try {
      final raw = prefs.getString(_pcs);
      if (raw == null) return [];
      return [for (final j in jsonDecode(raw) as List) PcRecord.fromJson(j as Map<String, dynamic>)];
    } catch (_) {
      return []; // damaged: the phone just pairs again
    }
  }

  @override
  Future<void> save(List<PcRecord> pcs) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_pcs, jsonEncode([for (final p in pcs) p.toJson()]));
  }

  @override
  Future<String?> activeId() async => (await SharedPreferences.getInstance()).getString(_active);

  @override
  Future<void> setActive(String? id) async {
    final prefs = await SharedPreferences.getInstance();
    id == null ? await prefs.remove(_active) : await prefs.setString(_active, id);
  }
}
