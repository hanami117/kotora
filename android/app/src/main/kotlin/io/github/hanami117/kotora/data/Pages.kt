package io.github.hanami117.kotora.data

import io.github.hanami117.kotora.core.AiringCard
import io.github.hanami117.kotora.core.AiringDay
import io.github.hanami117.kotora.core.AiringMine
import io.github.hanami117.kotora.core.AiringOrder
import io.github.hanami117.kotora.core.AiringPage
import io.github.hanami117.kotora.core.AiringSlot
import io.github.hanami117.kotora.core.AiringType
import io.github.hanami117.kotora.core.AiringUntimed
import io.github.hanami117.kotora.core.Kotora
import io.github.hanami117.kotora.core.KotoraSettings
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.NamedCount
import io.github.hanami117.kotora.core.Notice
import io.github.hanami117.kotora.core.NoticeKind
import io.github.hanami117.kotora.core.NotifySettings
import io.github.hanami117.kotora.core.PublishingLook
import io.github.hanami117.kotora.core.PublishingPage
import io.github.hanami117.kotora.core.PublishingRelease
import io.github.hanami117.kotora.core.PublishingReport
import io.github.hanami117.kotora.core.PublishingRow
import io.github.hanami117.kotora.core.PublishingSort
import io.github.hanami117.kotora.core.ReleaseSources
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.SeasonGrid
import io.github.hanami117.kotora.core.SeasonPick
import io.github.hanami117.kotora.core.Setting
import io.github.hanami117.kotora.core.ShownTitle
import io.github.hanami117.kotora.core.StatsData
import io.github.hanami117.kotora.core.StatsPeriod
import io.github.hanami117.kotora.core.StatsTitle
import io.github.hanami117.kotora.core.VolumeFill
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.TextStyle
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The pages iOS has too, over `kotora-mobile`'s `pages.rs`, `insights.rs` (shared with iOS),
 * `seasons.rs` and `prefs.rs`: Airing (week + season grid + picker), Publishing, the
 * notifications, Statistics, the desktop's settings in `config.toml`, Fill volumes.
 * Blocking calls go to [Dispatchers.IO] here. Errors: [io.github.hanami117.kotora.core.KotoraException].
 */
interface PagesSource {
    suspend fun airingPage(type: AiringType, mine: AiringMine, order: AiringOrder): AiringPage
    /** Network. */
    suspend fun airingRefresh(force: Boolean)
    fun seasonPicks(back: UInt): List<SeasonPick>
    suspend fun seasonGrid(offset: Int, type: AiringType, mine: AiringMine, order: AiringOrder): SeasonGrid
    /** Network. */
    suspend fun seasonFetch(offset: Int, force: Boolean)

    suspend fun publishingPage(sort: PublishingSort): PublishingPage
    /** Network, at the release sources' pace. */
    suspend fun publishingRefresh(force: Boolean): PublishingReport
    /** Network. */
    suspend fun publishingLink(id: UInt, link: String)
    suspend fun publishingReset(id: UInt)

    /** Started airing and New episode due now (under the switches). Local. */
    suspend fun noticesDue(): List<Notice>
    suspend fun notifySettings(): NotifySettings
    suspend fun setNotify(kind: NoticeKind, on: Boolean)

    suspend fun stats(kind: MediaKind): StatsData
    suspend fun settings(): KotoraSettings
    suspend fun setSetting(setting: Setting): KotoraSettings
    suspend fun titleLanguage(): String
    suspend fun setTitleLanguage(value: String)
    /** The list's titles in the chosen language; empty for Romaji. */
    suspend fun shownTitles(kind: MediaKind): List<ShownTitle>
    suspend fun releaseSources(): ReleaseSources
    suspend fun setReleaseSources(sources: ReleaseSources)
    /** `null`: not available (sample data). */
    suspend fun volumeFill(): VolumeFill?
}

