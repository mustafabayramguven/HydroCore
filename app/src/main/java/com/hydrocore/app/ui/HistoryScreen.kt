package com.hydrocore.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
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
fun HistoryScreen(state: HydrationState, onBack: () -> Unit) {
    val bars = remember(state) { History.lastDays(state) }
    val stats = remember(state) { History.stats(state) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("History") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
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
                items(state.history, key = { it.date }) { day -> DayCard(day) }
            }
        }
    }
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
    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
        StatItem(label = "7-day average", value = stats.averageLast7Ml?.let { "$it ml" } ?: "–")
        StatItem(label = "Goal met", value = "${stats.goalMetDays} / ${stats.recordedDays}")
        StatItem(label = "Days saved", value = stats.recordedDays.toString())
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
