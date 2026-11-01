package com.hydrocore.app.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.History
import com.hydrocore.app.domain.HydrationEngine
import com.hydrocore.app.domain.HydrationState
import com.hydrocore.app.domain.Settings
import com.hydrocore.app.sync.SyncScheduler
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/** Result of the last manual sync, shown under the sync button. */
sealed interface SyncStatus {
    data object Idle : SyncStatus

    data object InProgress : SyncStatus

    data class Success(val message: String) : SyncStatus

    data class Error(val message: String) : SyncStatus
}

data class HydrationUiState(
    val state: HydrationState? = null,
    val sync: SyncStatus = SyncStatus.Idle,
    val settingsOpen: Boolean = false,
    /** Whether the history ("watch back") screen is showing instead of today's screen. */
    val historyOpen: Boolean = false,
    /** Non-null when the plain-text history export sheet/dialog is open. */
    val exportReport: String? = null,
    /** One-shot message for a snackbar; cleared via [HydrationViewModel.messageShown]. */
    val message: String? = null,
)

/**
 * Every engine call runs on [io] (the Rust core does disk and network I/O). Coordinates
 * user actions, day-rollover syncs on app launch/resume/midnight, and WorkManager
 * scheduling for nightly 00:00 background sync and offline retry.
 */
class HydrationViewModel(
    private val engine: HydrationEngine,
    private val dataDir: String,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val syncScheduler: SyncScheduler = SyncScheduler.NoOp,
) : ViewModel() {

    private val _uiState = MutableStateFlow(HydrationUiState())
    val uiState: StateFlow<HydrationUiState> = _uiState.asStateFlow()

    /** Serializes local engine commands (init, add, reset, ...). Sync is deliberately outside. */
    private val commands = Mutex()

    init {
        syncScheduler.scheduleMidnightSync()
        viewModelScope.launch {
            val result = commands.withLock {
                val r = withContext(io) { engine.initialize(dataDir) }
                apply(r)
                r
            }
            val state = result.state
            if (state != null && state.settings.isSyncConfigured && state.hasUnsyncedHistory) {
                sync()
            }
        }
    }

    /**
     * Re-reads state; applies the daily rollover if midnight passed while the app was away
     * (or on the foreground 00:00 tick) and syncs any newly archived history day.
     */
    fun refresh() {
        viewModelScope.launch {
            val previousDate = _uiState.value.state?.date
            val result = commands.withLock {
                val r = withContext(io) { engine.load() }
                apply(r)
                r
            }
            syncScheduler.scheduleMidnightSync()
            val newState = result.state
            val rolledOver = previousDate != null && newState != null && newState.date > previousDate
            if (newState != null &&
                newState.settings.isSyncConfigured &&
                (rolledOver || newState.hasUnsyncedHistory)
            ) {
                sync()
            }
        }
    }

    fun addWater(amountMl: Int) = runCommand { engine.addWater(amountMl) }

    fun reset() = runCommand { engine.reset() }

    fun openSettings() = _uiState.update { it.copy(settingsOpen = true) }

    fun closeSettings() = _uiState.update { it.copy(settingsOpen = false) }

    fun openHistory() = _uiState.update { it.copy(historyOpen = true) }

    fun closeHistory() = _uiState.update { it.copy(historyOpen = false, exportReport = null) }

    fun exportHistory() {
        viewModelScope.launch {
            val result = commands.withLock {
                withContext(io) { engine.exportHistory() }
            }
            _uiState.update { current ->
                val updatedState = result.state ?: current.state
                when (result) {
                    is EngineResult.Success -> {
                        val report = result.report ?: History.formatPlainTextReport(result.state)
                        current.copy(state = updatedState, exportReport = report)
                    }
                    is EngineResult.Failure ->
                        current.copy(state = updatedState, message = result.error)
                }
            }
        }
    }

    fun closeExport() = _uiState.update { it.copy(exportReport = null) }

    fun saveSettings(settings: Settings, dailyGoalMl: Int) {
        viewModelScope.launch {
            val result = commands.withLock {
                withContext(io) { engine.saveSettings(settings, dailyGoalMl) }
            }
            apply(result)
            if (result is EngineResult.Success) {
                syncScheduler.scheduleMidnightSync()
                closeSettings()
            }
        }
    }

    /** Saves the current settings in the Settings dialog and triggers an immediate full sync. */
    fun saveAndSync(settings: Settings, dailyGoalMl: Int) {
        viewModelScope.launch {
            val result = commands.withLock {
                withContext(io) { engine.saveSettings(settings, dailyGoalMl) }
            }
            apply(result)
            if (result is EngineResult.Success) {
                syncScheduler.scheduleMidnightSync()
                sync()
            }
        }
    }

    fun messageShown() = _uiState.update { it.copy(message = null) }

    /** Full sync of active day and archived history to `POST /api/sync`. */
    fun sync() {
        val current = _uiState.value
        if (current.sync is SyncStatus.InProgress) return
        if (current.state?.settings?.isSyncConfigured != true) {
            _uiState.update {
                it.copy(sync = SyncStatus.Error("Set User ID and Server URL in Settings first"))
            }
            return
        }
        _uiState.update { it.copy(sync = SyncStatus.InProgress) }
        viewModelScope.launch {
            val result = withContext(io) { engine.sync() }
            when (result) {
                is EngineResult.Success -> {
                    syncScheduler.cancelPendingSyncRetry()
                    syncScheduler.scheduleMidnightSync()
                }
                is EngineResult.Failure -> {
                    syncScheduler.enqueuePendingSyncRetry()
                }
            }
            _uiState.update {
                it.copy(
                    state = result.state ?: it.state,
                    sync = when (result) {
                        is EngineResult.Success ->
                            SyncStatus.Success(result.message ?: "Synced")
                        is EngineResult.Failure -> SyncStatus.Error(result.error)
                    },
                )
            }
        }
    }

    private fun runCommand(call: () -> EngineResult) {
        viewModelScope.launch {
            commands.withLock { apply(withContext(io) { call() }) }
        }
    }

    private fun apply(result: EngineResult) {
        _uiState.update {
            it.copy(
                state = result.state ?: it.state,
                message = (result as? EngineResult.Failure)?.error ?: it.message,
            )
        }
    }
}
