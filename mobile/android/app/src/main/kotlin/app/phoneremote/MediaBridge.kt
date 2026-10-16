package app.phoneremote

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/** The Flutter side of [MediaControlsService]: show/hide the controls, and pass taps back to Dart. */
class MediaBridge(private val activity: Activity, engine: FlutterEngine) : MethodChannel.MethodCallHandler {
    companion object {
        const val CHANNEL = "app.phoneremote/media"
        private const val REQUEST_NOTIFICATIONS = 4712
    }

    private val main = Handler(Looper.getMainLooper())
    private val channel = MethodChannel(engine.dartExecutor.binaryMessenger, CHANNEL)
    private var pendingPermission: MethodChannel.Result? = null

    init {
        channel.setMethodCallHandler(this)
        MediaControlsService.onCommand = { c, v ->
            main.post { channel.invokeMethod("command", mapOf("c" to c, "v" to v)) }
        }
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "show" -> {
                val args = call.arguments as? Map<*, *> ?: emptyMap<String, Any?>()
                val b = Bundle()
                for ((k, v) in args) {
                    val key = k as? String ?: continue
                    when (v) {
                        is String -> b.putString(key, v)
                        is Boolean -> b.putBoolean(key, v)
                        is Int -> b.putLong(key, v.toLong())
                        is Long -> b.putLong(key, v)
                    }
                }
                result.success(MediaControlsService.show(activity.applicationContext, b))
            }
            "hide" -> { MediaControlsService.hide(activity.applicationContext); result.success(null) }
            "requestPermission" -> requestPermission(result)
            "setCredentials" -> result.success(null) // iPhone only
            else -> result.notImplemented()
        }
    }

    /** Android 13+ asks before showing notifications. */
    private fun requestPermission(result: MethodChannel.Result) {
        if (Build.VERSION.SDK_INT < 33 ||
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        ) {
            result.success(true)
            return
        }
        pendingPermission?.success(false)
        pendingPermission = result
        activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
    }

    fun onPermissionResult(requestCode: Int, results: IntArray) {
        if (requestCode != REQUEST_NOTIFICATIONS) return
        pendingPermission?.success(results.isNotEmpty() && results[0] == PackageManager.PERMISSION_GRANTED)
        pendingPermission = null
    }

    fun dispose() {
        MediaControlsService.onCommand = null
        channel.setMethodCallHandler(null)
        // The Flutter engine (and the connection to the PC) ends with this activity: the controls would do nothing.
        MediaControlsService.hide(activity.applicationContext)
    }
}
