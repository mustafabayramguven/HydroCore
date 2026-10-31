package com.hydrocore.app

import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.HistoryDay
import com.hydrocore.app.domain.HydrationEntry
import com.hydrocore.app.domain.HydrationState
import com.hydrocore.app.domain.Settings
import com.hydrocore.app.domain.TimeSource

/** Fixed clock for deterministic tests. */
object FixedTime : TimeSource {
    override fun today(): String = "2026-10-31"

    override fun nowUtc(): String = "2026-10-31T10:00:00Z"
}

fun sampleState(
    intake: Int = 0,
    goal: Int = 2500,
    settings: Settings = Settings(),
    lastSync: String? = null,
    history: List<HistoryDay> = emptyList(),
): HydrationState =
    HydrationState(
        date = "2026-10-31",
        intakeMl = intake,
        dailyGoalMl = goal,
        progress = (intake.toFloat() / goal).coerceIn(0f, 1f),
        remainingMl = (goal - intake).coerceAtLeast(0),
        goalReached = intake >= goal,
        entries = if (intake > 0) listOf(HydrationEntry("2026-10-31T10:00:00Z", intake)) else emptyList(),
        settings = settings,
        lastSyncUtc = lastSync,
        history = history,
    )

fun historyDay(date: String, intake: Int, goal: Int = 2500): HistoryDay =
    HistoryDay(
        date = date,
        intakeMl = intake,
        dailyGoalMl = goal,
        goalReached = intake >= goal,
        entries = listOf(HydrationEntry("${date}T10:00:00Z", intake)),
    )

/** Scriptable [com.hydrocore.app.domain.HydrationEngine] that records calls. */
class FakeEngine(var next: EngineResult = EngineResult.Success(sampleState())) :
    com.hydrocore.app.domain.HydrationEngine {
    val calls = mutableListOf<String>()

    private fun record(call: String): EngineResult {
        calls += call
        return next
    }

    override fun initialize(dataDir: String) = record("init:$dataDir")

    override fun load() = record("load")

    override fun addWater(amountMl: Int) = record("add:$amountMl")

    override fun reset() = record("reset")

    override fun saveSettings(settings: Settings, dailyGoalMl: Int) =
        record("save:${settings.userId}:$dailyGoalMl")

    override fun sync() = record("sync")
}
