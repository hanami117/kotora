package io.github.hanami117.kotora.data

import io.github.hanami117.kotora.core.Edit
import io.github.hanami117.kotora.core.EntryEditor
import io.github.hanami117.kotora.core.Hit
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.QueueItem
import io.github.hanami117.kotora.core.QueueState
import io.github.hanami117.kotora.core.Removal
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.ScoreChoice
import io.github.hanami117.kotora.core.ScoreScale
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.ui.statusLabel
import java.util.Collections
import java.util.WeakHashMap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/**
 * The editor, rewatch/reread, remove, −1, the sync queue with its conflicts (`mobile/src/edit.rs`,
 * `remove.rs`) and Search and add (`search.rs`). Kept apart from [Repository] (as iOS's
 * `EntryEditing`/`SearchRepository`) so the other pages' code doesn't change.
 */
interface Editing {
    suspend fun editor(kind: MediaKind, id: UInt): EntryEditor
    suspend fun scoreScale(): ScoreScale
    fun scoreLabel(points: UInt?): String
    fun scoreStep(points: UInt?, delta: Int): UInt?
    /** The words of the saved change; `null`: nothing changed. */
    suspend fun saveEditor(editor: EntryEditor): String?
    suspend fun startRewatch(kind: MediaKind, id: UInt): Edit
    suspend fun stopRewatch(kind: MediaKind, id: UInt): Edit
    suspend fun minusOne(kind: MediaKind, id: UInt): Edit
    /** Move to › / Score › (the desktop's right-click menu); null: nothing changed. */
    suspend fun moveTo(kind: MediaKind, id: UInt, status: ListStatus): Edit?
    suspend fun setScore(kind: MediaKind, id: UInt, points: UInt?): Edit?
    /** "Episode 9 of X hasn't aired yet (expected …)." before a +1; null: +1 at once. */
    suspend fun notAiredQuestion(kind: MediaKind, id: UInt): String?
    suspend fun removeQuestion(kind: MediaKind, id: UInt): String
    suspend fun remove(kind: MediaKind, id: UInt): Removal

    suspend fun queueItems(): List<QueueItem>
    suspend fun conflictCount(): UInt
    suspend fun keepMine(item: QueueItem)
    suspend fun takeTheirs(item: QueueItem)
    suspend fun discard(service: Service, id: Long)
    suspend fun retryNow()

    /** Network. */
    suspend fun search(kind: MediaKind, query: String): List<Hit>
    suspend fun add(hit: Hit, status: ListStatus)
}

private val made = Collections.synchronizedMap(WeakHashMap<Repository, Editing>())

/** The [Editing] of a repository (one each: the sample's queue lives in it). */
fun Repository.editing(): Editing? = made.getOrPut(this) {
    when (this) {
        is CoreRepository -> CoreEditing(this)
        is SampleRepository -> SampleEditing(this)
        else -> return null
    }
}

private class CoreEditing(private val repo: CoreRepository) : Editing {
    private val k get() = repo.kotora
    private suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    override suspend fun editor(kind: MediaKind, id: UInt) = io { k.editor(kind, id) }
    override suspend fun scoreScale() = io { k.scoreScale() }
    override fun scoreLabel(points: UInt?) = k.scoreLabel(points)
    override fun scoreStep(points: UInt?, delta: Int) = k.scoreStep(points, delta)
    override suspend fun saveEditor(editor: EntryEditor) = io { k.saveEditor(editor) }
    override suspend fun startRewatch(kind: MediaKind, id: UInt) = io { k.startRewatch(kind, id) }
    override suspend fun stopRewatch(kind: MediaKind, id: UInt) = io { k.stopRewatch(kind, id) }
    override suspend fun minusOne(kind: MediaKind, id: UInt) = io { k.minusOne(kind, id) }
    override suspend fun moveTo(kind: MediaKind, id: UInt, status: ListStatus) = io { k.moveTo(kind, id, status) }
    override suspend fun setScore(kind: MediaKind, id: UInt, points: UInt?) = io { k.setScore(kind, id, points) }
    override suspend fun notAiredQuestion(kind: MediaKind, id: UInt) = io { k.notAiredQuestion(kind, id) }
    override suspend fun removeQuestion(kind: MediaKind, id: UInt) = io { k.removeQuestion(kind, id) }
    override suspend fun remove(kind: MediaKind, id: UInt) = io { k.remove(kind, id) }
    override suspend fun queueItems() = io { k.queueItems() }
    override suspend fun conflictCount() = io { k.conflictCount() }
    override suspend fun keepMine(item: QueueItem) = io { k.keepMine(item.service, item.id) }
    override suspend fun takeTheirs(item: QueueItem) = io { k.takeTheirs(item.service, item.id) }
    override suspend fun discard(service: Service, id: Long) = io { k.discard(service, id) }
    override suspend fun retryNow() = io { k.retryNow() }
    override suspend fun search(kind: MediaKind, query: String) = io { k.search(kind, query) }
    override suspend fun add(hit: Hit, status: ListStatus) = io { k.add(hit, status) }
}

