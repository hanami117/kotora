package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.ServiceLink
import io.github.hanami117.kotora.core.ListEmptyKind
import io.github.hanami117.kotora.core.ListSort
import io.github.hanami117.kotora.core.ListViewMode
import io.github.hanami117.kotora.core.PageLine
import io.github.hanami117.kotora.core.QuickFilter
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import io.github.hanami117.kotora.ui.theme.Kotora

fun serviceTitle(s: Service) = if (s == Service.MAL) "MyAnimeList" else "AniList"

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ListScreen(
    ui: ListUi,
    account: Account?,
    now: Long,
    onKind: (MediaKind) -> Unit,
    onStatus: (ListStatus) -> Unit,
    onSort: (ListSort) -> Unit,
    onSync: () -> Unit,
    onPlusOne: (ListRow) -> Unit,
    onOpen: (ListRow) -> Unit,
    modifier: Modifier = Modifier,
    expired: Service? = null,
    onSignIn: (Service) -> Unit = {},
    linksFor: (ListRow) -> List<ServiceLink> = { emptyList() },
    onOpenUrl: (String) -> Unit = {},
    onReverse: (Boolean) -> Unit = {},
    onFilter: (QuickFilter) -> Unit = {},
    onQuery: (String) -> Unit = {},
    onView: (ListViewMode) -> Unit = {},
) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg)) {
        TopBar("My list", account, ui.syncing, onSync)
        Segmented(
            listOf("Anime", "Manga"),
            if (ui.kind == MediaKind.ANIME) 0 else 1,
            { onKind(if (it == 0) MediaKind.ANIME else MediaKind.MANGA) },
            Modifier.padding(horizontal = 16.dp),
        )
        LazyRow(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(ui.counts, key = { it.status.name }) { c ->
                StatusChip(c.status, ui.kind, c.count, c.status == ui.status) { onStatus(c.status) }
            }
        }
        SyncLine(ui, account, now, onSort, onReverse)
        if (expired != null) SignInAgainBanner(expired, onSignIn)
        FilterBox(ui, onQuery)
        if (ui.query.isBlank()) QuickChips(ui, onFilter, onView)
        PullToRefreshBox(isRefreshing = ui.syncing, onRefresh = onSync, modifier = Modifier.weight(1f)) {
            if (ui.loaded && ui.lines.isEmpty()) {
                EmptyList(ui)
            } else if (ui.view != ListViewMode.LIST) {
                PosterGrid(ui.lines, ui.view == ListViewMode.LARGE, onOpen, onPlusOne)
            } else {
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 80.dp)) {
                    items(ui.lines, key = { "${it.row.kind}-${it.row.id}" }) { line ->
                        val row = line.row
                        ListRowItem(row, onPlusOne = { onPlusOne(row) }, onOpen = { onOpen(row) }, links = { linksFor(row) }, onOpenUrl = onOpenUrl, line = line)
                        HorizontalDivider(color = k.border, thickness = 1.dp)
                    }
                }
            }
        }
    }
}

@Composable
fun TopBar(title: String, account: Account?, syncing: Boolean, onSync: (() -> Unit)?) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(start = 16.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        KotoraLogo(30.dp)
        Text(title, style = MaterialTheme.typography.titleLarge, color = k.text, modifier = Modifier.weight(1f))
        if (onSync != null) {
            IconButton(onClick = onSync, enabled = !syncing, modifier = Modifier.semantics { contentDescription = "Sync now" }) {
                if (syncing) {
                    CircularProgressIndicator(Modifier.size(20.dp), color = k.dim, strokeWidth = 2.dp)
                } else {
                    Icon(KIcons.Sync, null, tint = k.dim)
                }
            }
        }
        if (account != null) AccountMenu(account)
    }
}

