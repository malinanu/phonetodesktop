package app.phoneremote

import android.Manifest
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

/**
 * Wi-Fi tab: the PC's own phone UI in a WebView, located via mDNS, paired once by QR.
 * Bluetooth tab: the phone acts as a Bluetooth media-key/arrow-key keyboard (see HidRemote).
 */
class MainActivity : Activity() {
    private object C {
        val BG = Color.parseColor("#15110E")
        val CARD = Color.parseColor("#1F1A16")
        val CARD2 = Color.parseColor("#2A231D")
        val FG = Color.parseColor("#F5EBDD")
        val DIM = Color.parseColor("#B3A594")
        val ACC = Color.parseColor("#FF9A3C")
        val ON_ACC = Color.parseColor("#1B1006")
        val BAD = Color.parseColor("#FF8A7A")
    }

    private val prefs by lazy { getSharedPreferences("pr", Context.MODE_PRIVATE) }
    private lateinit var web: WebView
    private lateinit var welcome: View
    private lateinit var banner: TextView
    private lateinit var guide: WebView
    private lateinit var guideBtn: Button
    private var guideOpen = false
    private lateinit var btView: View
    private lateinit var tabWifi: Button
    private lateinit var tabBt: Button
    private lateinit var discovery: Discovery
    private var hid: HidRemote? = null
    private var loadedHost: String? = null

    private val bold: Typeface by lazy { resources.getFont(R.font.bricolage_bold) }
    private val medium: Typeface by lazy { resources.getFont(R.font.bricolage_medium) }

    private fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    private fun bg(color: Int, radius: Int = 16) = GradientDrawable().apply { setColor(color); cornerRadius = dp(radius).toFloat() }

    private fun button(t: String, filled: Boolean = false, click: () -> Unit) = Button(this).apply {
        text = t
        isAllCaps = false
        textSize = 15f
        typeface = bold
        setTextColor(if (filled) C.ON_ACC else C.FG)
        background = bg(if (filled) C.ACC else C.CARD)
        stateListAnimator = null
        minHeight = dp(52)
        setPadding(dp(16), 0, dp(16), 0)
        setOnClickListener { click() }
    }

    private fun label(t: String, size: Float, color: Int = C.FG, bold: Boolean = false) = TextView(this).apply {
        text = t
        textSize = size
        setTextColor(color)
        typeface = if (bold) this@MainActivity.bold else medium
    }

    private fun lp(w: Int = MATCH_PARENT, h: Int = WRAP_CONTENT, weight: Float = 0f, m: Int = 0) =
        LinearLayout.LayoutParams(w, h, weight).apply { setMargins(dp(m), dp(m), dp(m), dp(m)) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Edge-to-edge is enforced on targetSdk 35; fitsSystemWindows keeps content below the status bar.
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(C.BG)
            fitsSystemWindows = true
        }

