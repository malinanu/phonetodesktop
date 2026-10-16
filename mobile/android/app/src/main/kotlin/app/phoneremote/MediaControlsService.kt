package app.phoneremote

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.SystemClock

/**
 * The media notification: play/pause, previous and next for the PC, in the notification shade and on the lock screen,
 * with the song the PC is playing. It is a foreground service ("connected device"), so Android keeps the app, and with
 * it the connection to the PC, running while the phone is locked. The Flutter side decides when it shows and what it
 * says ([MediaBridge]); taps go back to Flutter through [onCommand].
 */
class MediaControlsService : Service() {
    companion object {
        private const val CHANNEL_ID = "media_controls"
        private const val NOTIFICATION_ID = 7301
        private const val ACTION_SHOW = "app.phoneremote.media.SHOW"
        private const val ACTION_CMD = "app.phoneremote.media.CMD"
        private const val EXTRA_CMD = "c"

        /** Where taps go: set by [MediaBridge] while the Flutter engine is alive. */
        @Volatile var onCommand: ((String, Long?) -> Unit)? = null

        @Volatile private var instance: MediaControlsService? = null
        val running: Boolean get() = instance != null

        /** Shows or refreshes the controls. False when Android does not allow starting them right now. */
        fun show(context: Context, data: Bundle): Boolean {
            instance?.let { it.update(data); return true }
            val intent = Intent(context, MediaControlsService::class.java).setAction(ACTION_SHOW).putExtras(data)
            return try {
                context.startForegroundService(intent)
                true
            } catch (e: Exception) {
                // Android 12+ refuses to start a foreground service while the app is in the background; the next
                // update after the user opens the app starts it.
                false
            }
        }

        fun hide(context: Context) {
            instance?.stopSelf() ?: context.stopService(Intent(context, MediaControlsService::class.java))
        }
    }

    private lateinit var session: MediaSession
    private var data = Bundle()

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createChannel()
        session = MediaSession(this, "Lanyard").apply {
            setCallback(object : MediaSession.Callback() {
                override fun onPlay() = send("play_pause")
                override fun onPause() = send("play_pause")
                override fun onSkipToNext() = send("next")
                override fun onSkipToPrevious() = send("prev")
                override fun onFastForward() = send("seek_fwd")
                override fun onRewind() = send("seek_back")
                override fun onSeekTo(pos: Long) = send("seek_to", pos)
            })
            setSessionActivity(openApp())
            isActive = true
        }
        instance = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_SHOW -> update(intent.extras ?: Bundle())
            ACTION_CMD -> {
                intent.getStringExtra(EXTRA_CMD)?.let { send(it) }
                // A tap that arrived after the controls were taken down: nothing to keep running.
                if (data.isEmpty) stopSelf()
            }
            // Restarted by the system without data (should not happen with START_NOT_STICKY): nothing to show.
            else -> { startInForeground(build()); stopSelf() }
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        instance = null
        session.isActive = false
        session.release()
        stopForeground(STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    private fun send(command: String, value: Long? = null) {
        onCommand?.invoke(command, value)
    }

    fun update(next: Bundle) {
        data = next
        val connected = data.getBoolean("connected", true)
        val playing = data.getBoolean("playing", false)
        val canSeek = data.getBoolean("canSeek", false)
        val durMs = data.getLong("durMs", 0)
        val posMs = data.getLong("posMs", 0)
        val pc = data.getString("pc") ?: ""
        val app = data.getString("app") ?: ""

        session.setMetadata(
            MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, data.getString("title") ?: "Lanyard")
                .putString(MediaMetadata.METADATA_KEY_ARTIST, data.getString("artist") ?: "")
                .putString(MediaMetadata.METADATA_KEY_ALBUM, listOf(app, pc).filter { it.isNotEmpty() }.joinToString(" · "))
                .putLong(MediaMetadata.METADATA_KEY_DURATION, if (canSeek) durMs else -1)
                .putBitmap(MediaMetadata.METADATA_KEY_DISPLAY_ICON, BitmapFactory.decodeResource(resources, R.mipmap.ic_launcher))
                .build(),
        )
        var actions = PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
            PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS
        if (canSeek) actions = actions or PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_FAST_FORWARD or PlaybackState.ACTION_REWIND
        val state = when {
            !connected -> PlaybackState.STATE_CONNECTING
            playing -> PlaybackState.STATE_PLAYING
            else -> PlaybackState.STATE_PAUSED
        }
        session.setPlaybackState(
            PlaybackState.Builder()
                .setActions(actions)
                .setState(state, if (canSeek) posMs else PlaybackState.PLAYBACK_POSITION_UNKNOWN, if (playing) 1f else 0f, SystemClock.elapsedRealtime())
                .build(),
        )
        startInForeground(build())
    }

    private fun startInForeground(n: Notification) {
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(NOTIFICATION_ID, n)
        }
    }

    private fun build(): Notification {
        val playing = data.getBoolean("playing", false)
        val bluetooth = data.getString("mode") == "bluetooth"
        val pc = data.getString("pc") ?: ""
        val app = data.getString("app") ?: ""
        val artist = data.getString("artist") ?: ""
        val text = when {
            bluetooth -> "Bluetooth · $pc"
            artist.isNotEmpty() -> artist
            else -> listOf(app, pc).filter { it.isNotEmpty() }.joinToString(" · ")
        }
        val playIcon = when {
            bluetooth -> R.drawable.ic_media_play_pause
            playing -> R.drawable.ic_media_pause
            else -> R.drawable.ic_media_play
        }
        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_lanyard)
            .setContentTitle(data.getString("title") ?: "Lanyard")
            .setContentText(text)
            .setSubText(if (bluetooth) null else pc)
            .setLargeIcon(BitmapFactory.decodeResource(resources, R.mipmap.ic_launcher))
            .setContentIntent(openApp())
            .setOngoing(true)
            .setShowWhen(false)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .setColor(0xFFFF9A3C.toInt())
            .addAction(action(R.drawable.ic_media_prev, "Previous", "prev", 1))
            .addAction(action(playIcon, if (playing) "Pause" else "Play", "play_pause", 2))
            .addAction(action(R.drawable.ic_media_next, "Next", "next", 3))
            .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken).setShowActionsInCompactView(0, 1, 2))
            .build()
    }

    private fun action(icon: Int, title: String, command: String, requestCode: Int): Notification.Action {
        val intent = Intent(this, MediaControlsService::class.java).setAction(ACTION_CMD).putExtra(EXTRA_CMD, command)
        val pending = PendingIntent.getService(this, requestCode, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        return Notification.Action.Builder(Icon.createWithResource(this, icon), title, pending).build()
    }

    private fun openApp(): PendingIntent {
        val intent = Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
        return PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    private fun createChannel() {
        val manager = getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL_ID) != null) return
        val channel = NotificationChannel(CHANNEL_ID, "Media controls", NotificationManager.IMPORTANCE_LOW).apply {
            description = "Play, pause and skip on your PC from the notification shade and the lock screen."
            setShowBadge(false)
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
        }
        manager.createNotificationChannel(channel)
    }
}
