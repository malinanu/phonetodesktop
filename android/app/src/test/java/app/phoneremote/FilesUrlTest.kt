package app.phoneremote

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FilesUrlTest {
    @Test fun acceptsHttpsAndNormalises() {
        assertEquals("https://files.example.com", FilesUrl.clean("https://files.example.com"))
        assertEquals("https://files.example.com", FilesUrl.clean("  FILES.example.com "))
        assertEquals("https://files.example.com:8443/app?x=1", FilesUrl.clean("https://Files.Example.com:8443/app?x=1"))
    }

    @Test fun rejectsAnythingUnsafeOrEmpty() {
        for (bad in listOf("", "   ", null, "http://files.example.com", "ftp://x.com", "javascript:alert(1)",
            "https://user:pw@files.example.com", "https://a b.com", "https://", "https://:443",
            "https://files.example.com:0", "https://files.example.com:99999", "https://files.example.com:x",
            "https://-bad.example.com", "https://exa_mple.com", "https://files.example.com\\evil",
            "https://files.example.com:+443", "https://files.example.com:\u0664\u0664\u0663",
            "https://files.example.com/?a=1&calc.exe", "https://files.example.com/%PATH%", "https://files.example.com/a|b",
            "https://files.example.com/a^b", "https://files.example.com/a\"b", "https://files.example.com/a'b", "https://files.example.com/<x>")) {
            assertNull("should reject: $bad", FilesUrl.clean(bad))
        }
    }

    @Test fun settingsOverrideBeatsBuildDefault() {
        val json = """{"theme":"dark","filesUrl":"https://mine.example.org"}"""
        assertEquals("https://mine.example.org", FilesUrl.resolve(json, "https://default.example.com"))
    }

    @Test fun fallsBackWhenOverrideMissingOrInvalid() {
        assertEquals("https://default.example.com", FilesUrl.resolve("""{"theme":"dark"}""", "https://default.example.com"))
        assertEquals("https://default.example.com", FilesUrl.resolve("""{"filesUrl":"http://insecure.example.org"}""", "https://default.example.com"))
        assertEquals("https://default.example.com", FilesUrl.resolve("""{"filesUrl":""}""", "default.example.com"))
        assertNull(FilesUrl.resolve("", ""))
    }

    @Test fun withOverrideKeepsOtherSettings() {
        assertEquals("""{"filesUrl":"https://a.example.com"}""", FilesUrl.withOverride("{}", "https://a.example.com"))
        assertEquals("""{"filesUrl":"https://a.example.com"}""", FilesUrl.withOverride("", "https://a.example.com"))
        assertEquals("""{"theme":"dark","skip":15,"filesUrl":"https://a.example.com"}""", FilesUrl.withOverride("""{"theme":"dark","skip":15}""", "https://a.example.com"))
        assertEquals("""{"theme":"dark","filesUrl":"https://b.example.com"}""", FilesUrl.withOverride("""{"theme":"dark","filesUrl":"https://a.example.com"}""", "https://b.example.com"))
        assertEquals("""{"filesUrl":""}""", FilesUrl.withOverride("""{"filesUrl":"https://a.example.com"}""", ""))
    }

    @Test fun withOverrideRoundTrips() {
        val json = FilesUrl.withOverride("""{"theme":"light"}""", "https://files.example.com")
        assertEquals("https://files.example.com", FilesUrl.resolve(json, ""))
        assertEquals("light", Theme.themeOf(json))
        assertNull(FilesUrl.resolve(FilesUrl.withOverride(json, ""), ""))
    }
}