/**
 * Sample data: the rules are the core's pure functions where there are some (the screens call
 * `editorStep` themselves); the saved values and the queue stay in memory. One made-up conflict
 * shows the queue page, as on iOS.
 */
class SampleEditing(private val repo: SampleRepository, private val now: Long = System.currentTimeMillis() / 1000) : Editing {
    private val rows get() = repo.rows
    val queue = mutableListOf(
        QueueItem(
            Service.MAL, 1, MediaKind.ANIME, 23u, "Sakamichi Cycle Club", null, QueueState.CONFLICT, "Progress 14 → 15", 1u, null, false, false,
            now - 1_800, null, "Progress 15 / 24 · Score 8", "Progress 13 / 24 · Score 7", "13 / 24, Watching, score 7", now - 600,
        ),
        QueueItem(
            Service.MAL, 2, MediaKind.ANIME, 3u, "Kagerou Bakery", null, QueueState.WAITING, "Progress 4 → 5", 1u, null, false, false,
            now - 300, null, "", null, null, null,
        ),
        QueueItem(
            Service.MAL, 3, MediaKind.ANIME, 31u, "Shiosai Diner", null, QueueState.SENT, "Score 7", 1u, null, false, false,
            now - 7_200, now - 7_100, "", null, null, null,
        ),
    )
    private val removed = mutableMapOf<Long, Row>()
    private var nextId = 10L

    private fun index(kind: MediaKind, id: UInt): Int =
        rows.indexOfFirst { it.kind == kind && it.id == id }.takeIf { it >= 0 } ?: throw KotoraException.NotFound("That isn't on your list.")

    override suspend fun editor(kind: MediaKind, id: UInt): EntryEditor {
        val r = rows[index(kind, id)]
        return EntryEditor(
            kind, id, r.title, r.status, r.progress, r.total, r.volumes, r.totalVolumes,
            r.score?.toUIntOrNull()?.let { it * 10u }, r.timesRewatched, r.rewatching, r.status,
        )
    }

    override suspend fun scoreScale() = ScoreScale((1..10).map { ScoreChoice((it * 10).toUInt(), it.toString()) }, false)
    override fun scoreLabel(points: UInt?) = points?.let { ((it + 5u) / 10u).toString() } ?: "No score"
    override fun scoreStep(points: UInt?, delta: Int) = points

    override suspend fun saveEditor(editor: EntryEditor): String? {
        val i = index(editor.kind, editor.id)
        val r = rows[i]
        val n = r.copy(
            status = editor.status, progress = editor.progress, volumes = editor.volumes,
            score = editor.score?.let { ((it + 5u) / 10u).toString() }, timesRewatched = editor.timesRewatched,
        )
        if (n == r) return null
        rows[i] = n.copy(waiting = true)
        return "${r.title}: saved"
    }

    private fun edit(r: Row, message: String) = Edit(r.kind, r.id, message, r.status, r.progress, r.rewatching, r.timesRewatched, r.volumes)

    private fun rewatch(kind: MediaKind, id: UInt, on: Boolean): Edit {
        val i = index(kind, id)
        val r = rows[i]
        rows[i] = r.copy(rewatching = on, progress = if (on) 0u else (r.total ?: r.progress), waiting = true)
        val word = if (kind == MediaKind.MANGA) "reread" else "rewatch"
        return edit(r, "${r.title}: $word ${if (on) "started" else "stopped"}")
    }

    override suspend fun startRewatch(kind: MediaKind, id: UInt) = rewatch(kind, id, true)
    override suspend fun stopRewatch(kind: MediaKind, id: UInt) = rewatch(kind, id, false)

    override suspend fun minusOne(kind: MediaKind, id: UInt): Edit {
        val i = index(kind, id)
        val r = rows[i]
        if (r.progress == 0u) throw KotoraException.Other("${r.title} is at ${SampleRepository.unit(kind)} 0.")
        rows[i] = r.copy(progress = r.progress - 1u, status = if (r.status == ListStatus.COMPLETED && !r.rewatching) ListStatus.WATCHING else r.status, waiting = true)
        return edit(r, "${r.title}: ${SampleRepository.unit(kind)} ${r.progress - 1u} saved")
    }

    override suspend fun moveTo(kind: MediaKind, id: UInt, status: ListStatus): Edit? {
        val i = index(kind, id)
        val r = rows[i]
        if (r.status == status) return null
        rows[i] = r.copy(status = status, progress = if (status == ListStatus.COMPLETED) r.total ?: r.progress else r.progress, waiting = true)
        return edit(r, "Moved ${r.title} to ${statusLabel(status, kind)}").copy(beforeScore = r.score?.toUIntOrNull()?.let { it * 10u }, restoreScore = true)
    }

