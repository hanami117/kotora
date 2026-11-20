package io.github.hanami117.kotora.ui

import io.github.hanami117.kotora.core.ListEmptyKind
import io.github.hanami117.kotora.core.ListPage
import io.github.hanami117.kotora.core.ListQuery
import io.github.hanami117.kotora.core.ListSort
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.ListViewMode
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.PageLine
import io.github.hanami117.kotora.core.QuickFilter
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.ScoreMark
import io.github.hanami117.kotora.core.ScoreStyle

// The list page's power features (desktop `model.rs`/`manga.rs`): the four orders and their two
// directions, the chips, the "Filter your list" box, the view per tab, "Ep N out" / "On hiatus",
// the score in the user's format and the empty states. The core computes the page
// (`mobile/src/listopts.rs`); the words are here too so that the screenshot tests need no
// native library.

/** What a repository gives the list page beyond the plain list. */
interface ListPageSource {
    suspend fun listPage(kind: MediaKind, query: ListQuery): ListPage
    suspend fun listView(kind: MediaKind, status: ListStatus): ListViewMode
    suspend fun setListView(kind: MediaKind, status: ListStatus, view: ListViewMode)
}

fun listSortLabel(s: ListSort) = when (s) {
    ListSort.UPDATED -> "Last updated"
    ListSort.TITLE -> "Title"
    ListSort.SCORE -> "Score"
    ListSort.PROGRESS -> "Progress"
}

/** The menu's two directions: the order's own first, then the reverse. */
fun listSortDirections(s: ListSort) = when (s) {
    ListSort.UPDATED -> listOf("Newest first", "Oldest first")
    ListSort.TITLE -> listOf("A to Z", "Z to A")
    ListSort.SCORE -> listOf("Highest first", "Lowest first")
    ListSort.PROGRESS -> listOf("Most watched first", "Least watched first")
}

fun quickFilters(kind: MediaKind) = if (kind == MediaKind.ANIME) {
    listOf(QuickFilter.ALL, QuickFilter.AIRING, QuickFilter.FINISHED_AIRING, QuickFilter.NEW_EPISODE)
} else {
    listOf(QuickFilter.ALL, QuickFilter.PUBLISHING, QuickFilter.FINISHED, QuickFilter.ON_HIATUS)
}

fun quickFilterLabel(f: QuickFilter) = when (f) {
    QuickFilter.ALL -> "All"
    QuickFilter.AIRING -> "Airing"
    QuickFilter.FINISHED_AIRING -> "Finished airing"
    QuickFilter.NEW_EPISODE -> "New episode"
    QuickFilter.PUBLISHING -> "Publishing"
    QuickFilter.FINISHED -> "Finished"
    QuickFilter.ON_HIATUS -> "On hiatus"
}

fun listViewLabel(v: ListViewMode) = when (v) {
    ListViewMode.LIST -> "List"
    ListViewMode.GRID -> "Grid"
    ListViewMode.LARGE -> "Large"
}

fun listFilterHint(kind: MediaKind) = if (kind == MediaKind.ANIME) "Filter your list" else "Filter your manga"

/** The tags under a title, with "Ep N out" and "On hiatus" (desktop `grid.rs`, `mgrid.rs`). */
fun lineTags(line: PageLine, base: List<Pair<String, TagKind>>): List<Pair<String, TagKind>> {
    val out = base.toMutableList()
    line.newEpisode?.let { out.add(if (line.row.rewatching) minOf(1, out.size) else 0, "Ep $it out" to TagKind.NEW) }
    if (line.onHiatus && !line.row.rewatching) out.add(0, "On hiatus" to TagKind.HIATUS)
    return out.take(3)
}

/** The core's rules (`listopts.rs`) on the sample data's rows. */
fun samplePage(all: List<Row>, kind: MediaKind, q: ListQuery, showsIn: (Row, ListStatus) -> Boolean): ListPage {
    val rows = all.filter { it.kind == kind }
    val needle = q.query.trim().lowercase()
    // Made up: the airing shows being watched with an odd id have one more episode out.
    fun newEp(r: Row): UInt? =
        if (r.kind == MediaKind.ANIME && r.airStatus == "currently_airing" && r.status == ListStatus.WATCHING && r.id % 2u == 1u) r.progress + 1u else null
    fun keeps(r: Row) = when (q.filter) {
        QuickFilter.ALL -> true
        QuickFilter.AIRING -> r.airStatus == "currently_airing"
        QuickFilter.FINISHED_AIRING -> r.airStatus == "finished_airing"
        QuickFilter.NEW_EPISODE -> newEp(r) != null
        QuickFilter.PUBLISHING -> r.airStatus == "currently_publishing"
        QuickFilter.FINISHED -> r.airStatus == "finished"
        QuickFilter.ON_HIATUS -> r.airStatus == "on_hiatus"
    }
    var l = rows.filter { r ->
        if (needle.isEmpty()) showsIn(r, q.status) && keeps(r)
        else listOfNotNull(r.title, r.titleEn).any { it.lowercase().contains(needle) }
    }
    l = when (q.sort) {
        ListSort.UPDATED -> l.sortedByDescending { it.updatedAt }
        ListSort.TITLE -> l.sortedBy { it.title.lowercase() }
        ListSort.SCORE -> l.sortedByDescending { it.score?.toFloatOrNull() ?: -1f }
        ListSort.PROGRESS -> l.sortedWith(compareByDescending<Row> { it.progress }.thenByDescending { it.volumes })
    }
    if (q.reverse) l = l.reversed()
    if (needle.isNotEmpty()) l = l.sortedBy { if (it.status == q.status) 0 else 1 + ListStatus.entries.indexOf(it.status) }
    val lines = l.map { r ->
        PageLine(r, newEp(r), r.airStatus == "on_hiatus", r.score?.let { ScoreMark(ScoreStyle.NUMBER, it, 0u, null, it) })
    }
    if (lines.isNotEmpty()) return ListPage(lines, ListEmptyKind.NONE, "", "")
    val noun = if (kind == MediaKind.MANGA) "manga" else "anime"
    if (needle.isEmpty() && rows.none { showsIn(it, q.status) }) {
        return ListPage(emptyList(), ListEmptyKind.EMPTY, "No $noun in ${statusLabel(q.status, kind)}", "")
    }
    val t = if (needle.isEmpty()) "No $noun match this filter." else "Nothing matches “${q.query.trim()}”."
    return ListPage(emptyList(), ListEmptyKind.NO_MATCH, t, "")
}
