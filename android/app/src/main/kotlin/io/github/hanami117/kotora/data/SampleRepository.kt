package io.github.hanami117.kotora.data

import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.BackgroundReport
import io.github.hanami117.kotora.core.AiringDay
import io.github.hanami117.kotora.core.AiringMine
import io.github.hanami117.kotora.core.AiringSlot
import io.github.hanami117.kotora.core.AiringType
import io.github.hanami117.kotora.core.AiringUntimed
import io.github.hanami117.kotora.core.AiringWeek
import io.github.hanami117.kotora.core.Edit
import io.github.hanami117.kotora.core.FlushReport
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Note
import io.github.hanami117.kotora.core.NotifySettings
import io.github.hanami117.kotora.core.PendingSignIn
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.Sort
import io.github.hanami117.kotora.core.ListPage
import io.github.hanami117.kotora.core.ListQuery
import io.github.hanami117.kotora.core.ListViewMode
import io.github.hanami117.kotora.ui.ListPageSource
import io.github.hanami117.kotora.ui.samplePage
import io.github.hanami117.kotora.core.StatusCount
import io.github.hanami117.kotora.core.SyncReport
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.TextStyle
import java.util.Locale

/**
 * Made-up lists in memory (the mockup's titles; nothing real). Edits change the rows here only;
 * nothing goes anywhere. Used by the screenshot tests and by "Look around" in debug builds.
 */
class SampleRepository(private val now: Long = 1_795_000_000L) : Repository, AccountRepository, ListPageSource {
    override val isSample = true

    internal val rows: MutableList<Row> = sampleRows(now).toMutableList()

    override fun hasClientId(service: Service) = true

    private val views = mutableMapOf<Pair<MediaKind, ListStatus>, ListViewMode>()
    override suspend fun listPage(kind: MediaKind, query: ListQuery): ListPage = samplePage(rows.toList(), kind, query) { r, s -> showsIn(r, s) }
    override suspend fun listView(kind: MediaKind, status: ListStatus) = views[kind to status] ?: ListViewMode.LIST
    override suspend fun setListView(kind: MediaKind, status: ListStatus, view: ListViewMode) { views[kind to status] = view }

    override suspend fun accounts() = listOf(Account(Service.MAL, "sample-user", null, true))

    override suspend fun list(kind: MediaKind, status: ListStatus?, sort: Sort): List<Row> {
        val l = rows.filter { it.kind == kind && (status == null || showsIn(it, status)) }
        return when (sort) {
            Sort.UPDATED -> l.sortedByDescending { it.updatedAt }
            Sort.TITLE -> l.sortedBy { it.title.lowercase() }
            Sort.SCORE -> l.sortedByDescending { it.score?.toFloatOrNull() ?: -1f }
        }
    }

    override suspend fun counts(kind: MediaKind) =
        ListStatus.entries.map { st -> StatusCount(st, rows.count { it.kind == kind && showsIn(it, st) }.toUInt()) }

    override suspend fun entry(kind: MediaKind, id: UInt) = rows.find { it.kind == kind && it.id == id }
    override suspend fun syncedAt(kind: MediaKind): Long = now - 240
    override suspend fun waitingCount() = rows.count { it.waiting }.toUInt()
    override suspend fun mangaEnabled() = true
    override suspend fun setMangaEnabled(on: Boolean) {}

    override suspend fun plusOne(kind: MediaKind, id: UInt): Edit {
        val i = rows.indexOfFirst { it.kind == kind && it.id == id }
        if (i < 0) throw KotoraException.NotFound("That isn't on your list.")
        val r = rows[i]
        val total = r.total
        if (total != null && r.progress >= total) throw KotoraException.Other("${r.title} is at its last ${unit(kind)}.")
        val next = r.progress + 1u
        val completes = total != null && next == total
        rows[i] = r.copy(progress = next, status = if (completes) ListStatus.COMPLETED else r.status, waiting = true, updatedAt = now)
        return Edit(kind, id, "${r.title}: ${unit(kind)} $next saved" + if (completes) ", completed" else "", r.status, r.progress, r.rewatching, r.timesRewatched, r.volumes)
    }

