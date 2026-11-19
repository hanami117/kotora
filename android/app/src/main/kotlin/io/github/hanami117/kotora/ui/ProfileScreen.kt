package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.ProfileInfo
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.ui.theme.Kotora
import java.text.NumberFormat
import java.util.Locale

/**
 * The Profile page, as the desktop's (profile.rs): the picture, the name and when they joined,
 * "Open on {service}", the service's numbers (days, episodes, mean score, a stacked bar per status),
 * how the user scores (a 1-10 chart from the saved list) and Sign out.
 */
@Composable
fun ProfileScreen(info: ProfileInfo?, sample: Boolean, onBack: () -> Unit, onOpenUrl: (String) -> Unit, onSignOut: (Service) -> Unit, modifier: Modifier = Modifier) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg)) {
        Row(Modifier.statusBarsPadding().fillMaxWidth().height(56.dp).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(KIcons.Back, "Back", tint = k.text) }
            Text("Profile", style = MaterialTheme.typography.titleLarge, color = k.text)
        }
        if (info == null) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = k.accent) }
            return
        }
        Column(Modifier.verticalScroll(rememberScrollState()).padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Head(info, onOpenUrl)
            AnimeCard(info)
            ScoresCard(info)
            val shape = RoundedCornerShape(14.dp)
            Row(
                Modifier.fillMaxWidth().border(1.dp, k.border, shape).padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(info.signedInLine, color = k.dim, fontSize = 14.sp, modifier = Modifier.weight(1f))
                KButton("Sign out", { onSignOut(info.service) }, kind = ButtonKind.SECONDARY, height = 34.dp, enabled = !sample, fontSize = 13.sp)
            }
        }
    }
}

@Composable
private fun ProfileCard(content: @Composable () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(14.dp)
    Column(
        Modifier.fillMaxWidth().background(k.surface, shape).border(1.dp, k.border, shape).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) { content() }
}

@Composable
private fun Head(info: ProfileInfo, onOpenUrl: (String) -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(14.dp)
    Row(
        Modifier.fillMaxWidth().background(k.surface, shape).border(1.dp, k.border, shape).padding(18.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        val brush = if (info.service == Service.MAL) Brush.linearGradient(listOf(Color(0xFF2E51A2), Color(0xFF1D3170)))
        else Brush.linearGradient(listOf(Color(0xFF2B9FD8), Color(0xFF1D6FA5)))
        Box(Modifier.size(64.dp).clip(CircleShape).background(brush), contentAlignment = Alignment.Center) {
            Text(info.userName.take(1).uppercase().ifEmpty { "?" }, color = Color(0xFFEDF0F7), fontSize = 26.sp, fontWeight = FontWeight.SemiBold)
            if (info.pictureUrl != null) coil3.compose.AsyncImage(model = info.pictureUrl, contentDescription = null, modifier = Modifier.fillMaxSize())
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(info.userName, color = k.text, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(info.joinedLine, color = k.dim, fontSize = 14.sp)
            OpenOnButton(info.openLabel, { onOpenUrl(info.profileUrl) }, Modifier.padding(top = 6.dp))
        }
    }
}

@Composable
private fun Figure(big: String, small: String, modifier: Modifier) {
    val k = Kotora.colors
    Column(modifier, verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(big, color = k.text, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Text(small, color = k.faint, fontSize = 12.sp)
    }
}

private fun grouped(n: UInt): String = NumberFormat.getIntegerInstance(Locale.US).format(n.toLong())

@Composable
private fun AnimeCard(info: ProfileInfo) {
    val k = Kotora.colors
    ProfileCard {
        Text("Anime", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Figure(info.days?.let { String.format(Locale.US, "%.1f", it) } ?: "–", "Days watched", Modifier.weight(1f))
            Figure(info.episodes?.let(::grouped) ?: "–", "Episodes", Modifier.weight(1f))
            Figure(info.meanScore?.let { String.format(Locale.US, "%.2f", it) } ?: "–", "Mean score", Modifier.weight(1f))
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row {
                Text("By status", color = k.faint, fontSize = 12.sp, modifier = Modifier.weight(1f))
                Text("${grouped(info.total)} entries", color = k.dim, fontSize = 12.sp)
            }
            Row(Modifier.fillMaxWidth().height(12.dp).clip(RoundedCornerShape(6.dp)).background(if (info.total == 0u) k.border else Color.Transparent), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                for (c in info.counts.filter { it.count > 0u }) Box(Modifier.weight(c.count.toFloat()).fillMaxHeight().background(k.status(c.status)))
            }
        }
        Column {
            for (c in info.counts) {
                Row(Modifier.padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    StatusDot(c.status)
                    Text(statusLabel(c.status, MediaKind.ANIME), color = k.dim, fontSize = 14.sp, modifier = Modifier.weight(1f))
                    Text(grouped(c.count), color = k.text, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                }
            }
        }
        (info.rewatchedLine ?: info.pendingLine)?.let { Text(it, color = k.faint, fontSize = 12.sp) }
    }
}

@Composable
private fun ScoresCard(info: ProfileInfo) {
    val k = Kotora.colors
    val max = (info.scoreSpread.maxOrNull() ?: 0u).coerceAtLeast(1u)
    ProfileCard {
        Text("Your scores", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
        Row(Modifier.fillMaxWidth().height(164.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.Bottom) {
            info.scoreSpread.forEachIndexed { i, c ->
                Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(if (c == 0u) "" else c.toString(), color = k.dim, fontSize = 10.sp)
                    val h = if (c == 0u) 2f else (110f * c.toFloat() / max.toFloat()).coerceAtLeast(4f)
                    Spacer(Modifier.fillMaxWidth().height(h.dp).background(if (c == 0u) k.border else k.accent, RoundedCornerShape(3.dp)))
                    Text("${i + 1}", color = k.faint, fontSize = 11.sp)
                }
            }
        }
        Text(info.scoredLine, color = k.dim, fontSize = 12.sp)
    }
}
