package com.hydrocore.app.sync

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkRequest
import com.hydrocore.app.domain.Midnight
import java.time.ZonedDateTime
import java.util.concurrent.TimeUnit

/**
 * Coordinates the nightly `00:00` local-time rollover & sync job and the network-constrained
 * retry job when the device is offline at midnight.
 */
interface SyncScheduler {
    /** Schedules (or refreshes) the next automatic `00:00` local-time rollover & sync job. */
    fun scheduleMidnightSync()

    /** Enqueues a network-constrained retry job when a sync fails or unsynced data is pending. */
    fun enqueuePendingSyncRetry()

    /** Cancels any pending offline-retry job once all data has been synced. */
    fun cancelPendingSyncRetry()

    object NoOp : SyncScheduler {
        override fun scheduleMidnightSync() = Unit
        override fun enqueuePendingSyncRetry() = Unit
        override fun cancelPendingSyncRetry() = Unit
    }
}

class WorkManagerSyncScheduler(context: Context) : SyncScheduler {
    private val appContext = context.applicationContext

    override fun scheduleMidnightSync() {
        // Schedule 1 second past local 00:00 so the date rollover has definitely crossed midnight.
        val delayMs = Midnight.millisUntilNext(ZonedDateTime.now()) + 1_000L
        val request = OneTimeWorkRequestBuilder<MidnightSyncWorker>()
            .setInitialDelay(delayMs, TimeUnit.MILLISECONDS)
            .setBackoffCriteria(
                BackoffPolicy.EXPONENTIAL,
                WorkRequest.MIN_BACKOFF_MILLIS,
                TimeUnit.MILLISECONDS,
            )
            .addTag(MIDNIGHT_WORK_NAME)
            .build()
        WorkManager.getInstance(appContext).enqueueUniqueWork(
            MIDNIGHT_WORK_NAME,
            ExistingWorkPolicy.REPLACE,
            request,
        )
    }

    override fun enqueuePendingSyncRetry() {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .build()
        val request = OneTimeWorkRequestBuilder<MidnightSyncWorker>()
            .setConstraints(constraints)
            .setBackoffCriteria(
                BackoffPolicy.EXPONENTIAL,
                WorkRequest.MIN_BACKOFF_MILLIS,
                TimeUnit.MILLISECONDS,
            )
            .addTag(CONNECTIVITY_RETRY_WORK_NAME)
            .build()
        WorkManager.getInstance(appContext).enqueueUniqueWork(
            CONNECTIVITY_RETRY_WORK_NAME,
            ExistingWorkPolicy.REPLACE,
            request,
        )
    }

    override fun cancelPendingSyncRetry() {
        WorkManager.getInstance(appContext).cancelUniqueWork(CONNECTIVITY_RETRY_WORK_NAME)
    }

    companion object {
        const val MIDNIGHT_WORK_NAME = "hydrocore_midnight_sync"
        const val CONNECTIVITY_RETRY_WORK_NAME = "hydrocore_connectivity_sync"
    }
}
