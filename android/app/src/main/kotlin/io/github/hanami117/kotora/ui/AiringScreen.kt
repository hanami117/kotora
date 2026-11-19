package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.AiringCard
import io.github.hanami117.kotora.core.AiringMine
import io.github.hanami117.kotora.core.AiringOrder
import io.github.hanami117.kotora.core.AiringSlot
import io.github.hanami117.kotora.core.AiringType
import io.github.hanami117.kotora.core.AiringUntimed
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.delay

/*
 * The Airing page's week, the desktop's (`app/src/airing.rs`, view Week) on a phone: the next 7
 * days from today in the user's time zone, one line per episode (the core's estimate), the list's
 * shows lit; then the airing shows with no time to estimate. The type and list filters as chips.
 * The texts are the core's (the desktop's words). A line of a show on the list opens its page.
 * While on screen it is made again each minute (the countdowns).
 */

private val TYPES = listOf(AiringType.ALL to "All types", AiringType.TV to "TV", AiringType.ONA to "ONA", AiringType.MOVIE to "Movie")
private val MINE = listOf(AiringMine.ALL to "Everything", AiringMine.ON to "On my list", AiringMine.OFF to "Not on my list")
private val ORDERS = listOf(AiringOrder.POPULAR to "Most popular", AiringOrder.TO_WATCH to "Most to watch")

