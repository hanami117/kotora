package io.github.hanami117.kotora.work

import android.content.Context
import android.util.Log
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import io.github.hanami117.kotora.KotoraApplication
import java.util.concurrent.TimeUnit

/**
 * Sends the waiting changes (the core's queue) when there is a network. Enqueued after every
 * edit; edits made while it runs are picked up by the next run (one unique chain). Offline or
 * rate limited: WorkManager tries again with backoff. The queue itself survives restarts.
 */
class FlushWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val repo = (applicationContext as KotoraApplication).repository ?: return Result.failure()
        if (repo.isSample) return Result.success()
        return try {
            val r = repo.flush()
            Log.i("kotora", "flush: sent ${r.sent}, conflicts ${r.conflicts}, failed ${r.failed}, waiting ${r.waiting}")
            if (r.waiting > 0u && !r.signInAgain && r.conflicts == 0u) Result.retry() else Result.success()
        } catch (e: Exception) {
            Log.w("kotora", "flush failed: ${e.message}")
            Result.retry()
        }
    }

    companion object {
        private const val NAME = "flush"

        fun enqueue(context: Context) {
            val req = OneTimeWorkRequestBuilder<FlushWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork(NAME, ExistingWorkPolicy.APPEND_OR_REPLACE, req)
        }
    }
}
