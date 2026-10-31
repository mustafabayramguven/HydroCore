package com.hydrocore.app.domain

import java.time.Duration
import java.time.LocalDate
import java.time.ZonedDateTime
import java.time.temporal.ChronoUnit

/** One bar of the "last days" chart. */
data class DayBar(
    val date: LocalDate,
    val intakeMl: Int,
    val goalMl: Int,
    val isToday: Boolean,
)

/** Summary numbers shown above the history list, including records > 7 days old. */
data class HistoryStats(
    /** Average over the finished days of the last 7 days that have data; null if none. */
    val averageLast7Ml: Int?,
    /** Average over the finished days of the last 30 days that have data; null if none. */
    val averageLast30Ml: Int? = null,
    /** Average over finished days strictly older than 7 days (> 7 days ago); null if none. */
    val averageExtendedMl: Int? = null,
    /** All-time average across all archived days; null if none. */
    val averageAllTimeMl: Int? = null,
    /** Count of archived days strictly older than 7 days (> 7 days ago). */
    val extendedDaysCount: Int = 0,
    /** Total archived water volume in ml across all recorded days. */
    val totalVolumeMl: Long = 0L,
    val goalMetDays: Int,
    val recordedDays: Int,
)

/** Categorized history splitting the recent 7-day window from the extended (> 7 days) archive. */
data class CategorizedHistory(
    val recentLast7Days: List<HistoryDay>,
    val extendedOlderThan7Days: List<HistoryDay>,
)

object History {

    /**
     * The last [days] calendar days ending today. Today uses the live intake; past days
     * come from the archive, and days without records show as zero.
     */
    fun lastDays(state: HydrationState, days: Int = 7): List<DayBar> {
        val today = LocalDate.parse(state.date)
        val byDate = state.history.associateBy { it.date }
        return (days - 1 downTo 0).map { back ->
            val date = today.minusDays(back.toLong())
            if (back == 0) {
                DayBar(date, state.intakeMl, state.dailyGoalMl, isToday = true)
            } else {
                val record = byDate[date.toString()]
                DayBar(
                    date = date,
                    intakeMl = record?.intakeMl ?: 0,
                    goalMl = record?.dailyGoalMl ?: state.dailyGoalMl,
                    isToday = false,
                )
            }
        }
    }

    /** Splits archived days into the recent 7-day window and records strictly older than 7 days. */
    fun categorize(state: HydrationState): CategorizedHistory {
        val today = LocalDate.parse(state.date)
        val cutoff7 = today.minusDays(7)
        val sorted = state.history.sortedByDescending { it.date }
        val recent = mutableListOf<HistoryDay>()
        val extended = mutableListOf<HistoryDay>()
        for (day in sorted) {
            val d = LocalDate.parse(day.date)
            if (d.isBefore(today) && !d.isBefore(cutoff7)) {
                recent += day
            } else if (d.isBefore(cutoff7)) {
                extended += day
            }
        }
        return CategorizedHistory(recentLast7Days = recent, extendedOlderThan7Days = extended)
    }

    fun stats(state: HydrationState): HistoryStats {
        val today = LocalDate.parse(state.date)
        val cutoff7 = today.minusDays(7)
        val cutoff30 = today.minusDays(30)

        val recent7 = state.history.filter {
            val d = LocalDate.parse(it.date)
            d.isBefore(today) && !d.isBefore(cutoff7)
        }
        val recent30 = state.history.filter {
            val d = LocalDate.parse(it.date)
            d.isBefore(today) && !d.isBefore(cutoff30)
        }
        val extended = state.history.filter {
            LocalDate.parse(it.date).isBefore(cutoff7)
        }
        val totalVolume = state.history.sumOf { it.intakeMl.toLong() }

        fun avg(list: List<HistoryDay>): Int? =
            if (list.isEmpty()) null else (list.sumOf { it.intakeMl.toLong() } / list.size).toInt()

        return HistoryStats(
            averageLast7Ml = avg(recent7),
            averageLast30Ml = avg(recent30),
            averageExtendedMl = avg(extended),
            averageAllTimeMl = avg(state.history),
            extendedDaysCount = extended.size,
            totalVolumeMl = totalVolume,
            goalMetDays = state.history.count { it.goalReached },
            recordedDays = state.history.size,
        )
    }

