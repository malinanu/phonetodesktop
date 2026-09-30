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

    private val store by lazy { Store(this) }
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
    private var loadedKey: String? = null
    private var currentPcId: String? = null
    private lateinit var titleView: TextView

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
        // Status and navigation bars take the app background so nothing clashes with the page below.
        window.statusBarColor = C.BG
        window.navigationBarColor = C.BG
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
        titleView = label("Phone Remote", 20f, bold = true).apply { setOnClickListener { if (store.all().isNotEmpty()) showPcs() } }
        top.addView(titleView, lp(0, WRAP_CONTENT, 1f))
        guideBtn = button("Guide") { toggleGuide() }.apply { minHeight = dp(40); textSize = 13f; background = null; setTextColor(C.DIM) }
        top.addView(guideBtn, lp(WRAP_CONTENT, dp(40)))
        top.addView(button("Scan QR") { scanQr() }.apply { minHeight = dp(40); textSize = 13f }, lp(WRAP_CONTENT, dp(40)))
        root.addView(top)

        banner = label("", 14f, C.ON_ACC, bold = true).apply {
            background = bg(C.BAD, 12)
            setPadding(dp(16), dp(12), dp(16), dp(12))
            visibility = View.GONE
            setOnClickListener { visibility = View.GONE; loadedKey = null; reconnect() }
        }
        root.addView(banner, lp(m = 12))

        val content = FrameLayout(this)
        web = WebView(this).apply {
            setBackgroundColor(C.BG)
            overScrollMode = View.OVER_SCROLL_NEVER
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            addJavascriptInterface(Bridge(), "AndroidBridge")
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

        discovery = Discovery(this) { id, host, port -> runOnUiThread { onSighting(id, host, port) } }
        showMode(false)
        // Load the last known address right away, then let mDNS correct it if the IP changed.
        refreshTitle()
        openActive()
    }

    override fun onStart() { super.onStart(); if (paired()) discovery.start() }
    override fun onStop() { super.onStop(); discovery.stop() }
    override fun onDestroy() { hid?.stop(); super.onDestroy() }

    private fun paired() = store.active() != null

    private fun showBanner(msg: String) { banner.text = msg; banner.visibility = View.VISIBLE }

    private fun reconnect() {
        openActive()
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
            "Double-click its tray icon and open Phones",
            "Scan the code, then tap Allow on the PC",
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

    /** Called by the PC's page (see index.html) when the PC owner approves or removes this phone. */
    inner class Bridge {
        @android.webkit.JavascriptInterface
        fun onPaired(token: String, deviceId: String) {
            val id = currentPcId ?: return
            runOnUiThread { store.markPaired(id, token, deviceId) }
        }

        @android.webkit.JavascriptInterface
        fun onRevoked() {
            val id = currentPcId ?: return
            runOnUiThread {
                store.forget(id)
                loadedKey = null
                refreshTitle()
                showMode(false)
                openActive()
            }
        }
    }

    private fun refreshTitle() {
        val pcs = store.all()
        titleView.text = store.active()?.let { if (pcs.size > 1) "${it.name} ▾" else it.name } ?: "Phone Remote"
    }

    /** Load the active PC's controller. The pairing never changes here, only where we look for the PC. */
    private fun openActive() {
        val pc = store.active() ?: return
        if (pc.host.isEmpty()) return
        val key = "${pc.id}@${pc.host}:${pc.port}"
        if (loadedKey == key) return
        loadedKey = key
        currentPcId = pc.id
        val auth = if (pc.paired) "mode=auth" else "mode=pair&dn=${java.net.URLEncoder.encode(Build.MODEL, "UTF-8")}"
        web.loadUrl("http://${pc.host}:${pc.port}/#k=${pc.token}&dev=${pc.deviceId}&$auth")
        updateVisibility(bluetooth = btView.visibility == View.VISIBLE)
    }

    /** mDNS saw a PC. Match it to a saved pairing by identity; a stranger's PC is ignored. */
    private fun onSighting(id: String?, host: String, port: Int) {
        val known = when {
            id != null -> store.updateAddress(id, host, port) ?: store.adoptLegacy(id, host, port)
            // Agents older than this version advertise no id: only trust it if we know exactly one PC.
            store.all().size == 1 -> store.active()?.let { store.updateAddress(it.id, host, port) }
            else -> null
        } ?: return
        if (known.id == store.active()?.id) openActive()
    }

    private fun showPcs() {
        val pcs = store.all()
        val names = pcs.map { it.name }.toTypedArray()
        android.app.AlertDialog.Builder(this)
            .setTitle("Your PCs")
            .setItems(names) { _, i ->
                store.setActive(pcs[i].id)
                loadedKey = null
                refreshTitle()
                showMode(false)
                openActive()
            }
            .setPositiveButton("Add a PC") { _, _ -> scanQr() }
            .setNegativeButton("Forget ${store.active()?.name ?: ""}") { _, _ ->
                store.active()?.let { store.forget(it.id) }
                loadedKey = null
                refreshTitle()
                showMode(false)
                openActive()
            }
            .show()
    }

    private fun scanQr() {
        val opts = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(this, opts).startScan()
            .addOnSuccessListener { code ->
                val m = Regex("^http://([^:/]+):(\\d+)/#(.+)$").find(code.rawValue ?: "")
                val params = m?.groupValues?.get(3)?.split("&")?.mapNotNull { it.split("=", limit = 2).takeIf { p -> p.size == 2 } }?.associate { it[0] to it[1] }
                val token = params?.get("k")
                if (m == null || token == null) { Toast.makeText(this, "That is not a Phone Remote code", Toast.LENGTH_SHORT).show(); return@addOnSuccessListener }
                val host = m.groupValues[1]
                val port = m.groupValues[2].toInt()
                val name = params["n"]?.let { java.net.URLDecoder.decode(it, "UTF-8") } ?: host
                // Scanning the same PC again just refreshes its entry; it never creates a duplicate.
                val pcId = params["id"] ?: "$host:$port"
                val deviceId = store.all().firstOrNull { it.id == pcId }?.deviceId ?: newDeviceId()
                store.upsert(Pc(pcId, name, token, host, port, deviceId, paired = false))
                loadedKey = null
                refreshTitle()
                showMode(false)
                openActive()
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

    // ---- Bluetooth: remote, touchpad and keyboard -----------------------------------------

    private val btPrefs by lazy { getSharedPreferences("bt", Context.MODE_PRIVATE) }
    private lateinit var btDot: View
    private lateinit var btTitle: TextView
    private lateinit var btSub: TextView
    private lateinit var btAction: Button
    private lateinit var btHelp: View
    private lateinit var btMigrate: View
    private lateinit var btRemote: View
    private lateinit var btPadWeb: WebView
    private lateinit var btTabRemote: Button
    private lateinit var btTabPad: Button
    private var btDevice: android.bluetooth.BluetoothDevice? = null
    private var btShowAll = false
    private var btStarted = false

    private fun card(title: String, body: String, action: String?, onAction: () -> Unit): LinearLayout {
        val c = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; background = bg(C.CARD, 18); setPadding(dp(16), dp(14), dp(16), dp(14)) }
        c.addView(label(title, 13f, C.ACC, bold = true))
        c.addView(label(body, 14f).apply { setPadding(0, dp(4), 0, if (action != null) dp(10) else 0) })
        if (action != null) c.addView(button(action) { onAction() }.apply { background = bg(C.CARD2); minHeight = dp(44) })
        return c
    }

    private fun iconButton(icon: Int, caption: String? = null, filled: Boolean = false, click: () -> Unit): View {
        val box = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            background = bg(if (filled) C.ACC else C.CARD, 22)
            isClickable = true
            setOnClickListener { needConnection { click() } }
        }
        val tint = if (filled) C.ON_ACC else C.FG
        box.addView(android.widget.ImageView(this).apply { setImageResource(icon); setColorFilter(tint) }, LinearLayout.LayoutParams(dp(if (filled) 40 else 30), dp(if (filled) 40 else 30)))
        if (caption != null) box.addView(label(caption, 12f, if (filled) C.ON_ACC else C.DIM).apply { setPadding(0, dp(4), 0, 0) })
        return box
    }

    private fun needConnection(run: () -> Unit) {
        if (hid?.connected == true) run() else {
            Toast.makeText(this, "Not connected to a PC yet", Toast.LENGTH_SHORT).show()
            autoConnect()
        }
    }

    private fun buildBluetoothView(): View {
        val col = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(16), dp(4), dp(16), dp(8)) }

        // Status: who we are connected to, and the one button to change it.
        val status = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL; background = bg(C.CARD, 18); setPadding(dp(16), dp(10), dp(10), dp(10)) }
        btDot = View(this).apply { background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(C.DIM) } }
        status.addView(btDot, LinearLayout.LayoutParams(dp(10), dp(10)).apply { rightMargin = dp(12) })
        val texts = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        btTitle = label("Not connected", 16f, bold = true)
        btSub = label("Bluetooth, no PC software needed", 13f, C.DIM)
        texts.addView(btTitle); texts.addView(btSub)
        status.addView(texts, lp(0, WRAP_CONTENT, 1f))
        btAction = button("Connect") { onBtAction() }.apply { minHeight = dp(40); textSize = 13f }
        status.addView(btAction, lp(WRAP_CONTENT, dp(40)))
        col.addView(status, lp())

        btMigrate = card(
            "Update needed, once",
            "To use the mouse and keyboard, remove this phone in Windows (Settings, Bluetooth & devices, select the phone, Remove device) and pair it again.",
            "Done, I paired it again",
        ) { btPrefs.edit().putInt("hid_v", HidRemote.DESCRIPTOR_VERSION).apply(); refreshBtUi() }
        col.addView(btMigrate, lp().apply { topMargin = dp(10) })

        btHelp = card(
            "First time",
            "On the PC: Settings, Bluetooth & devices, Add device, Bluetooth. Make this phone visible, then pick it on the PC.",
            "Make phone visible",
        ) {
            startActivity(Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE).putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, 120))
        }
        col.addView(btHelp, lp().apply { topMargin = dp(10) })

        // Remote | Touchpad
        val seg = LinearLayout(this).apply { background = bg(C.CARD, 99); setPadding(dp(4), dp(4), dp(4), dp(4)) }
        btTabRemote = button("Remote") { showBtTab(false) }.apply { minHeight = dp(40) }
        btTabPad = button("Touchpad") { showBtTab(true) }.apply { minHeight = dp(40) }
        seg.addView(btTabRemote, lp(0, WRAP_CONTENT, 1f)); seg.addView(btTabPad, lp(0, WRAP_CONTENT, 1f))
        col.addView(seg, lp().apply { topMargin = dp(12) })

        val frame = FrameLayout(this)
        btRemote = buildRemotePad()
        btPadWeb = WebView(this).apply {
            setBackgroundColor(C.BG)
            overScrollMode = View.OVER_SCROLL_NEVER
            settings.javaScriptEnabled = true
            isFocusable = true
            isFocusableInTouchMode = true
            addJavascriptInterface(HidBridge(), "AndroidHid")
            loadUrl("file:///android_asset/pad.html")
            visibility = View.GONE
        }
        frame.addView(btRemote, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        frame.addView(btPadWeb, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        col.addView(frame, lp(h = 0, weight = 1f).apply { topMargin = dp(10) })
        showBtTab(false)
        return col
    }

    private fun buildRemotePad(): View {
        val pad = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        fun row(weight: Float, vararg v: Pair<View, Float>) = LinearLayout(this).apply {
            v.forEach { (view, w) -> addView(view, lp(0, MATCH_PARENT, w, 4)) }
        }.also { pad.addView(it, lp(h = 0, weight = weight)) }
        row(2f, iconButton(R.drawable.ic_prev) { hid?.media(HidRemote.PREV) } to 1f,
            iconButton(R.drawable.ic_play_pause, filled = true) { hid?.media(HidRemote.PLAY_PAUSE) } to 2f,
            iconButton(R.drawable.ic_next) { hid?.media(HidRemote.NEXT) } to 1f)
        // Arrow keys reach whichever window has focus; players skip 5–10 s per press.
        row(1.4f, iconButton(R.drawable.ic_back10, "Back") { hid?.key(0x50, 2) } to 1f,
            iconButton(R.drawable.ic_play, "Space") { hid?.key(0x2C) } to 1f,
            iconButton(R.drawable.ic_fwd10, "Forward") { hid?.key(0x4F, 2) } to 1f)
        row(1.4f, iconButton(R.drawable.ic_vol_down) { hid?.media(HidRemote.VOL_DOWN) } to 1f,
            iconButton(R.drawable.ic_mute) { hid?.media(HidRemote.MUTE) } to 1f,
            iconButton(R.drawable.ic_vol_up) { hid?.media(HidRemote.VOL_UP) } to 1f)
        pad.addView(label("Back and Forward press the arrow keys in the window in front on the PC.", 12f, C.DIM).apply { setPadding(dp(4), dp(6), 0, 0) })
        return pad
    }

    private fun showBtTab(touchpad: Boolean) {
        btRemote.visibility = if (touchpad) View.GONE else View.VISIBLE
        btPadWeb.visibility = if (touchpad) View.VISIBLE else View.GONE
        btTabRemote.background = bg(if (!touchpad) C.ACC else C.CARD, 99)
        btTabPad.background = bg(if (touchpad) C.ACC else C.CARD, 99)
        btTabRemote.setTextColor(if (!touchpad) C.ON_ACC else C.DIM)
        btTabPad.setTextColor(if (touchpad) C.ON_ACC else C.DIM)
        if (touchpad) btPadWeb.requestFocus()
    }

    /** Called from the touchpad page; every call becomes a real Bluetooth HID report. */
    inner class HidBridge {
        @android.webkit.JavascriptInterface fun move(dx: Int, dy: Int) { hid?.move(dx, dy) }
        @android.webkit.JavascriptInterface fun button(b: String, action: String) { hid?.button(b, action) }
        @android.webkit.JavascriptInterface fun scroll(dx: Int, dy: Int) { hid?.scroll(dx, dy) }
        @android.webkit.JavascriptInterface fun key(name: String, mods: String) { hid?.namedKey(name, mods.split(",").filter { it.isNotEmpty() }) }
        @android.webkit.JavascriptInterface fun text(s: String) {
            val skipped = hid?.typeText(s) ?: 0
            if (skipped > 0) runOnUiThread { Toast.makeText(this@MainActivity, "That character needs Wi-Fi mode (Bluetooth types US-layout keys)", Toast.LENGTH_SHORT).show() }
        }
    }

    @Suppress("MissingPermission")
    private fun computers(): List<android.bluetooth.BluetoothDevice> {
        val all = hid?.bondedDevices().orEmpty()
        return if (btShowAll) all else all.filter { HidRemote.isComputer(it) }
    }

    @Suppress("MissingPermission")
    private fun autoConnect() {
        val h = hid ?: return
        if (h.connected) return
        val bonded = h.bondedDevices()
        val last = btPrefs.getString("last", null)
        val target = bonded.firstOrNull { it.address == last } ?: bonded.filter { HidRemote.isComputer(it) }.singleOrNull()
        if (target != null) h.connect(target)
    }

    @Suppress("MissingPermission")
    private fun onBtAction() {
        val list = computers()
        when {
            list.isEmpty() && !btShowAll -> {
                btShowAll = true
                if (computers().isEmpty()) { Toast.makeText(this, "No paired devices yet. Pair this phone from the PC first.", Toast.LENGTH_LONG).show(); btShowAll = false; btHelp.visibility = View.VISIBLE }
                else showChooser()
            }
            list.size == 1 && hid?.connected != true -> hid?.connect(list[0])
            else -> showChooser()
        }
    }

    @Suppress("MissingPermission")
    private fun showChooser() {
        val list = computers()
        val names = list.map { it.name ?: it.address }.toTypedArray()
        android.app.AlertDialog.Builder(this)
            .setTitle(if (btShowAll) "All paired devices" else "Choose your PC")
            .setItems(names) { _, i -> hid?.connect(list[i]) }
            .setNeutralButton(if (btShowAll) "Computers only" else "Show all devices") { _, _ -> btShowAll = !btShowAll; showChooser() }
            .setNegativeButton("Cancel", null)
            .show()
    }

    @Suppress("MissingPermission")
    private fun refreshBtUi() {
        val d = btDevice
        (btDot.background as GradientDrawable).setColor(if (d != null) Color.parseColor("#86D9A6") else C.DIM)
        btTitle.text = if (d != null) "Connected to ${d.name ?: d.address}" else "Not connected"
        btSub.text = if (d != null) "Remote, mouse and keyboard are ready" else "Pick your PC to start"
        btAction.text = if (d != null) "Change" else if (computers().size == 1) "Connect" else "Choose PC"
        val seen = btPrefs.getBoolean("seen", false)
        btHelp.visibility = if (!seen && d == null) View.VISIBLE else View.GONE
        btMigrate.visibility = if (seen && btPrefs.getInt("hid_v", 0) != HidRemote.DESCRIPTOR_VERSION) View.VISIBLE else View.GONE
        btRemote.alpha = if (d != null) 1f else 0.45f
        btPadWeb.alpha = if (d != null) 1f else 0.45f
    }

    private fun startBluetooth() {
        if (Build.VERSION.SDK_INT >= 31 && checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_ADVERTISE), 1)
            return
        }
        if (hid == null) {
            hid = HidRemote(
                this,
                onChange = { device, message ->
                    runOnUiThread {
                        btDevice = device
                        if (device != null) {
                            // First success: no need to show the setup help again; the descriptor is already current.
                            if (!btPrefs.getBoolean("seen", false)) btPrefs.edit().putBoolean("seen", true).putInt("hid_v", HidRemote.DESCRIPTOR_VERSION).apply()
                            btPrefs.edit().putString("last", device.address).apply()
                        }
                        refreshBtUi()
                        if (device == null) btSub.text = message
                    }
                },
                onReady = { runOnUiThread { refreshBtUi(); autoConnect() } },
            )
        }
        if (!btStarted) { btStarted = true; hid?.start() } else autoConnect()
        refreshBtUi()
    }

    override fun onRequestPermissionsResult(code: Int, perms: Array<out String>, results: IntArray) {
        if (results.isNotEmpty() && results.all { it == PackageManager.PERMISSION_GRANTED }) startBluetooth()
        else btSub.text = "Allow Bluetooth permission to use this tab"
    }
}
