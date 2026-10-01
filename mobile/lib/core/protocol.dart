import 'dart:convert';
import 'dart:typed_data';

/// The exact bytes a phone signs to log in. Must match `auth_message` in agent/src/auth.rs:
/// `"PRv2-auth" 0x00 pcId 0x00 deviceId 0x00 nonce` (raw nonce bytes).
Uint8List authMessage(String pcId, String deviceId, List<int> nonce) {
  final b = BytesBuilder(copy: false)
    ..add(utf8.encode('PRv2-auth'))
    ..addByte(0)
    ..add(utf8.encode(pcId))
    ..addByte(0)
    ..add(utf8.encode(deviceId))
    ..addByte(0)
    ..add(nonce);
  return b.toBytes();
}

/// What a pairing QR carries: `http://IP:PORT/#k=CODE&id=PCID&n=NAME&fp=FINGERPRINT`.
class PairingInfo {
  const PairingInfo({required this.host, required this.port, required this.code, required this.pcId, required this.name, required this.fingerprint});

  final String host;
  final int port;
  final String code;
  final String pcId;
  final String name;

  /// SHA-256 of the PC's TLS public key (base64url). Empty for a PC that has no secure connection.
  final String fingerprint;

  /// Null when [text] is not a Phone Remote code.
  static PairingInfo? parse(String? text) {
    if (text == null) return null;
    final uri = Uri.tryParse(text.trim());
    if (uri == null || uri.scheme != 'http' || uri.host.isEmpty || !uri.hasPort || uri.fragment.isEmpty) return null;
    final Map<String, String> params;
    try {
      params = Uri.splitQueryString(uri.fragment);
    } on FormatException {
      return null;
    }
    final code = params['k'];
    final id = params['id'];
    if (code == null || code.isEmpty || id == null || id.isEmpty) return null;
    return PairingInfo(host: uri.host, port: uri.port, code: code, pcId: id, name: params['n'] ?? uri.host, fingerprint: params['fp'] ?? '');
  }
}

class PlayerInfo {
  const PlayerInfo({required this.id, required this.app, required this.title, required this.artist, required this.playing, required this.posMs, required this.durMs, required this.canSeek});

  final String id, app, title, artist;
  final bool playing, canSeek;
  final int posMs, durMs;

  factory PlayerInfo.fromJson(Map<String, dynamic> j) => PlayerInfo(
        id: j['id'] as String? ?? '',
        app: j['app'] as String? ?? '',
        title: j['title'] as String? ?? '',
        artist: j['artist'] as String? ?? '',
        playing: j['playing'] as bool? ?? false,
        posMs: (j['pos_ms'] as num?)?.toInt() ?? 0,
        durMs: (j['dur_ms'] as num?)?.toInt() ?? 0,
        canSeek: j['can_seek'] as bool? ?? false,
      );
}

class AgentState {
  const AgentState({required this.host, required this.backend, required this.version, required this.current, required this.players, required this.volume, required this.muted});

  final String host, backend, version;
  final String? current;
  final List<PlayerInfo> players;
  final int? volume;
  final bool? muted;

  factory AgentState.fromJson(Map<String, dynamic> j) => AgentState(
        host: j['host'] as String? ?? '',
        backend: j['backend'] as String? ?? '',
        version: j['version'] as String? ?? '',
        current: j['current'] as String?,
        players: [for (final p in (j['players'] as List? ?? const [])) PlayerInfo.fromJson(p as Map<String, dynamic>)],
        volume: (j['volume'] as num?)?.toInt(),
        muted: j['muted'] as bool?,
      );

  /// The player the commands go to, if any.
  PlayerInfo? get nowPlaying {
    for (final p in players) {
      if (p.id == current) return p;
    }
    return players.isEmpty ? null : players.first;
  }
}
