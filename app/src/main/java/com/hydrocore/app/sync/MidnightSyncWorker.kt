package com.hydrocore.app.sync

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.HydrationEngine
import com.hydrocore.app.domain.Midnight
import com.hydrocore.app.engine.NativeHydrationEngine
import java.time.ZonedDateTime
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/**
 * Background worker that executes the nightly `00:00` local-time rollover and syncs the
 * completed day's snapshot (plus any pending history archive) to the companion server.
 *
 * If the device is offline at midnight, the completed day is still finalized and archived
 * atomically in `state.json`, and a network-constrained retry job is queued so the sync
 * runs automatically as soon as internet connectivity is restored.
 */
class MidnightSyncWorker(
    appContext: Context,
    params: WorkerParameters,
) : CoroutineWorker(appContext, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val scheduler = WorkManagerSyncScheduler(applicationContext)
        try {
            // If WorkManager woke the worker a few seconds before 00:00:00, wait until midnight
            // passes so the daily rollover archives the completed day before syncing.
            val untilMidnight = Midnight.millisUntilNext(ZonedDateTime.now())
            if (untilMidnight in 1L..5_000L) {
                delay(untilMidnight + 500L)
            }

            runMidnightRolloverAndSync(
                engine = NativeHydrationEngine(),
                dataDir = applicationContext.filesDir.absolutePath,
                scheduler = scheduler,
            )
        } finally {
            scheduler.scheduleMidnightSync()
        }
    }

    companion object {
        /**
         * Pure-logic entry point (unit-testable without an Android Context):
         * 1. Initializes/loads the engine so the completed day's snapshot is finalized into
         *    `history` before the counter resets for the new day.
         * 2. Syncs the finalized history + active day to `POST /api/sync`.
         * 3. Queues a connectivity-constrained retry if the sync fails while offline.
         */
        fun runMidnightRolloverAndSync(
            engine: HydrationEngine,
            dataDir: String,
            scheduler: SyncScheduler,
        ): Result {
            val initResult = engine.initialize(dataDir)
            val state = initResult.state ?: return Result.failure()

            if (!state.settings.isSyncConfigured) {
                return Result.success()
            }

            return when (engine.sync()) {
                is EngineResult.Success -> {
                    scheduler.cancelPendingSyncRetry()
                    Result.success()
                }
                is EngineResult.Failure -> {
                    scheduler.enqueuePendingSyncRetry()
                    Result.retry()
                }
            }
        }
    }
}