@Composable
fun AiringScreen(
    ui: AiringUi,
    account: Account?,
    onOpen: () -> Unit,
    onRefresh: () -> Unit,
    onTick: suspend () -> Unit,
    onFilter: (AiringType?, AiringMine?) -> Unit,
    onShow: (UInt) -> Unit,
    onView: (grid: Boolean) -> Unit = {},
    onOrder: (AiringOrder) -> Unit = {},
    onPick: (Int) -> Unit = {},
    modifier: Modifier = Modifier,
    ticking: Boolean = true,
) {
    val k = Kotora.colors
    LaunchedEffect(Unit) { onOpen() }
    if (ticking) {
        LaunchedEffect(Unit) {
            while (true) {
                val ms = System.currentTimeMillis()
                delay(60_000L - ms % 60_000L + 50L)
                onTick()
            }
        }
    }
    val week = ui.week
    Column(modifier.fillMaxSize().background(k.bg)) {
        TopBar("Airing", account, ui.loading, onRefresh)
        Row(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            SeasonPicker(ui, week?.season ?: "", onPick)
            if (ui.loading && week?.hasShows == true) {
                Text("Updating…", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(start = 12.dp))
            }
        }
        val shows = if (week?.hasShows == true) 1u else 0u
        Segmented(listOf("This week", if (ui.offset == 0) week?.season ?: "Season" else ui.other?.season ?: "Season"), if (ui.grid) 1 else 0, { onView(it == 1) }, Modifier.padding(horizontal = 16.dp).padding(bottom = 10.dp))
        ui.error?.let { e ->
            Alert(if (shows == 0u && ui.other?.hasShows != true) e else "$e Showing the saved season.", onRefresh)
        }
        if (ui.grid && ui.offset != 0) {
            val g = ui.other
            when {
                g == null || (!g.hasShows && ui.loading) -> Loading(g?.season ?: "")
                !g.hasShows -> Empty(if (ui.error != null) "No season to show" else "Nothing listed", "Kotora shows the season once ${week?.serviceTitle ?: "the service"} answers.")
                else -> {
                    Filters(ui, onFilter, onOrder)
                    Grid(g.cards, onShow)
                }
            }
            return@Column
        }
        if (week == null || shows == 0u) {
            val svc = week?.serviceTitle ?: "MyAnimeList"
            when {
                ui.loading || (week != null && week.fetchedAt == null && ui.error == null) -> Loading(week?.season ?: "")
                week == null -> Loading("")
                ui.error != null -> Empty("No season to show", "Kotora shows the season once $svc answers.")
                else -> Empty("Nothing airing", "$svc lists no anime for this season yet.")
            }
            return@Column
        }
        Filters(ui, onFilter, onOrder)
        if (ui.grid) {
            Grid(week.cards, onShow)
            return@Column
        }
        if (week.days.all { it.slots.isEmpty() } && week.untimed.isEmpty()) {
            Empty("Nothing airs this week with these filters", "Try Everything, or All types.")
            return@Column
        }
        LazyColumn(Modifier.fillMaxSize()) {
            week.days.forEachIndexed { d, day ->
                item(key = "day/$d") { DayHead(day.label, day.today, day.onList.ifEmpty { null }) }
                if (day.slots.isEmpty()) {
                    item(key = "nothing/$d") {
                        Text("Nothing airs", color = k.faint, fontSize = 13.sp, modifier = Modifier.height(36.dp).padding(start = 82.dp, top = 8.dp))
                    }
                }
                for (s in day.slots) item(key = "slot/$d/${s.id}") { SlotRow(s, onShow) }
            }
            if (week.untimed.isNotEmpty()) {
                item(key = "untimed") { DayHead("No broadcast time", false, "airing on ${week.serviceTitle}, no time to estimate") }
                for (u in week.untimed) item(key = "untimed/${u.id}") { UntimedRow(u, onShow) }
            }
            item(key = "foot") {
                Text(
                    "Times are estimated; ~ marks estimated counts",
                    color = k.faint,
                    fontSize = 11.sp,
                    modifier = Modifier.fillMaxWidth().padding(16.dp),
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

@Composable
private fun Filters(ui: AiringUi, onFilter: (AiringType?, AiringMine?) -> Unit, onOrder: (AiringOrder) -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for ((t, label) in TYPES) Chip(label, ui.type == t) { onFilter(t, null) }
        Box(Modifier.padding(horizontal = 4.dp).size(1.dp, 18.dp).background(k.border))
        for ((m, label) in MINE) Chip(label, ui.mine == m) { onFilter(null, m) }
        if (ui.grid) {
            Box(Modifier.padding(horizontal = 4.dp).size(1.dp, 18.dp).background(k.border))
            for ((o, label) in ORDERS) Chip(label, ui.order == o) { onOrder(o) }
        }
    }
}

@Composable
private fun Chip(label: String, on: Boolean, onClick: () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(16.dp)
    Text(
        label,
        color = if (on) k.accentHi else k.dim,
        fontSize = 13.sp,
        fontWeight = FontWeight.Medium,
        maxLines = 1,
        modifier = Modifier
            .clip(shape)
            .background(if (on) k.tint else k.field, shape)
            .border(1.dp, if (on) k.accent.copy(alpha = .45f) else k.border, shape)
            .clickable(role = Role.Tab, onClick = onClick)
            .semantics { selected = on }
            .padding(horizontal = 12.dp, vertical = 6.dp),
    )
}

@Composable
private fun DayHead(label: String, today: Boolean, end: String?) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().height(40.dp).padding(start = 16.dp, end = 16.dp, top = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(label.uppercase(), color = if (today) k.accentHi else k.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = .6.sp)
        if (end != null) Text(end, color = k.faint, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
        if (today) Box(Modifier.size(6.dp).background(k.accent, CircleShape))
    }
}

@Composable
private fun SmallCover(id: UInt, title: String, url: String?) {
    val shape = RoundedCornerShape(4.dp)
    Box(Modifier.size(30.dp, 42.dp).clip(shape).background(tileBrush(id.toInt()), shape), contentAlignment = Alignment.Center) {
        Text(initials(title), color = Color.White.copy(alpha = .92f), style = TextStyle(fontWeight = FontWeight.SemiBold, fontSize = 11.sp, shadow = Shadow(Color.Black.copy(alpha = .35f), Offset(0f, 1f), 2f)))
        if (url != null) AsyncImage(model = url, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
    }
}

@Composable
private fun StatusTag(status: ListStatus) {
    val c = Kotora.colors.status(status)
    Text(
        statusLabel(status, MediaKind.ANIME),
        color = c,
        fontSize = 11.sp,
        fontWeight = FontWeight.Medium,
        maxLines = 1,
        modifier = Modifier.background(c.copy(alpha = .13f), RoundedCornerShape(4.dp)).padding(horizontal = 7.dp, vertical = 2.dp),
    )
}

@Composable
private fun SlotRow(s: AiringSlot, onShow: (UInt) -> Unit) {
    val k = Kotora.colors
    val mine = s.status != null
    val lit = if (mine) Modifier.background(k.accent.copy(alpha = .06f)) else Modifier
    Box(Modifier.fillMaxWidth().then(lit)) {
        Row(
            Modifier.fillMaxWidth().height(56.dp).then(if (s.status != null) Modifier.clickable { onShow(s.id) } else Modifier).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(s.time, color = if (mine) k.text else k.dim, fontSize = 14.sp, fontWeight = FontWeight.Medium, modifier = Modifier.width(42.dp))
            SmallCover(s.id, s.title, s.coverUrl)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    // A phone has no room for the desktop's status tag: its colour, as on the list.
                    s.status?.let { StatusDot(it) }
                    Text(s.title, color = if (mine) k.text else k.dim, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Ep ${s.episode}", color = k.dim, fontSize = 12.sp)
                    if (s.kindLabel.isNotEmpty()) Tag(s.kindLabel)
                    if (s.note.isNotEmpty()) Text(s.note, color = if (s.note == "aired") k.faint else k.accentHi, fontSize = 12.sp, maxLines = 1)
                    s.toWatch?.let { w ->
                        Text("·", color = k.faint, fontSize = 12.sp)
                        Text(w, color = if (w.contains("to watch")) k.text else k.faint, fontSize = 12.sp, maxLines = 1)
                    }
                }
            }
        }
        if (mine) Box(Modifier.padding(top = 8.dp).size(3.dp, 40.dp).background(k.accent, RoundedCornerShape(1.5.dp)))
    }
    Box(Modifier.fillMaxWidth().height(1.dp).background(k.border.copy(alpha = .5f)))
}

@Composable
private fun UntimedRow(u: AiringUntimed, onShow: (UInt) -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().height(56.dp).then(if (u.status != null) Modifier.clickable { onShow(u.id) } else Modifier).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("–", color = k.faint, fontSize = 14.sp, modifier = Modifier.width(42.dp))
        SmallCover(u.id, u.title, u.coverUrl)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(u.title, color = k.dim, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(u.why, color = k.faint, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        u.status?.let { StatusTag(it) }
    }
}

@Composable
private fun Alert(text: String, onRetry: () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 10.dp)
            .background(k.dropped.copy(alpha = .10f), RoundedCornerShape(10.dp))
            .border(1.dp, k.dropped.copy(alpha = .35f), RoundedCornerShape(10.dp))
            .padding(start = 12.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, color = k.text, fontSize = 13.sp, modifier = Modifier.weight(1f))
        KButton("Retry", onRetry, kind = ButtonKind.SECONDARY, height = 32.dp, fontSize = 13.sp)
    }
}

@Composable
private fun Loading(season: String) {
    val k = Kotora.colors
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(top = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(if (season.isEmpty()) "Loading…" else "Loading $season…", color = k.faint, fontSize = 13.sp, modifier = Modifier.padding(bottom = 2.dp))
        repeat(4) { Box(Modifier.fillMaxWidth().height(44.dp).background(k.raised, RoundedCornerShape(8.dp))) }
    }
}

@Composable
private fun Empty(title: String, help: String) {
    val k = Kotora.colors
    Column(Modifier.fillMaxSize().padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        Text(title, color = k.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
        Spacer(Modifier.height(4.dp))
        Text(help, color = k.dim, fontSize = 13.sp, textAlign = TextAlign.Center)
        Spacer(Modifier.height(60.dp))
    }
}

/** Now, Next and back about ten years, as the desktop's picker; the current season is the week's. */
@Composable
private fun SeasonPicker(ui: AiringUi, current: String, onPick: (Int) -> Unit) {
    val k = Kotora.colors
    var open by remember { mutableStateOf(false) }
    val label = if (ui.offset == 0) current else ui.picks.firstOrNull { it.offset == ui.offset }?.label ?: current
    Box {
        Row(
            Modifier.clip(RoundedCornerShape(6.dp)).clickable(enabled = ui.picks.isNotEmpty(), role = Role.DropdownList) { open = true }.padding(vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(label, color = k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            if (ui.picks.isNotEmpty()) Text("\u25BE", color = k.faint, fontSize = 12.sp)
        }
        DropdownMenu(open, { open = false }, containerColor = k.raised) {
            for (p in ui.picks) {
                val tag = when (p.offset) { 0 -> " \u00b7 Now"; 1 -> " \u00b7 Next"; else -> "" }
                DropdownMenuItem(
                    text = { Text(p.label + tag, color = if (p.offset == ui.offset) k.accentHi else k.text, fontSize = 14.sp) },
                    onClick = { open = false; onPick(p.offset) },
                )
            }
        }
    }
}

/** The season grid, two cards a row: cover, title, meta, when; the list's shows lit. */
@Composable
private fun Grid(cards: List<AiringCard>, onShow: (UInt) -> Unit) {
    val k = Kotora.colors
    if (cards.isEmpty()) {
        Empty("No anime with these filters", "Try Everything, or All types.")
        return
    }
    LazyVerticalGrid(GridCells.Adaptive(160.dp), Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 16.dp), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        items(cards, key = { it.id.toLong() }) { c -> SeasonCard(c, onShow) }
        item(span = { GridItemSpan(maxLineSpan) }) {
            Text("Times are estimated; ~ marks estimated counts", color = k.faint, fontSize = 11.sp, modifier = Modifier.fillMaxWidth().padding(8.dp), textAlign = TextAlign.Center)
        }
    }
}

@Composable
private fun SeasonCard(c: AiringCard, onShow: (UInt) -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    Column(
        Modifier.clip(shape).background(k.surface, shape).border(1.dp, if (c.lit) k.accent.copy(alpha = .55f) else k.border, shape)
            .then(if (c.status != null) Modifier.clickable { onShow(c.id) } else Modifier),
    ) {
        Box(Modifier.fillMaxWidth().height(120.dp).background(tileBrush(c.id.toInt())), contentAlignment = Alignment.Center) {
            Text(initials(c.title), color = Color.White.copy(alpha = .92f), fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
            if (c.coverUrl != null) AsyncImage(model = c.coverUrl, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
            c.status?.let { Box(Modifier.align(Alignment.TopStart).padding(6.dp)) { StatusTag(it) } }
        }
        Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(c.title, color = k.text, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(c.meta, color = k.faint, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (c.`when`.isNotEmpty()) Text(c.`when`, color = if (c.lit) k.accentHi else k.dim, fontSize = 11.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
            c.toWatch?.let { Text(it, color = if (it.contains("to watch")) k.text else k.faint, fontSize = 11.sp) }
        }
    }
}
