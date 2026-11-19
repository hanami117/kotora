package io.github.hanami117.kotora.notify

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.github.hanami117.kotora.MainActivity
import io.github.hanami117.kotora.R
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Note
import io.github.hanami117.kotora.core.NoteKind

/**
 * Kotora's notifications on Android: one channel per desktop switch (Started airing, New
 * episode, New chapter), so the system's own settings can mute each one too. A tap opens the
 * anime's or manga's page. The words are the core's (the desktop's).
 */
object Notifier {
    const val EXTRA_KIND = "kotora.open.kind"
    const val EXTRA_ID = "kotora.open.id"

    private const val CH_STARTED = "started_airing"
    private const val CH_EPISODE = "new_episode"
    private const val CH_CHAPTER = "new_chapter"

    fun ensureChannels(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        fun ch(id: String, name: String, about: String) =
            NotificationChannel(id, name, NotificationManager.IMPORTANCE_DEFAULT).apply { description = about }
        nm.createNotificationChannels(
            listOf(
                ch(CH_STARTED, "Started airing", "When a show on Plan to watch starts airing."),
                ch(CH_EPISODE, "New episode", "When the next episode of a show you're watching is out, by the estimated broadcast time."),
                ch(CH_CHAPTER, "New chapter", "When a manga you're reading gets a new chapter (MangaUpdates, MangaDex)."),
            ),
        )
    }

    /** Whether Kotora may post at all (Android 13+ asks; any version can turn it off). */
    fun allowed(context: Context): Boolean {
        if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return false
        return NotificationManagerCompat.from(context).areNotificationsEnabled()
    }

    fun show(context: Context, notes: List<Note>) {
        if (notes.isEmpty() || !allowed(context)) return
        ensureChannels(context)
        val nm = NotificationManagerCompat.from(context)
        for (n in notes) {
            val open = Intent(context, MainActivity::class.java)
                .putExtra(EXTRA_KIND, n.mediaKind.name)
                .putExtra(EXTRA_ID, n.mediaId.toLong())
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            val pending = PendingIntent.getActivity(context, n.id.hashCode(), open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            val channel = when (n.kind) {
                NoteKind.STARTED -> CH_STARTED
                NoteKind.EPISODE -> CH_EPISODE
                NoteKind.CHAPTER -> CH_CHAPTER
            }
            val b = NotificationCompat.Builder(context, channel)
                .setSmallIcon(R.drawable.ic_stat_kotora)
                .setColor(0xFFF28A22.toInt())
                .setContentTitle(n.summary)
                .setContentText(n.body)
                .setStyle(NotificationCompat.BigTextStyle().bigText(n.body))
                .setContentIntent(pending)
                .setAutoCancel(true)
                .setCategory(NotificationCompat.CATEGORY_RECOMMENDATION)
            try {
                // The desktop's id: the same one replaces the other.
                nm.notify(n.id, 1, b.build())
            } catch (_: SecurityException) {
                return
            }
        }
    }

    /** A notification's page: (kind, id) from the Activity's intent, if it came from one. */
    fun target(intent: Intent?): Pair<MediaKind, UInt>? {
        val kind = intent?.getStringExtra(EXTRA_KIND)?.let { k -> MediaKind.entries.find { it.name == k } } ?: return null
        val id = intent.getLongExtra(EXTRA_ID, -1L).takeIf { it > 0 } ?: return null
        return kind to id.toUInt()
    }
}
