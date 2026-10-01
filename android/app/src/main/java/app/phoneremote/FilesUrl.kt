package app.phoneremote

/**
 * Address of the file-sending server (FileSync). Pure logic, no Android classes, so plain JVM tests cover it.
 * Only https is accepted: the page can ask for files and talks to a relay, so it must not be reachable in clear text.
 */
object FilesUrl {
    // The Settings page stores {"filesUrl":"https://..."}; JSON.stringify adds no escapes for URLs.
    private val overrideRe = Regex("\"filesUrl\"\\s*:\\s*\"([^\"\\\\]*)\"")
    // Path/query characters allowed after the host. Same set as the PC agent (it hands the address to cmd.exe).
    private val tailRe = Regex("^[A-Za-z0-9._~/?#=+,:;@!*-]*$")
    private val hostRe = Regex("^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?(\\.[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?)*$")

    /**
     * A normalised https URL, or null when [raw] is empty or unsafe. A bare host such as "files.example.com"
     * gets https:// added; http://, credentials, spaces and other schemes are rejected.
     */
    fun clean(raw: String?): String? {
        var s = raw?.trim().orEmpty()
        if (s.isEmpty() || s.any { it.isWhitespace() }) return null
        if (!s.contains("://")) s = "https://$s"
        if (!s.startsWith("https://", ignoreCase = true)) return null
        val rest = s.substring("https://".length)
        val cut = rest.indexOfFirst { it == '/' || it == '?' || it == '#' }.let { if (it < 0) rest.length else it }
        val authority = rest.substring(0, cut)
        val tail = rest.substring(cut)
        if (authority.isEmpty() || '@' in authority || '\\' in s) return null
        val host = authority.substringBefore(':')
        val port = authority.substringAfter(':', "")
        if (!hostRe.matches(host)) return null
        if (':' in authority) {
            // toIntOrNull alone would accept "+443" and non-ASCII digits.
            val n = if (port.all { it in '0'..'9' }) port.toIntOrNull() else null
            if (n == null || n !in 1..65535) return null
        }
        if (!tailRe.matches(tail)) return null
        return "https://" + authority.lowercase() + tail
    }

    /** The address the user typed in Settings ("" if none). */
    fun overrideOf(settingsJson: String): String = overrideRe.find(settingsJson)?.groupValues?.get(1) ?: ""

    /**
     * [settingsJson] with the address set to [url] (an already cleaned address, or "" to clear it). Every other
     * setting is kept as it is. Used by the Files tab so the address can be entered where it is needed.
     */
    fun withOverride(settingsJson: String, url: String): String {
        val entry = "\"filesUrl\":\"$url\""
        if (overrideRe.containsMatchIn(settingsJson)) return overrideRe.replaceFirst(settingsJson, Regex.escapeReplacement(entry))
        val body = settingsJson.trim()
        if (!body.startsWith("{") || !body.endsWith("}")) return "{$entry}"
        val inner = body.substring(1, body.length - 1).trim()
        return if (inner.isEmpty()) "{$entry}" else "{$inner,$entry}"
    }

    /**
     * The built-in Send files page of a PC: the Phone Remote program serves it on its own port plus one.
     * [host] is the address the phone already uses to reach the PC. Null if either part is unusable.
     */
    fun local(host: String, agentPort: Int): String? {
        if (!hostRe.matches(host) || agentPort !in 1..65534) return null
        return "http://$host:${agentPort + 1}/"
    }

    /** Is [raw] (a scanned code) a link to that PC's built-in Send files page, e.g. a room link? */
    fun isLocalLink(raw: String?, host: String, agentPort: Int): Boolean {
        val base = local(host, agentPort) ?: return false
        val s = raw?.trim() ?: return false
        if (s.any { it.isWhitespace() }) return false
        val prefix = base.dropLast(1) // http://host:port
        if (!s.startsWith(prefix, ignoreCase = true)) return false
        val rest = s.substring(prefix.length)
        return (rest.isEmpty() || rest[0] == '/' || rest[0] == '?' || rest[0] == '#') && tailRe.matches(rest)
    }

    /** The address to open: the Settings override if valid, else the address baked in at build time, else null. */
    fun resolve(settingsJson: String, buildDefault: String): String? =
        clean(overrideOf(settingsJson)) ?: clean(buildDefault)
}
