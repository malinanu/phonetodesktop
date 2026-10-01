package app.phoneremote

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothClass
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothHidDevice
import android.bluetooth.BluetoothHidDeviceAppSdpSettings
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.Context
import java.util.concurrent.Executors

/**
 * Makes the phone a Bluetooth media remote, keyboard and mouse. The PC treats it as ordinary
 * hardware, so nothing has to be installed there.
 *
 * Reports: 1 = consumer control (media keys), 2 = keyboard, 3 = mouse (buttons, X, Y, wheel, pan).
 * Windows caches this descriptor per paired device: after it changes, the phone must be removed
 * from Windows' Bluetooth list and paired again ([DESCRIPTOR_VERSION]).
 */
@SuppressLint("MissingPermission") // BLUETOOTH_CONNECT is requested by MainActivity before start()
class HidRemote(
    private val ctx: Context,
    private val onChange: (connected: BluetoothDevice?, message: String) -> Unit,
    private val onReady: () -> Unit,
) {
    companion object {
        const val DESCRIPTOR_VERSION = 2
        const val PLAY_PAUSE = 0xCD
        const val NEXT = 0xB5
        const val PREV = 0xB6
        const val VOL_UP = 0xE9
        const val VOL_DOWN = 0xEA
        const val MUTE = 0xE2

        private val DESCRIPTOR = intArrayOf(
            // Consumer control, report id 1, one 16-bit usage
            0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x01,
            0x15, 0x00, 0x26, 0xFF, 0x03, 0x19, 0x00, 0x2A, 0xFF, 0x03,
            0x75, 0x10, 0x95, 0x01, 0x81, 0x00, 0xC0,
            // Keyboard, report id 2: modifiers, reserved, 6 key slots
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x85, 0x02,
            0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01,
            0x75, 0x01, 0x95, 0x08, 0x81, 0x02,
            0x95, 0x01, 0x75, 0x08, 0x81, 0x01,
            0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x65,
            0x05, 0x07, 0x19, 0x00, 0x29, 0x65, 0x81, 0x00, 0xC0,
            // Mouse, report id 3: 3 buttons, relative X/Y, wheel, horizontal pan
            0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x85, 0x03, 0x09, 0x01, 0xA1, 0x00,
            0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02,
            0x95, 0x01, 0x75, 0x05, 0x81, 0x03,
            0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x03, 0x81, 0x06,
            0x05, 0x0C, 0x0A, 0x38, 0x02, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06,
            0xC0, 0xC0,
        ).map { it.toByte() }.toByteArray()

        /** Devices that are computers, which is what the user wants to pick from. */
        fun isComputer(d: BluetoothDevice): Boolean = d.bluetoothClass?.majorDeviceClass == BluetoothClass.Device.Major.COMPUTER
    }

    private val adapter: BluetoothAdapter? =
        (ctx.getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager).adapter
    private var hid: BluetoothHidDevice? = null
    @Volatile private var host: BluetoothDevice? = null
    private val executor = Executors.newSingleThreadExecutor()

    private var buttons = 0
    private var wheelAcc = 0.0
    private var panAcc = 0.0

    private val callback = object : BluetoothHidDevice.Callback() {
        override fun onAppStatusChanged(pluggedDevice: BluetoothDevice?, registered: Boolean) {
            if (registered) onReady() else onChange(null, "Bluetooth remote is not available")
        }

        override fun onConnectionStateChanged(device: BluetoothDevice, state: Int) {
            when (state) {
                BluetoothProfile.STATE_CONNECTED -> { host = device; onChange(device, "Connected") }
                BluetoothProfile.STATE_CONNECTING -> onChange(null, "Connecting to ${device.name ?: device.address}…")
                BluetoothProfile.STATE_DISCONNECTED -> { if (host == device) host = null; buttons = 0; onChange(null, "Not connected") }
            }
        }
    }

    fun start() {
        val a = adapter ?: return onChange(null, "This phone has no Bluetooth")
        if (!a.isEnabled) return onChange(null, "Turn Bluetooth on")
        a.getProfileProxy(ctx, object : BluetoothProfile.ServiceListener {
            override fun onServiceConnected(profile: Int, proxy: BluetoothProfile) {
                val h = proxy as BluetoothHidDevice
                hid = h
                val sdp = BluetoothHidDeviceAppSdpSettings(
                    "Phone Remote", "Remote, keyboard and mouse", "PhoneRemote",
                    BluetoothHidDevice.SUBCLASS1_COMBO, DESCRIPTOR,
                )
                h.registerApp(sdp, null, null, executor, callback)
            }
            override fun onServiceDisconnected(profile: Int) { hid = null }
        }, BluetoothProfile.HID_DEVICE)
    }

    fun stop() {
        hid?.unregisterApp()
        hid?.let { adapter?.closeProfileProxy(BluetoothProfile.HID_DEVICE, it) }
        hid = null
        host = null
    }

    fun bondedDevices(): List<BluetoothDevice> = adapter?.bondedDevices?.toList() ?: emptyList()

    fun connect(d: BluetoothDevice) { hid?.connect(d) }

    val connected: Boolean get() = host != null

    // ---- media keys ----

    fun media(usage: Int) {
        val h = hid ?: return
        val d = host ?: return
        h.sendReport(d, 1, byteArrayOf((usage and 0xFF).toByte(), (usage shr 8).toByte()))
        h.sendReport(d, 1, byteArrayOf(0, 0))
    }

    // ---- keyboard ----

    private fun keyReport(modifiers: Int, usage: Int) {
        val h = hid ?: return
        val d = host ?: return
        h.sendReport(d, 2, byteArrayOf(modifiers.toByte(), 0, usage.toByte(), 0, 0, 0, 0, 0))
        h.sendReport(d, 2, ByteArray(8))
    }

    /** Press a HID usage `times` times (used by the media pad's arrow keys). */
    fun key(usage: Int, times: Int = 1) {
        repeat(times) { keyReport(0, usage); if (times > 1) Thread.sleep(25) }
    }

    /** A named key with modifiers, as sent by the pad page ("win" alone is a modifier-only tap). */
    fun namedKey(name: String, mods: List<String>): Boolean {
        val bits = HidKeys.modifierBits(mods) or if (name.equals("win", true)) HidKeys.MOD_GUI else 0
        val usage = HidKeys.forName(name)
        if (usage == null && !name.equals("win", true)) return false
        keyReport(bits, usage ?: 0)
        return true
    }

    /** Type text with the US layout. Returns how many characters could not be typed. */
    fun typeText(s: String): Int {
        var skipped = 0
        for (c in s) {
            val k = HidKeys.forChar(c)
            if (k == null) { skipped++; continue }
            keyReport(if (k.shift) HidKeys.MOD_SHIFT else 0, k.usage)
        }
        return skipped
    }

    // ---- mouse ----

    private fun mouseReport(b: Int, x: Int, y: Int, wheel: Int, pan: Int) {
        val h = hid ?: return
        val d = host ?: return
        h.sendReport(d, 3, byteArrayOf(b.toByte(), x.toByte(), y.toByte(), wheel.toByte(), pan.toByte()))
    }

    /** Move the cursor; deltas beyond one report (±127) are split into several. */
    fun move(dx: Int, dy: Int) {
        var x = dx
        var y = dy
        while (x != 0 || y != 0) {
            val sx = x.coerceIn(-127, 127)
            val sy = y.coerceIn(-127, 127)
            mouseReport(buttons, sx, sy, 0, 0)
            x -= sx
            y -= sy
        }
    }

    fun button(which: String, action: String) {
        val bit = when (which) { "left" -> 1; "right" -> 2; "middle" -> 4; else -> return }
        when (action) {
            "down" -> { buttons = buttons or bit; mouseReport(buttons, 0, 0, 0, 0) }
            "up" -> { buttons = buttons and bit.inv(); mouseReport(buttons, 0, 0, 0, 0) }
            "click" -> { mouseReport(buttons or bit, 0, 0, 0, 0); mouseReport(buttons, 0, 0, 0, 0) }
        }
    }

    /** `dx`/`dy` are wheel units as sent by the pad (120 = one notch); HID counts whole notches. */
    fun scroll(dx: Int, dy: Int) {
        wheelAcc += dy / 120.0
        panAcc += dx / 120.0
        val w = wheelAcc.toInt().coerceIn(-127, 127)
        val p = panAcc.toInt().coerceIn(-127, 127)
        if (w == 0 && p == 0) return
        wheelAcc -= w
        panAcc -= p
        mouseReport(buttons, 0, 0, w, p)
    }
}