@Composable
fun Avatar(account: Account) {
    val k = Kotora.colors
    val brush = if (account.service == Service.MAL) {
        Brush.linearGradient(listOf(Color(0xFF3B5BA8), Color(0xFF2D4C8F)))
    } else {
        Brush.linearGradient(listOf(Color(0xFF2B9FD8), Color(0xFF1D6FA5)))
    }
    Box(Modifier.size(32.dp)) {
        Box(Modifier.size(32.dp).clip(CircleShape).background(brush), contentAlignment = Alignment.Center) {
            Text(account.userName.take(1).uppercase().ifEmpty { "?" }, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            if (account.pictureUrl != null) {
                coil3.compose.AsyncImage(model = account.pictureUrl, contentDescription = null, modifier = Modifier.fillMaxSize())
            }
        }
        Box(
            Modifier.size(14.dp).offset(21.dp, 21.dp).clip(CircleShape).background(k.bg).padding(2.dp).clip(CircleShape).background(k.watching),
        )
    }
}

@Composable
private fun SyncLine(ui: ListUi, account: Account?, now: Long, onSort: (ListSort) -> Unit, onReverse: (Boolean) -> Unit) {
    val k = Kotora.colors
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        val service = account?.let { serviceTitle(it.service) } ?: ""
        val (dot, text) = when {
            ui.problem != null -> k.onHold to ui.problem
            ui.syncing -> k.faint to "Syncing with $service…"
            ui.syncedAt != null -> k.watching to "Synced ${ago(ui.syncedAt, now)} · $service"
            else -> k.faint to "Not synced yet · $service"
        }
        Box(Modifier.size(8.dp).clip(CircleShape).background(dot))
        Text(text, color = k.faint, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        Box {
            Row(
                Modifier.clip(RoundedCornerShape(6.dp)).clickable(role = Role.DropdownList) { menu = true }.padding(horizontal = 8.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text("Sort: ${listSortLabel(ui.sort)}", color = k.dim, fontSize = 12.sp)
                Text(if (ui.reverse) "↑" else "↓", color = k.dim, fontSize = 12.sp)
            }
            // The desktop's Sort menu: "Sort by" the four orders, then "Order" the two directions.
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = k.raised) {
                MenuHeading("Sort by")
                for (s in ListSort.entries) {
                    DropdownMenuItem(
                        text = { Text(listSortLabel(s), color = k.text) },
                        leadingIcon = { CheckSlot(s == ui.sort) },
                        onClick = { menu = false; onSort(s) },
                    )
                }
                HorizontalDivider(color = k.border, modifier = Modifier.padding(vertical = 4.dp))
                MenuHeading("Order")
                listSortDirections(ui.sort).forEachIndexed { i, d ->
                    DropdownMenuItem(
                        text = { Text(d, color = k.text) },
                        leadingIcon = { CheckSlot(ui.reverse == (i == 1)) },
                        onClick = { menu = false; onReverse(i == 1) },
                    )
                }
            }
        }
    }
}

@Composable
private fun MenuHeading(text: String) {
    Text(text.uppercase(), color = Kotora.colors.faint, fontSize = 11.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(start = 16.dp, top = 6.dp, bottom = 2.dp))
}

@Composable
private fun CheckSlot(on: Boolean) {
    Box(Modifier.size(16.dp)) { if (on) Icon(KIcons.Check, null, tint = Kotora.colors.accentHi, modifier = Modifier.size(16.dp)) }
}

/** "Filter your list": the whole list's matches while it holds text (desktop ⌘F). */
@Composable
private fun FilterBox(ui: ListUi, onQuery: (String) -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    Row(
        Modifier.padding(start = 16.dp, end = 16.dp, top = 6.dp, bottom = 2.dp).fillMaxWidth().height(40.dp)
            .clip(shape).background(k.field, shape).border(1.dp, k.border, shape).padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(KIcons.Search, null, tint = k.faint, modifier = Modifier.size(16.dp))
        Box(Modifier.weight(1f)) {
            if (ui.query.isEmpty()) Text(listFilterHint(ui.kind), color = k.faint, fontSize = 14.sp)
            BasicTextField(
                ui.query,
                onQuery,
                singleLine = true,
                textStyle = TextStyle(color = k.text, fontSize = 14.sp),
                cursorBrush = SolidColor(k.accent),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, imeAction = ImeAction.Search),
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = listFilterHint(ui.kind) },
            )
        }
        if (ui.query.isNotEmpty()) {
            Box(Modifier.size(24.dp).clip(CircleShape).clickable(role = Role.Button) { onQuery("") }.semantics { contentDescription = "Clear filter" }, contentAlignment = Alignment.Center) {
                Text("✕", color = k.faint, fontSize = 13.sp)
            }
        }
    }
}

