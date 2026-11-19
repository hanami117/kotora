package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.HomeAiring
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.StatsData
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.ui.theme.Kotora

/** Home's shelves, from the saved list. */
data class Shelves(
    val watching: List<ListRow> = emptyList(),
    val reading: List<ListRow> = emptyList(),
    val plan: List<ListRow> = emptyList(),
    /** Airing this week: the next episode of the list's airing shows (the core's `home_airing`). */
    val airing: List<HomeAiring> = emptyList(),
    val loaded: Boolean = false,
) {
    companion object {
        /** Cards a shelf holds at most; "See all" opens the rest. */
        const val MAX = 20

        /** Continue watching / reading: Watching (and rewatches) with a next episode or chapter. */
        fun continuing(r: ListRow) = (r.status == ListStatus.WATCHING || r.rewatching) && (r.total?.let { r.progress < it } ?: true)

        /** From the whole lists, last updated first. */
        fun of(anime: List<ListRow>, manga: List<ListRow>, airing: List<HomeAiring> = emptyList()) = Shelves(
            watching = anime.filter(::continuing),
            reading = manga.filter(::continuing),
            plan = anime.filter { it.status == ListStatus.PLAN_TO_WATCH },
            airing = airing,
            loaded = true,
        )
    }
}

/**
 * Home, as the desktop's (`app/src/home.rs`) and iOS's: Continue watching (wide cards with the
 * next episode's +1), Continue reading with Manga on, Airing this week (today's chip filled) and
 * Plan to watch. Each shelf scrolls
 * sideways and links to its full page ("See all"). Only reads the saved list.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    shelves: Shelves,
    account: Account?,
    mangaEnabled: Boolean,
    syncing: Boolean,
    onSync: () -> Unit,
    onPlusOne: (ListRow) -> Unit,
    onOpen: (ListRow) -> Unit,
    onSeeAll: (MediaKind, ListStatus) -> Unit,
    modifier: Modifier = Modifier,
    /** The Statistics page's numbers for the "Your stats" card (null: not read yet). */
    stats: StatsData? = null,
    onStats: () -> Unit = {},
    /** An Airing this week card (the details of that show). */
    onOpenAiring: (HomeAiring) -> Unit = {},
    /** Airing this week's "See all": the Airing page. */
    onAiring: () -> Unit = {},
) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg)) {
        TopBar("Home", account, syncing, onSync)
        PullToRefreshBox(isRefreshing = syncing, onRefresh = onSync, modifier = Modifier.weight(1f)) {
            Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = 8.dp, bottom = 80.dp), verticalArrangement = Arrangement.spacedBy(26.dp)) {
                Shelf("Continue watching", shelves.watching, shelves.loaded, "Nothing to continue: start something from Plan to watch.", { onSeeAll(MediaKind.ANIME, ListStatus.WATCHING) }) {
                    WideCard(it, { onPlusOne(it) }, { onOpen(it) })
                }
                if (mangaEnabled) {
                    Shelf("Continue reading", shelves.reading, shelves.loaded, "Nothing to continue: start something from Plan to read.", { onSeeAll(MediaKind.MANGA, ListStatus.WATCHING) }) {
                        WideCard(it, { onPlusOne(it) }, { onOpen(it) })
                    }
                }
                Shelf("Airing this week", shelves.airing, shelves.loaded, "None of your shows airs in the next seven days.", onAiring, { "air-${it.id}" }) {
                    AiringCard(it) { onOpenAiring(it) }
                }
                Shelf("Plan to watch", shelves.plan, shelves.loaded, "Your Plan to watch list is empty.", { onSeeAll(MediaKind.ANIME, ListStatus.PLAN_TO_WATCH) }) {
                    PosterCard(it) { onOpen(it) }
                }
                StatsCard(stats, onStats)
            }
        }
    }
}

@Composable
private fun Shelf(title: String, rows: List<ListRow>, loaded: Boolean, none: String, onSeeAll: () -> Unit, card: @Composable (ListRow) -> Unit) =
    Shelf(title, rows, loaded, none, onSeeAll, { "${it.kind}-${it.id}" }, card)

@Composable
private fun <T> Shelf(title: String, rows: List<T>, loaded: Boolean, none: String, onSeeAll: () -> Unit, key: (T) -> Any, card: @Composable (T) -> Unit) {
    val k = Kotora.colors
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(title, color = k.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
            Text("${rows.size}", color = k.faint, fontSize = 13.sp)
            Fill()
            Row(
                Modifier.clip(RoundedCornerShape(6.dp)).clickable(role = Role.Button, onClick = onSeeAll).padding(horizontal = 4.dp, vertical = 4.dp)
                    .semantics { contentDescription = "See all: $title" },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text("See all", color = k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium)
                Icon(KIcons.Chevron, null, tint = k.dim, modifier = Modifier.size(14.dp))
            }
        }
        if (rows.isEmpty()) {
            Text(if (loaded) none else " ", color = k.faint, fontSize = 13.sp, modifier = Modifier.padding(horizontal = 16.dp))
        } else {
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                items(rows.take(Shelves.MAX), key = key) { card(it) }
            }
        }
    }
}