class CorePages(private val k: Kotora) : PagesSource {
    private suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    override suspend fun airingPage(type: AiringType, mine: AiringMine, order: AiringOrder) = io { k.airingPage(type, mine, order) }
    override suspend fun airingRefresh(force: Boolean) = io { k.airingRefresh(force) }
    override fun seasonPicks(back: UInt) = k.seasonPicks(back)
    override suspend fun seasonGrid(offset: Int, type: AiringType, mine: AiringMine, order: AiringOrder) = io { k.seasonGrid(offset, type, mine, order) }
    override suspend fun seasonFetch(offset: Int, force: Boolean) = io { k.seasonFetch(offset, force) }
    override suspend fun publishingPage(sort: PublishingSort) = io { k.publishingPage(sort) }
    override suspend fun publishingRefresh(force: Boolean) = io { k.publishingRefresh(force) }
    override suspend fun publishingLink(id: UInt, link: String) = io { k.publishingLink(id, link) }
    override suspend fun publishingReset(id: UInt) = io { k.publishingReset(id) }
    override suspend fun noticesDue() = io { k.noticesDue() }
    override suspend fun notifySettings() = io { k.notifySettings() }
    override suspend fun setNotify(kind: NoticeKind, on: Boolean) = io { k.setNotify(kind, on) }
    override suspend fun stats(kind: MediaKind) = io { k.stats(kind) }
    override suspend fun settings() = io { k.settings() }
    override suspend fun setSetting(setting: Setting) = io { k.setSetting(setting) }
    override suspend fun titleLanguage() = io { k.titleLanguage() }
    override suspend fun setTitleLanguage(value: String) = io { k.setTitleLanguage(value) }
    override suspend fun shownTitles(kind: MediaKind) = io { k.shownTitles(kind) }
    override suspend fun releaseSources() = io { k.releaseSources() }
    override suspend fun setReleaseSources(sources: ReleaseSources) = io { k.setReleaseSources(sources) }
    override suspend fun volumeFill(): VolumeFill? = io { k.volumeFill() }
}

/** Made-up pages for the screenshot tests and "Look around" (the texts made by hand in the core's words). */
class SamplePages(private val now: Long, private val rows: () -> List<Row>) : PagesSource {
    private var notify = NotifySettings(startedAiring = true, newEpisode = true, newChapter = false)
    private var settings = KotoraSettings("system", true, "home", "last", "list", MediaKind.ANIME, true, true, false)
    private var titles = "romaji"
    private var sources = ReleaseSources(mangaupdates = true, mangadex = true)

    override suspend fun airingPage(type: AiringType, mine: AiringMine, order: AiringOrder) = samplePage(now, rows(), type, mine, order)
    override suspend fun airingRefresh(force: Boolean) {}
    override fun seasonPicks(back: UInt): List<SeasonPick> {
        val names = listOf("Winter", "Spring", "Summer", "Fall")
        val cur = 2026 * 4 + 3
        return (1 downTo -back.toInt()).map { o -> val i = cur + o; SeasonPick(o, "${names[i % 4]} ${i / 4}", (i / 4).toUShort()) }
    }
    override suspend fun seasonGrid(offset: Int, type: AiringType, mine: AiringMine, order: AiringOrder): SeasonGrid {
        val p = samplePage(now, rows(), type, mine, order)
        val label = seasonPicks(8u).firstOrNull { it.offset == offset }?.label ?: p.season
        val cards = p.cards.map { it.copy(`when` = if (offset < 0) "Finished airing" else "Starts Jan 9, 2027", lit = false, toWatch = null) }
        return SeasonGrid(label, now - 7_200, false, true, cards)
    }
    override suspend fun seasonFetch(offset: Int, force: Boolean) {}
    override suspend fun publishingPage(sort: PublishingSort) = samplePublishing(now, sort)
    override suspend fun publishingRefresh(force: Boolean) = PublishingReport(0u, null, emptyList())
    override suspend fun publishingLink(id: UInt, link: String) {}
    override suspend fun publishingReset(id: UInt) {}
    override suspend fun noticesDue(): List<Notice> = emptyList()
    override suspend fun notifySettings() = notify
    override suspend fun setNotify(kind: NoticeKind, on: Boolean) {
        notify = when (kind) {
            NoticeKind.STARTED_AIRING -> notify.copy(startedAiring = on)
            NoticeKind.NEW_EPISODE -> notify.copy(newEpisode = on)
            NoticeKind.NEW_CHAPTER -> notify.copy(newChapter = on)
        }
    }
    override suspend fun stats(kind: MediaKind) = sampleStats(kind, now)
    override suspend fun settings() = settings
    override suspend fun setSetting(setting: Setting): KotoraSettings {
        settings = when (setting) {
            is Setting.Theme -> settings.copy(theme = setting.value)
            is Setting.Intro -> settings.copy(intro = setting.on)
            is Setting.OpenOn -> settings.copy(openOn = setting.value)
            is Setting.StartMode -> settings.copy(startMode = setting.value)
            is Setting.LastPage -> settings.copy(lastPage = setting.value)
            is Setting.ListKind -> settings.copy(listKind = setting.kind)
            is Setting.NotifyNewEpisode -> settings.copy(notifyNewEpisode = setting.on)
            is Setting.NotifyStartedAiring -> settings.copy(notifyStartedAiring = setting.on)
            is Setting.NotifyNewChapter -> settings.copy(notifyNewChapter = setting.on)
        }
        return settings
    }
    override suspend fun titleLanguage() = titles
    override suspend fun setTitleLanguage(value: String) { titles = value }
    override suspend fun shownTitles(kind: MediaKind): List<ShownTitle> =
        if (titles == "english") rows().filter { it.kind == kind && it.titleEn != null }.map { ShownTitle(it.id, it.titleEn!!) } else emptyList()
    override suspend fun releaseSources() = sources
    override suspend fun setReleaseSources(sources: ReleaseSources) { this.sources = sources }
    override suspend fun volumeFill(): VolumeFill? = null

