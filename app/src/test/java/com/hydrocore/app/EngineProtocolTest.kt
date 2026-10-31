package com.hydrocore.app

import com.hydrocore.app.domain.EngineResult
import com.hydrocore.app.domain.Settings
import com.hydrocore.app.engine.EngineProtocol
import com.hydrocore.app.engine.NativeHydrationEngine
import com.hydrocore.app.engine.Transport
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EngineProtocolTest {

    private val okResponse = """
        {"ok":true,"message":"Synced 1 entries","state":{
          "date":"2026-10-31","intake_ml":750,"daily_goal_ml":2500,"progress":0.3,
          "remaining_ml":1750,"goal_reached":false,
          "entries":[{"timestamp_utc":"2026-10-31T10:00:00Z","amount_ml":750}],
          "settings":{"user_id":"alice","server_url":"https://h","api_token":"t"},
          "last_sync_utc":"2026-10-31T11:00:00Z","future_field":123}}
    """.trimIndent()

    @Test
    fun parsesSuccessAndIgnoresUnknownFields() {
        val result = EngineProtocol.parse(okResponse) as EngineResult.Success
        assertEquals("Synced 1 entries", result.message)
        assertEquals(750, result.state.intakeMl)
        assertEquals(2500, result.state.dailyGoalMl)
        assertEquals("alice", result.state.settings.userId)
        assertEquals("2026-10-31T11:00:00Z", result.state.lastSyncUtc)
        assertEquals(750, result.state.entries.single().amountMl)
    }

    @Test
    fun parsesFailureWithState() {
        val raw = """{"ok":false,"error":"server responded with HTTP 500","state":{
            "date":"2026-10-31","intake_ml":0,"daily_goal_ml":2500,"progress":0.0,
            "remaining_ml":2500,"goal_reached":false}}"""
        val result = EngineProtocol.parse(raw) as EngineResult.Failure
        assertEquals("server responded with HTTP 500", result.error)
        assertEquals(2500, result.state?.dailyGoalMl)
    }

    @Test
    fun failureWithoutStateIsAllowed() {
        val result = EngineProtocol.parse("""{"ok":false,"error":"engine is not initialized"}""")
        assertTrue(result is EngineResult.Failure)
        assertEquals(null, result.state)
    }

    @Test
    fun nullAndMalformedResponsesBecomeFailures() {
        assertTrue(EngineProtocol.parse(null) is EngineResult.Failure)
        assertTrue(EngineProtocol.parse("garbage") is EngineResult.Failure)
        assertTrue(EngineProtocol.parse("""{"ok":true}""") is EngineResult.Failure)
        assertTrue(EngineProtocol.parse("""{"ok":false}""") is EngineResult.Failure)
    }

    @Test
    fun requestCarriesCommandAndClockValues() {
        val raw = EngineProtocol.request("add", FixedTime)
        val obj = Json.parseToJsonElement(raw).jsonObject
        assertEquals("add", obj.str("cmd"))
        assertEquals("2026-10-31", obj.str("today"))
        assertEquals("2026-10-31T10:00:00Z", obj.str("now_utc"))
    }
}

class NativeHydrationEngineTest {

    private class RecordingTransport(var reply: String?) : Transport {
        val requests = mutableListOf<JsonObject>()

        override fun dispatch(request: String): String? {
            requests += Json.parseToJsonElement(request).jsonObject
            return reply
        }
    }

    private val success = """{"ok":true,"state":{"date":"2026-10-31","intake_ml":250,
        "daily_goal_ml":2500,"progress":0.1,"remaining_ml":2250,"goal_reached":false}}"""

    @Test
    fun addWaterSendsAmount() {
        val transport = RecordingTransport(success)
        val result = NativeHydrationEngine(transport, FixedTime).addWater(250)
        assertTrue(result is EngineResult.Success)
        val request = transport.requests.single()
        assertEquals("add", request.str("cmd"))
        assertEquals(250, request["amount_ml"]!!.jsonPrimitive.int)
    }

    @Test
    fun saveSettingsSendsAllFields() {
        val transport = RecordingTransport(success)
        NativeHydrationEngine(transport, FixedTime)
            .saveSettings(Settings("alice", "https://h", "tok"), 3000)
        val request = transport.requests.single()
        assertEquals("save_settings", request.str("cmd"))
        assertEquals("alice", request.str("user_id"))
        assertEquals("https://h", request.str("server_url"))
        assertEquals("tok", request.str("api_token"))
        assertEquals(3000, request["daily_goal_ml"]!!.jsonPrimitive.int)
    }

    @Test
    fun simpleCommandsUseExpectedNames() {
        val transport = RecordingTransport(success)
        val engine = NativeHydrationEngine(transport, FixedTime)
        engine.initialize("/data")
        engine.load()
        engine.reset()
        engine.sync()
        assertEquals(
            listOf("init", "get_state", "reset", "sync"),
            transport.requests.map { it.str("cmd") },
        )
        assertEquals("/data", transport.requests.first().str("data_dir"))
    }

    @Test
    fun transportExceptionsBecomeFailuresInsteadOfCrashing() {
        val engine = NativeHydrationEngine(
            { throw UnsatisfiedLinkError("no libhydrocore") },
            FixedTime,
        )
        val result = engine.load()
        assertTrue(result is EngineResult.Failure)
        assertTrue((result as EngineResult.Failure).error.contains("no libhydrocore"))
    }

    @Test
    fun nullTransportResponseIsAFailure() {
        val result = NativeHydrationEngine(RecordingTransport(null), FixedTime).load()
        assertTrue(result is EngineResult.Failure)
    }
}

private fun JsonObject.str(key: String): String = this[key]!!.jsonPrimitive.content
