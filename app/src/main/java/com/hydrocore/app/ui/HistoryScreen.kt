package com.hydrocore.app.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.hydrocore.app.domain.DayBar
import com.hydrocore.app.domain.History
import com.hydrocore.app.domain.HistoryDay
import com.hydrocore.app.domain.HistoryStats
import com.hydrocore.app.domain.HydrationState
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

private const val CHART_HEIGHT_DP = 110f

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HistoryScreen(
    state: HydrationState,
    exportReport: String? = null,
    onBack: () -> Unit,
    onExport: () -> Unit = {},
    onCloseExport: () -> Unit = {},
) {
    val bars = remember(state) { History.lastDays(state) }
    val stats = remember(state) { History.stats(state) }
    val categorized = remember(state) { History.categorize(state) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("History") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    TextButton(onClick = onExport) {
                        Text("Export .txt")
                    }
                },
            )
        },
    ) { padding ->
        LazyColumn(
            modifier = Modifier.padding(padding),
            contentPadding = PaddingValues(horizontal = 24.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item { WeekChart(bars) }
            item { StatsRow(stats) }
            if (state.history.isEmpty()) {
                item {
                    Text(
                        text = "Past days appear here. Each day is saved automatically at midnight.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(vertical = 24.dp),
                    )
                }
            } else {
                if (categorized.recentLast7Days.isNotEmpty()) {
                    item { SectionHeader("Recent (last 7 days)") }
                    items(categorized.recentLast7Days, key = { it.date }) { day -> DayCard(day) }
                }
                if (categorized.extendedOlderThan7Days.isNotEmpty()) {
                    item {
                        SectionHeader(
                            "Extended archive (> 7 days ago · ${categorized.extendedOlderThan7Days.size} days)",
                        )
                    }
                    items(categorized.extendedOlderThan7Days, key = { it.date }) { day -> DayCard(day) }
                }
            }
        }
    }

    if (exportReport != null) {
        ExportHistoryDialog(
            date = state.date,
            report = exportReport,
            onDismiss = onCloseExport,
        )
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(top = 4.dp),
    )
}

@Composable
private fun WeekChart(bars: List<DayBar>) {
    val scaleMax = (bars.maxOfOrNull { maxOf(it.intakeMl, it.goalMl) } ?: 1).coerceAtLeast(1)
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Last 7 days", style = MaterialTheme.typography.titleMedium)
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.Bottom,
            ) {
                bars.forEach { bar ->
                    val fraction = bar.intakeMl.toFloat() / scaleMax
                    val color = when {
                        bar.intakeMl == 0 -> MaterialTheme.colorScheme.surfaceVariant
                        bar.intakeMl >= bar.goalMl -> MaterialTheme.colorScheme.primary
                        else -> MaterialTheme.colorScheme.secondary
                    }
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Box(
                            modifier = Modifier
                                .width(24.dp)
                                .height((CHART_HEIGHT_DP * fraction).coerceAtLeast(4f).dp)
                                .clip(RoundedCornerShape(6.dp))
                                .background(color),
                        )
                        Text(
                            text = if (bar.isToday) "Today" else weekdayFormat.format(bar.date),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            Text(
                text = "Filled bars reached the daily goal.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun StatsRow(stats: HistoryStats) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
            StatItem(label = "7-day avg", value = stats.averageLast7Ml?.let { "$it ml" } ?: "–")
            StatItem(label = "30-day avg", value = stats.averageLast30Ml?.let { "$it ml" } ?: "–")
            StatItem(
                label = ">7d avg",
                value = stats.averageExtendedMl?.let { "$it ml" } ?: "–",
            )
        }
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
            StatItem(label = "Goal met", value = "${stats.goalMetDays} / ${stats.recordedDays}")
            StatItem(label = ">7d records", value = "${stats.extendedDaysCount} days")
            StatItem(label = "Total saved", value = "${stats.recordedDays} days")
        }
    }
}

@Composable
private fun StatItem(label: String, value: String) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(value, style = MaterialTheme.typography.titleMedium)
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun DayCard(day: HistoryDay) {
    var expanded by rememberSaveable(day.date) { mutableStateOf(false) }
    val progress = if (day.dailyGoalMl > 0) {
        (day.intakeMl.toFloat() / day.dailyGoalMl).coerceIn(0f, 1f)
    } else {
        0f
    }
    Card(modifier = Modifier.fillMaxWidth().clickable { expanded = !expanded }) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(formatDay(day.date), style = MaterialTheme.typography.titleSmall)
                Text("${day.intakeMl} / ${day.dailyGoalMl} ml", style = MaterialTheme.typography.bodyMedium)
            }
            LinearProgressIndicator(
                progress = { progress },
                modifier = Modifier.fillMaxWidth(),
                strokeCap = StrokeCap.Round,
            )
            Text(
                text = if (day.goalReached) "Goal reached" else "${day.dailyGoalMl - day.intakeMl} ml short of goal",
                style = MaterialTheme.typography.bodySmall,
                color = if (day.goalReached) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
            if (expanded) {
                day.entries.forEach { entry ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(formatTime(entry.timestampUtc), style = MaterialTheme.typography.bodySmall)
                        Text("+${entry.amountMl} ml", style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
    }
}

@Composable
private fun ExportHistoryDialog(
    date: String,
    report: String,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    var statusText by rememberSaveable {
        mutableStateOf("Saved to app storage (hydration_history.txt). You can also save a .txt file anywhere on your phone.")
    }

    val createDocument = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.CreateDocument("text/plain"),
    ) { uri ->
        if (uri != null) {
            statusText = try {
                context.contentResolver.openOutputStream(uri)?.use { stream ->
                    stream.write(report.toByteArray(Charsets.UTF_8))
                }
                "Saved .txt file to device."
            } catch (e: Exception) {
                "Could not write file: ${e.message}"
            }
        }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Plain-Text History Export") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    text = statusText,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 160.dp, max = 340.dp)
                        .clip(RoundedCornerShape(8.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(12.dp)
                        .verticalScroll(rememberScrollState())
                        .horizontalScroll(rememberScrollState()),
                ) {
                    Text(
                        text = report,
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        },
        confirmButton = {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                TextButton(
                    onClick = {
                        clipboard.setText(AnnotatedString(report))
                        statusText = "Copied plain text to clipboard."
                    },
                ) {
                    Text("Copy")
                }
                TextButton(
                    onClick = { createDocument.launch("hydrocore-history-$date.txt") },
                ) {
                    Text("Save .txt")
                }
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Close") }
        },
    )
}

private val weekdayFormat: DateTimeFormatter =
    DateTimeFormatter.ofPattern("EEE", Locale.getDefault())

private val dayFormat: DateTimeFormatter =
    DateTimeFormatter.ofPattern("EEE, MMM d", Locale.getDefault())

private val timeFormat: DateTimeFormatter =
    DateTimeFormatter.ofPattern("HH:mm", Locale.getDefault())

private fun formatDay(iso: String): String =
    try {
        dayFormat.format(LocalDate.parse(iso))
    } catch (e: Exception) {
        iso
    }

private fun formatTime(iso: String): String =
    try {
        timeFormat.format(Instant.parse(iso).atZone(ZoneId.systemDefault()))
    } catch (e: Exception) {
        iso
    }
