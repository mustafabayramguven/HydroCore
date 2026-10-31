package com.hydrocore.app

import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.History
import com.hydrocore.app.domain.Midnight
import com.hydrocore.app.engine.EngineProtocol
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HistoryTest {

    // sampleState().date is 2026-10-31
    private val state = sampleState(
        intake = 600,
        goal = 2500,
        history = listOf( // newest first, as the core sends it
            historyDay("2026-10-30", 2600),
            historyDay("2026-10-28", 1000),
            historyDay("2026-10-20", 500), // older than 7 days
        ),
    )

    @Test
    fun lastDaysCoversSevenDaysEndingToday() {
        val bars = History.lastDays(state)
        assertEquals(7, bars.size)
        assertEquals(LocalDate.parse("2026-10-25"), bars.first().date)
        assertEquals(LocalDate.parse("2026-10-31"), bars.last().date)
        assertTrue(bars.last().isToday)
        assertTrue(bars.dropLast(1).none { it.isToday })
    }

    @Test
    fun lastDaysUsesLiveIntakeForTodayAndArchiveForPastDays() {
        val bars = History.lastDays(state).associateBy { it.date.toString() }
        assertEquals(600, bars.getValue("2026-10-31").intakeMl)
        assertEquals(2600, bars.getValue("2026-10-30").intakeMl)
        assertEquals(1000, bars.getValue("2026-10-28").intakeMl)
    }

    @Test
    fun daysWithoutRecordsAreZero() {
        val bars = History.lastDays(state).associateBy { it.date.toString() }
        assertEquals(0, bars.getValue("2026-10-29").intakeMl)
        assertEquals(0, bars.getValue("2026-10-25").intakeMl)
    }

    @Test
    fun lastDaysHandlesMonthBoundaries() {
        val s = sampleState().copy(date = "2026-03-02")
        val dates = History.lastDays(s, days = 4).map { it.date.toString() }
        assertEquals(listOf("2026-02-27", "2026-02-28", "2026-03-01", "2026-03-02"), dates)
    }

    @Test
    fun statsAverageOnlyCoversFinishedDaysOfLastWeek() {
        val stats = History.stats(state)
        assertEquals((2600 + 1000) / 2, stats.averageLast7Ml)
        assertEquals((2600 + 1000 + 500) / 3, stats.averageLast30Ml)
        assertEquals(500, stats.averageExtendedMl)
        assertEquals(1, stats.extendedDaysCount)
        assertEquals(4100L, stats.totalVolumeMl)
        assertEquals(1, stats.goalMetDays)
        assertEquals(3, stats.recordedDays)
    }

    @Test
    fun categorizeSplitsRecentSevenDaysAndExtendedHistory() {
        val categorized = History.categorize(state)
        assertEquals(listOf("2026-10-30", "2026-10-28"), categorized.recentLast7Days.map { it.date })
        assertEquals(listOf("2026-10-20"), categorized.extendedOlderThan7Days.map { it.date })
    }

    @Test
    fun formatPlainTextReportCompilesExtendedHistoryWithoutTruncation() {
        val report = History.formatPlainTextReport(state, "2026-10-31T22:00:00Z")
        assertTrue(report.contains("# HYDROCORE HISTORICAL HYDRATION REPORT"))
        assertTrue(report.contains("- Extended Archive (> 7 Days Ago): 1 days"))
        assertTrue(report.contains("## EXTENDED HISTORICAL ARCHIVE (> 7 DAYS AGO)"))
        assertTrue(report.contains("### 2026-10-20 (11 days ago)"))
        assertTrue(report.contains("## RECENT HISTORICAL ARCHIVE (LAST 7 DAYS)"))
        assertTrue(report.contains("### 2026-10-30 (1 days ago)"))
        assertTrue(report.contains("### 2026-10-28 (3 days ago)"))
        assertTrue(report.contains("## COMPLETE CHRONOLOGICAL LOG (OLDEST TO NEWEST)"))
    }

    @Test
    fun statsWithoutHistoryHaveNoAverage() {
        val stats = History.stats(sampleState())
        assertNull(stats.averageLast7Ml)
        assertEquals(0, stats.goalMetDays)
        assertEquals(0, stats.recordedDays)
    }

    @Test
    fun protocolParsesHistoryFromCore() {
        val raw = """{"ok":true,"state":{
            "date":"2026-11-01","intake_ml":0,"daily_goal_ml":2500,"progress":0.0,
            "remaining_ml":2500,"goal_reached":false,
            "history":[{"date":"2026-10-31","intake_ml":2600,"daily_goal_ml":2500,
              "goal_reached":true,
              "entries":[{"timestamp_utc":"2026-10-31T10:00:00Z","amount_ml":2600}]}]}}"""
        val result = EngineProtocol.parse(raw) as EngineResult.Success
        val day = result.state.history.single()
        assertEquals("2026-10-31", day.date)
        assertEquals(2600, day.intakeMl)
        assertTrue(day.goalReached)
        assertEquals(1, day.entries.size)
    }

    @Test
    fun protocolAcceptsStateWithoutHistoryField() {
        val raw = """{"ok":true,"state":{"date":"2026-10-31","intake_ml":0,
            "daily_goal_ml":2500,"progress":0.0,"remaining_ml":2500,"goal_reached":false}}"""
        val result = EngineProtocol.parse(raw) as EngineResult.Success
        assertTrue(result.state.history.isEmpty())
    }
}

class MidnightTest {

    private val istanbul = ZoneId.of("Europe/Istanbul")

    @Test
    fun countsDownToNextLocalMidnight() {
        val now = ZonedDateTime.of(2026, 10, 31, 23, 0, 0, 0, istanbul)
        assertEquals(60 * 60 * 1000L, Midnight.millisUntilNext(now))
    }

    @Test
    fun justAfterMidnightWaitsAlmostAFullDay() {
        val now = ZonedDateTime.of(2026, 11, 1, 0, 0, 1, 0, istanbul)
        assertEquals(24 * 60 * 60 * 1000L - 1000, Midnight.millisUntilNext(now))
    }

    @Test
    fun exactlyAtMidnightTargetsTheNextOne() {
        val now = ZonedDateTime.of(2026, 11, 1, 0, 0, 0, 0, istanbul)
        assertEquals(24 * 60 * 60 * 1000L, Midnight.millisUntilNext(now))
    }

    @Test
    fun isDstSafe() {
        // 2026-03-29 is a 23-hour day in Berlin (clocks go forward at 02:00).
        val berlin = ZoneId.of("Europe/Berlin")
        val now = ZonedDateTime.of(2026, 3, 29, 0, 0, 0, 0, berlin)
        assertEquals(23 * 60 * 60 * 1000L, Midnight.millisUntilNext(now))
        assertFalse(Midnight.millisUntilNext(now) <= 0)
    }
}
