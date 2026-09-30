package app.phoneremote

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ThemeTest {
    @Test fun systemFollowsThePhone() {
        assertTrue(Theme.isDark("system", true))
        assertFalse(Theme.isDark("system", false))
        assertTrue(Theme.isDark("garbage", true))
    }

    @Test fun explicitChoiceWinsOverThePhone() {
        assertTrue(Theme.isDark("dark", false))
        assertFalse(Theme.isDark("light", true))
    }

    @Test fun settingsJsonParsing() {
        assertEquals("light", Theme.themeOf("""{"theme":"light","skip":15}"""))
        assertEquals("dark", Theme.themeOf("""{ "skip": 10, "theme" : "dark" }"""))
        assertEquals("system", Theme.themeOf(""))
        assertEquals("system", Theme.themeOf("""{"theme":"purple"}"""))
        assertTrue(Theme.keepAwakeOf("""{"keepAwake": true}"""))
        assertFalse(Theme.keepAwakeOf("""{"keepAwake":false}"""))
    }

    @Test fun bothPalettesAreReadable() {
        for ((name, p) in listOf("dark" to Theme.DARK, "light" to Theme.LIGHT)) {
            for ((label, fg, bg) in listOf(
                Triple("text on background", p.fg, p.bg),
                Triple("text on card", p.fg, p.card),
                Triple("dim text on background", p.dim, p.bg),
                Triple("dim text on card", p.dim, p.card),
                Triple("button text on accent", p.onAcc, p.acc),
                Triple("error text on background", p.bad, p.bg),
                Triple("accent text on background", p.accText, p.bg),
                Triple("accent text on card", p.accText, p.card),
                Triple("success text on background", p.good, p.bg),
            )) {
                val c = Theme.contrast(fg, bg)
                assertTrue("$name: $label has contrast $c", c >= 4.5)
            }
        }
    }
}
