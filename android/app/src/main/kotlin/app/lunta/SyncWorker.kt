package app.lunta

import android.content.Context
import androidx.work.*
import java.util.concurrent.TimeUnit

/** Periodic background sync with the library's remote (Google Drive). */
class SyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val model = (applicationContext as LuntaApp).model
        return try {
            model.ensureOpen()
            model.syncNow()
            Result.success()
        } catch (e: Exception) {
            Result.retry()
        }
    }

    companion object {
        private const val NAME = "lunta-sync"

        fun schedule(context: Context, minutes: Int, enabled: Boolean) {
            val wm = WorkManager.getInstance(context)
            if (!enabled) { wm.cancelUniqueWork(NAME); return }
            val req = PeriodicWorkRequestBuilder<SyncWorker>(minutes.coerceAtLeast(15).toLong(), TimeUnit.MINUTES)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            wm.enqueueUniquePeriodicWork(NAME, ExistingPeriodicWorkPolicy.UPDATE, req)
        }

        fun cancel(context: Context) = WorkManager.getInstance(context).cancelUniqueWork(NAME)
    }
}
