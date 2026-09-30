package app.phoneremote

/** One colour set for the native shell. Values match the tokens in shared/base.css. */
data class Palette(
    val bg: Int, val card: Int, val card2: Int, val fg: Int, val dim: Int,
    val acc: Int, val accText: Int, val onAcc: Int, val bad: Int, val good: Int,
)

/** Pure theme logic (no Android classes), so it is covered by plain JVM tests. */
object Theme {
    val DARK = Palette(
        bg = 0xFF15110E.toInt(), card = 0xFF1F1A16.toInt(), card2 = 0xFF2A231D.toInt(),
        fg = 0xFFF5EBDD.toInt(), dim = 0xFFB3A594.toInt(),
        acc = 0xFFFF9A3C.toInt(), accText = 0xFFFF9A3C.toInt(), onAcc = 0xFF1B1006.toInt(), bad = 0xFFFF8A7A.toInt(), good = 0xFF86D9A6.toInt(),
    )
    val LIGHT = Palette(
        bg = 0xFFF7F1E8.toInt(), card = 0xFFFFFAF2.toInt(), card2 = 0xFFEFE6D8.toInt(),
        fg = 0xFF241C14.toInt(), dim = 0xFF6C6050.toInt(),
        acc = 0xFFE8710A.toInt(), accText = 0xFFA04A00.toInt(), onAcc = 0xFF1B1006.toInt(), bad = 0xFFB0352A.toInt(), good = 0xFF17703F.toInt(),
    )

    /** `setting` is "system", "dark" or "light" (anything else counts as system). */
    fun isDark(setting: String, systemIsNight: Boolean): Boolean = when (setting) {
        "dark" -> true
        "light" -> false
        else -> systemIsNight
    }

    fun palette(dark: Boolean): Palette = if (dark) DARK else LIGHT

    private val themeRe = Regex("\"theme\"\\s*:\\s*\"(system|dark|light)\"")
    private val awakeRe = Regex("\"keepAwake\"\\s*:\\s*true")

    /** Read the theme out of the settings JSON the pages save ("system" if absent or malformed). */
    fun themeOf(settingsJson: String): String = themeRe.find(settingsJson)?.groupValues?.get(1) ?: "system"

    fun keepAwakeOf(settingsJson: String): Boolean = awakeRe.containsMatchIn(settingsJson)

    /** WCAG relative luminance of an ARGB colour. */
    fun luminance(argb: Int): Double {
        fun ch(v: Int): Double { val c = v / 255.0; return if (c <= 0.03928) c / 12.92 else Math.pow((c + 0.055) / 1.055, 2.4) }
        return 0.2126 * ch((argb shr 16) and 0xFF) + 0.7152 * ch((argb shr 8) and 0xFF) + 0.0722 * ch(argb and 0xFF)
    }

    fun contrast(a: Int, b: Int): Double {
        val (hi, lo) = luminance(a).let { la -> luminance(b).let { lb -> if (la >= lb) la to lb else lb to la } }
        return (hi + 0.05) / (lo + 0.05)
    }
}
