package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import io.github.hanami117.kotora.core.ServiceLink
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.PageLine
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.core.ScoreFace
import io.github.hanami117.kotora.core.ScoreMark
import io.github.hanami117.kotora.core.ScoreStyle
import io.github.hanami117.kotora.ui.theme.Kotora

/** The score in the user's format (desktop `scores.rs`): the number, five small stars, or a face. */
@Composable
fun ScoreMarkView(mark: ScoreMark?, size: TextUnit = 12.sp, star: Boolean = true) {
    val k = Kotora.colors
    val desc = mark?.let { "Score ${it.words}" } ?: "No score"
    Box(Modifier.clearAndSetSemantics { contentDescription = desc }) {
        when {
            mark == null -> Text("★ –", color = k.faint, fontSize = size)
            mark.style == ScoreStyle.STARS -> Row {
                for (i in 1..5) Text("★", color = if (i <= mark.stars.toInt()) k.onHold else k.borderStrong, fontSize = size * .9f)
            }
            mark.style == ScoreStyle.SMILEY -> {
                val (icon, tint) = when (mark.face ?: ScoreFace.NEUTRAL) {
                    ScoreFace.SAD -> KIcons.FaceSad to k.dropped
                    ScoreFace.NEUTRAL -> KIcons.FaceNeutral to k.dim
                    ScoreFace.HAPPY -> KIcons.FaceHappy to k.watching
                }
                Icon(icon, null, tint = tint, modifier = Modifier.size(16.dp))
            }
            else -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                if (star) Text("★", color = k.onHold, fontSize = size * .85f)
                Text(mark.text, color = k.dim, fontSize = size, fontWeight = FontWeight.Medium)
            }
        }
    }
}

