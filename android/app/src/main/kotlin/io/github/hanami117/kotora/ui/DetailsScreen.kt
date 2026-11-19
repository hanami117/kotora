package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.core.ServiceLink
import io.github.hanami117.kotora.ui.theme.Kotora

/**
 * One entry: the header and the "Your list" card from the saved list. The service's own details
 * (synopsis, score, rank, the editor) come in M3.
 */
@Composable
fun DetailsScreen(
    row: ListRow?,
    account: Account?,
    onBack: () -> Unit,
    onPlusOne: (ListRow) -> Unit,
    modifier: Modifier = Modifier,
    links: List<ServiceLink> = emptyList(),
    onOpenUrl: (String) -> Unit = {},
    /** Edit, −1, Rewatch / Stop, Remove (`EntryActions`); `null`: the saved row only. */
    actions: (@Composable (ListRow) -> Unit)? = null,
    /** The service's information (stats, other titles, Synopsis, Information). */
    info: DetailsInfoUi? = null,
    onRetry: () -> Unit = {},
) {
    val k = Kotora.colors
    if (row == null) {
        Column(modifier.fillMaxSize().background(k.bg).statusBarsPadding()) {
            IconButton(onClick = onBack) { Icon(KIcons.Back, "Back", tint = k.text) }
            Text("That entry isn't on your list any more.", color = k.faint, modifier = Modifier.padding(24.dp))
        }
        return
    }
    val manga = row.kind == MediaKind.MANGA
    val unit = if (manga) "chapter" else "episode"
    Column(modifier.fillMaxSize().background(k.bg).verticalScroll(rememberScrollState())) {
        Box(Modifier.fillMaxWidth().height(250.dp)) {
            Box(Modifier.fillMaxSize().blur(30.dp).alpha(.55f).background(tileBrush(row.id.toInt())))
            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(.3f to Color.Transparent, 1f to k.bg)))
            Row(Modifier.statusBarsPadding().fillMaxWidth().height(56.dp).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack, modifier = Modifier.clip(CircleShape).background(k.bg.copy(alpha = .6f))) {
                    Icon(KIcons.Back, "Back", tint = k.text)
                }
            }
        }
        Row(
            Modifier.padding(horizontal = 16.dp).offset(y = (-170).dp).height(170.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            Cover(row, 120.dp, 170.dp, 12.dp, 30.sp)
            Column(Modifier.padding(bottom = 4.dp)) {
                Text(row.title, color = k.text, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, lineHeight = 26.sp)
                row.titleEn?.takeIf { it != row.title }?.let { Text(it, color = k.dim, fontSize = 13.sp, modifier = Modifier.padding(top = 4.dp)) }
                Text(meta(row), color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 6.dp))
            }
        }
        Column(Modifier.offset(y = (-170).dp)) {
            Row(Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                airPill(row)?.let { (text, live) -> Pill(text, live) }
                if (row.waiting) Pill("Waiting to sync", false, k.onHold)
            }
            info?.info?.let { i ->
                DetailsStats(i, Modifier.padding(bottom = 12.dp))
                DetailsTitles(i, listOfNotNull(row.title, row.titleEn))
            }
            // "Open on MyAnimeList" / "Open on AniList" (desktop details.rs `det/open-mal`).
            if (links.isNotEmpty()) {
                Row(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (l in links) OpenOnButton(l.label, { onOpenUrl(l.url) })
                }
            }
            Card {
                Column {
                    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text("Your list", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                        if (account != null) Text(serviceTitle(account.service), color = k.faint, fontSize = 12.sp)
                    }
                    Row(Modifier.padding(horizontal = 16.dp, vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Field("Status", Modifier.weight(1f)) {
                            StatusDot(row.status)
                            Text(statusLabel(row.status, row.kind), color = k.text, fontSize = 14.sp)
                        }
                        Field("Score", Modifier.weight(1f)) { Text("★ ${row.score ?: "–"}", color = k.text, fontSize = 14.sp) }
                    }
                    Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 2.dp, bottom = 14.dp)) {
                        Text(if (manga) "Chapters" else "Episodes", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(bottom = 6.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
                            Box(
                                Modifier.height(44.dp).border(1.dp, k.borderStrong, RoundedCornerShape(8.dp)).background(k.field, RoundedCornerShape(8.dp)).padding(horizontal = 14.dp),
                                contentAlignment = Alignment.Center,
                            ) {
                                Text("${row.progress} of ${row.total ?: "?"}", color = k.text, fontWeight = FontWeight.Medium)
                            }
                            val canPlus = row.total?.let { row.progress >= it } != true && (row.status == ListStatus.WATCHING || row.rewatching)
                            KButton(
                                "+1 ${unit.replaceFirstChar { it.uppercase() }} ${row.progress + 1u}",
                                { onPlusOne(row) },
                                Modifier.weight(1f).semantics { contentDescription = "Plus one $unit" },
                                ButtonKind.PRIMARY,
                                44.dp,
                                canPlus,
                            )
                        }
                        if (manga) {
                            Text("Volumes ${row.volumes} of ${row.totalVolumes ?: "?"}", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 10.dp))
                        }
                    }
                    if (actions != null) {
                        actions(row)
                    } else if (row.rewatching || row.timesRewatched > 0u) {
                        HorizontalDivider(color = k.border)
                        Row(Modifier.fillMaxWidth().padding(16.dp)) {
                            Text(if (manga) "Rereading" else "Rewatching", color = k.text, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                            Text(if (row.rewatching) "On" else "Done ${row.timesRewatched}×", color = k.dim, fontSize = 13.sp)
                        }
                    }
                }
            }
            if (info != null) DetailsBody(info, account?.let { serviceTitle(it.service) } ?: "MyAnimeList", onRetry)
            if (actions == null && info == null) Text(
                "The editor, synopsis and the service's details come in a later build.",
                color = k.faint,
                fontSize = 12.sp,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
            )
        }
    }
}