    override suspend fun undo(edit: Edit) {
        val i = rows.indexOfFirst { it.kind == edit.kind && it.id == edit.id }
        if (i >= 0) rows[i] = rows[i].copy(status = edit.beforeStatus, progress = edit.beforeProgress, rewatching = edit.beforeRewatching, waiting = false)
    }

    override suspend fun sync() = SyncReport(Service.MAL, rows.count { it.kind == MediaKind.ANIME }.toUInt(), rows.count { it.kind == MediaKind.MANGA }.toUInt(), now)
    override suspend fun flush() = FlushReport(0u, 0u, 0u, 0u, false, null)
    override suspend fun signInStart(service: Service): PendingSignIn = throw KotoraException.NoClientId("Sample data: no sign-in.")
    override suspend fun signInFinish(pending: PendingSignIn, redirect: String): Account = throw KotoraException.NoClientId("Sample data: no sign-in.")
    override suspend fun signOut(service: Service) {}
    override suspend fun setActive(service: Service) {}
    override suspend fun backgroundRefresh() = BackgroundReport(flush(), false, false, null)

    private var notify = NotifySettings(startedAiring = true, newEpisode = true, newChapter = false)

    override suspend fun airingWeek(type: AiringType, mine: AiringMine): AiringWeek = sampleWeek(now, rows, type, mine)
    override suspend fun airingFetch(force: Boolean) = false
    override suspend fun notifySettings() = notify
    override suspend fun setNotifySettings(settings: NotifySettings) {
        notify = settings
    }
    override suspend fun airNotes(): List<Note> = emptyList()
    override suspend fun chapterNotes(): List<Note> = emptyList()
    override suspend fun profile(service: Service) = sampleProfile()
    override fun serviceLinks(kind: MediaKind, id: UInt) =
        listOf(io.github.hanami117.kotora.core.ServiceLink(Service.MAL, "Open on MyAnimeList", Links.media(Service.MAL, kind, id)))
    override suspend fun signInWithToken(raw: String): Account = throw io.github.hanami117.kotora.core.KotoraException.NoClientId("Sample data: no sign-in.")

    companion object {
        /**
         * A made-up week in UTC: the sample list's Watching shows and a few others, a slot a day
         * or two, as the core lays it out (the texts are the core's words, made here by hand).
         */
        fun sampleWeek(now: Long, rows: List<Row>, type: AiringType, mine: AiringMine): AiringWeek {
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
                AiringType.TV -> f == "tv" || f == "tv_special"
                AiringType.ONA -> f == "ona"
                AiringType.MOVIE -> f == "movie"
            }
            val days = (0 until 7).map { d ->
                val slots = mutableListOf<AiringSlot>()
                watching.getOrNull(d)?.let { r ->
                    val at = day0 + d * 86_400L + plan[d] * 60L
                    val note = if (at <= now) "aired" else "in ${countdown(at - now)}"
                    val behind = if (d == 0 && at <= now) "~1 to watch" else "Caught up"
                    slots += AiringSlot(r.id, r.title, r.coverUrl, label(r.format), r.progress + 1u, at, hhmm(plan[d]), true, r.status, note, behind)
                }
                others.getOrNull(d)?.let { (id, t, f) ->
                    val m = plan[(d + 3) % 7]
                    val at = day0 + d * 86_400L + m * 60L
                    slots += AiringSlot(id, t, null, label(f), (3 + d).toUInt(), at, hhmm(m), false, null, if (at <= now) "aired" else "", null)
                }
                val kept = slots.filter { keeps(when (it.formatLabel) { "TV" -> "tv"; "ONA" -> "ona"; "Movie" -> "movie"; else -> "" }) && (mine == AiringMine.ALL || (mine == AiringMine.ON) == it.mine) }.sortedBy { it.at }
                val date = Instant.ofEpochSecond(day0 + d * 86_400L + 12 * 3600).atZone(ZoneOffset.UTC)
                val name = "${date.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.ENGLISH)} ${date.month.getDisplayName(TextStyle.SHORT, Locale.ENGLISH)} ${date.dayOfMonth}"
                val dayLabel = when (d) { 0 -> "Today · $name"; 1 -> "Tomorrow · $name"; else -> name }
                AiringDay(dayLabel, d == 0, kept.count { it.mine }.toUInt(), kept)
            }
            val untimed = if (mine == AiringMine.ON) emptyList() else listOf(AiringUntimed(207u, "Kumo Kujira Radio", null, "Sundays in Japan, time not known", null))
            return AiringWeek("Fall 2026", "MyAnimeList", now - 3_600, 40u, false, days, untimed)
        }

