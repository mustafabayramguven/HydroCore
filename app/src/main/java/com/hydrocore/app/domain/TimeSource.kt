package com.hydrocore.app.domain

import java.time.Instant
import java.time.LocalDate

/**
 * The Rust core never reads the clock; it is handed these values with every request.
 * [today] is the device-local date (`YYYY-MM-DD`), [nowUtc] an RFC 3339 instant.
 */
interface TimeSource {
    fun today(): String

    fun nowUtc(): String
}

object SystemTimeSource : TimeSource {
    override fun today(): String = LocalDate.now().toString()

    override fun nowUtc(): String = Instant.now().toString()
}