    override suspend fun setScore(kind: MediaKind, id: UInt, points: UInt?): Edit? {
        val i = index(kind, id)
        val r = rows[i]
        val score = points?.let { ((it + 5u) / 10u).toString() }
        if (r.score == score) return null
        rows[i] = r.copy(score = score, waiting = true)
        return edit(r, score?.let { "Scored ${r.title} $it" } ?: "Removed the score of ${r.title}").copy(beforeScore = r.score?.toUIntOrNull()?.let { it * 10u }, restoreScore = true)
    }

    override suspend fun notAiredQuestion(kind: MediaKind, id: UInt): String? {
        val r = rows.firstOrNull { it.kind == kind && it.id == id } ?: return null
        return if (r.airStatus == "not_yet_aired") "Episode ${r.progress + 1u} of ${r.title} hasn't aired yet." else null
    }

    override suspend fun removeQuestion(kind: MediaKind, id: UInt): String =
        "Remove ${rows[index(kind, id)].title} from your list? It goes from your MyAnimeList list too, with its progress and score."

    override suspend fun remove(kind: MediaKind, id: UInt): Removal {
        val r = rows.removeAt(index(kind, id))
        val op = nextId++
        removed[op] = r
        queue += QueueItem(Service.MAL, op, kind, id, r.title, r.coverUrl, QueueState.WAITING, "Remove from list", 1u, null, false, true, System.currentTimeMillis() / 1000, null, "Remove from list", null, null, null)
        return Removal(Service.MAL, op, "Removed ${r.title} from your list")
    }

    override suspend fun queueItems() = queue.toList()
    override suspend fun conflictCount() = queue.count { it.state == QueueState.CONFLICT }.toUInt()
    override suspend fun keepMine(item: QueueItem) {
        queue.removeAll { it.id == item.id }
    }
    override suspend fun takeTheirs(item: QueueItem) {
        queue.removeAll { it.id == item.id }
    }
    override suspend fun discard(service: Service, id: Long) {
        queue.removeAll { it.id == id }
        removed.remove(id)?.let { rows += it }
    }
    override suspend fun retryNow() {}

    /** Made-up results (the desktop's scene titles), a short pause as if the service answered. */
    override suspend fun search(kind: MediaKind, query: String): List<Hit> {
        delay(350)
        val q = query.lowercase()
        return sampleHits(kind).filter { h ->
            q.split(' ').filter { it.isNotEmpty() }.all { w -> (listOf(h.title, h.titleEn ?: "") + h.synonyms).any { it.lowercase().contains(w) } } ||
                "lanterns".startsWith(q) || q.contains("lantern")
        }
    }

    override suspend fun add(hit: Hit, status: ListStatus) {
        val i = rows.indexOfFirst { it.kind == hit.kind && it.id == hit.id }
        if (i >= 0) {
            rows[i] = rows[i].copy(status = status, waiting = true)
            return
        }
        val done = status == ListStatus.COMPLETED
        rows += Row(
            hit.kind, hit.id, hit.title, hit.titleEn, status, if (done) hit.episodes ?: 0u else 0u, hit.episodes,
            if (done) hit.volumes ?: 0u else 0u, hit.volumes, null, hit.format, hit.airStatus, hit.startDate, hit.coverUrl,
            false, 0u, true, System.currentTimeMillis() / 1000,
        )
    }

    companion object {
        fun sampleHits(kind: MediaKind): List<Hit> {
            fun h(id: Int, title: String, en: String?, format: String, eps: Int?, air: String, type: String, meta: String) = Hit(
                kind, id.toUInt(), title, en, null, emptyList(), format, eps?.toUInt(), null, air, null, null, null, null,
                emptyList(), null, type, meta,
            )
            if (kind == MediaKind.MANGA) {
                return listOf(
                    h(9201, "Lanterns of Kirigaoka", null, "manga", 48, "currently_publishing", "Manga", "48 ch · 6 vols · Publishing · ★ 8.12"),
                    h(9202, "Lanterns of Kirigaoka: Night Market", null, "light_novel", null, "finished", "Light novel", "Finished · ★ 7.40"),
                    h(101, "Paper Lanterns", null, "manga", null, "currently_publishing", "Manga", "Publishing · ★ 8.01"),
                )
            }
            return listOf(
                h(9101, "Lanterns of Kirigaoka: Festival Night", "Lanterns: Festival Night", "movie", 1, "finished_airing", "Movie", "1 ep · Summer 2026 · ★ 8.31"),
                h(9102, "Lanterns of Kirigaoka 2nd Season", null, "tv", 12, "currently_airing", "TV", "12 eps · Airing · Fall 2026 · ★ 8.05"),
                h(9103, "Kirigaoka Lantern Shop", "The Lantern Shop on the Hill", "ona", 24, "finished_airing", "ONA", "24 eps · Spring 2023 · ★ 7.12"),
                h(9104, "Lanterns of Kirigaoka: Side Stories", null, "special", 6, "not_yet_aired", "Special", "6 eps · Not yet aired"),
                h(19, "Ashita no Lantern", null, "tv", 25, "finished_airing", "TV", "25 eps · Spring 2019 · ★ 8.44"),
            )
        }
    }
}