/** A pill on a poster: the page colour, nearly opaque, so it reads on any cover (desktop `badge`). */
@Composable
private fun Badge(text: String, dot: Color?, filled: Boolean = false) {
    val k = Kotora.colors
    Row(
        Modifier.height(20.dp).clip(CircleShape).background(if (filled) k.accent else k.bg.copy(alpha = .86f)).padding(horizontal = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        if (dot != null) Box(Modifier.size(6.dp).clip(CircleShape).background(dot))
        Text(text, color = if (filled) k.onAccent else k.text, fontSize = 10.5.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
    }
}

/** The badges at a poster's top-left: rewatch or the run, a new episode, a change waiting. */
@Composable
private fun Badges(line: PageLine) {
    val k = Kotora.colors
    val r = line.row
    Column(Modifier.padding(6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        when {
            r.rewatching -> Badge(if (r.kind == MediaKind.MANGA) "Rereading" else "Rewatching", k.completed)
            r.kind == MediaKind.ANIME && r.airStatus == "currently_airing" -> Badge("Airing", k.air)
            r.airStatus == "currently_publishing" || (r.kind == MediaKind.MANGA && r.airStatus == "currently_airing") -> Badge("Publishing", k.air)
            line.onHiatus -> Badge("On hiatus", k.dropped)
        }
        line.newEpisode?.let { Badge("Ep $it out", null, filled = true) }
        if (r.waiting) Badge(r.waitingText, if (r.waitingProblem) k.dropped else k.onHold)
    }
}

private fun canPlus(r: ListRow) = r.canPlusOne

private fun epText(r: ListRow) = if (r.plusVolumes) "Vol ${r.volumes} / ${r.totalVolumes ?: "?"}" else "${if (r.kind == MediaKind.MANGA) "Ch" else "Ep"} ${r.progress} / ${r.total ?: "?"}"

private fun fraction(r: ListRow): Float {
    val t = r.total
    return if (t != null && t > 0u) (r.progress.toFloat() / t.toFloat()).coerceAtMost(1f) else if (r.progress > 0u) .15f else 0f
}

/** Grid (dense posters, three across) and Large (cards, two across): desktop `grid.rs`/`mgrid.rs`. */
@Composable
@OptIn(ExperimentalFoundationApi::class)
fun PosterGrid(
    lines: List<PageLine>,
    large: Boolean,
    onOpen: (ListRow) -> Unit,
    onPlusOne: (ListRow) -> Unit,
    footer: @Composable () -> Unit = {},
    links: (ListRow) -> List<ServiceLink> = { emptyList() },
    onOpenUrl: (String) -> Unit = {},
) {
    val k = Kotora.colors
    LazyVerticalGrid(
        columns = GridCells.Fixed(if (large) 2 else 3),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 80.dp),
        horizontalArrangement = Arrangement.spacedBy(if (large) 12.dp else 10.dp),
        verticalArrangement = Arrangement.spacedBy(if (large) 14.dp else 12.dp),
    ) {
        items(lines, key = { "${it.row.kind}-${it.row.id}" }) { line ->
            val r = line.row
            val shape = RoundedCornerShape(if (large) 12.dp else 8.dp)
            // Long press: Move to › / Score › and the Open on links (`QuickMenu.kt`).
            var menu by remember { mutableStateOf(false) }
            QuickMenu(r, menu, if (menu) links(r) else emptyList(), onOpenUrl) { menu = false }
            Column(
                Modifier.clip(shape).then(if (large) Modifier.background(k.surface).border(1.dp, k.border, shape) else Modifier)
                    .combinedClickable(role = Role.Button, onClick = { onOpen(r) }, onLongClick = { menu = true })
                    .semantics { contentDescription = "About ${r.title}" },
            ) {
                BoxWithConstraints(Modifier.fillMaxWidth()) {
                    val w = maxWidth
                    Cover(r, w, w * 1.5f, if (large) 0.dp else 8.dp, if (large) 26.sp else 15.sp)
                    Badges(line)
                    if (!large && canPlus(r)) {
                        Box(
                            Modifier.align(Alignment.BottomEnd).padding(6.dp).height(28.dp).clip(CircleShape)
                                .background(k.bg.copy(alpha = .88f)).border(1.dp, k.borderStrong, CircleShape)
                                .clickable(role = Role.Button) { onPlusOne(r) }
                                .semantics { contentDescription = "${r.plusOneText}: ${r.title}" }
                                .padding(horizontal = 11.dp),
                            contentAlignment = Alignment.Center,
                        ) { Text("+1", color = k.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold) }
                    }
                }
                Column(
                    Modifier.padding(if (large) 12.dp else 0.dp).padding(top = if (large) 0.dp else 5.dp),
                    verticalArrangement = Arrangement.spacedBy(if (large) 10.dp else 5.dp),
                ) {
                    Text(
                        r.title, color = k.text, fontSize = if (large) 14.sp else 12.5.sp,
                        fontWeight = if (large) FontWeight.SemiBold else FontWeight.Medium,
                        maxLines = 2, minLines = 2, overflow = TextOverflow.Ellipsis,
                    )
                    ProgressBar(fraction(r), k.status(if (r.rewatching) ListStatus.COMPLETED else r.status), Modifier.fillMaxWidth())
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(epText(r), color = k.dim, fontSize = if (large) 12.sp else 11.5.sp)
                        Spacer(Modifier.weight(1f))
                        if (line.scoreMark != null) ScoreMarkView(line.scoreMark, if (large) 12.sp else 11.5.sp)
                        if (large && canPlus(r)) {
                            val s = RoundedCornerShape(8.dp)
                            Box(
                                Modifier.size(44.dp, 30.dp).clip(s).background(k.raised).border(1.dp, k.borderStrong, s)
                                    .clickable(role = Role.Button) { onPlusOne(r) }
                                    .semantics { contentDescription = "${r.plusOneText}: ${r.title}" },
                                contentAlignment = Alignment.Center,
                            ) { Text("+1", color = k.text, fontSize = 14.sp, fontWeight = FontWeight.SemiBold) }
                        }
                    }
                }
            }
        }
        item(span = { androidx.compose.foundation.lazy.grid.GridItemSpan(maxLineSpan) }) { footer() }
    }
}
