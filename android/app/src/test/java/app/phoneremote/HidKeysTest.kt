package app.phoneremote

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class HidKeysTest {
    @Test fun lettersAndCase() {
        assertEquals(HidKeys.Stroke(0x04, false), HidKeys.forChar('a'))
        assertEquals(HidKeys.Stroke(0x04, true), HidKeys.forChar('A'))
        assertEquals(HidKeys.Stroke(0x1D, false), HidKeys.forChar('z'))
    }

    @Test fun digitsAndShiftedDigits() {
        assertEquals(HidKeys.Stroke(0x1E, false), HidKeys.forChar('1'))
        assertEquals(HidKeys.Stroke(0x27, false), HidKeys.forChar('0'))
        assertEquals(HidKeys.Stroke(0x1E, true), HidKeys.forChar('!'))
        assertEquals(HidKeys.Stroke(0x1F, true), HidKeys.forChar('@'))
        assertEquals(HidKeys.Stroke(0x26, true), HidKeys.forChar('('))
        assertEquals(HidKeys.Stroke(0x27, true), HidKeys.forChar(')'))
    }

    @Test fun punctuationAndWhitespace() {
        assertEquals(HidKeys.Stroke(0x2C, false), HidKeys.forChar(' '))
        assertEquals(HidKeys.Stroke(0x28, false), HidKeys.forChar('\n'))
        assertEquals(HidKeys.Stroke(0x38, true), HidKeys.forChar('?'))
        assertEquals(HidKeys.Stroke(0x2D, true), HidKeys.forChar('_'))
        assertEquals(HidKeys.Stroke(0x34, true), HidKeys.forChar('"'))
    }

    @Test fun unsupportedCharactersAreNull() {
        for (c in listOf('é', 'ස', '\uD83D', '€')) assertNull("$c", HidKeys.forChar(c))
    }

    @Test fun everyPrintableAsciiIsMapped() {
        for (c in ' '..'~') assertEquals("$c", true, HidKeys.forChar(c) != null)
    }

    @Test fun namedKeysAndChords() {
        assertEquals(0x28, HidKeys.forName("Enter"))
        assertEquals(0x2A, HidKeys.forName("backspace"))
        assertEquals(0x50, HidKeys.forName("left"))
        assertEquals(0x45, HidKeys.forName("f12"))
        assertEquals(0x06, HidKeys.forName("c"))
        assertNull(HidKeys.forName("win"))
        assertEquals(HidKeys.MOD_CTRL or HidKeys.MOD_ALT, HidKeys.modifierBits(listOf("CTRL", "alt")))
        assertEquals(HidKeys.MOD_GUI, HidKeys.modifierBits(listOf("win")))
    }
}
