/// Address of the Send files server. Same rules as FilesUrl.kt (the old Android app) and `clean_files_url` in the
/// PC agent: https only, a strict host and port, no credentials, and only inert characters in the path or query.
class FilesUrl {
  static final _host = RegExp(r'^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?)*$');
  static final _tail = RegExp(r'^[A-Za-z0-9._~/?#=+,:;@!*-]*$');

  /// A normalised https URL, or null when [raw] is empty or unsafe. A bare host gets https:// added.
  static String? clean(String? raw) {
    var s = raw?.trim() ?? '';
    if (s.isEmpty || s.contains(RegExp(r'\s'))) return null;
    if (!s.contains('://')) s = 'https://$s';
    if (!s.toLowerCase().startsWith('https://')) return null;
    final rest = s.substring(8);
    var cut = rest.indexOf(RegExp(r'[/?#]'));
    if (cut < 0) cut = rest.length;
    final authority = rest.substring(0, cut);
    final tail = rest.substring(cut);
    if (authority.isEmpty || authority.contains('@') || s.contains(r'\')) return null;
    final colon = authority.indexOf(':');
    final host = colon < 0 ? authority : authority.substring(0, colon);
    if (colon >= 0) {
      final port = authority.substring(colon + 1);
      final n = RegExp(r'^[0-9]{1,5}$').hasMatch(port) ? int.parse(port) : null;
      if (n == null || n < 1 || n > 65535) return null;
    }
    if (!_host.hasMatch(host) || !_tail.hasMatch(tail)) return null;
    return 'https://${authority.toLowerCase()}$tail';
  }

  /// The built-in Send files page of a PC: the Phone Remote program serves it on its own port plus one.
  /// [host] is the address the phone already uses to reach the PC.
  static String? local(String host, int agentPort) {
    if (!_host.hasMatch(host) || agentPort < 1 || agentPort > 65534) return null;
    return 'http://$host:${agentPort + 1}/';
  }

  /// Is [raw] (a scanned code) a link to that PC's built-in Send files page, such as a room link?
  static bool isLocalLink(String? raw, String host, int agentPort) {
    final base = local(host, agentPort);
    final s = raw?.trim();
    if (base == null || s == null || s.contains(RegExp(r'\s'))) return false;
    final prefix = base.substring(0, base.length - 1);
    if (!s.toLowerCase().startsWith(prefix.toLowerCase())) return false;
    final rest = s.substring(prefix.length);
    return (rest.isEmpty || '/?#'.contains(rest[0])) && _tail.hasMatch(rest);
  }

  /// The address typed in Settings if valid, else the one baked in at build time (`--dart-define=FILES_URL=...`).
  static String? resolve(String override, String buildDefault) => clean(override) ?? clean(buildDefault);
}
