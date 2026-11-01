package com.hydrocore.app

import androidx.work.ListenableWorker
import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.Settings
import com.hydrocore.app.sync.MidnightSyncWorker
import com.hydrocore.app.sync.SyncScheduler
import com.hydrocore.app.ui.HydrationViewModel
import com.hydrocore.app.ui.SyncStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class HydrationViewModelTest {

    private val dispatcher = UnconfinedTestDispatcher()
    private val configured = Settings("alice", "https://h.example/sync", "tok")

    @Before
    fun setUp() = Dispatchers.setMain(dispatcher)

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun viewModel(engine: FakeEngine) = HydrationViewModel(engine, "/data", dispatcher)

    @Test
    fun initializesEngineAndPublishesState() {
        val engine = FakeEngine(EngineResult.Success(sampleState(intake = 300)))
        val vm = viewModel(engine)
        assertEquals(listOf("init:/data"), engine.calls)
        assertEquals(300, vm.uiState.value.state?.intakeMl)
    }

    @Test
    fun quickAddAndResetDelegateToEngine() {
        val engine = FakeEngine()
        val vm = viewModel(engine)
        engine.next = EngineResult.Success(sampleState(intake = 250))
        vm.addWater(250)
        assertEquals(250, vm.uiState.value.state?.intakeMl)
        engine.next = EngineResult.Success(sampleState(intake = 0))
        vm.reset()
        assertEquals(0, vm.uiState.value.state?.intakeMl)
        assertEquals(listOf("init:/data", "add:250", "reset"), engine.calls)
    }

    @Test
    fun refreshReloadsState() {
        val engine = FakeEngine()
        val vm = viewModel(engine)
        engine.next = EngineResult.Success(sampleState(intake = 100))
        vm.refresh()
        assertEquals("load", engine.calls.last())
        assertEquals(100, vm.uiState.value.state?.intakeMl)
    }

    @Test
    fun failureKeepsPreviousStateAndSurfacesMessage() {
        val engine = FakeEngine(EngineResult.Success(sampleState(intake = 500)))
        val vm = viewModel(engine)
        engine.next = EngineResult.Failure("amount must be between 1 and 5000 ml")
        vm.addWater(99_999)
        assertEquals(500, vm.uiState.value.state?.intakeMl)
        assertEquals("amount must be between 1 and 5000 ml", vm.uiState.value.message)
        vm.messageShown()
        assertNull(vm.uiState.value.message)
    }

    @Test
    fun syncWithoutConfigurationNeverCallsEngine() {
        val engine = FakeEngine()
        val vm = viewModel(engine)
        vm.sync()
        assertFalse(engine.calls.contains("sync"))
        assertTrue(vm.uiState.value.sync is SyncStatus.Error)
    }

    @Test
    fun successfulSyncUpdatesStatusAndLastSync() {
        val engine = FakeEngine(EngineResult.Success(sampleState(settings = configured)))
        val vm = viewModel(engine)
        engine.next = EngineResult.Success(
            sampleState(intake = 250, settings = configured, lastSync = "2026-10-31T11:00:00Z"),
            message = "Synced 1 entries",
        )
        vm.sync()
        assertEquals(SyncStatus.Success("Synced 1 entries"), vm.uiState.value.sync)
        assertEquals("2026-10-31T11:00:00Z", vm.uiState.value.state?.lastSyncUtc)
        assertEquals(1, engine.calls.count { it == "sync" })
    }

    @Test
    fun failedSyncShowsErrorAndKeepsState() {
        val engine = FakeEngine(EngineResult.Success(sampleState(intake = 400, settings = configured)))
        val vm = viewModel(engine)
        engine.next = EngineResult.Failure(
            "server responded with HTTP 500",
            sampleState(intake = 400, settings = configured),
        )
        vm.sync()
        assertEquals(SyncStatus.Error("server responded with HTTP 500"), vm.uiState.value.sync)
        assertEquals(400, vm.uiState.value.state?.intakeMl)
    }

    @Test
    fun settingsDialogOpensAndClosesAndClosesAfterSuccessfulSave() {
        val engine = FakeEngine()
        val vm = viewModel(engine)
        vm.openSettings()
        assertTrue(vm.uiState.value.settingsOpen)
        vm.closeSettings()
        assertFalse(vm.uiState.value.settingsOpen)

        vm.openSettings()
        engine.next = EngineResult.Success(sampleState(goal = 3000, settings = configured))
        vm.saveSettings(configured, 3000)
        assertEquals("save:alice:3000", engine.calls.last())
        assertFalse(vm.uiState.value.settingsOpen)
        assertEquals(3000, vm.uiState.value.state?.dailyGoalMl)
    }

    @Test
    fun historyScreenOpensAndCloses() {
        val vm = viewModel(FakeEngine())
        assertFalse(vm.uiState.value.historyOpen)
        vm.openHistory()
        assertTrue(vm.uiState.value.historyOpen)
        vm.closeHistory()
        assertFalse(vm.uiState.value.historyOpen)
    }

    @Test
    fun refreshAfterMidnightPublishesArchivedDay() {
        val engine = FakeEngine(EngineResult.Success(sampleState(intake = 800)))
        val vm = viewModel(engine)
        engine.next = EngineResult.Success(
            sampleState(intake = 0).copy(
                date = "2026-11-01",
                history = listOf(historyDay("2026-10-31", 800)),
            ),
        )
        vm.refresh()
        val state = vm.uiState.value.state!!
        assertEquals(0, state.intakeMl)
        assertEquals("2026-10-31", state.history.single().date)
        assertEquals(800, state.history.single().intakeMl)
    }

    @Test
    fun exportHistoryPopulatesReportAndClosesCleanly() {
        val engine = FakeEngine(
            EngineResult.Success(
                sampleState(
                    intake = 400,
                    history = listOf(historyDay("2026-10-15", 2000)),
                ),
            ),
        )
        val vm = viewModel(engine)
        vm.exportHistory()
        assertEquals("export", engine.calls.last())
        val report = vm.uiState.value.exportReport!!
        assertTrue(report.contains("### 2026-10-15 (16 days ago)"))
        vm.closeExport()
        assertNull(vm.uiState.value.exportReport)
    }

    @Test
    fun invalidSettingsKeepDialogOpen() {
        val engine = FakeEngine()
        val vm = viewModel(engine)
        vm.openSettings()
        engine.next = EngineResult.Failure("server URL must start with http:// or https://")
        vm.saveSettings(Settings("a", "ftp://x", ""), 2000)
        assertTrue(vm.uiState.value.settingsOpen)
        assertEquals("server URL must start with http:// or https://", vm.uiState.value.message)
    }

    @Test
    fun refreshAfterMidnightAutoSyncsWhenConfiguredAndQueuesOfflineRetryOnFailure() {
        var midnightScheduled = 0
        var retryQueued = 0
        var retryCancelled = 0
        val scheduler = object : SyncScheduler {
            override fun scheduleMidnightSync() {
                midnightScheduled++
            }
            override fun enqueuePendingSyncRetry() {
                retryQueued++
            }
            override fun cancelPendingSyncRetry() {
                retryCancelled++
            }
        }
        val engine = FakeEngine(EngineResult.Success(sampleState(intake = 800, settings = configured)))
        val vm = HydrationViewModel(engine, "/data", dispatcher, scheduler)
        assertEquals(1, midnightScheduled)

        // Rollover into 2026-11-01 archives 2026-10-31 (unsynced) and triggers automatic sync.
        val rolledState = sampleState(intake = 0, settings = configured).copy(
            date = "2026-11-01",
            history = listOf(historyDay("2026-10-31", 800)),
        )
        engine.next = EngineResult.Success(rolledState, message = "Synced 1 entries (1 history days)")
        vm.refresh()
        assertEquals(listOf("init:/data", "load", "sync"), engine.calls)
        assertEquals(1, retryCancelled)

        // If sync fails (e.g. offline at midnight), a network-constrained retry is queued.
        engine.next = EngineResult.Failure("network error: offline", rolledState)
        vm.sync()
        assertEquals(1, retryQueued)
    }

    @Test
    fun midnightWorkerFinalizesDayAndSyncsOrRetriesWhenOffline() {
        var retryQueued = 0
        var retryCancelled = 0
        val scheduler = object : SyncScheduler {
            override fun scheduleMidnightSync() = Unit
            override fun enqueuePendingSyncRetry() {
                retryQueued++
            }
            override fun cancelPendingSyncRetry() {
                retryCancelled++
            }
        }
        val engine = FakeEngine(EngineResult.Success(sampleState(intake = 0, settings = configured)))
        val ok = MidnightSyncWorker.runMidnightRolloverAndSync(engine, "/data", scheduler)
        assertEquals(ListenableWorker.Result.success(), ok)
        assertEquals(listOf("init:/data", "sync"), engine.calls)
        assertEquals(1, retryCancelled)

        engine.calls.clear()
        engine.next = EngineResult.Failure("network error: offline", sampleState(settings = configured))
        // init still succeeds, sync fails:
        var step = 0
        val offlineEngine = object : com.hydrocore.app.domain.HydrationEngine by engine {
            override fun initialize(dataDir: String): EngineResult {
                step++
                return EngineResult.Success(sampleState(settings = configured))
            }
            override fun sync(): EngineResult {
                step++
                return EngineResult.Failure("network error: offline")
            }
        }
        val retry = MidnightSyncWorker.runMidnightRolloverAndSync(offlineEngine, "/data", scheduler)
        assertEquals(ListenableWorker.Result.retry(), retry)
        assertEquals(2, step)
        assertEquals(1, retryQueued)
    }
}
