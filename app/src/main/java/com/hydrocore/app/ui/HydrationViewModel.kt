package com.hydrocore.app.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.HydrationEngine
import com.hydrocore.app.domain.HydrationState
import com.hydrocore.app.domain.Settings
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
    /** One-shot message for a snackbar; cleared via [HydrationViewModel.messageShown]. */
    val message: String? = null,
)

/**
 * Every engine call runs on [io] (the Rust core does disk and network I/O). Nothing here
 * schedules background work: calls happen only in response to a user action or when the
 * screen is shown ([refresh]).
 */
class HydrationViewModel(
    private val engine: HydrationEngine,
    private val dataDir: String,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {

    private val _uiState = MutableStateFlow(HydrationUiState())
    val uiState: StateFlow<HydrationUiState> = _uiState.asStateFlow()

    /** Serializes local engine commands (init, add, reset, ...). Sync is deliberately outside. */
    private val commands = Mutex()

    init {
        viewModelScope.launch {
            commands.withLock {
                apply(withContext(io) { engine.initialize(dataDir) })
            }
        }
    }

    /** Re-reads state; applies the daily rollover if midnight passed while the app was away. */
    fun refresh() = runCommand { engine.load() }

    fun addWater(amountMl: Int) = runCommand { engine.addWater(amountMl) }

    fun reset() = runCommand { engine.reset() }

    fun openSettings() = _uiState.update { it.copy(settingsOpen = true) }

    fun closeSettings() = _uiState.update { it.copy(settingsOpen = false) }

    fun openHistory() = _uiState.update { it.copy(historyOpen = true) }

    fun closeHistory() = _uiState.update { it.copy(historyOpen = false) }

    fun saveSettings(settings: Settings, dailyGoalMl: Int) {
        viewModelScope.launch {
            val result = commands.withLock {
                withContext(io) { engine.saveSettings(settings, dailyGoalMl) }
            }
            apply(result)
            if (result is EngineResult.Success) closeSettings()
        }
    }

    fun messageShown() = _uiState.update { it.copy(message = null) }

    /** Manual sync, triggered only by the user tapping the button. */
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