        val top = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(20), dp(8), dp(12), dp(8))
        }
        top.addView(label("Phone Remote", 20f, bold = true), lp(0, WRAP_CONTENT, 1f))
        guideBtn = button("Guide") { toggleGuide() }.apply { minHeight = dp(40); textSize = 13f; background = null; setTextColor(C.DIM) }
        top.addView(guideBtn, lp(WRAP_CONTENT, dp(40)))
        top.addView(button("Scan QR") { scanQr() }.apply { minHeight = dp(40); textSize = 13f }, lp(WRAP_CONTENT, dp(40)))
        root.addView(top)

        banner = label("", 14f, C.ON_ACC, bold = true).apply {
            background = bg(C.BAD, 12)
            setPadding(dp(16), dp(12), dp(16), dp(12))
            visibility = View.GONE
            setOnClickListener { visibility = View.GONE; loadedHost = null; reconnect() }
        }
        root.addView(banner, lp(m = 12))

        val content = FrameLayout(this)
        web = WebView(this).apply {
            setBackgroundColor(C.BG)
            overScrollMode = View.OVER_SCROLL_NEVER
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            webViewClient = object : WebViewClient() {
                override fun onReceivedError(v: WebView, r: WebResourceRequest, e: WebResourceError) {
                    if (r.isForMainFrame) showBanner("Can't reach your PC. Same Wi-Fi? Tap to retry, or use Bluetooth.")
                }
                override fun onPageFinished(v: WebView, url: String) { banner.visibility = View.GONE }
            }
        }
        guide = WebView(this).apply {
            setBackgroundColor(C.BG)
            overScrollMode = View.OVER_SCROLL_NEVER
            settings.javaScriptEnabled = true
            visibility = View.GONE
            loadUrl("file:///android_asset/guide.html")
        }
        welcome = buildWelcome()
        btView = buildBluetoothView()
        content.addView(web, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(welcome, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(guide, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(btView, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        root.addView(content, lp(h = 0, weight = 1f))

        val nav = LinearLayout(this).apply { setPadding(dp(12), dp(8), dp(12), dp(12)) }
        tabWifi = button("Wi-Fi") { showMode(false) }
        tabBt = button("Bluetooth") { showMode(true) }
        nav.addView(tabWifi, lp(0, WRAP_CONTENT, 1f, 4))
        nav.addView(tabBt, lp(0, WRAP_CONTENT, 1f, 4))
        root.addView(nav)
        setContentView(root)

        discovery = Discovery(this) { host, port -> runOnUiThread { openPc(host, port) } }
        showMode(false)
        // Load the last known address right away, then let mDNS correct it if the IP changed.
        prefs.getString("host", null)?.let { openPc(it, prefs.getInt("port", 8765)) }
    }

    override fun onStart() { super.onStart(); if (paired()) discovery.start() }
    override fun onStop() { super.onStop(); discovery.stop() }
    override fun onDestroy() { hid?.stop(); super.onDestroy() }

    private fun paired() = prefs.getString("token", null) != null

    private fun showBanner(msg: String) { banner.text = msg; banner.visibility = View.VISIBLE }

    private fun reconnect() {
        prefs.getString("host", null)?.let { openPc(it, prefs.getInt("port", 8765)) }
        discovery.start()
    }

    private fun buildWelcome(): View {
        val col = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.BOTTOM
            setPadding(dp(24), dp(16), dp(24), dp(20))
        }
        col.addView(label("SETUP", 12f, C.DIM, bold = true).apply { letterSpacing = 0.12f })
        col.addView(label("Take the remote.", 38f, bold = true).apply { setPadding(0, dp(8), 0, dp(10)); setLineSpacing(0f, 0.95f) })
        col.addView(label("Pause, skip and seek your PC's media from the couch. Pair once; it reconnects by itself.", 17f, C.DIM).apply { setPadding(0, 0, 0, dp(20)) })
        listOf(
            "Open Phone Remote on your PC",
            "Right-click its tray icon, then Pair a phone",
            "Tap the button below and scan the code",
        ).forEachIndexed { i, t ->
            val row = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(0, dp(12), 0, dp(12)) }
            row.addView(label("${i + 1}", 26f, C.ACC, bold = true), lp(dp(36), WRAP_CONTENT))
            row.addView(label(t, 16f), lp(0, WRAP_CONTENT, 1f))
            col.addView(row)
        }
        col.addView(button("Scan QR code", filled = true) { scanQr() }, lp().apply { topMargin = dp(20) })
        col.addView(button("How it works and troubleshooting") { toggleGuide(true, "how") }.apply { background = null; setTextColor(C.DIM) }, lp().apply { topMargin = dp(4) })
        return col
    }

    private fun openPc(host: String, port: Int) {
        val token = prefs.getString("token", null) ?: return
        if (loadedHost == "$host:$port") return
        loadedHost = "$host:$port"
        prefs.edit().putString("host", host).putInt("port", port).apply()
        web.loadUrl("http://$host:$port/#k=$token")
        updateVisibility(bluetooth = btView.visibility == View.VISIBLE)
    }

    private fun scanQr() {
        val opts = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(this, opts).startScan()
            .addOnSuccessListener { code ->
                val m = Regex("^http://([^:/]+):(\\d+)/#k=([\\w-]+)$").find(code.rawValue ?: "")
                if (m == null) { Toast.makeText(this, "That is not a Phone Remote code", Toast.LENGTH_SHORT).show(); return@addOnSuccessListener }
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
        guideOpen = false
        guideBtn.text = "Guide"
        updateVisibility(bluetooth)
        if (bluetooth) startBluetooth()
    }

    private fun toggleGuide(open: Boolean = !guideOpen, anchor: String = "connect") {
        guideOpen = open
        guideBtn.text = if (open) "Close" else "Guide"
        if (open) guide.loadUrl("file:///android_asset/guide.html#$anchor")
        updateVisibility(btView.visibility == View.VISIBLE)
    }

    private fun updateVisibility(bluetooth: Boolean) {
        guide.visibility = if (guideOpen) View.VISIBLE else View.GONE
        btView.visibility = if (bluetooth && !guideOpen) View.VISIBLE else View.GONE
        web.visibility = if (!bluetooth && !guideOpen && paired()) View.VISIBLE else View.GONE
        welcome.visibility = if (!bluetooth && !guideOpen && !paired()) View.VISIBLE else View.GONE
        tabWifi.background = bg(if (!bluetooth) C.ACC else C.CARD)
        tabBt.background = bg(if (bluetooth) C.ACC else C.CARD)
        tabWifi.setTextColor(if (!bluetooth) C.ON_ACC else C.DIM)
        tabBt.setTextColor(if (bluetooth) C.ON_ACC else C.DIM)
        if (bluetooth) banner.visibility = View.GONE
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        when {
            guideOpen -> toggleGuide(false)
            web.visibility == View.VISIBLE && web.canGoBack() -> web.goBack()
            else -> super.onBackPressed()
        }
    }

    // ---- Bluetooth HID mode -------------------------------------------------------------

    private lateinit var btStatus: TextView
    private lateinit var deviceList: LinearLayout

    private fun buildBluetoothView(): View {
        val v = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(16), dp(4), dp(16), dp(16)) }
        btStatus = label("Bluetooth remote. No PC software needed.", 14f, C.DIM).apply { setPadding(dp(4), dp(4), dp(4), dp(12)) }
        v.addView(btStatus)

        val setup = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; background = bg(C.CARD, 20); setPadding(dp(16), dp(16), dp(16), dp(16)) }
        setup.addView(label("First time only", 13f, C.DIM))
        setup.addView(label("On the PC: Settings → Bluetooth & devices → Add device → Bluetooth. Then make this phone visible:", 14f).apply { setPadding(0, dp(6), 0, dp(10)) })
        setup.addView(button("Make phone visible") {
            startActivity(Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE).putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, 120))
        }.apply { background = bg(C.CARD2) })
        v.addView(setup, lp())

        deviceList = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        v.addView(deviceList, lp().apply { topMargin = dp(12) })

        fun row(vararg b: Button) = LinearLayout(this).apply { b.forEach { addView(it, lp(0, dp(72), 1f, 4)) } }
        fun key(t: String, filled: Boolean = false, f: () -> Unit) = button(t, filled, f).apply { textSize = 18f }
        val padView = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        padView.addView(row(key("⏮") { hid?.media(HidRemote.PREV) }, key("▶ / ⏸", true) { hid?.media(HidRemote.PLAY_PAUSE) }, key("⏭") { hid?.media(HidRemote.NEXT) }))
        // Arrow keys reach whichever window has focus; players skip 5–10 s per press.
        padView.addView(row(key("« Back") { hid?.key(HidRemote.KEY_LEFT, 2) }, key("Space") { hid?.key(HidRemote.KEY_SPACE) }, key("Fwd »") { hid?.key(HidRemote.KEY_RIGHT, 2) }))
        padView.addView(row(key("Vol −") { hid?.media(HidRemote.VOL_DOWN) }, key("Mute") { hid?.media(HidRemote.MUTE) }, key("Vol +") { hid?.media(HidRemote.VOL_UP) }))
        padView.addView(label("Back/Fwd send arrow keys to the window that has focus on the PC.", 12f, C.DIM).apply { setPadding(dp(4), dp(8), 0, 0) })
        v.addView(padView, lp().apply { topMargin = dp(12) })

        return ScrollView(this).apply { addView(v); isFillViewport = true }
    }

    private fun startBluetooth() {
        if (Build.VERSION.SDK_INT >= 31 && checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_ADVERTISE), 1)
            return
        }
        if (hid == null) hid = HidRemote(this) { s -> runOnUiThread { btStatus.text = s; refreshDevices() } }
        hid?.start()
        refreshDevices()
    }

    override fun onRequestPermissionsResult(code: Int, perms: Array<out String>, results: IntArray) {
        if (results.isNotEmpty() && results.all { it == PackageManager.PERMISSION_GRANTED }) startBluetooth()
        else btStatus.text = "Bluetooth permission is required"
    }

    private fun refreshDevices() {
        deviceList.removeAllViews()
        val devices = hid?.bondedDevices().orEmpty()
        if (devices.isNotEmpty()) deviceList.addView(label("Paired computers — tap to connect", 13f, C.DIM).apply { setPadding(dp(4), 0, 0, dp(6)) })
        devices.forEach { d ->
            @Suppress("MissingPermission")
            deviceList.addView(button(d.name ?: d.address) { hid?.connect(d) }, lp().apply { bottomMargin = dp(8) })
        }
    }
}
