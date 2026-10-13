import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Preferences, saved on the phone. The names and defaults match the old app and the PC's settings page.
class AppSettings extends ChangeNotifier {
  AppSettings._(this._prefs, Map<String, dynamic> stored) : _values = {...defaults, ...stored};

  static const defaults = <String, dynamic>{
    'theme': 'system', // system | dark | light
    'skip': 10, // seconds for the skip buttons
    'volStep': 2,
    'padSpeed': 1.0,
    'scrollNatural': true,
    'tapToClick': true,
    'vibrate': true,
    'keepAwake': false,
    'filesUrl': '',
  };

  static const _key = 'pr.settings';
  final SharedPreferences _prefs;
  final Map<String, dynamic> _values;

  static Future<AppSettings> load() async {
    final prefs = await SharedPreferences.getInstance();
    Map<String, dynamic> stored = {};
    try {
      final raw = prefs.getString(_key);
      if (raw != null) stored = jsonDecode(raw) as Map<String, dynamic>;
    } on FormatException {
      // damaged: start from the defaults
    }
    final s = AppSettings._(prefs, {});
    // Keep only known keys whose type matches the default, like the web settings page does.
    for (final e in stored.entries) {
      final d = defaults[e.key];
      if (d != null && e.value.runtimeType == d.runtimeType || (d is double && e.value is num)) {
        s._values[e.key] = d is double ? (e.value as num).toDouble() : e.value;
      }
    }
    return s;
  }

  T _get<T>(String k) => _values[k] as T;

  String get theme => _get('theme');
  int get skip => _get('skip');
  int get volStep => _get('volStep');
  double get padSpeed => _get('padSpeed');
  bool get scrollNatural => _get('scrollNatural');
  bool get tapToClick => _get('tapToClick');
  bool get vibrate => _get('vibrate');
  bool get keepAwake => _get('keepAwake');
  String get filesUrl => _get('filesUrl');

  Future<void> set(String key, Object value) async {
    if (!defaults.containsKey(key)) return;
    _values[key] = value;
    notifyListeners();
    await _prefs.setString(_key, jsonEncode(_values));
  }

  Future<void> reset() async {
    _values
      ..clear()
      ..addAll(defaults);
    notifyListeners();
    await _prefs.setString(_key, jsonEncode(_values));
  }
}
