package com.hydrocore.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.hydrocore.app.engine.NativeHydrationEngine
import com.hydrocore.app.ui.HydrationScreen
import com.hydrocore.app.ui.HydrationViewModel
import com.hydrocore.app.ui.theme.HydroCoreTheme

class MainActivity : ComponentActivity() {

    private val viewModel: HydrationViewModel by viewModels {
        viewModelFactory {
            initializer {
                HydrationViewModel(
                    engine = NativeHydrationEngine(),
                    dataDir = applicationContext.filesDir.absolutePath,
                )
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
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
                    onMessageShown = viewModel::messageShown,
                )
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Apply the daily rollover if the app stayed open across midnight.
        viewModel.refresh()
    }
}
