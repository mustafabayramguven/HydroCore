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
}
