package app.phoneremote

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    private var hid: HidBridge? = null
    private var media: MediaBridge? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        hid = HidBridge(this, flutterEngine)
        media = MediaBridge(this, flutterEngine)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        hid?.onPermissionResult(requestCode, grantResults)
        media?.onPermissionResult(requestCode, grantResults)
    }

    /**
     * Back on the home screen of the app: while the media controls are up, go to the background instead of closing,
     * so the controls (and the connection to the PC) keep working from the notification and the lock screen.
     */
    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (MediaControlsService.running) moveTaskToBack(true) else @Suppress("DEPRECATION") super.onBackPressed()
    }

    override fun onDestroy() {
        hid?.dispose()
        media?.dispose()
        super.onDestroy()
    }
}
