package com.hydrocore.app.domain

/**
 * Type-safe facade over the Rust hydration core. All functions are blocking
 * (disk and, for [sync], network I/O) and must be called off the main thread.
 * They never throw: problems are reported as [EngineResult.Failure].
 */
interface HydrationEngine {
    /** Opens the persisted state in [dataDir]; must be called once before anything else. */
    fun initialize(dataDir: String): EngineResult

    /** Returns the current state, applying the daily rollover if the date changed. */
    fun load(): EngineResult

    fun addWater(amountMl: Int): EngineResult

    /** Clears today's intake. */
    fun reset(): EngineResult

    fun saveSettings(settings: Settings, dailyGoalMl: Int): EngineResult

    /** Manual sync: a single HTTP POST to the configured server. */
    fun sync(): EngineResult

    /**
     * Compiles the full multi-week historical hydration archive into structured plain-text
     * blocks and writes `hydration_history.txt` atomically in the engine directory.
     */
    fun exportHistory(): EngineResult
}
