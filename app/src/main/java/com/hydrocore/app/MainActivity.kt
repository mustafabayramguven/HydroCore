package com.hydrocore.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.getValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.hydrocore.app.domain.Midnight
import com.hydrocore.app.engine.NativeHydrationEngine
import com.hydrocore.app.sync.WorkManagerSyncScheduler
import com.hydrocore.app.ui.HydrationScreen
import com.hydrocore.app.ui.HydrationViewModel
import com.hydrocore.app.ui.theme.HydroCoreTheme
import java.time.ZonedDateTime
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    private val viewModel: HydrationViewModel by viewModels {
        viewModelFactory {
            initializer {
                HydrationViewModel(
                    engine = NativeHydrationEngine(),
                    dataDir = applicationContext.filesDir.absolutePath,
                    syncScheduler = WorkManagerSyncScheduler(applicationContext),
                )
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        // Foreground midnight tick: while the screen is visible, archive the finished day
        // and sync at 00:00 (complementing WorkManager's background midnight job).
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                while (true) {
                    delay(Midnight.millisUntilNext(ZonedDateTime.now()) + 500)
                    viewModel.refresh()
                }
            }
        }

        setContent {
            HydroCoreTheme {
                val ui by viewModel.uiState.collectAsStateWithLifecycle()
                HydrationScreen(
                    ui = ui,
                    onAdd = viewModel::addWater,
                    onReset = viewModel::reset,
                    onSync = viewModel::sync,
                    onOpenSettings = viewModel::openSettings,
                    onCloseSettings = viewModel::closeSettings,
                    onSaveSettings = viewModel::saveSettings,
                    onSyncFromSettings = viewModel::saveAndSync,
                    onOpenHistory = viewModel::openHistory,
                    onCloseHistory = viewModel::closeHistory,
                    onExportHistory = viewModel::exportHistory,
                    onCloseExport = viewModel::closeExport,
                    onMessageShown = viewModel::messageShown,
                )
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Apply the daily rollover if midnight passed while the app was away.
        viewModel.refresh()
    }
}
