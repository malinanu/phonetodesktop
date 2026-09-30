package app.phoneremote

/**
 * Pure mapping from characters and key names to USB HID keyboard usages (US layout).
 * No Android classes, so it is covered by plain JVM unit tests.
 *
 * A Bluetooth keyboard sends key *positions*; the PC's layout decides which character appears,
 * so only US-layout characters can be typed this way.
 */
object HidKeys {
    data class Stroke(val usage: Int, val shift: Boolean)

    const val MOD_CTRL = 0x01
    const val MOD_SHIFT = 0x02
    const val MOD_ALT = 0x04
    const val MOD_GUI = 0x08

    private const val SHIFTED_DIGITS = ")!@#$%^&*("

    fun forChar(c: Char): Stroke? = when (c) {
        in 'a'..'z' -> Stroke(0x04 + (c - 'a'), false)
        in 'A'..'Z' -> Stroke(0x04 + (c - 'A'), true)
        in '1'..'9' -> Stroke(0x1E + (c - '1'), false)
        '0' -> Stroke(0x27, false)
        ' ' -> Stroke(0x2C, false)
        '\n', '\r' -> Stroke(0x28, false)
        '\t' -> Stroke(0x2B, false)
        '-' -> Stroke(0x2D, false)
        '_' -> Stroke(0x2D, true)
        '=' -> Stroke(0x2E, false)
        '+' -> Stroke(0x2E, true)
        '[' -> Stroke(0x2F, false)
        '{' -> Stroke(0x2F, true)
        ']' -> Stroke(0x30, false)
        '}' -> Stroke(0x30, true)
        '\\' -> Stroke(0x31, false)
        '|' -> Stroke(0x31, true)
        ';' -> Stroke(0x33, false)
        ':' -> Stroke(0x33, true)
        '\'' -> Stroke(0x34, false)
        '"' -> Stroke(0x34, true)
        '`' -> Stroke(0x35, false)
        '~' -> Stroke(0x35, true)
        ',' -> Stroke(0x36, false)
        '<' -> Stroke(0x36, true)
        '.' -> Stroke(0x37, false)
        '>' -> Stroke(0x37, true)
        '/' -> Stroke(0x38, false)
        '?' -> Stroke(0x38, true)
        else -> {
            val i = SHIFTED_DIGITS.indexOf(c)
            if (i >= 0) Stroke(if (i == 0) 0x27 else 0x1E + i - 1, true) else null
        }
    }

    private val named = mapOf(
        "enter" to 0x28, "esc" to 0x29, "backspace" to 0x2A, "tab" to 0x2B, "space" to 0x2C,
        "insert" to 0x49, "home" to 0x4A, "pageup" to 0x4B, "delete" to 0x4C, "end" to 0x4D, "pagedown" to 0x4E,
        "right" to 0x4F, "left" to 0x50, "down" to 0x51, "up" to 0x52,
    ) + (1..12).associate { "f$it" to (0x3A + it - 1) }

    /** Usage for a key name as sent by the pad page, or null (e.g. "win", which is only a modifier). */
    fun forName(name: String): Int? {
        val n = name.lowercase()
        named[n]?.let { return it }
        return if (n.length == 1) forChar(n[0])?.usage else null
    }

    fun modifierBits(mods: List<String>): Int = mods.fold(0) { acc, m ->
        acc or when (m.lowercase()) {
            "ctrl" -> MOD_CTRL
            "shift" -> MOD_SHIFT
            "alt" -> MOD_ALT
            "win" -> MOD_GUI
            else -> 0
        }
    }
}
