package io.github.hanami117.kotora.ui

import io.github.hanami117.kotora.core.mangadexCredit
import io.github.hanami117.kotora.core.statsDateText
import io.github.hanami117.kotora.core.statsDays
import io.github.hanami117.kotora.core.statsDuration
import io.github.hanami117.kotora.core.statsGrouped
import io.github.hanami117.kotora.core.statsMonth
import io.github.hanami117.kotora.core.statsWeekday
import java.time.LocalDate

/**
 * The core's words for numbers and days (`kotora_core::stats`), with the same rules in Kotlin
 * when the native library isn't there (the screenshot tests run without it).
 */
object StatsText {
    private inline fun <T> core(f: () -> T, fallback: () -> T): T = try { f() } catch (_: Throwable) { fallback() }

    fun duration(minutes: ULong): String = core({ statsDuration(minutes) }) {
        val d = minutes / 1440u; val h = minutes % 1440u / 60u; val m = minutes % 60u
        when {
            d == 0uL && h == 0uL -> "$m min"
            d == 0uL && m == 0uL -> "$h h"
            d == 0uL -> "$h h $m min"
            h == 0uL -> "$d d"
            else -> "$d d $h h"
        }
    }

    fun grouped(n: ULong): String = core({ statsGrouped(n) }) { "%,d".format(java.util.Locale.ENGLISH, n.toLong()) }

    fun dateText(day: Long): String = core({ statsDateText(day) }) {
        val d = LocalDate.ofEpochDay(day)
        "${MONTH_NAMES[d.monthValue - 1]} ${d.dayOfMonth}, ${d.year}"
    }

    fun days(n: UInt): String = core({ statsDays(n) }) { if (n == 1u) "1 day" else "$n days" }

    fun month(day: Long): UInt = core({ statsMonth(day) }) { LocalDate.ofEpochDay(day).monthValue.toUInt() }

    /** Monday 0. */
    fun weekday(day: Long): Int = core({ statsWeekday(day).toInt() }) { LocalDate.ofEpochDay(day).dayOfWeek.value - 1 }

    fun mangadex(): String = core({ mangadexCredit() }) { "Volume data: MangaDex" }

    private val MONTH_NAMES = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
}