/**
 * The chips above the list (All / Airing / Finished airing / New episode; manga Publishing /
 * Finished / On hiatus) and, at the end, the view switch (List / Grid / Large).
 */
@Composable
private fun QuickChips(ui: ListUi, onFilter: (QuickFilter) -> Unit, onView: (ListViewMode) -> Unit) {
    val k = Kotora.colors
    Row(Modifier.fillMaxWidth().padding(top = 6.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        LazyRow(
            Modifier.weight(1f),
            contentPadding = PaddingValues(start = 16.dp, end = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            items(quickFilters(ui.kind), key = { it.name }) { f ->
                val on = f == ui.filter
                Text(
                    quickFilterLabel(f),
                    color = if (on) k.text else k.dim,
                    fontSize = 12.5.sp,
                    fontWeight = FontWeight.Medium,
                    modifier = Modifier.height(30.dp).clip(CircleShape)
                        .background(if (on) k.raised else Color.Transparent)
                        .border(1.dp, if (on) k.borderStrong else k.border, CircleShape)
                        .clickable(role = Role.Tab) { onFilter(f) }
                        .semantics { selected = on }
                        .padding(horizontal = 11.dp, vertical = 6.dp),
                )
            }
        }
        val shape = RoundedCornerShape(8.dp)
        Row(Modifier.padding(end = 14.dp).border(1.dp, k.border, shape).padding(2.dp)) {
            for (v in ListViewMode.entries) {
                val on = v == ui.view
                val icon = when (v) {
                    ListViewMode.LIST -> KIcons.List
                    ListViewMode.GRID -> KIcons.Grid
                    ListViewMode.LARGE -> KIcons.Large
                }
                Box(
                    Modifier.size(34.dp, 28.dp).clip(RoundedCornerShape(6.dp)).background(if (on) k.raised else Color.Transparent)
                        .clickable(role = Role.Tab) { onView(v) }
                        .semantics { contentDescription = "${listViewLabel(v)} view"; selected = on },
                    contentAlignment = Alignment.Center,
                ) { Icon(icon, null, tint = if (on) k.text else k.faint, modifier = Modifier.size(16.dp)) }
            }
        }
    }
}

@Composable
private fun EmptyList(ui: ListUi) {
    val k = Kotora.colors
    // Loading, the status is empty (desktop `empty`), or nothing matches (`no_match`).
    Column(
        Modifier.fillMaxSize().padding(horizontal = 32.dp).padding(top = 80.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (ui.empty == ListEmptyKind.LOADING) {
            Text(ui.emptyTitle, color = k.faint, textAlign = TextAlign.Center)
        } else {
            Text(ui.emptyTitle, color = k.text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
            if (ui.emptyHelp.isNotEmpty()) Text(ui.emptyHelp, color = k.dim, fontSize = 14.sp, textAlign = TextAlign.Center)
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ListRowItem(row: ListRow, onPlusOne: () -> Unit, onOpen: () -> Unit, links: () -> List<ServiceLink> = { emptyList() }, onOpenUrl: (String) -> Unit = {}, line: PageLine? = null) {
    val k = Kotora.colors
    // Long press: the desktop context menu's "Open on {service}" items (ctxmenu.rs `open_items`).
    var menu by remember { mutableStateOf<List<ServiceLink>>(emptyList()) }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 84.dp)
            .combinedClickable(onClick = onOpen, onLongClick = { menu = links() })
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Cover(row, 44.dp, 64.dp, 5.dp, 14.sp)
        Column(Modifier.weight(1f)) {
            Text(row.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Row(Modifier.padding(top = 4.dp, bottom = 7.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for ((text, kind) in line?.let { lineTags(it, tags(row)) } ?: tags(row)) Tag(text, kind)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                val total = row.total
                val fraction = if (total != null && total > 0u) row.progress.toFloat() / total.toFloat() else if (row.progress > 0u) .5f else 0f
                ProgressBar(fraction, k.status(row.status), Modifier.weight(1f))
                Text(
                    "${row.progress} / ${total ?: "?"}",
                    color = k.dim,
                    fontSize = 12.sp,
                    modifier = Modifier.semantics { contentDescription = "${row.progress} of ${total ?: "unknown"}" },
                )
                if (line != null) ScoreMarkView(line.scoreMark, 12.sp) else Text("★ ${row.score ?: "–"}", color = k.faint, fontSize = 12.sp, modifier = Modifier.width(34.dp))
            }
        }
        val atEnd = row.total?.let { row.progress >= it } == true
        val canPlus = !atEnd && (row.status == ListStatus.WATCHING || row.rewatching)
        if (canPlus) PlusButton(onPlusOne, row)
        DropdownMenu(expanded = menu.isNotEmpty(), onDismissRequest = { menu = emptyList() }, containerColor = k.raised) {
            for (l in menu) {
                DropdownMenuItem(
                    text = { Text(l.label, color = k.text) },
                    trailingIcon = { Icon(KIcons.External, null, tint = k.dim, modifier = Modifier.size(16.dp)) },
                    onClick = {
                        menu = emptyList()
                        onOpenUrl(l.url)
                    },
                )
            }
        }
    }
}

@Composable
private fun PlusButton(onClick: () -> Unit, row: ListRow) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(8.dp)
    val unit = if (row.kind == MediaKind.MANGA) "chapter" else "episode"
    Box(
        Modifier.size(56.dp, 40.dp).clip(shape).background(k.raised, shape).border(1.dp, k.borderStrong, shape)
            .clickable(role = Role.Button, onClick = onClick)
            .semantics { contentDescription = "Plus one $unit: ${row.title}" },
        contentAlignment = Alignment.Center,
    ) {
        Text("+1", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
    }
}

/** The small tags under a title: waiting, rewatch, airing, format. At most three. */
private fun tags(row: ListRow): List<Pair<String, TagKind>> {
    val out = mutableListOf<Pair<String, TagKind>>()
    if (row.rewatching) out += (if (row.kind == MediaKind.MANGA) "Rereading" else "Rewatching") to TagKind.PLAIN
    when (row.airStatus) {
        "currently_airing", "RELEASING" -> out += "Airing" to TagKind.AIR
        "currently_publishing" -> out += "Publishing" to TagKind.AIR
        "not_yet_aired", "NOT_YET_RELEASED", "not_yet_published" -> out += "Not aired yet" to TagKind.PLAIN
    }
    if (row.waiting) out += "Waiting to sync" to TagKind.WAIT
    if (out.size < 2) {
        val f = formatLabel(row.format)
        val total = row.total
        val unit = if (row.kind == MediaKind.MANGA) "ch" else "eps"
        if (f.isNotEmpty()) out += (if (total != null && row.kind == MediaKind.ANIME && f == "TV") "$f · $total $unit" else f) to TagKind.PLAIN
    }
    return out.take(3)
}

@Composable
fun Placeholder(title: String, text: String, account: Account?, modifier: Modifier = Modifier) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg)) {
        TopBar(title, account, false, null)
        Spacer(Modifier.height(48.dp))
        Text(text, color = k.faint, textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp))
    }
}
