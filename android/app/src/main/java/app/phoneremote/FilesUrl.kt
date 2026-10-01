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

    /** The address to open: the Settings override if valid, else the address baked in at build time, else null. */
    fun resolve(settingsJson: String, buildDefault: String): String? =
        clean(overrideOf(settingsJson)) ?: clean(buildDefault)
}
