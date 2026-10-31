package com.hydrocore.app.engine

import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.HydrationState
import com.hydrocore.app.domain.TimeSource
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** Wire format of the Rust core's response envelope. */
@Serializable
internal data class EngineResponse(
    val ok: Boolean,
    val error: String? = null,
    val message: String? = null,
    val state: HydrationState? = null,
)

/** JSON request building and response parsing; pure Kotlin so it is unit-testable on the JVM. */
internal object EngineProtocol {
    val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    fun request(
        cmd: String,
        time: TimeSource,
        extra: JsonObjectBuilder.() -> Unit = {},
    ): String =
        buildJsonObject {
            put("cmd", cmd)
            put("today", time.today())
            put("now_utc", time.nowUtc())
            extra()
        }.toString()

    fun parse(raw: String?): EngineResult {
        if (raw == null) {
            return EngineResult.Failure("Native engine returned no response")
        }
        val response = try {
            json.decodeFromString<EngineResponse>(raw)
        } catch (e: Exception) {
            return EngineResult.Failure("Malformed engine response: ${e.message}")
        }
        return when {
            response.ok && response.state != null ->
                EngineResult.Success(response.state, response.message)
            response.ok ->
                EngineResult.Failure("Engine response is missing state")
            else ->
                EngineResult.Failure(response.error ?: "Unknown engine error", response.state)
        }
    }
}