        /** As the core: a rewatch (status Completed) shows under Watching too. */
        fun showsIn(r: Row, page: ListStatus) = r.status == page || (page == ListStatus.WATCHING && r.rewatching)

        fun unit(kind: MediaKind) = if (kind == MediaKind.MANGA) "chapter" else "episode"

        private fun anime(
            id: Int, title: String, status: ListStatus, progress: Int, total: Int?, score: String?, updated: Long,
            format: String = "tv", air: String = "currently_airing", rewatching: Boolean = false, waiting: Boolean = false,
            en: String? = null, start: String? = "2026-10-03",
        ) = Row(
            MediaKind.ANIME, id.toUInt(), title, en, status, progress.toUInt(), total?.toUInt(), 0u, null, score,
            format, air, start, null, rewatching, 0u, waiting, updated,
        )

        private fun manga(id: Int, title: String, status: ListStatus, chapters: Int, total: Int?, vols: Int, score: String?, updated: Long) = Row(
            MediaKind.MANGA, id.toUInt(), title, null, status, chapters.toUInt(), total?.toUInt(), vols.toUInt(), null, score,
            "manga", if (total == null) "currently_publishing" else "finished", "2021-04-01", null, false, 0u, false, updated,
        )

        /** The mockup's list, and enough filler for the counts. All made up. */
        fun sampleRows(now: Long): List<Row> {
            val w = ListStatus.WATCHING
            val out = mutableListOf(
                anime(7, "Hoshikuzu Relay", w, 6, 12, "8", now - 60, en = "Stardust Relay"),
                anime(3, "Kagerou Bakery", w, 5, 13, "7", now - 300, waiting = true),
                anime(11, "Tsukimi Station 2nd Season", w, 3, 12, null, now - 3_600),
                anime(19, "Ashita no Lantern", ListStatus.COMPLETED, 20, 25, "9", now - 7_200, air = "finished_airing", rewatching = true, start = "2019-04-06"),
                anime(5, "Neko Tantei Rin", w, 1, 8, null, now - 86_400, format = "ona"),
                anime(23, "Sakamichi Cycle Club", w, 14, 24, "8", now - 2 * 86_400, air = "finished_airing", start = "2025-01-10"),
                anime(29, "Kumo no Ue no Toshokan", w, 2, null, null, now - 3 * 86_400),
                anime(31, "Shiosai Diner", w, 9, 12, "7", now - 4 * 86_400, air = "finished_airing"),
            )
            val plan = listOf("Garasu no Umi", "Tenbin Academy", "Hanabi Courier", "Yoru no Kissaten", "Mizuiro Signal")
            plan.forEachIndexed { i, t -> out += anime(40 + i, t, ListStatus.PLAN_TO_WATCH, 0, 12, null, now - (10 + i) * 86_400L, air = "not_yet_aired") }
            val done = listOf("Kitsune Postman", "Aozora Workshop", "Ginga Ramen", "Sora Tobu Shoten", "Kaze no Orchestra", "Momiji Line")
            done.forEachIndexed { i, t -> out += anime(60 + i, t, ListStatus.COMPLETED, 12, 12, (9 - i % 3).toString(), now - (30 + i) * 86_400L, air = "finished_airing") }
            out += anime(80, "Fuyu no Robot", ListStatus.ON_HOLD, 4, 11, null, now - 60 * 86_400L, air = "finished_airing")
            out += anime(81, "Hikari Tantei Office", ListStatus.DROPPED, 2, 12, "4", now - 90 * 86_400L, air = "finished_airing")
            out += manga(101, "Paper Lanterns", ListStatus.WATCHING, 21, null, 3, "8", now - 600)
            out += manga(102, "Umibe no Atelier", ListStatus.WATCHING, 44, 52, 5, "9", now - 86_400)
            out += manga(103, "Kaminari Bento", ListStatus.PLAN_TO_WATCH, 0, null, 0, null, now - 5 * 86_400L)
            out += manga(104, "Hoshi no Tegami", ListStatus.COMPLETED, 60, 60, 7, "8", now - 40 * 86_400L)
            return out
        }
    }
}
