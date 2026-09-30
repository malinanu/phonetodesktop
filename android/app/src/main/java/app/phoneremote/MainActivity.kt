package app.phoneremote

import android.Manifest
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

/**
 * LAN mode: WebView on the agent's own phone UI (same page a browser gets), located via mDNS.
 * Bluetooth mode: the phone acts as a Bluetooth media-key/arrow-key keyboard (see HidRemote).
 */
class MainActivity : Activity() {
    private val prefs by lazy { getSharedPreferences("pr", Context.MODE_PRIVATE) }
    private lateinit var web: WebView
    private lateinit var status: TextView
    private lateinit var lanView: LinearLayout
    private lateinit var btView: LinearLayout
    private lateinit var discovery: Discovery
    private var hid: HidRemote? = null
    private var loadedHost: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setBackgroundColor(Color.parseColor("#0e0f13")) }

        val bar = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        bar.addView(button("Scan QR") { scanQr() }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        bar.addView(button("Wi-Fi") { showMode(false) }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        bar.addView(button("Bluetooth") { showMode(true) }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        root.addView(bar)

        status = TextView(this).apply { setTextColor(Color.LTGRAY); setPadding(24, 8, 24, 8); text = "Looking for your PC…" }
        root.addView(status)

        web = WebView(this).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            webViewClient = object : WebViewClient() {
                override fun onReceivedError(v: WebView, r: WebResourceRequest, e: WebResourceError) {
                    if (r.isForMainFrame) status.text = "Can't reach PC — same Wi-Fi? Try Bluetooth."
                }
            }
        }
        lanView = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        lanView.addView(web, LinearLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        root.addView(lanView, LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f))

        btView = buildBluetoothView()
        btView.visibility = View.GONE
        root.addView(btView, LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f))
        setContentView(root)

        discovery = Discovery(this) { host, port -> runOnUiThread { openPc(host, port) } }
        // Load the last known address right away, then let mDNS correct it if the IP changed.
        prefs.getString("host", null)?.let { openPc(it, prefs.getInt("port", 8765)) }
    }

    override fun onStart() { super.onStart(); if (prefs.getString("token", null) != null) discovery.start() }
    override fun onStop() { super.onStop(); discovery.stop() }
    override fun onDestroy() { hid?.stop(); super.onDestroy() }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() { if (web.canGoBack()) web.goBack() else super.onBackPressed() }

    private fun button(t: String, click: () -> Unit) = Button(this).apply { text = t; setOnClickListener { click() } }

    private fun openPc(host: String, port: Int) {
        val token = prefs.getString("token", null) ?: return
        if (loadedHost == "$host:$port") return
        loadedHost = "$host:$port"
        prefs.edit().putString("host", host).putInt("port", port).apply()
        status.text = "PC at $host"
        web.loadUrl("http://$host:$port/#k=$token")
    }

    private fun scanQr() {
        val opts = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(this, opts).startScan()
            .addOnSuccessListener { code ->
                val m = Regex("^http://([^:/]+):(\\d+)/#k=([\\w-]+)$").find(code.rawValue ?: "")
                if (m == null) { Toast.makeText(this, "Not a Phone Remote code", Toast.LENGTH_SHORT).show(); return@addOnSuccessListener }
                val (host, port, token) = m.destructured
                prefs.edit().putString("token", token).apply()
                loadedHost = null
                showMode(false)
                openPc(host, port.toInt())
                discovery.start()
            }
            .addOnFailureListener { Toast.makeText(this, "Scan failed: ${it.message}", Toast.LENGTH_SHORT).show() }
    }

    private fun showMode(bluetooth: Boolean) {
        lanView.visibility = if (bluetooth) View.GONE else View.VISIBLE
        btView.visibility = if (bluetooth) View.VISIBLE else View.GONE
        if (bluetooth) startBluetooth()
    }

    // ---- Bluetooth HID mode -------------------------------------------------------------

    private lateinit var btStatus: TextView
    private lateinit var deviceList: LinearLayout

    private fun buildBluetoothView(): LinearLayout {
        val v = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(16, 16, 16, 16) }
        btStatus = TextView(this).apply { setTextColor(Color.WHITE); text = "Bluetooth remote (no PC software needed)" }
        v.addView(btStatus)
        v.addView(button("Make phone discoverable (pair from PC)") {
            startActivity(Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE)
                .putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, 120))
        })
        deviceList = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        v.addView(deviceList)
        fun row(vararg b: Button) = LinearLayout(this).apply { b.forEach { addView(it, LinearLayout.LayoutParams(0, 160, 1f)) } }
        v.addView(row(button("⏮") { hid?.media(HidRemote.PREV) }, button("⏯") { hid?.media(HidRemote.PLAY_PAUSE) }, button("⏭") { hid?.media(HidRemote.NEXT) }))
        // Arrow keys reach whichever window has focus; players use 5–10 s per press.
        v.addView(row(button("« ←←") { hid?.key(HidRemote.KEY_LEFT, 2) }, button("Space") { hid?.key(HidRemote.KEY_SPACE) }, button("→→ »") { hid?.key(HidRemote.KEY_RIGHT, 2) }))
        v.addView(row(button("Vol −") { hid?.media(HidRemote.VOL_DOWN) }, button("Mute") { hid?.media(HidRemote.MUTE) }, button("Vol +") { hid?.media(HidRemote.VOL_UP) }))
        return v
    }

    private fun startBluetooth() {
        if (Build.VERSION.SDK_INT >= 31 &&
            checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_ADVERTISE), 1)
            return
        }
        if (hid == null) hid = HidRemote(this) { s -> runOnUiThread { btStatus.text = s; refreshDevices() } }
        hid?.start()
        refreshDevices()
    }

    override fun onRequestPermissionsResult(code: Int, perms: Array<out String>, results: IntArray) {
        if (results.isNotEmpty() && results.all { it == android.content.pm.PackageManager.PERMISSION_GRANTED }) startBluetooth()
        else btStatus.text = "Bluetooth permission is required"
    }

    private fun refreshDevices() {
        deviceList.removeAllViews()
        hid?.bondedDevices()?.forEach { d ->
            @Suppress("MissingPermission")
            deviceList.addView(button("Connect: ${d.name}") { hid?.connect(d) })
        }
    }
}
