package com.hydrocore.app.engine

import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.HydrationEngine
import com.hydrocore.app.domain.Settings
import com.hydrocore.app.domain.SystemTimeSource
import com.hydrocore.app.domain.TimeSource
import kotlinx.serialization.json.put

/** [HydrationEngine] backed by the Rust core over JSON. */
class NativeHydrationEngine(
    // Lazy default: NativeTransport (and System.loadLibrary) is only touched when used.
    private val transport: Transport = Transport { NativeTransport.dispatch(it) },
    private val time: TimeSource = SystemTimeSource,
) : HydrationEngine {

    override fun initialize(dataDir: String): EngineResult =
        call("init") { put("data_dir", dataDir) }

    override fun load(): EngineResult = call("get_state")

    override fun addWater(amountMl: Int): EngineResult =
        call("add") { put("amount_ml", amountMl) }

    override fun reset(): EngineResult = call("reset")

    override fun saveSettings(settings: Settings, dailyGoalMl: Int): EngineResult =
        call("save_settings") {
            put("user_id", settings.userId)
            put("server_url", settings.serverUrl)
            put("api_token", settings.apiToken)
            put("daily_goal_ml", dailyGoalMl)
        }

    override fun sync(): EngineResult = call("sync")

    private fun call(
        cmd: String,
        extra: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit = {},
    ): EngineResult =
        try {
            EngineProtocol.parse(transport.dispatch(EngineProtocol.request(cmd, time, extra)))
        } catch (e: Throwable) {
            // UnsatisfiedLinkError etc. must not crash the UI.
            EngineResult.Failure("Native engine unavailable: ${e.message}")
        }
}
