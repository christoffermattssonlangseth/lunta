package app.lunta.foryou

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
import androidx.work.*
import app.lunta.LuntaApp
import app.lunta.MainActivity
import app.lunta.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.concurrent.TimeUnit

/**
 * The day's New for You in the background: every few hours (with a network), if the papers are due
 * (past the morning time and not made yet today, by this phone or another device), the library is
 * synced (so foryou.json is the latest), the feed made without the on-device model (a phone in the
 * background has neither the memory nor the time for it), synced back, and the notification posted:
 * "New for You · 5 papers" with the digest's first sentence or the top titles, once per feed.
 * Tapping it opens New for You.
 */
class ForYouWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val model = (applicationContext as LuntaApp).model
        return try {
            model.ensureOpen()
            val fy = model.forYou.core() ?: return Result.success()
            val now = System.currentTimeMillis()
            if (withContext(Dispatchers.IO) { fy.isDue(now, ForYouModel.tz(now), true) }) {
                // another device may have made today's feed already
                model.syncNow()
                val t = System.currentTimeMillis()
                if (withContext(Dispatchers.IO) { fy.isDue(t, ForYouModel.tz(t), true) && !fy.isRunning() }) {
                    withContext(Dispatchers.IO) { fy.run(t, ForYouModel.tz(t), null, null) }
                    model.syncNow()
                }
            }
            // the journals' latest papers (the digest is written in the app, with the model)
            val h = model.forYou.highlightsCore()
            val t2 = System.currentTimeMillis()
            if (h != null && withContext(Dispatchers.IO) { h.isDue(t2, ForYouModel.tz(t2), true) }) {
                withContext(Dispatchers.IO) { runCatching { h.run(t2, ForYouModel.tz(t2)) } }
                model.syncNow()
            }
            notify(applicationContext, model.forYou)
            model.forYou.refresh()
            Result.success()
        } catch (e: Exception) {
            Result.retry()
        }
    }

    companion object {
        private const val NAME = "lunta-foryou"
        const val CHANNEL = "foryou"
        const val EXTRA_OPEN = "lunta.open"

        /** Every six hours, so a run comes soon after the morning time; it only runs when due. */
        fun schedule(context: Context) {
            val req = PeriodicWorkRequestBuilder<ForYouWorker>(6, TimeUnit.HOURS, 1, TimeUnit.HOURS)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(NAME, ExistingPeriodicWorkPolicy.KEEP, req)
        }

        fun cancel(context: Context) = WorkManager.getInstance(context).cancelUniqueWork(NAME)

        fun canNotify(context: Context): Boolean =
            (Build.VERSION.SDK_INT < 33 || ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) &&
                NotificationManagerCompat.from(context).areNotificationsEnabled()

        /** Posts the morning notification for the latest feed, if there is one not yet seen or announced. */
        suspend fun notify(context: Context, fy: ForYouModel) {
            val core = fy.core() ?: return
            val now = System.currentTimeMillis()
            val n = withContext(Dispatchers.IO) { core.morningNotice(now, ForYouModel.tz(now)) } ?: return
            if (!canNotify(context)) return
            val nm = context.getSystemService(NotificationManager::class.java)
            if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(NotificationChannel(CHANNEL, "New for You", NotificationManager.IMPORTANCE_DEFAULT).apply {
                    description = "The day's new papers for your research"
                })
            }
            val open = Intent(context, MainActivity::class.java).apply {
                putExtra(EXTRA_OPEN, "foryou")
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            }
            val pi = PendingIntent.getActivity(context, 7, open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
            val note = NotificationCompat.Builder(context, CHANNEL)
                .setSmallIcon(R.drawable.ic_stat_foryou)
                .setContentTitle(n.title)
                .setContentText(n.body)
                .setStyle(NotificationCompat.BigTextStyle().bigText(n.body))
                .setContentIntent(pi)
                .setAutoCancel(true)
                .build()
            try {
                NotificationManagerCompat.from(context).notify(CHANNEL, 1, note)
                withContext(Dispatchers.IO) { core.markNotified() }
            } catch (_: SecurityException) {
                // permission taken back meanwhile: the badge in the app still counts
            }
        }
    }
}
