package app.phoneremote

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/** One paired PC. `id` is the PC's stable public identity; the token is the pairing secret. */
data class Pc(
    val id: String,
    val name: String,
    /** The QR pairing code until this phone is approved, then this phone's own key. */
    val token: String,
    val host: String,
    val port: Int,
    /** This phone's identity on that PC; stable across IP changes. */
    val deviceId: String = newDeviceId(),
    val paired: Boolean = false,
)

fun newDeviceId(): String = java.util.UUID.randomUUID().toString().replace("-", "").take(18)

/**
 * Every PC this phone has been paired with, kept until the user forgets it. Pairing is by QR once;
 * after that only the address is refreshed (via mDNS), never the pairing.
 */
class Store(ctx: Context) {
    private val prefs = ctx.getSharedPreferences("pr", Context.MODE_PRIVATE)

    init { migrate() }

    fun all(): List<Pc> {
        val a = JSONArray(prefs.getString("pcs", "[]"))
        return (0 until a.length()).map {
            val o = a.getJSONObject(it)
            Pc(
                o.getString("id"), o.getString("name"), o.getString("token"), o.getString("host"), o.getInt("port"),
                o.optString("deviceId").ifEmpty { newDeviceId() }, o.optBoolean("paired", false),
            )
        }
    }

    private fun save(list: List<Pc>) {
        val a = JSONArray()
        list.forEach { a.put(JSONObject().put("id", it.id).put("name", it.name).put("token", it.token).put("host", it.host).put("port", it.port).put("deviceId", it.deviceId).put("paired", it.paired)) }
        prefs.edit().putString("pcs", a.toString()).apply()
    }

    fun active(): Pc? {
        val id = prefs.getString("active", null)
        val l = all()
        return l.firstOrNull { it.id == id } ?: l.firstOrNull()
    }

    /** The PC owner approved this phone: keep the key it issued instead of the QR code. */
    fun markPaired(id: String, token: String, deviceId: String) {
        save(all().map { if (it.id == id) it.copy(token = token, deviceId = deviceId, paired = true) else it })
    }

    fun setActive(id: String) { prefs.edit().putString("active", id).apply() }

    /** Add a PC, or refresh an already-known one (same id), and make it the active one. */
    fun upsert(pc: Pc) {
        save(all().filter { it.id != pc.id } + pc)
        setActive(pc.id)
    }

    /** Only the address changed (DHCP, new router). Returns the updated PC if it was known. */
    fun updateAddress(id: String, host: String, port: Int): Pc? {
        val list = all()
        val pc = list.firstOrNull { it.id == id } ?: return null
        val updated = pc.copy(host = host, port = port)
        if (updated != pc) save(list.map { if (it.id == id) updated else it })
        return updated
    }

    /** A pairing made before PCs had ids: adopt the id when the sighting is at the saved address. */
    fun adoptLegacy(id: String, host: String, port: Int): Pc? {
        val list = all()
        val legacy = list.firstOrNull { it.id == "legacy" && it.host == host } ?: return null
        val adopted = legacy.copy(id = id, port = port)
        save(list.map { if (it === legacy) adopted else it })
        if (prefs.getString("active", null) == "legacy") setActive(id)
        return adopted
    }

    fun forget(id: String) {
        save(all().filter { it.id != id })
        if (prefs.getString("active", null) == id) prefs.edit().remove("active").apply()
    }

    /** Older builds kept a single token/host/port. */
    private fun migrate() {
        val token = prefs.getString("token", null) ?: return
        if (prefs.getString("pcs", null) == null) {
            upsert(Pc("legacy", "My PC", token, prefs.getString("host", "") ?: "", prefs.getInt("port", 8765)))
        }
        prefs.edit().remove("token").remove("host").remove("port").apply()
    }
}
