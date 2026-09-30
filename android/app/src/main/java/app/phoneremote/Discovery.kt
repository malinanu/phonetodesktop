package app.phoneremote

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo

/** Finds the desktop agent via mDNS (`_phoneremote._tcp`) so an IP change never needs re-pairing. */
class Discovery(ctx: Context, private val onFound: (id: String?, host: String, port: Int) -> Unit) {
    private val nsd = ctx.getSystemService(Context.NSD_SERVICE) as NsdManager
    private var listener: NsdManager.DiscoveryListener? = null

    @Suppress("DEPRECATION") // resolveService is deprecated on 34+ but still works on 28+
    fun start() {
        stop()
        val l = object : NsdManager.DiscoveryListener {
            override fun onServiceFound(s: NsdServiceInfo) {
                nsd.resolveService(s, object : NsdManager.ResolveListener {
                    override fun onResolveFailed(s: NsdServiceInfo, code: Int) {}
                    override fun onServiceResolved(s: NsdServiceInfo) {
                        val addr = s.host as? java.net.Inet4Address ?: return // IPv6 link-local breaks the URL
                        val host = addr.hostAddress ?: return
                        val id = s.attributes?.get("id")?.let { String(it) }
                        onFound(id, host, s.port)
                    }
                })
            }
            override fun onServiceLost(s: NsdServiceInfo) {}
            override fun onDiscoveryStarted(t: String) {}
            override fun onDiscoveryStopped(t: String) {}
            override fun onStartDiscoveryFailed(t: String, e: Int) {}
            override fun onStopDiscoveryFailed(t: String, e: Int) {}
        }
        listener = l
        nsd.discoverServices("_phoneremote._tcp", NsdManager.PROTOCOL_DNS_SD, l)
    }

    fun stop() {
        listener?.let { runCatching { nsd.stopServiceDiscovery(it) } }
        listener = null
    }
}
