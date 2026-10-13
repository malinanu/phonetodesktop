package app.phoneremote

import android.Manifest
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/**
 * Lets the Flutter app use the phone as a Bluetooth media remote, keyboard and mouse through
 * [HidRemote] (the same code the native app uses). Android only: iOS cannot act as a Bluetooth keyboard.
 */
class HidBridge(private val activity: Activity, engine: FlutterEngine) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {
    companion object {
        const val CHANNEL = "app.phoneremote/hid"
        const val EVENTS = "app.phoneremote/hid/events"
        private const val REQUEST_PERMISSIONS = 4711
    }

    private val main = Handler(Looper.getMainLooper())
    private var events: EventChannel.EventSink? = null
    private var hid: HidRemote? = null
    private var pendingStart: MethodChannel.Result? = null
    private var ready = false
    private var connected = false
    private var message = "Not started"
    private var device: String? = null

    init {
        MethodChannel(engine.dartExecutor.binaryMessenger, CHANNEL).setMethodCallHandler(this)
        EventChannel(engine.dartExecutor.binaryMessenger, EVENTS).setStreamHandler(this)
    }

    override fun onListen(arguments: Any?, sink: EventChannel.EventSink?) { events = sink; emit() }
    override fun onCancel(arguments: Any?) { events = null }

    private fun state(): Map<String, Any?> = mapOf(
        "ready" to ready,
        "connected" to connected,
        "message" to message,
        "device" to device,
        "descriptorVersion" to HidRemote.DESCRIPTOR_VERSION,
    )

    private fun emit() { events?.success(state()) }

    private fun needed(): List<String> =
        if (Build.VERSION.SDK_INT >= 31) listOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_ADVERTISE) else emptyList()

    private fun granted(): Boolean = needed().all { activity.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }

    fun onPermissionResult(requestCode: Int, results: IntArray) {
        if (requestCode != REQUEST_PERMISSIONS) return
        val result = pendingStart ?: return
        pendingStart = null
        if (results.isNotEmpty() && results.all { it == PackageManager.PERMISSION_GRANTED }) {
            begin(result)
        } else {
            message = "Allow Nearby devices so this phone can act as a Bluetooth remote"
            emit()
            result.success(false)
        }
    }

    private fun begin(result: MethodChannel.Result) {
        if (hid == null) {
            hid = HidRemote(
                activity,
                onChange = { dev, msg -> main.post { connected = dev != null; device = dev?.name ?: dev?.address; message = msg; emit() } },
                onReady = { main.post { ready = true; if (!connected) message = "Ready. Pick your PC below"; emit() } },
            )
        }
        hid?.start()
        result.success(true)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        val h = hid
        when (call.method) {
            "start" -> {
                if (granted()) begin(result) else {
                    pendingStart?.success(false)
                    pendingStart = result
                    activity.requestPermissions(needed().toTypedArray(), REQUEST_PERMISSIONS)
                }
            }
            "stop" -> { h?.stop(); hid = null; ready = false; connected = false; message = "Not started"; device = null; emit(); result.success(null) }
            "state" -> result.success(state())
            "bonded" -> result.success(
                (h?.bondedDevices() ?: emptyList()).map { mapOf("name" to (it.name ?: it.address), "address" to it.address, "computer" to HidRemote.isComputer(it)) },
            )
            "connect" -> {
                val address = call.argument<String>("address")
                val dev = h?.bondedDevices()?.firstOrNull { it.address == address }
                if (dev != null) h.connect(dev)
                result.success(dev != null)
            }
            "discoverable" -> {
                activity.startActivity(Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE).putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, 120))
                result.success(null)
            }
            "media" -> { h?.media(call.argument<Int>("usage") ?: 0); result.success(null) }
            "key" -> result.success(h?.namedKey(call.argument<String>("name") ?: "", call.argument<List<String>>("mods") ?: emptyList()) ?: false)
            "text" -> result.success(h?.typeText(call.argument<String>("text") ?: "") ?: 0)
            "move" -> { h?.move(call.argument<Int>("dx") ?: 0, call.argument<Int>("dy") ?: 0); result.success(null) }
            "button" -> { h?.button(call.argument<String>("button") ?: "", call.argument<String>("action") ?: ""); result.success(null) }
            "scroll" -> { h?.scroll(call.argument<Int>("dx") ?: 0, call.argument<Int>("dy") ?: 0); result.success(null) }
            else -> result.notImplemented()
        }
    }

    fun dispose() { hid?.stop(); hid = null }
}
