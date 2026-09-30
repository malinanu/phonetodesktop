package app.phoneremote

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothHidDevice
import android.bluetooth.BluetoothHidDeviceAppSdpSettings
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.Context
import java.util.concurrent.Executors

/**
 * Makes the phone a Bluetooth keyboard + consumer-control device. Windows (and any other OS)
 * treats it as a normal remote: media keys always, arrow keys as the focused-window seek fallback.
 * No desktop software is involved, and there is no now-playing feedback in this mode.
 */
@SuppressLint("MissingPermission") // BLUETOOTH_CONNECT is requested by MainActivity before start()
class HidRemote(private val ctx: Context, private val onState: (String) -> Unit) {
    companion object {
        const val PLAY_PAUSE = 0xCD
        const val NEXT = 0xB5
        const val PREV = 0xB6
        const val VOL_UP = 0xE9
        const val VOL_DOWN = 0xEA
        const val MUTE = 0xE2
        const val KEY_RIGHT = 0x4F
        const val KEY_LEFT = 0x50
        const val KEY_SPACE = 0x2C

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
        ).map { it.toByte() }.toByteArray()
    }

    private val adapter: BluetoothAdapter? =
        (ctx.getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager).adapter
    private var hid: BluetoothHidDevice? = null
    private var host: BluetoothDevice? = null
    private val executor = Executors.newSingleThreadExecutor()

    private val callback = object : BluetoothHidDevice.Callback() {
        override fun onAppStatusChanged(pluggedDevice: BluetoothDevice?, registered: Boolean) {
            onState(if (registered) "Ready — pick your PC" else "HID not registered")
        }
        override fun onConnectionStateChanged(device: BluetoothDevice, state: Int) {
            when (state) {
                BluetoothProfile.STATE_CONNECTED -> { host = device; onState("Connected to ${device.name}") }
                BluetoothProfile.STATE_DISCONNECTED -> { if (host == device) host = null; onState("Disconnected") }
            }
        }
    }

    fun start() {
        val a = adapter ?: return onState("No Bluetooth on this phone")
        if (!a.isEnabled) return onState("Turn Bluetooth on")
        a.getProfileProxy(ctx, object : BluetoothProfile.ServiceListener {
            override fun onServiceConnected(profile: Int, proxy: BluetoothProfile) {
                val h = proxy as BluetoothHidDevice
                hid = h
                val sdp = BluetoothHidDeviceAppSdpSettings(
                    "Phone Remote", "Media remote", "PhoneRemote",
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

    fun media(usage: Int) {
        val h = hid ?: return
        val d = host ?: return onState("Not connected")
        h.sendReport(d, 1, byteArrayOf((usage and 0xFF).toByte(), (usage shr 8).toByte()))
        h.sendReport(d, 1, byteArrayOf(0, 0))
    }

    fun key(code: Int, times: Int = 1) {
        val h = hid ?: return
        val d = host ?: return onState("Not connected")
        repeat(times) {
            h.sendReport(d, 2, byteArrayOf(0, 0, code.toByte(), 0, 0, 0, 0, 0))
            h.sendReport(d, 2, ByteArray(8))
            Thread.sleep(25)
        }
    }
}
