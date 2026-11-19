package io.github.hanami117.kotora.work

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import io.github.hanami117.kotora.KotoraApplication
import io.github.hanami117.kotora.notify.Notifier
import java.util.concurrent.TimeUnit

/**
 * The desktop's airing-notes thread and Publishing timer on a phone: every 15 minutes (WorkManager's
 * shortest period; the desktop looks every minute, and an episode is announced up to two days
 * after its slot, so nothing is lost), and right after a list sync.
 *
 * - Started airing and New episode: the saved list only, no request (the core's `airnotes::pass`).
 * - New chapter (Manga and the switch on, with a network): MangaUpdates and MangaDex for what is
 *   older than their 4 hours.
 *
 * Each is announced once, whatever the switches say; the switches only decide what shows.
 *
 * It is also the app's one periodic background job for the list (no second scheduler): with a
 * network, the waiting changes go and the list is fetched again once the saved copy is an hour
 * old (`background_refresh` in kotora-mobile), before the notes look at it. The desktop fetches
 * the list each time it starts; a suspended phone app gets this instead. [FlushWorker] still
 * sends each edit at once.
 */
class NotifyWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val repo = (applicationContext as KotoraApplication).repository ?: return Result.success()
        if (repo.isSample) return Result.success()
        if (online(applicationContext)) {
            try {
                val r = repo.backgroundRefresh()
                Log.i("kotora", "background: sent ${r.flush.sent}, waiting ${r.flush.waiting}, list ${if (r.synced) "fetched" else "kept"}${r.problem?.let { " ($it)" } ?: ""}")
            } catch (e: Exception) {
                Log.w("kotora", "background refresh: ${e.message}")
            }
        }
        try {
            Notifier.show(applicationContext, repo.airNotes())
        } catch (e: Exception) {
            Log.w("kotora", "airing notifications: ${e.message}")
        }
        if (online(applicationContext)) {
            try {
                Notifier.show(applicationContext, repo.chapterNotes())
            } catch (e: Exception) {
                Log.w("kotora", "chapter notifications: ${e.message}")
            }
        }
        return Result.success()
    }

    companion object {
        private const val PERIODIC = "notes"
        private const val NOW = "notes-now"

        /** While signed in: the periodic look (kept if already there). */
        fun schedule(context: Context) {
            val req = PeriodicWorkRequestBuilder<NotifyWorker>(15, TimeUnit.MINUTES).build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(PERIODIC, ExistingPeriodicWorkPolicy.KEEP, req)
        }

        /** The saved list changed (a sync): look again now. */
        fun runNow(context: Context) {
            WorkManager.getInstance(context).enqueueUniqueWork(NOW, ExistingWorkPolicy.REPLACE, OneTimeWorkRequestBuilder<NotifyWorker>().build())
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(PERIODIC)
        }

        private fun online(context: Context): Boolean {
            val cm = context.getSystemService(ConnectivityManager::class.java) ?: return false
            val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
            return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
        }
    }
}