    /**
     * Compiles the full hydration history (specifically separating records > 7 days old,
     * the recent 7-day window, and the complete chronological log without truncation)
     * into structured plain-text blocks optimized for saving to a local `.txt` file.
     */
    fun formatPlainTextReport(
        state: HydrationState,
        generatedAtUtc: String = state.date,
    ): String = buildString {
        val today = LocalDate.parse(state.date)
        val categorized = categorize(state)
        val stats = stats(state)
        val user = state.settings.userId.ifBlank { "(not set)" }

        appendLine("# HYDROCORE HISTORICAL HYDRATION REPORT")
        appendLine("- Generated (UTC): $generatedAtUtc")
        appendLine("- Active Date: ${state.date}")
        appendLine("- User ID: $user")
        appendLine("- Active Daily Goal: ${state.dailyGoalMl} ml")
        appendLine()

        appendLine("## SUMMARY STATISTICS")
        appendLine("- Total Archived Days: ${stats.recordedDays}")
        appendLine("- Extended Archive (> 7 Days Ago): ${stats.extendedDaysCount} days")
        appendLine("- Recent Window (Last 7 Days): ${categorized.recentLast7Days.size} days")
        appendLine("- Total Archived Volume: ${stats.totalVolumeMl} ml")
        appendLine("- All-Time Daily Average: ${stats.averageAllTimeMl?.let { "$it ml" } ?: "N/A"}")
        appendLine("- Extended (> 7 Days) Daily Average: ${stats.averageExtendedMl?.let { "$it ml" } ?: "N/A"}")
        appendLine("- Last 30 Days Daily Average: ${stats.averageLast30Ml?.let { "$it ml" } ?: "N/A"}")
        appendLine("- Last 7 Days Daily Average: ${stats.averageLast7Ml?.let { "$it ml" } ?: "N/A"}")
        appendLine("- Goal Met: ${stats.goalMetDays} / ${stats.recordedDays} days")
        appendLine()

        val todayGoal = state.dailyGoalMl.coerceAtLeast(1)
        val todayPct = (state.intakeMl.toLong() * 100L) / todayGoal
        appendLine("## CURRENT DAY (LIVE / UNARCHIVED)")
        appendLine("- Date: ${state.date}")
        appendLine("- Intake: ${state.intakeMl} / ${state.dailyGoalMl} ml ($todayPct%)")
        if (state.goalReached) {
            appendLine("- Status: GOAL REACHED")
        } else {
            appendLine("- Status: ${state.remainingMl} ml remaining")
        }
        appendLine("- Entries (${state.entries.size}):")
        if (state.entries.isEmpty()) {
            appendLine("  * (no entries logged today)")
        } else {
            for (entry in state.entries) {
                appendLine("  * ${entry.timestampUtc} : +${entry.amountMl} ml")
            }
        }
        appendLine()

        fun appendSection(title: String, days: List<HistoryDay>) {
            appendLine("## $title")
            if (days.isEmpty()) {
                appendLine("- (no records in this timespan)")
                appendLine()
                return
            }
            for (day in days) {
                val age = ChronoUnit.DAYS.between(LocalDate.parse(day.date), today).coerceAtLeast(1)
                val goal = day.dailyGoalMl.coerceAtLeast(1)
                val pct = (day.intakeMl.toLong() * 100L) / goal
                appendLine("### ${day.date} ($age days ago)")
                appendLine("- Total Intake: ${day.intakeMl} ml")
                appendLine("- Daily Goal: ${day.dailyGoalMl} ml")
                if (day.goalReached) {
                    appendLine("- Goal Status: GOAL REACHED ($pct%)")
                } else {
                    appendLine("- Goal Status: SHORT BY ${day.dailyGoalMl - day.intakeMl} ml ($pct%)")
                }
                appendLine("- Logged Entries (${day.entries.size}):")
                for (entry in day.entries) {
                    appendLine("  * ${entry.timestampUtc} : +${entry.amountMl} ml")
                }
                appendLine()
            }
        }

        appendSection("EXTENDED HISTORICAL ARCHIVE (> 7 DAYS AGO)", categorized.extendedOlderThan7Days)
        appendSection("RECENT HISTORICAL ARCHIVE (LAST 7 DAYS)", categorized.recentLast7Days)

        appendLine("## COMPLETE CHRONOLOGICAL LOG (OLDEST TO NEWEST)")
        val chronological = state.history.sortedBy { it.date }
        if (chronological.isEmpty()) {
            appendLine("- (no archived days)")
        } else {
            for (day in chronological) {
                val status = if (day.goalReached) "GOAL REACHED" else "BELOW GOAL"
                appendLine(
                    "- ${day.date} : ${day.intakeMl} / ${day.dailyGoalMl} ml [$status] (${day.entries.size} entries)",
                )
            }
        }
    }
}

/** Used by the foreground-only midnight tick; no background scheduling is involved. */
object Midnight {
    /** Milliseconds from [now] to the next local midnight (DST-safe). */
    fun millisUntilNext(now: ZonedDateTime): Long {
        val next = now.toLocalDate().plusDays(1).atStartOfDay(now.zone)
        return Duration.between(now, next).toMillis().coerceAtLeast(1)
    }
}