    companion object {
        /** A made-up week and season in UTC: the sample list's Watching shows and a few others. */
        fun samplePage(now: Long, rows: List<Row>, type: AiringType, mine: AiringMine, order: AiringOrder): AiringPage {
            val day0 = now - Math.floorMod(now, 86_400L)
            val others = listOf(
                Triple(201u, "Garasu no Umi", "tv"), Triple(202u, "Mizuiro Signal", "ona"), Triple(203u, "Ryuusei Kitchen", "tv"),
                Triple(204u, "Hanabi Courier", "tv"), Triple(205u, "Kurenai Archive", "movie"), Triple(206u, "Tenbin Academy", "tv"),
            )
            val watching = rows.filter { it.kind == MediaKind.ANIME && it.status == ListStatus.WATCHING && it.airStatus == "currently_airing" }
            val plan = listOf(19 * 60 + 30, 23 * 60, 22 * 60 + 30, 16 * 60, 1 * 60 + 5, 21 * 60, 0 * 60 + 30)
            fun hhmm(m: Int) = "%02d:%02d".format(m / 60, m % 60)
            fun countdown(secs: Long): String {
                val m = maxOf(secs, 0) / 60
                return when {
                    m == 0L -> "now"
                    m < 60 -> "$m min"
                    m < 1440 -> if (m % 60 == 0L) "${m / 60} h" else "${m / 60} h ${m % 60} min"
                    (m % 1440) / 60 == 0L -> "${m / 1440} d"
                    else -> "${m / 1440} d ${(m % 1440) / 60} h"
                }
            }
            fun label(f: String) = when (f) { "tv" -> "TV"; "ona" -> "ONA"; "movie" -> "Movie"; else -> "" }
            fun keeps(f: String) = when (type) {
                AiringType.ALL -> true
                AiringType.TV -> f == "tv"
                AiringType.ONA -> f == "ona"
                AiringType.MOVIE -> f == "movie"
            }
            fun keepsMine(on: Boolean) = mine == AiringMine.ALL || (mine == AiringMine.ON) == on
            val days = (0 until 7).map { d ->
                val slots = mutableListOf<AiringSlot>()
                watching.getOrNull(d)?.let { r ->
                    val at = day0 + d * 86_400L + plan[d] * 60L
                    val note = if (at <= now) "aired" else "in ${countdown(at - now)}"
                    val behind = if (d == 0 && at <= now) "~1 to watch" else "Caught up"
                    if (keeps(r.format) && keepsMine(true)) slots += AiringSlot(r.id, r.title, r.titleEn, r.coverUrl, label(r.format), r.progress + 1u, at, hhmm(plan[d]), r.status, note, behind)
                }
                others.getOrNull(d)?.let { (id, t, f) ->
                    val m = plan[(d + 3) % 7]
                    val at = day0 + d * 86_400L + m * 60L
                    if (keeps(f) && keepsMine(false)) slots += AiringSlot(id, t, null, null, label(f), (3 + d).toUInt(), at, hhmm(m), null, if (at <= now) "aired" else "", null)
                }
                val date = Instant.ofEpochSecond(day0 + d * 86_400L + 12 * 3600).atZone(ZoneOffset.UTC)
                val name = "${date.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.ENGLISH)} ${date.month.getDisplayName(TextStyle.SHORT, Locale.ENGLISH)} ${date.dayOfMonth}"
                val dayLabel = when (d) { 0 -> "Today · $name"; 1 -> "Tomorrow · $name"; else -> name }
                val n = slots.count { it.status != null }
                AiringDay(dayLabel, d == 0, if (n > 0) "$n on your list" else "", slots.sortedBy { it.at })
            }
            val untimed = if (mine == AiringMine.ON) emptyList() else listOf(AiringUntimed(207u, "Kumo Kujira Radio", null, null, "Sundays in Japan, time not known", null))
            val cards = mutableListOf<Pair<AiringCard, Int>>()
            watching.forEachIndexed { i, r ->
                if (!keeps(r.format) || !keepsMine(true)) return@forEachIndexed
                val behind = if (i == 0) 2 else 0
                cards += AiringCard(
                    r.id, r.title, r.titleEn, r.coverUrl, "${label(r.format)} · ${r.total ?: "?"} eps · ★ ${"%.2f".format(8.4 - i * .2)}",
                    "${listOf("Fri", "Sat", "Sun", "Mon")[i % 4]} ${hhmm(plan[i])} · ep ${r.progress + 1u} in ${countdown(plan[i] * 60L + i * 86_400L + 3600)}",
                    r.status, i < 2, if (behind > 0) "~$behind to watch" else "Caught up",
                ) to behind
            }
            others.forEachIndexed { i, (id, t, f) ->
                if (!keeps(f) || !keepsMine(false)) return@forEachIndexed
                cards += AiringCard(id, t, null, null, "${label(f)} · ${if (f == "movie") "1 ep" else "12 eps"} · ★ ${"%.2f".format(7.9 - i * .15)}", if (f == "movie") "Starts Dec 12, 2026" else "${listOf("Tue", "Wed", "Thu")[i % 3]} ${hhmm(plan[(i + 3) % 7])} · ep ${3 + i}", null, false, null) to -1
            }
            val sorted = if (order == AiringOrder.TO_WATCH) cards.sortedByDescending { it.second } else cards
            return AiringPage("Fall 2026", "MyAnimeList", now - 3_600, false, true, days, untimed, sorted.map { it.first })
        }

        fun samplePublishing(now: Long, sort: PublishingSort): PublishingPage {
            fun rel(l: String, w: String) = PublishingRelease(l, w)
            val rows = listOf(
                PublishingRow(101u, "Paper Lanterns", null, null, 21u, null, "Ch 23", "1 day ago", 2u, "Weekly", "next ~Mon", PublishingLook.SURE, true, false, false, false, "MangaUpdates", "Paper Lanterns", "https://www.mangaupdates.com/series/sample", listOf(rel("Ch 23", "1 day ago"), rel("Ch 22", "8 days ago"), rel("Ch 21", "15 days ago")), null),
                PublishingRow(102u, "Umibe no Atelier", null, null, 44u, 52u, "Ch 44", "12 days ago", 0u, "About every 2 weeks", "due today", PublishingLook.GUESS, false, false, false, false, "MangaDex", "Umibe no Atelier (Official)", "https://mangadex.org/title/sample", listOf(rel("Ch 44", "12 days ago")), null),
                PublishingRow(105u, "Kaze no Shiori", null, null, 80u, null, "Ch 79", "ahead of release data", 0u, "On hiatus", "", PublishingLook.PICKED, false, true, false, true, "MangaUpdates", "Kaze no Shiori", "https://www.mangaupdates.com/series/sample2", emptyList(), null),
                PublishingRow(106u, "Tsuki to Kamome", null, null, 3u, null, "", "", null, "", "", PublishingLook.NOT_FOUND, false, false, false, false, null, null, null, emptyList(), null),
            )
            val sorted = when (sort) {
                PublishingSort.NEXT -> rows
                PublishingSort.BEHIND -> rows.sortedByDescending { it.behind?.toInt() ?: -1 }
                PublishingSort.LATEST -> rows.sortedBy { if (it.latest.isEmpty()) 2 else if (it.fresh) 0 else 1 }
            }
            return PublishingPage(sorted, now - 1_800, "Release data: MangaUpdates, MangaDex", false)
        }

        /** The desktop's made-up numbers (`app/src/stats.rs`, `fake`), as iOS's `fakeStats`. */
        fun sampleStats(kind: MediaKind, now: Long): StatsData {
            val n = 371
            val today = Math.floorDiv(now, 86_400L)
            val days = MutableList(n) { 0u }
            val start = n - 120
            for (i in start until n) {
                val day = today - (n - 1 - i)
                val wd = io.github.hanami117.kotora.ui.StatsText.weekday(day)
                val seed = ((i.toLong() * 2_654_435_761L) and 0xFFFFFFFFL).toInt().ushr(28)
                days[i] = when {
                    wd >= 5 -> (1 + seed % 5).toUInt()
                    seed < 6 -> 0u
                    else -> (seed % 3).toUInt()
                }
                if (i - start in 60 until 70) days[i] = 0u
            }
            for (i in n - 6 until n) days[i] = maxOf(days[i], 1u)
            fun sum(k: Int) = days.takeLast(k).sum()
            val per: ULong = if (kind == MediaKind.ANIME) 24u else 0u
            fun period(k: Int, titles: UInt) = StatsPeriod(sum(k), if (kind == MediaKind.MANGA) sum(k) / 8u else 0u, sum(k).toULong() * per, titles)
            val names = if (kind == MediaKind.ANIME) listOf("Hoshikuzu Relay", "Kagerou Bakery", "Tsukimi Station 2nd Season", "Neko Tantei Rin", "Sakamichi Cycle Club")
            else listOf("Paper Lanterns", "Umibe no Atelier", "Hoshi no Tegami", "Kaminari Bento", "Kaze no Shiori")
            val ids = if (kind == MediaKind.ANIME) listOf(7u, 3u, 11u, 5u, 23u) else listOf(101u, 102u, 104u, 103u, 105u)
            fun titles(k: Int) = (0 until k).map { i -> StatsTitle(ids[i], names[i], null, (6 + i).toUInt(), 12u, maxOf(0, 14 - i * 3).toUInt()) }
            fun named(vararg v: Pair<String, Int>) = v.map { NamedCount(it.first, it.second.toUInt()) }
            val anime = kind == MediaKind.ANIME
            return StatsData(
                kind = kind, today = today, sinceDay = today - 118,
                titles = 304u, statuses = listOf(11u, 48u, 236u, 7u, 2u), scores = listOf(0u, 1u, 2u, 4u, 9u, 22u, 48u, 71u, 40u, 15u),
                units = if (anime) 3790u else 6120u, volumes = if (anime) 0u else 512u, minutes = if (anime) 90_870u else 0u,
                guessed = 12u, watched = 256u, withDetails = 61u,
                genres = named("Comedy" to 98, "Slice of Life" to 77, "Romance" to 51, "Drama" to 44, "Fantasy" to 39, "Mystery" to 21),
                makers = if (anime) named("Kyoto Animation" to 14, "Shaft" to 9, "P.A. Works" to 8, "Bones" to 6, "Madhouse" to 5)
                else named("Hoshino Aki" to 4, "Mizuno Ren" to 3, "Takeda Yui" to 2),
                airing = if (anime) titles(3) else emptyList(),
                days = days, firstDay = today - (n - 1),
                week = period(7, 4u), month = period(30, 7u), year = period(365, 31u), all = period(n, 34u),
                streak = 6u, bestStreak = 19u,
                weekdays = listOf(41u, 38u, 44u, 40u, 52u, 96u, 88u),
                hours = listOf(9u, 4u, 1u, 0u, 0u, 0u, 0u, 1u, 2u, 1u, 0u, 1u, 6u, 3u, 2u, 2u, 4u, 8u, 12u, 22u, 31u, 38u, 33u, 19u),
                lately = titles(5),
            )
        }
    }
}
