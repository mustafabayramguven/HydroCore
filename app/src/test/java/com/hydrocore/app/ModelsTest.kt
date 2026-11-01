package com.hydrocore.app

import com.hydrocore.app.domain.Settings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelsTest {

    @Test
    fun syncIsConfiguredOnlyWithUserIdAndServerUrl() {
        assertFalse(Settings().isSyncConfigured)
        assertFalse(Settings(userId = "a").isSyncConfigured)
        assertFalse(Settings(serverUrl = "https://h").isSyncConfigured)
        assertFalse(Settings(userId = " ", serverUrl = "https://h").isSyncConfigured)
        assertTrue(Settings(userId = "a", serverUrl = "https://h").isSyncConfigured)
    }

    @Test
    fun apiTokenIsOptionalForSyncConfiguration() {
        assertTrue(Settings(userId = "a", serverUrl = "http://h", apiToken = "").isSyncConfigured)
    }

    @Test
    fun progressPercentIsClampedAndTruncated() {
        assertEquals(0, sampleState(intake = 0).progressPercent)
        assertEquals(50, sampleState(intake = 1250).progressPercent)
        assertEquals(100, sampleState(intake = 9000).progressPercent)
    }

    @Test
    fun serverUrlValidationAndNormalizationTargetApiSync() {
        assertTrue(Settings.isValidServerUrl("https://hydro.example.com"))
        assertTrue(Settings.isValidServerUrl("http://192.168.1.10:8080/"))
        assertTrue(Settings.isValidServerUrl("https://hydro.example.com/api/sync"))
        assertFalse(Settings.isValidServerUrl("ftp://hydro.example.com"))
        assertFalse(Settings.isValidServerUrl("https://"))
        assertFalse(Settings.isValidServerUrl("https:///api/sync"))

        assertEquals(
            "https://hydro.example.com/api/sync",
            Settings.normalizeServerUrl("https://hydro.example.com"),
        )
        assertEquals(
            "http://192.168.1.10:8080/api/sync",
            Settings.normalizeServerUrl("http://192.168.1.10:8080/"),
        )
        assertEquals(
            "https://hydro.example.com/api/sync",
            Settings.normalizeServerUrl("https://hydro.example.com/api"),
        )
        assertEquals(
            "https://hydro.example.com/api/sync",
            Settings.normalizeServerUrl("https://hydro.example.com/api/sync/"),
        )
    }
}