@Composable
private fun Field(label: String, modifier: Modifier, content: @Composable () -> Unit) {
    val k = Kotora.colors
    Column(modifier) {
        Text(label, color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(bottom = 6.dp))
        Row(
            Modifier.fillMaxWidth().height(44.dp).border(1.dp, k.borderStrong, RoundedCornerShape(8.dp)).background(k.field, RoundedCornerShape(8.dp)).padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) { content() }
    }
}

@Composable
private fun Pill(text: String, live: Boolean, color: Color? = null) {
    val k = Kotora.colors
    val fg = color ?: if (live) k.air else k.dim
    val shape = RoundedCornerShape(16.dp)
    Row(
        Modifier.height(30.dp).background(k.surface, shape).border(1.dp, if (live || color != null) fg.copy(alpha = .4f) else k.border, shape).padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (live) Box(Modifier.size(8.dp).clip(CircleShape).background(fg))
        Text(text, color = fg, fontSize = 12.sp)
    }
}

private fun airPill(row: ListRow): Pair<String, Boolean>? = when (row.airStatus) {
    "currently_airing", "RELEASING" -> "Airing" to true
    "currently_publishing" -> "Publishing" to true
    "finished_airing", "FINISHED" -> "Finished" to false
    "finished" -> "Finished" to false
    "not_yet_aired", "NOT_YET_RELEASED", "not_yet_published" -> "Not aired yet" to false
    else -> null
}

private fun meta(row: ListRow): String {
    val parts = mutableListOf<String>()
    formatLabel(row.format).takeIf { it.isNotEmpty() }?.let { parts += it }
    row.total?.let { n -> parts += "$n ${if (row.kind == MediaKind.MANGA) "chapters" else if (n == 1u) "episode" else "episodes"}" }
    row.startDate?.let { d -> season(d)?.let { parts += it } }
    return parts.joinToString(" · ")
}

/** `2026-10-03` → "Fall 2026" (anime seasons by month). */
fun season(date: String): String? {
    val y = date.take(4).toIntOrNull() ?: return null
    val m = date.drop(5).take(2).toIntOrNull() ?: return y.toString()
    val s = when (m) {
        in 1..3 -> "Winter"
        in 4..6 -> "Spring"
        in 7..9 -> "Summer"
        else -> "Fall"
    }
    return "$s $y"
}

