package com.hydrocore.app.domain

import java.time.Duration
import java.time.LocalDate
import java.time.ZonedDateTime

/** One bar of the "last days" chart. */
data class DayBar(
    val date: LocalDate,
    val intakeMl: Int,
    val goalMl: Int,
    val isToday: Boolean,
)

/** Summary numbers shown above the history list. */
data class HistoryStats(
    /** Average over the finished days of the last 7 days that have data; null if none. */
    val averageLast7Ml: Int?,
    val goalMetDays: Int,
    val recordedDays: Int,
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

    fun stats(state: HydrationState): HistoryStats {
        val today = LocalDate.parse(state.date)
        val recent = state.history.filter {
            val d = LocalDate.parse(it.date)
            d.isBefore(today) && !d.isBefore(today.minusDays(7))
        }
        return HistoryStats(
            averageLast7Ml = if (recent.isEmpty()) null else recent.sumOf { it.intakeMl } / recent.size,
            goalMetDays = state.history.count { it.goalReached },
            recordedDays = state.history.size,
        )
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
