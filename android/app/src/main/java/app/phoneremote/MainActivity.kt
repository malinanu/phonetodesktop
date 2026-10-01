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
    /** Colours of the current theme; filled in by [C.apply] before any view is built. */
    private object C {
        var BG = 0
        var CARD = 0
        var CARD2 = 0
        var FG = 0
        var DIM = 0
        var ACC = 0
        var ACC_TEXT = 0
        var ON_ACC = 0
        var BAD = 0
        var GOOD = 0

        fun apply(p: Palette) {
            BG = p.bg; CARD = p.card; CARD2 = p.card2; FG = p.fg; DIM = p.dim
            ACC = p.acc; ACC_TEXT = p.accText; ON_ACC = p.onAcc; BAD = p.bad; GOOD = p.good
        }
    }

    private var dark = true

    private val store by lazy { Store(this) }
    private lateinit var web: WebView
    private lateinit var welcome: View
    private lateinit var banner: TextView
    private lateinit var guide: WebView
    private lateinit var settingsPage: WebView
    private lateinit var nav: LinearLayout
    /** The full-screen page on top of everything: "guide", "settings" or none. */
    private var overlay: String? = null
    private lateinit var btView: View
    private lateinit var navWifi: LinearLayout
    private lateinit var navBt: LinearLayout
    private lateinit var navFiles: LinearLayout
    private lateinit var filesView: View
    private lateinit var filesBody: TextView
    private lateinit var filesOpen: Button
    private var filesMode = false
    private lateinit var discovery: Discovery
    private var hid: HidRemote? = null
    private var loadedKey: String? = null
    private var currentPcId: String? = null

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
        // Theme first, so every native view, dialog and popup agrees with the pages.
        val savedSettings = store.settingsJson()
        val night = (resources.configuration.uiMode and android.content.res.Configuration.UI_MODE_NIGHT_MASK) == android.content.res.Configuration.UI_MODE_NIGHT_YES
        dark = Theme.isDark(Theme.themeOf(savedSettings), night)
        C.apply(Theme.palette(dark))
        setTheme(if (dark) android.R.style.Theme_Material_NoActionBar else android.R.style.Theme_Material_Light_NoActionBar)
        super.onCreate(savedInstanceState)
        window.statusBarColor = C.BG
        window.navigationBarColor = C.BG
        applyKeepAwake(Theme.keepAwakeOf(savedSettings))
        // Edge-to-edge is enforced on targetSdk 35; fitsSystemWindows keeps content below the status bar.
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(C.BG)
            fitsSystemWindows = true
        }

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
        guide = overlayPage()
        settingsPage = overlayPage()
        welcome = buildWelcome()
        btView = buildBluetoothView()
        filesView = buildFilesView()
        content.addView(web, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(welcome, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(guide, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(settingsPage, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(btView, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        content.addView(filesView, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        root.addView(content, lp(h = 0, weight = 1f))

        nav = LinearLayout(this).apply {
            setBackgroundColor(C.CARD)
            setPadding(dp(8), dp(6), dp(8), dp(6))
        }
        navWifi = navItem(R.drawable.ic_wifi, "Wi-Fi") { showMode(false) }
        navBt = navItem(R.drawable.ic_bluetooth, "Bluetooth") { showMode(true) }
        nav.addView(navWifi, lp(0, WRAP_CONTENT, 1f))
        navFiles = navItem(R.drawable.ic_files, "Files") { showFiles() }
        nav.addView(navBt, lp(0, WRAP_CONTENT, 1f))
        nav.addView(navFiles, lp(0, WRAP_CONTENT, 1f))
        root.addView(nav)
        // The bar is only useful when the keyboard is closed.
        root.setOnApplyWindowInsetsListener { v, insets ->
            val imeUp = if (Build.VERSION.SDK_INT >= 30) insets.isVisible(android.view.WindowInsets.Type.ime()) else false
            nav.visibility = if (imeUp) View.GONE else View.VISIBLE
            v.onApplyWindowInsets(insets)
        }
        setContentView(root)
        styleSystemBars()

        discovery = Discovery(this) { id, host, port -> runOnUiThread { onSighting(id, host, port) } }
        showMode(false)
        // Load the last known address right away, then let mDNS correct it if the IP changed.
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
            row.addView(label("${i + 1}", 26f, C.ACC_TEXT, bold = true), lp(dp(36), WRAP_CONTENT))
            row.addView(label(t, 16f), lp(0, WRAP_CONTENT, 1f))
            col.addView(row)
        }
        col.addView(button("Scan QR code", filled = true) { scanQr() }, lp().apply { topMargin = dp(20) })
        col.addView(button("How it works and troubleshooting") { openOverlay("guide", "how") }.apply { background = null; setTextColor(C.DIM) }, lp().apply { topMargin = dp(4) })
        return col
    }

    /**
     * What the pages loaded in this app can ask of it. Every WebView (remote, guide, settings and
     * the Bluetooth touchpad) gets the same object as `window.AndroidBridge`; in an ordinary
     * browser the object does not exist and the pages fall back to their own behaviour.
     */
    inner class Bridge {
        // -- pairing (from the PC's remote page) --
        @android.webkit.JavascriptInterface
        fun onPaired(token: String, deviceId: String) {
            val id = currentPcId ?: return
            runOnUiThread { store.markPaired(id, token, deviceId) }
        }

        @android.webkit.JavascriptInterface
        fun onRevoked() {
            val id = currentPcId ?: return
            runOnUiThread { forgetPc(id) }
        }

        // -- navigation --
        @android.webkit.JavascriptInterface fun openGuide(anchor: String) { runOnUiThread { openOverlay("guide", anchor) } }
        @android.webkit.JavascriptInterface fun openSettings() { runOnUiThread { openOverlay("settings") } }
        @android.webkit.JavascriptInterface fun closeOverlay() { runOnUiThread { this@MainActivity.closeOverlay() } }
        @android.webkit.JavascriptInterface fun addPc() { runOnUiThread { scanQr() } }
        @android.webkit.JavascriptInterface fun showBluetooth() { runOnUiThread { showMode(true) } }
        @android.webkit.JavascriptInterface fun switchPc() { runOnUiThread { showPcs() } }
        @android.webkit.JavascriptInterface fun pcCount(): Int = store.all().size
        @android.webkit.JavascriptInterface fun forgetCurrentPc() { runOnUiThread { store.active()?.let { forgetPc(it.id) } } }

        // -- settings (one JSON blob kept in the app, shared by every page) --
        @android.webkit.JavascriptInterface fun getSettings(): String = store.settingsJson()
        @android.webkit.JavascriptInterface fun saveSettings(json: String) {
            val old = Theme.themeOf(store.settingsJson())
            store.saveSettings(json)
            runOnUiThread {
                applyKeepAwake(Theme.keepAwakeOf(json))
                // A theme change needs the native views rebuilt in the new colours.
                if (Theme.themeOf(json) != old) recreate()
            }
        }

        // -- Bluetooth mouse, keyboard (from the touchpad page in Bluetooth mode) --
        @android.webkit.JavascriptInterface fun hidConnected(): Boolean = hid?.connected == true
        @android.webkit.JavascriptInterface fun hidMove(dx: Int, dy: Int) { hid?.move(dx, dy) }
        @android.webkit.JavascriptInterface fun hidButton(b: String, action: String) { hid?.button(b, action) }
        @android.webkit.JavascriptInterface fun hidScroll(dx: Int, dy: Int) { hid?.scroll(dx, dy) }
        @android.webkit.JavascriptInterface fun hidKey(name: String, mods: String) { hid?.namedKey(name, mods.split(",").filter { it.isNotEmpty() }) }
        @android.webkit.JavascriptInterface fun hidText(s: String) {
            val skipped = hid?.typeText(s) ?: 0
            if (skipped > 0) runOnUiThread { Toast.makeText(this@MainActivity, "That character needs Wi-Fi mode (Bluetooth types US-layout keys)", Toast.LENGTH_SHORT).show() }
        }
    }

    private fun forgetPc(id: String) {
        store.forget(id)
        loadedKey = null
        showMode(false)
        openActive()
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
                showMode(false)
                openActive()
            }
            .setPositiveButton("Add a PC") { _, _ -> scanQr() }
            .setNegativeButton("Forget ${store.active()?.name ?: ""}") { _, _ -> store.active()?.let { forgetPc(it.id) } }
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
                showMode(false)
                openActive()
                discovery.start()
            }
            .addOnFailureListener { Toast.makeText(this, "Scan failed: ${it.message}", Toast.LENGTH_SHORT).show() }
    }

    private fun showMode(bluetooth: Boolean) {
        overlay = null
        filesMode = false
        updateVisibility(bluetooth)
        if (bluetooth) startBluetooth()
    }

    private fun showFiles() {
        overlay = null
        filesMode = true
        refreshFiles()
        updateVisibility(false)
    }

    /** The configured file-server address, or null when none is set. */
    private fun filesUrl() = FilesUrl.resolve(store.settingsJson(), BuildConfig.FILES_URL)

    /**
     * Files tab. Sending and receiving run in the phone's browser, not in a WebView here: receiving streams to disk
     * through a service worker or the File System Access API, which a WebView cannot hand to the system downloader.
     */
    private fun buildFilesView(): View {
        val col = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.BOTTOM
            setPadding(dp(24), dp(16), dp(24), dp(20))
        }
        col.addView(label("FILES", 12f, C.DIM, bold = true).apply { letterSpacing = 0.12f })
        col.addView(label("Send files.", 38f, bold = true).apply { setPadding(0, dp(8), 0, dp(10)); setLineSpacing(0f, 0.95f) })
        filesBody = label("", 17f, C.DIM).apply { setPadding(0, 0, 0, dp(20)) }
        col.addView(filesBody)
        listOf(
            "Open it here and on the other device",
            "Share the room link or QR code",
            "Pick files; they go straight between devices",
        ).forEachIndexed { i, t ->
            val row = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(0, dp(12), 0, dp(12)) }
            row.addView(label("${i + 1}", 26f, C.ACC_TEXT, bold = true), lp(dp(36), WRAP_CONTENT))
            row.addView(label(t, 16f), lp(0, WRAP_CONTENT, 1f))
            col.addView(row)
        }
        filesOpen = button("Open Send files", filled = true) {
            val url = filesUrl()
            if (url == null) openOverlay("settings")
            else try {
                startActivity(Intent(Intent.ACTION_VIEW, android.net.Uri.parse(url)))
            } catch (_: android.content.ActivityNotFoundException) {
                Toast.makeText(this, "No browser found", Toast.LENGTH_SHORT).show()
            }
        }
        col.addView(filesOpen, lp().apply { topMargin = dp(20) })
        col.addView(button("Change server address") { openOverlay("settings") }.apply { background = null; setTextColor(C.DIM) }, lp().apply { topMargin = dp(4) })
        return col
    }

    private fun refreshFiles() {
        val url = filesUrl()
        if (url == null) {
            filesBody.text = "Send files of any size to any device, privately. First add your file server address in Settings."
            filesOpen.text = "Open Settings"
        } else {
            filesBody.text = "Send files of any size to any device, privately. They open in your browser, which is what lets big files save straight to storage."
            filesOpen.text = "Open Send files"
        }
    }

    /** Full-screen Guide or Settings on top of the current screen. */
    private fun openOverlay(name: String, anchor: String = "") {
        overlay = name
        val url = if (name == "guide") "file:///android_asset/guide.html#${anchor.ifEmpty { "connect" }}" else "file:///android_asset/settings.html"
        (if (name == "guide") guide else settingsPage).loadUrl(url)
        updateVisibility(btView.visibility == View.VISIBLE)
    }

    private fun closeOverlay() {
        overlay = null
        if (filesMode) refreshFiles()
        updateVisibility(btView.visibility == View.VISIBLE)
    }

    private fun updateVisibility(bluetooth: Boolean) {
        val base = overlay == null
        guide.visibility = if (overlay == "guide") View.VISIBLE else View.GONE
        settingsPage.visibility = if (overlay == "settings") View.VISIBLE else View.GONE
        btView.visibility = if (bluetooth && base) View.VISIBLE else View.GONE
        filesView.visibility = if (filesMode && base) View.VISIBLE else View.GONE
        web.visibility = if (!bluetooth && !filesMode && base && paired()) View.VISIBLE else View.GONE
        welcome.visibility = if (!bluetooth && !filesMode && base && !paired()) View.VISIBLE else View.GONE
        styleNav(navWifi, !bluetooth && !filesMode)
        styleNav(navBt, bluetooth)
        styleNav(navFiles, filesMode)
        if (bluetooth) banner.visibility = View.GONE
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        when {
            overlay != null -> closeOverlay()
            web.visibility == View.VISIBLE && web.canGoBack() -> web.goBack()
            else -> super.onBackPressed()
        }
    }

    // ---- shell building blocks ---------------------------------------------------------------

    /** A full-screen WebView used for the Guide and Settings pages; hidden until opened. */
    private fun overlayPage() = WebView(this).apply {
        setBackgroundColor(C.BG)
        overScrollMode = View.OVER_SCROLL_NEVER
        settings.javaScriptEnabled = true
        addJavascriptInterface(Bridge(), "AndroidBridge")
        visibility = View.GONE
    }

    /** Bottom navigation entry: icon over label, accent when selected. */
    private fun navItem(icon: Int, text: String, click: () -> Unit): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER
        minimumHeight = dp(56)
        isClickable = true
        contentDescription = text
        setOnClickListener { click() }
        addView(android.widget.ImageView(this@MainActivity).apply { setImageResource(icon) }, LinearLayout.LayoutParams(dp(24), dp(24)))
        addView(label(text, 12f, bold = true).apply { setPadding(0, dp(2), 0, 0) })
    }

    private fun styleNav(item: LinearLayout, selected: Boolean) {
        val color = if (selected) C.ACC_TEXT else C.DIM
        (item.getChildAt(0) as android.widget.ImageView).setColorFilter(color)
        (item.getChildAt(1) as TextView).setTextColor(color)
    }

    /** Light icons on dark bars and dark icons on light bars. */
    private fun styleSystemBars() = runCatching {
        if (Build.VERSION.SDK_INT >= 30) {
            val mask = android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
            window.decorView.windowInsetsController?.setSystemBarsAppearance(if (dark) 0 else mask, mask)
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = if (dark) 0 else {
                var f = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
                if (Build.VERSION.SDK_INT >= 26) f = f or View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
                f
            }
        }
    }

    private fun applyKeepAwake(on: Boolean) {
        if (on) window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
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
        c.addView(label(title, 13f, C.ACC_TEXT, bold = true))
        c.addView(label(body, 14f).apply { setPadding(0, dp(4), 0, if (action != null) dp(10) else 0) })
        if (action != null) c.addView(button(action) { onAction() }.apply { background = bg(C.CARD2); minHeight = dp(44) })
        return c
    }

    private fun iconButton(icon: Int, desc: String, caption: String? = null, filled: Boolean = false, click: () -> Unit): View {
        val box = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            background = bg(if (filled) C.ACC else C.CARD, 22)
            isClickable = true
            contentDescription = desc
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
        val more = android.widget.ImageView(this).apply {
            setImageResource(R.drawable.ic_more)
            setColorFilter(C.DIM)
            setPadding(dp(10), dp(10), dp(10), dp(10))
            contentDescription = "More options"
            isClickable = true
            setOnClickListener { v ->
                android.widget.PopupMenu(this@MainActivity, v).apply {
                    menu.add(0, 1, 0, "Settings")
                    menu.add(0, 2, 1, "Guide")
                    menu.add(0, 3, 2, "Add a PC with a QR code")
                    setOnMenuItemClickListener {
                        when (it.itemId) { 1 -> openOverlay("settings"); 2 -> openOverlay("guide"); else -> scanQr() }
                        true
                    }
                }.show()
            }
        }
        status.addView(more, LinearLayout.LayoutParams(dp(44), dp(44)))
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
            addJavascriptInterface(Bridge(), "AndroidBridge")
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
        row(2f, iconButton(R.drawable.ic_prev, "Previous") { hid?.media(HidRemote.PREV) } to 1f,
            iconButton(R.drawable.ic_play_pause, "Play or pause", filled = true) { hid?.media(HidRemote.PLAY_PAUSE) } to 2f,
            iconButton(R.drawable.ic_next, "Next") { hid?.media(HidRemote.NEXT) } to 1f)
        // Arrow keys reach whichever window has focus; players skip 5–10 s per press.
        row(1.4f, iconButton(R.drawable.ic_back10, "Back 10 seconds", "Back") { hid?.key(0x50, 2) } to 1f,
            iconButton(R.drawable.ic_play, "Space key", "Space") { hid?.key(0x2C) } to 1f,
            iconButton(R.drawable.ic_fwd10, "Forward 10 seconds", "Forward") { hid?.key(0x4F, 2) } to 1f)
        row(1.4f, iconButton(R.drawable.ic_vol_down, "Volume down") { hid?.media(HidRemote.VOL_DOWN) } to 1f,
            iconButton(R.drawable.ic_mute, "Mute") { hid?.media(HidRemote.MUTE) } to 1f,
            iconButton(R.drawable.ic_vol_up, "Volume up") { hid?.media(HidRemote.VOL_UP) } to 1f)
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
        (btDot.background as GradientDrawable).setColor(if (d != null) C.GOOD else C.DIM)
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
