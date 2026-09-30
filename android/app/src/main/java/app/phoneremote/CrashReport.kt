package app.phoneremote

import android.app.Activity
import android.app.Application
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Typeface
import android.os.Bundle
import android.util.TypedValue
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast

/** Turns a crash into a readable screen, so a failure on a phone can be reported without a computer. */
class PhoneRemoteApp : Application() {
    override fun onCreate() {
        super.onCreate()
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            try {
                val trace = "Phone Remote crashed on ${android.os.Build.MODEL} (Android ${android.os.Build.VERSION.RELEASE})\n\n" + e.stackTraceToString().take(6000)
                startActivity(Intent(this, CrashActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("trace", trace))
            } catch (_: Throwable) {
            }
            previous?.uncaughtException(t, e)
        }
    }
}

class CrashActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val trace = intent.getStringExtra("trace") ?: "No details"
        val pad = (16 * resources.displayMetrics.density).toInt()
        val col = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(pad, pad * 2, pad, pad) }
        col.addView(TextView(this).apply { text = "Something went wrong"; setTextSize(TypedValue.COMPLEX_UNIT_SP, 20f); typeface = Typeface.DEFAULT_BOLD })
        col.addView(TextView(this).apply { text = "Tap Copy and send this text to fix it."; setPadding(0, pad / 2, 0, pad / 2) })
        col.addView(ScrollView(this).apply { addView(TextView(this@CrashActivity).apply { text = trace; typeface = Typeface.MONOSPACE; setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f); setTextIsSelectable(true) }) }, LinearLayout.LayoutParams(-1, 0, 1f))
        col.addView(Button(this).apply {
            text = "Copy"
            setOnClickListener {
                (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("crash", trace))
                Toast.makeText(this@CrashActivity, "Copied", Toast.LENGTH_SHORT).show()
            }
        })
        setContentView(col)
    }
}
