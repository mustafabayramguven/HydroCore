package com.hydrocore.app.domain

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** A single logged drink. Mirrors `Entry` in the Rust core. */
@Serializable
data class HydrationEntry(
    @SerialName("timestamp_utc") val timestampUtc: String,
    @SerialName("amount_ml") val amountMl: Int,
)

/** Sync configuration. The API token is optional. */
@Serializable
data class Settings(
    @SerialName("user_id") val userId: String = "",
    @SerialName("server_url") val serverUrl: String = "",
    @SerialName("api_token") val apiToken: String = "",
) {
    val isSyncConfigured: Boolean
        get() = userId.isNotBlank() && serverUrl.isNotBlank()
}

/** A finished day, archived by the core at midnight. */
@Serializable
data class HistoryDay(
    val date: String,
    @SerialName("intake_ml") val intakeMl: Int,
    @SerialName("daily_goal_ml") val dailyGoalMl: Int,
    @SerialName("goal_reached") val goalReached: Boolean,
    val entries: List<HydrationEntry> = emptyList(),
)

/** The Rust core's view of today. All derived values are computed by the core. */
@Serializable
data class HydrationState(
    val date: String,
    @SerialName("intake_ml") val intakeMl: Int,
    @SerialName("daily_goal_ml") val dailyGoalMl: Int,
    val progress: Float,
    @SerialName("remaining_ml") val remainingMl: Int,
    @SerialName("goal_reached") val goalReached: Boolean,
    val entries: List<HydrationEntry> = emptyList(),
    val settings: Settings = Settings(),
    @SerialName("last_sync_utc") val lastSyncUtc: String? = null,
    /** Finished days, newest first. */
    val history: List<HistoryDay> = emptyList(),
) {
    val progressPercent: Int
        get() = (progress * 100f).toInt().coerceIn(0, 100)
}

/** Outcome of any engine call. A failure may still carry the latest known state. */
sealed interface EngineResult {
    val state: HydrationState?

    data class Success(
        override val state: HydrationState,
        val message: String? = null,
    ) : EngineResult

    data class Failure(
        val error: String,
        override val state: HydrationState? = null,
    ) : EngineResult
}

/** Quick-add amounts offered by the UI. */
object QuickAdd {
    val amountsMl: List<Int> = listOf(200, 250, 500)
}

/** Limits shared with the Rust core, used for pre-validation in the UI. */
object Limits {
    const val MIN_GOAL_ML = 500
    const val MAX_GOAL_ML = 10_000
}
