package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.NamedCount
import io.github.hanami117.kotora.core.StatsData
import io.github.hanami117.kotora.core.StatsPeriod
import io.github.hanami117.kotora.core.StatsTitle
import io.github.hanami117.kotora.data.PagesSource
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlin.math.floor

/*
 * Statistics, the desktop's (`app/src/stats.rs`) and iOS's at a phone's width: totals, this week /
 * month / year and the streak, the activity calendar, when you watch, most watched lately, your
 * list (statuses and scores), genres and studios (authors), the airing shows you follow. Follows
 * the Anime | Manga switch. Local numbers (the list and Kotora's activity log): no request.
 */

internal class Words(kind: MediaKind) {
    val unit = if (kind == MediaKind.ANIME) "episode" else "chapter"
    val units = if (kind == MediaKind.ANIME) "episodes" else "chapters"
    val verb = if (kind == MediaKind.ANIME) "watched" else "read"
    fun count(n: ULong) = if (n == 1uL) "1 $unit" else "${StatsText.grouped(n)} $units"
}

@Composable
fun StatsScreen(
    pages: PagesSource,
    startKind: MediaKind,
    mangaEnabled: Boolean,
    listKey: Any?,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    /** The calendar's measured width is the phone's; tests may hand the data in. */
    initial: StatsData? = null,
) {
    val k = Kotora.colors
    var kind by remember { mutableStateOf(startKind) }
    var data by remember { mutableStateOf(initial) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(kind, listKey) {
        try {
            data = pages.stats(kind)
            error = null
        } catch (e: Exception) {
            error = "Can't read the statistics: ${e.message ?: ""}".trimEnd()
        }
    }
    Column(modifier.fillMaxSize().background(k.bg)) {
        Row(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack, modifier = Modifier.semantics { contentDescription = "Back" }) { Icon(KIcons.Back, null, tint = k.text) }
            Text("${if (kind == MediaKind.MANGA) "Manga" else "Anime"} statistics", color = k.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        }
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            if (mangaEnabled) Segmented(listOf("Anime", "Manga"), if (kind == MediaKind.MANGA) 1 else 0, { kind = if (it == 1) MediaKind.MANGA else MediaKind.ANIME })
            val d = data
            if (d != null && d.kind == kind) StatsContent(d)
            else Text(error ?: "Counting…", color = k.dim, fontSize = 15.sp, modifier = Modifier.fillMaxWidth().padding(vertical = 40.dp))
        }
    }
}

@Composable
private fun StatsContent(st: StatsData) {
    val w = Words(st.kind)
    val hasActivity = st.all.units > 0u || st.days.any { it > 0u }
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Figures(st, w)
        CalendarCard(st, w)
        WhenCard(st, w, hasActivity)
        LatelyCard(st, w, hasActivity)
        ListCard(st)
        TagsCard(st)
        if (st.kind == MediaKind.ANIME && st.airing.isNotEmpty()) StatsCard("Airing that you watch") {
            for (t in st.airing) TitleLine(t, t.total?.let { "Ep ${t.progress} of $it" } ?: "Ep ${t.progress}")
            Note("${st.airing.size} airing · Airing shows when the next ones come out.")
        }
    }
}

internal fun periodLine(st: StatsData, p: StatsPeriod, w: Words): String = when {
    st.kind == MediaKind.ANIME && p.minutes > 0uL -> StatsText.duration(p.minutes)
    st.kind == MediaKind.MANGA && p.volumes > 0u -> "${p.volumes} vol."
    p.titles == 1u -> "1 title"
    p.titles > 0u -> "${p.titles} titles"
    else -> "Nothing ${w.verb} yet"
}

