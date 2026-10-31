package com.hydrocore.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DateRange
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.hydrocore.app.domain.HydrationState
import com.hydrocore.app.domain.Limits
import com.hydrocore.app.domain.QuickAdd
import com.hydrocore.app.domain.Settings
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

@Composable
fun HydrationScreen(
    ui: HydrationUiState,
    onAdd: (Int) -> Unit,
    onReset: () -> Unit,
    onSync: () -> Unit,
    onOpenSettings: () -> Unit,
    onCloseSettings: () -> Unit,
    onSaveSettings: (Settings, Int) -> Unit,
    onOpenHistory: () -> Unit,
    onCloseHistory: () -> Unit,
    onMessageShown: () -> Unit,
) {
    val state = ui.state
    if (ui.historyOpen && state != null) {
        // BackHandler uses the predictive back gesture (enableOnBackInvokedCallback).
        BackHandler(onBack = onCloseHistory)
        HistoryScreen(state = state, onBack = onCloseHistory)
    } else {
        TodayScreen(
            ui = ui,
            onAdd = onAdd,
            onReset = onReset,
            onSync = onSync,
            onOpenSettings = onOpenSettings,
            onCloseSettings = onCloseSettings,
            onSaveSettings = onSaveSettings,
            onOpenHistory = onOpenHistory,
            onMessageShown = onMessageShown,
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TodayScreen(
    ui: HydrationUiState,
    onAdd: (Int) -> Unit,
    onReset: () -> Unit,
    onSync: () -> Unit,
    onOpenSettings: () -> Unit,
    onCloseSettings: () -> Unit,
    onSaveSettings: (Settings, Int) -> Unit,
    onOpenHistory: () -> Unit,
    onMessageShown: () -> Unit,
) {
    val snackbarHost = remember { SnackbarHostState() }
    LaunchedEffect(ui.message) {
        ui.message?.let {
            snackbarHost.showSnackbar(it)
            onMessageShown()
        }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHost) },
        topBar = {
            CenterAlignedTopAppBar(
                title = { Text("HydroCore") },
                actions = {
                    IconButton(onClick = onOpenHistory) {
                        Icon(Icons.Filled.DateRange, contentDescription = "History")
                    }
                    IconButton(onClick = onOpenSettings) {
                        Icon(Icons.Filled.Settings, contentDescription = "Settings")
                    }
                },
            )
        },
    ) { padding ->
        val state = ui.state
        if (state == null) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else {
            HydrationContent(
                state = state,
                sync = ui.sync,
                onAdd = onAdd,
                onReset = onReset,
                onSync = onSync,
                modifier = Modifier.padding(padding),
            )
        }
    }

    val state = ui.state
    if (ui.settingsOpen && state != null) {
        SettingsDialog(
            settings = state.settings,
            dailyGoalMl = state.dailyGoalMl,
            onDismiss = onCloseSettings,
            onSave = onSaveSettings,
        )
    }
}

@Composable
private fun HydrationContent(
    state: HydrationState,
    sync: SyncStatus,
    onAdd: (Int) -> Unit,
    onReset: () -> Unit,
    onSync: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var confirmReset by rememberSaveable { mutableStateOf(false) }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        IntakeRing(state)
        GoalIndicator(state)

        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                QuickAdd.amountsMl.forEach { amount ->
                    FilledTonalButton(
                        onClick = { onAdd(amount) },
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("+$amount ml", maxLines = 1)
                    }
                }
            }
            OutlinedButton(
                onClick = { confirmReset = true },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Reset")
            }
        }

        SyncSection(state = state, sync = sync, onSync = onSync)
        Spacer(Modifier.height(16.dp))
    }

    if (confirmReset) {
        AlertDialog(
            onDismissRequest = { confirmReset = false },
            title = { Text("Reset today's intake?") },
            text = { Text("This clears everything you logged today. It cannot be undone.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmReset = false
                    onReset()
                }) { Text("Reset") }
            },
            dismissButton = {
                TextButton(onClick = { confirmReset = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun IntakeRing(state: HydrationState) {
    val progress by animateFloatAsState(targetValue = state.progress, label = "progress")
    Box(modifier = Modifier.size(248.dp), contentAlignment = Alignment.Center) {
        CircularProgressIndicator(
            progress = { progress },
            modifier = Modifier.fillMaxSize(),
            strokeWidth = 18.dp,
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
            strokeCap = StrokeCap.Round,
        )
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                text = "Today's intake",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = "${state.intakeMl} ml",
                style = MaterialTheme.typography.displayMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text = "${state.progressPercent}% of ${state.dailyGoalMl} ml",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun GoalIndicator(state: HydrationState) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        LinearProgressIndicator(
            progress = { state.progress },
            modifier = Modifier.fillMaxWidth(),
            strokeCap = StrokeCap.Round,
        )
        Text(
            text = if (state.goalReached) {
                "Daily goal reached (${state.dailyGoalMl} ml)"
            } else {
                "Daily goal: ${state.dailyGoalMl} ml · ${state.remainingMl} ml to go"
            },
            style = MaterialTheme.typography.bodyMedium,
            color = if (state.goalReached) {
                MaterialTheme.colorScheme.primary
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        )
    }
}

@Composable
private fun SyncSection(state: HydrationState, sync: SyncStatus, onSync: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Button(
            onClick = onSync,
            enabled = sync !is SyncStatus.InProgress,
            modifier = Modifier.fillMaxWidth(),
        ) {
            if (sync is SyncStatus.InProgress) {
                CircularProgressIndicator(
                    modifier = Modifier.size(18.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary,
                )
            } else {
                Icon(Icons.Filled.Refresh, contentDescription = null, modifier = Modifier.size(18.dp))
            }
            Spacer(Modifier.size(8.dp))
            Text("Manual Sync")
        }

        val (text, isError) = when (sync) {
            SyncStatus.Idle ->
                (state.lastSyncUtc?.let { "Last synced ${formatInstant(it)}" } ?: "Not synced yet") to false
            SyncStatus.InProgress -> "Syncing…" to false
            is SyncStatus.Success -> "${sync.message} · ${formatInstant(state.lastSyncUtc)}" to false
            is SyncStatus.Error -> sync.message to true
        }
        Text(
            text = text,
            textAlign = TextAlign.Center,
            style = MaterialTheme.typography.bodySmall,
            color = if (isError) {
                MaterialTheme.colorScheme.error
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        )
    }
}

@Composable
private fun SettingsDialog(
    settings: Settings,
    dailyGoalMl: Int,
    onDismiss: () -> Unit,
    onSave: (Settings, Int) -> Unit,
) {
    var userId by rememberSaveable { mutableStateOf(settings.userId) }
    var serverUrl by rememberSaveable { mutableStateOf(settings.serverUrl) }
    var apiToken by rememberSaveable { mutableStateOf(settings.apiToken) }
    var goalText by rememberSaveable { mutableStateOf(dailyGoalMl.toString()) }

    val goal = goalText.toIntOrNull()
    val goalValid = goal != null && goal in Limits.MIN_GOAL_ML..Limits.MAX_GOAL_ML
    val urlTrimmed = serverUrl.trim()
    val urlValid = urlTrimmed.isEmpty() ||
        urlTrimmed.startsWith("http://") || urlTrimmed.startsWith("https://")

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Settings") },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(
                    value = goalText,
                    onValueChange = { goalText = it.filter(Char::isDigit).take(5) },
                    label = { Text("Daily goal (ml)") },
                    isError = !goalValid,
                    supportingText = if (goalValid) null else {
                        { Text("${Limits.MIN_GOAL_ML}–${Limits.MAX_GOAL_ML} ml") }
                    },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = userId,
                    onValueChange = { userId = it },
                    label = { Text("User ID") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = serverUrl,
                    onValueChange = { serverUrl = it },
                    label = { Text("Server URL") },
                    placeholder = { Text("https://example.com/hydration") },
                    isError = !urlValid,
                    supportingText = if (urlValid) null else {
                        { Text("Must start with http:// or https://") }
                    },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = apiToken,
                    onValueChange = { apiToken = it },
                    label = { Text("API Token (optional)") },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(
                enabled = goalValid && urlValid,
                onClick = { onSave(Settings(userId, serverUrl, apiToken), goal ?: dailyGoalMl) },
            ) { Text("Save") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

private val syncTimeFormat: DateTimeFormatter =
    DateTimeFormatter.ofPattern("MMM d, HH:mm", Locale.getDefault())

private fun formatInstant(iso: String?): String =
    try {
        if (iso == null) "" else syncTimeFormat.format(Instant.parse(iso).atZone(ZoneId.systemDefault()))
    } catch (e: Exception) {
        iso.orEmpty()
    }
