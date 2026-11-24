package io.github.hanami117.kotora.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.LinearGradientShader
import androidx.compose.ui.graphics.Shader
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import io.github.hanami117.kotora.R
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.sin

@Composable
fun KotoraLogo(size: Dp, modifier: Modifier = Modifier) {
    Image(painterResource(R.drawable.kotora_logo), contentDescription = null, modifier = modifier.size(size))
}

/** The cover, or, until it loads (and when there is none), a tile: a 160° gradient from the id and the initials. */
@Composable
fun Cover(row: ListRow, width: Dp, height: Dp, radius: Dp, fontSize: TextUnit, modifier: Modifier = Modifier) =
    Cover(row.id, row.title, row.coverUrl, width, height, radius, fontSize, modifier)

@Composable
fun Cover(id: UInt, title: String, coverUrl: String?, width: Dp, height: Dp, radius: Dp, fontSize: TextUnit, modifier: Modifier = Modifier) {
    val shape = RoundedCornerShape(radius)
    Box(modifier.size(width, height).clip(shape).background(tileBrush(id.toInt()), shape), contentAlignment = Alignment.Center) {
        Text(
            initials(title),
            color = Color.White.copy(alpha = .92f),
            style = TextStyle(fontWeight = FontWeight.SemiBold, fontSize = fontSize, shadow = Shadow(Color.Black.copy(alpha = .35f), Offset(0f, 1f), 2f)),
        )
        if (coverUrl != null) {
            AsyncImage(model = coverUrl, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
    }
}

fun initials(title: String): String =
    title.split(' ', '-', ':').filter { it.isNotBlank() && it[0].isLetterOrDigit() }.take(2).joinToString("") { it.take(1).uppercase() }

/** The mockup's tile colours: `hsl(id*47 55% 48%)` to a second hue 40-90° on, at CSS's 160°. */
fun tileBrush(id: Int): Brush {
    val h1 = (id * 47) % 360
    val h2 = (h1 + 40 + (id * 13) % 50) % 360
    val colors = listOf(Color.hsl(h1.toFloat(), .55f, .48f), Color.hsl(h2.toFloat(), .50f, .30f))
    return object : ShaderBrush() {
        override fun createShader(size: Size): Shader {
            // CSS 160°: towards (sin 160°, -cos 160°), through the centre, corner to corner.
            val dx = sin(Math.toRadians(160.0)).toFloat()
            val dy = -cos(Math.toRadians(160.0)).toFloat()
            val half = (abs(size.width * dx) + abs(size.height * dy)) / 2
            val c = Offset(size.width / 2, size.height / 2)
            return LinearGradientShader(c - Offset(dx, dy) * half, c + Offset(dx, dy) * half, colors)
        }
    }
}

enum class TagKind { PLAIN, AIR, NEW, WAIT, HIATUS }

@Composable
fun Tag(text: String, kind: TagKind = TagKind.PLAIN) {
    val k = Kotora.colors
    val (fg, bg) = when (kind) {
        TagKind.PLAIN -> k.dim to k.raised
        TagKind.AIR -> k.air to k.air.copy(alpha = .13f)
        TagKind.NEW -> k.accentHi to k.tint
        TagKind.WAIT -> k.onHold to k.onHold.copy(alpha = .13f)
        TagKind.HIATUS -> k.dropped to k.dropped.copy(alpha = .13f)
    }
    Text(
        text,
        color = fg,
        fontSize = 11.sp,
        fontWeight = FontWeight.Medium,
        maxLines = 1,
        modifier = Modifier.background(bg, RoundedCornerShape(4.dp)).padding(horizontal = 7.dp, vertical = 2.dp),
    )
}

@Composable
fun StatusDot(status: ListStatus, size: Dp = 8.dp) {
    Box(Modifier.size(size).background(Kotora.colors.status(status), CircleShape))
}

fun statusLabel(status: ListStatus, kind: MediaKind): String = when (status) {
    ListStatus.WATCHING -> if (kind == MediaKind.MANGA) "Reading" else "Watching"
    ListStatus.PLAN_TO_WATCH -> if (kind == MediaKind.MANGA) "Plan to read" else "Plan to watch"
    ListStatus.COMPLETED -> "Completed"
    ListStatus.ON_HOLD -> "On hold"
    ListStatus.DROPPED -> "Dropped"
}

@Composable
fun StatusChip(status: ListStatus, kind: MediaKind, count: UInt, selected: Boolean, onClick: () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(17.dp)
    Row(
        Modifier.height(34.dp).clip(shape)
            .background(if (selected) k.tint else Color.Transparent, shape)
            .border(1.dp, if (selected) k.accent else k.borderStrong, shape)
            .clickable(role = Role.Tab, onClick = onClick)
            .padding(horizontal = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        StatusDot(status)
        Text(statusLabel(status, kind), color = if (selected) k.text else k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium)
        Text(count.toString(), color = if (selected) k.text else k.faint, fontSize = 13.sp)
    }
}

/** Anime | Manga, and other two-way switches. */
@Composable
fun Segmented(options: List<String>, selected: Int, onSelect: (Int) -> Unit, modifier: Modifier = Modifier) {
    val k = Kotora.colors
    Row(
        modifier.fillMaxWidth().height(42.dp).background(k.field, RoundedCornerShape(10.dp))
            .border(1.dp, k.borderStrong, RoundedCornerShape(10.dp)).padding(3.dp),
    ) {
        options.forEachIndexed { i, label ->
            val on = i == selected
            Box(
                Modifier.weight(1f).fillMaxHeight()
                    .then(if (on) Modifier.shadow(1.dp, RoundedCornerShape(8.dp)).background(if (k.dark) k.raised else Color.White, RoundedCornerShape(8.dp)) else Modifier)
                    .clip(RoundedCornerShape(8.dp))
                    .clickable(role = Role.Tab) { onSelect(i) },
                contentAlignment = Alignment.Center,
            ) {
                Text(label, color = if (on) k.text else k.dim, fontWeight = FontWeight.Medium, fontSize = 14.sp)
            }
        }
    }
}

@Composable
fun ProgressBar(fraction: Float, color: Color, modifier: Modifier = Modifier) {
    val k = Kotora.colors
    Box(modifier.height(4.dp).clip(RoundedCornerShape(2.dp)).background(k.border)) {
        Box(Modifier.fillMaxHeight().fillMaxWidth(fraction.coerceIn(0f, 1f)).clip(RoundedCornerShape(2.dp)).background(color))
    }
}

enum class ButtonKind { PRIMARY, SECONDARY, GHOST }

@Composable
fun KButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    kind: ButtonKind = ButtonKind.SECONDARY,
    height: Dp = 40.dp,
    enabled: Boolean = true,
    fontSize: TextUnit = 14.sp,
) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(8.dp)
    val (bg, border, fg) = when (kind) {
        ButtonKind.PRIMARY -> Triple(k.accent, k.accent, k.onAccent)
        ButtonKind.SECONDARY -> Triple(k.raised, k.borderStrong, k.text)
        ButtonKind.GHOST -> Triple(Color.Transparent, Color.Transparent, k.accentHi)
    }
    Box(
        modifier.height(height).clip(shape).background(bg, shape).border(1.dp, border, shape)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick).padding(horizontal = 16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text,
            color = fg.copy(alpha = if (enabled) 1f else .5f),
            fontWeight = if (kind == ButtonKind.PRIMARY) FontWeight.SemiBold else FontWeight.Medium,
            fontSize = fontSize,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
fun Card(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(12.dp)
    Box(modifier.fillMaxWidth().padding(horizontal = 16.dp).clip(shape).background(k.surface, shape).border(1.dp, k.border, shape)) {
        content()
    }
}

@Composable
fun SectionLabel(text: String, modifier: Modifier = Modifier) {
    Text(
        text.uppercase(),
        color = Kotora.colors.faint,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        letterSpacing = .5.sp,
        modifier = modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 8.dp),
    )
}

@Composable
fun RowScope.Fill() {
    Box(Modifier.weight(1f))
}

@Composable
fun HSpace(w: Dp) {
    Box(Modifier.width(w))
}

/** "Synced 4 min ago". */
fun ago(then: Long, now: Long): String {
    val s = (now - then).coerceAtLeast(0)
    return when {
        s < 60 -> "just now"
        s < 3600 -> "${s / 60} min ago"
        s < 86_400 -> "${s / 3600} h ago"
        else -> "${s / 86_400} d ago"
    }
}

fun formatLabel(format: String): String = when (format.lowercase()) {
    "tv" -> "TV"
    "tv_special", "special" -> "Special"
    "movie" -> "Movie"
    "ova" -> "OVA"
    "ona" -> "ONA"
    "music" -> "Music"
    "manga" -> "Manga"
    "light_novel", "novel" -> "Light novel"
    "one_shot" -> "One-shot"
    "manhwa" -> "Manhwa"
    "manhua" -> "Manhua"
    "" -> ""
    else -> format.replace('_', ' ').replaceFirstChar { it.uppercase() }
}