@Composable
private fun Figures(st: StatsData, w: Words) {
    val tiles = mutableListOf<Triple<String, String, String>>()
    tiles += Triple(StatsText.grouped(st.units), if (st.kind == MediaKind.ANIME) "Episodes watched" else "Chapters read", "${StatsText.grouped(st.titles.toULong())} on your list")
    tiles += if (st.kind == MediaKind.ANIME) Triple(StatsText.duration(st.minutes), "Time watched", if (st.guessed > 0u) "Some lengths guessed" else "From episode lengths")
    else Triple(StatsText.grouped(st.volumes), "Volumes read", "From your list")
    for ((caption, per) in listOf("This week" to st.week, "This month" to st.month, "This year" to st.year)) tiles += Triple(StatsText.grouped(per.units.toULong()), caption, periodLine(st, per, w))
    tiles += Triple(StatsText.days(st.streak), "Streak", if (st.bestStreak > 0u) "Best ${StatsText.days(st.bestStreak)}" else "Days in a row")
    val k = Kotora.colors
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        for (pair in tiles.chunked(2)) Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            for (t in pair) {
                val shape = RoundedCornerShape(14.dp)
                Column(Modifier.weight(1f).background(k.surface, shape).border(1.dp, k.border, shape).padding(14.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Caption(t.second)
                    Text(t.first, color = k.text, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
                    Text(t.third, color = k.dim, fontSize = 12.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

@Composable
private fun WhenCard(st: StatsData, w: Words, hasActivity: Boolean) {
    StatsCard(if (st.kind == MediaKind.ANIME) "When you watch" else "When you read") {
        if (!hasActivity) {
            EmptyLine(st, w)
        } else {
            val topDay = st.weekdays.indices.maxByOrNull { st.weekdays[it] * 8u + (7 - it).toUInt() } ?: 0
            val topHour = st.hours.indices.maxByOrNull { st.hours[it] * 32u + (24 - it).toUInt() } ?: 0
            val days = listOf("Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday")
            Text("Mostly on ${days[topDay]}s, around ${"%02d".format(topHour)}:00.", color = Kotora.colors.dim, fontSize = 15.sp)
            Caption("By day")
            Bars(st.weekdays, listOf("M", "T", "W", "T", "F", "S", "S"), 56.dp, 8.dp)
            Caption("By hour")
            Bars(st.hours, (0 until 24).map { if (it % 6 == 0) "$it" else "" }, 56.dp, 2.dp)
        }
    }
}

@Composable
private fun EmptyLine(st: StatsData, w: Words) {
    val start = st.sinceDay?.let { "Kotora counts from ${StatsText.dateText(it)}" } ?: "Kotora counts from now on"
    Text("$start, and nothing is ${w.verb} yet. ${w.units.replaceFirstChar { it.uppercase() }} you record show up here.", color = Kotora.colors.dim, fontSize = 15.sp)
}

@Composable
private fun LatelyCard(st: StatsData, w: Words, hasActivity: Boolean) {
    StatsCard(if (st.kind == MediaKind.ANIME) "Most watched lately" else "Most read lately") {
        if (st.lately.isEmpty()) {
            Text(if (hasActivity) "Nothing ${w.verb} in the last 30 days." else "Your most ${w.verb} titles of the last 30 days show here.", color = Kotora.colors.dim, fontSize = 15.sp)
        } else {
            Caption("Last 30 days")
            for (t in st.lately) TitleLine(t, w.count(t.units.toULong()))
        }
    }
}

@Composable
private fun ListCard(st: StatsData) {
    val k = Kotora.colors
    val counts = ListStatus.entries.zip(st.statuses)
    val scored = st.scores.sumOf { it.toInt() }
    val mean = if (scored > 0) st.scores.withIndex().sumOf { (i, c) -> c.toDouble() * (i + 1) } / scored else null
    StatsCard("Your list") {
        Caption("By status")
        val total = maxOf(1, counts.sumOf { it.second.toInt() })
        Row(Modifier.fillMaxWidth().height(10.dp).clip(CircleShape).background(k.border), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            for ((s, c) in counts) if (c > 0u) Box(Modifier.weight(c.toFloat() / total).fillMaxHeight().background(k.status(s)))
        }
        for (pair in counts.chunked(2)) Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            for ((s, c) in pair) Row(Modifier.weight(1f).padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                StatusDot(s)
                Text(statusLabel(s, st.kind), color = k.dim, fontSize = 13.sp, maxLines = 1, modifier = Modifier.weight(1f))
                Text(StatsText.grouped(c.toULong()), color = k.text, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            }
            if (pair.size == 1) Spacer(Modifier.weight(1f))
        }
        Caption("Scores")
        Bars(st.scores, (1..10).map { "$it" }, 64.dp, 6.dp)
        Text(mean?.let { "${StatsText.grouped(scored.toULong())} scored · mean ${"%.2f".format(it)} (out of 10)" } ?: "No scores on your list yet.", color = k.dim, fontSize = 13.sp)
    }
}

@Composable
private fun TagsCard(st: StatsData) {
    val k = Kotora.colors
    val anime = st.kind == MediaKind.ANIME
    StatsCard(if (anime) "Genres and studios" else "Genres and authors") {
        Caption("Genres")
        if (st.genres.isEmpty()) Text("Genres come from the details pages Kotora has saved: open a title's page and it counts here.", color = k.dim, fontSize = 13.sp)
        else Ranked(st.genres)
        Caption(if (anime) "Studios" else "Authors")
        if (st.makers.isEmpty()) Text(if (anime) "None saved yet." else "None on your list yet.", color = k.dim, fontSize = 13.sp)
        else Ranked(st.makers)
        if (st.genres.isNotEmpty() || anime) {
            Note("From the ${StatsText.grouped(st.withDetails.toULong())} of your ${StatsText.grouped(st.watched.toULong())} titles (not Plan to ${if (anime) "watch" else "read"}) whose details are saved: open a title's page to add it.")
        }
    }
}

private val MONTHS = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")

/** The last weeks as columns of days (Monday on top), as many as fit; four steps of orange. */
@Composable
private fun CalendarCard(st: StatsData, w: Words) {
    val k = Kotora.colors
    val gap = 3.dp
    val dayCol = 26.dp
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val inner = maxWidth - 32.dp
        val room = (inner - dayCol).value
        val cell = floor((room + gap.value) / 53 - gap.value).coerceIn(10f, 15f)
        val weeks = floor((room + gap.value) / (cell + gap.value)).toInt().coerceIn(8, 53)
        val first = st.today - StatsText.weekday(st.today).toLong() - (weeks - 1) * 7L
        fun value(day: Long): UInt { val i = day - st.firstDay; return if (i >= 0 && i < st.days.size) st.days[i.toInt()] else 0u }
        val inView = (0 until weeks * 7).sumOf { i -> val d = first + i; if (d > st.today) 0uL else value(d).toULong() }
        val max = st.days.maxOrNull() ?: 0u
        fun shade(n: UInt, m: UInt): Color {
            if (n == 0u) return k.border
            val mm = maxOf(m, 1u)
            val step = ((4u * n + mm - 1u) / mm).coerceIn(1u, 4u).toInt()
            return k.accent.copy(alpha = listOf(.3f, .5f, .75f, 1f)[step - 1])
        }
        val shape = RoundedCornerShape(14.dp)
        Column(Modifier.fillMaxWidth().background(k.surface, shape).border(1.dp, k.border, shape).padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Activity", color = k.text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                Text("${w.count(inView)} in $weeks weeks", color = k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            }
            Box(Modifier.fillMaxWidth().height(14.dp)) {
                for (wk in 0 until weeks) {
                    val m = StatsText.month(first + wk * 7L + 6)
                    val prev = if (wk == 0) 0u else StatsText.month(first + (wk - 1) * 7L + 6)
                    if (m != prev && (wk > 0 || weeks < 10)) Text(MONTHS[m.toInt() - 1], color = k.faint, fontSize = 10.sp, modifier = Modifier.padding(start = dayCol + (cell.dp + gap) * wk))
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(gap), modifier = Modifier.semantics { contentDescription = "${w.count(inView)} in $weeks weeks" }) {
                Column(Modifier.width(dayCol - gap), verticalArrangement = Arrangement.spacedBy(gap)) {
                    for (d in listOf("Mon", "", "Wed", "", "Fri", "", "")) Box(Modifier.height(cell.dp), contentAlignment = Alignment.CenterStart) { Text(d, color = k.faint, fontSize = 9.sp, maxLines = 1, softWrap = false, lineHeight = 9.sp) }
                }
                for (wk in 0 until weeks) Column(verticalArrangement = Arrangement.spacedBy(gap)) {
                    for (d in 0 until 7) {
                        val day = first + wk * 7L + d
                        val cs = RoundedCornerShape(2.5.dp)
                        val m = Modifier.size(cell.dp)
                        when {
                            day > st.today -> Box(m)
                            st.sinceDay != null && day < st.sinceDay!! && value(day) == 0u -> Box(m.border(1.dp, k.border, cs))
                            else -> Box(m.background(shade(value(day), max), cs))
                        }
                    }
                }
            }
            Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                val since = st.sinceDay?.let { "Counted by Kotora since ${StatsText.dateText(it)}: what you record here, not what came before or from the website." }
                    ?: "Counted by Kotora: what you record here, not what came before or from the website."
                Text(since, color = k.faint, fontSize = 12.5.sp, modifier = Modifier.weight(1f))
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text("Less", color = k.faint, fontSize = 11.sp, modifier = Modifier.padding(end = 4.dp))
                    for (i in 0 until 5) Box(Modifier.size(11.dp).background(shade(i.toUInt(), 4u), RoundedCornerShape(2.5.dp)))
                    Text("More", color = k.faint, fontSize = 11.sp, modifier = Modifier.padding(start = 4.dp))
                }
            }
        }
    }
}

@Composable
private fun StatsCard(title: String, content: @Composable () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(14.dp)
    Column(Modifier.fillMaxWidth().background(k.surface, shape).border(1.dp, k.border, shape).padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(title, color = k.text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
        content()
    }
}

@Composable
private fun Caption(text: String) {
    Text(text.uppercase(), color = Kotora.colors.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = .4.sp, maxLines = 1)
}

@Composable
private fun Note(text: String) {
    Text(text, color = Kotora.colors.faint, fontSize = 12.sp)
}

/** Small vertical bars with a label under each. */
@Composable
private fun Bars(values: List<UInt>, labels: List<String>, height: Dp, gap: Dp) {
    val k = Kotora.colors
    val max = maxOf(values.maxOrNull() ?: 0u, 1u).toFloat()
    Row(Modifier.fillMaxWidth().height(height + 16.dp), horizontalArrangement = Arrangement.spacedBy(gap), verticalAlignment = Alignment.Bottom) {
        values.forEachIndexed { i, v ->
            Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Box(Modifier.fillMaxWidth().height(if (v == 0u) 2.dp else maxOf(3.dp, height * (v.toFloat() / max))).background(if (v == 0u) k.border else k.accent, RoundedCornerShape(2.dp)))
                Text(labels.getOrElse(i) { "" }, color = k.faint, fontSize = 10.sp, maxLines = 1, softWrap = false, modifier = Modifier.height(12.dp))
            }
        }
    }
}

/** Horizontal bars: a name, a bar, a count. */
@Composable
private fun Ranked(items: List<NamedCount>) {
    val k = Kotora.colors
    val max = maxOf(items.maxOfOrNull { it.count } ?: 1u, 1u).toFloat()
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for (it in items) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(it.name, color = k.text, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.width(120.dp))
            Box(Modifier.weight(1f).height(8.dp).clip(CircleShape).background(k.border)) {
                Box(Modifier.fillMaxHeight().fillMaxWidth(it.count.toFloat() / max).clip(CircleShape).background(k.accent))
            }
            Text("${it.count}", color = k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium, modifier = Modifier.width(28.dp))
        }
    }
}

@Composable
private fun TitleLine(t: StatsTitle, right: String) {
    val k = Kotora.colors
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        val shape = RoundedCornerShape(4.dp)
        Box(Modifier.size(26.dp, 38.dp).clip(shape).background(tileBrush(t.mediaId.toInt()), shape), contentAlignment = Alignment.Center) {
            Text(t.title.take(1), color = Color.White.copy(alpha = .92f), fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
            if (t.coverUrl != null) coil3.compose.AsyncImage(model = t.coverUrl, contentDescription = null, contentScale = androidx.compose.ui.layout.ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
        Text(t.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        Text(right, color = k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium)
    }
}