/** Continue watching / reading: the cover behind, the poster, the title, how far, the bar and +1. */
@Composable
private fun WideCard(row: ListRow, onPlusOne: () -> Unit, onOpen: () -> Unit) {
    val k = Kotora.colors
    val manga = row.kind == MediaKind.MANGA
    val unit = if (manga) "Ch" else "Ep"
    val next = row.progress + 1u
    val shape = RoundedCornerShape(14.dp)
    Box(
        Modifier.size(300.dp, 124.dp).clip(shape).background(k.surface, shape).border(1.dp, k.border, shape)
            .clickable(onClick = onOpen).semantics { contentDescription = "About ${row.title}" },
    ) {
        Box(Modifier.matchParentSize().blur(18.dp).alpha(.35f).background(tileBrush(row.id.toInt())))
        Row(Modifier.padding(10.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Cover(row, 70.dp, 104.dp, 6.dp, 20.sp)
            Column(Modifier.weight(1f).height(104.dp)) {
                Text(row.title, color = k.text, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text("$unit ${row.progress} / ${row.total ?: "?"}", color = k.dim, fontSize = 12.5.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(top = 4.dp))
                Box(Modifier.weight(1f))
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    val total = row.total
                    val fraction = if (total != null && total > 0u) row.progress.toFloat() / total.toFloat() else if (row.progress > 0u) .15f else 0f
                    ProgressBar(fraction, k.status(if (row.rewatching) ListStatus.COMPLETED else row.status), Modifier.weight(1f))
                    val bshape = RoundedCornerShape(7.dp)
                    Box(
                        Modifier.height(30.dp).clip(bshape).background(k.bg.copy(alpha = .7f), bshape).border(1.dp, k.borderStrong, bshape)
                            .clickable(role = Role.Button, onClick = onPlusOne).padding(horizontal = 10.dp)
                            .semantics { contentDescription = "+1, ${if (manga) "chapter" else "episode"} $next" },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text("+1  $unit $next", color = k.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
        }
    }
}

/** Plan to watch: the poster, the title, what it is. */
@Composable
private fun PosterCard(row: ListRow, onOpen: () -> Unit) {
    val k = Kotora.colors
    Column(Modifier.width(112.dp).clickable(onClick = onOpen).semantics { contentDescription = "About ${row.title}" }) {
        Cover(row, 112.dp, 160.dp, 8.dp, 24.sp)
        Text(row.title, color = k.text, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 6.dp))
        val f = formatLabel(row.format)
        val meta = listOfNotNull(f.takeIf { it.isNotEmpty() }, row.total?.let { "$it ep" }).joinToString(" · ")
        if (meta.isNotEmpty()) Text(meta, color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
    }
}

/** Airing this week: the poster with the episode chip (filled today), the title and when. */
@Composable
private fun AiringCard(a: HomeAiring, onOpen: () -> Unit) {
    val k = Kotora.colors
    Column(Modifier.width(112.dp).clickable(onClick = onOpen).semantics(mergeDescendants = true) { contentDescription = "${a.title}, ${a.episode}, ${a.`when`}" }) {
        Box {
            Cover(a.id, a.title, a.coverUrl, 112.dp, 160.dp, 8.dp, 24.sp)
            val shape = RoundedCornerShape(6.dp)
            Text(
                a.episode,
                color = if (a.today) Color.White else k.text,
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(6.dp).clip(shape).background(if (a.today) k.accent else k.bg.copy(alpha = .75f), shape).padding(horizontal = 6.dp, vertical = 2.dp),
            )
        }
        Text(a.title, color = k.text, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 6.dp))
        Text(a.`when`, color = if (a.today) k.accent else k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
    }
}

/** Home's last card: this week from the Statistics page (same number and line), and a link to the page. */
@Composable
private fun StatsCard(st: StatsData?, onOpen: () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(14.dp)
    Column(
        Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(shape).background(k.surface, shape).border(1.dp, k.border, shape)
            .clickable(role = Role.Button, onClick = onOpen).padding(14.dp)
            .semantics(mergeDescendants = true) { contentDescription = "Your stats, see all" },
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Your stats", color = k.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
            Fill()
            Text("See all", color = k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            Icon(KIcons.Chevron, null, tint = k.dim, modifier = Modifier.padding(start = 4.dp).size(14.dp))
        }
        if (st == null) Text(" ", fontSize = 13.sp) else Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text("THIS WEEK", color = k.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = .4.sp)
            Text(StatsText.grouped(st.week.units.toULong()), color = k.text, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
            Text(periodLine(st, st.week, Words(st.kind)), color = k.dim, fontSize = 12.5.sp)
        }
    }
}
