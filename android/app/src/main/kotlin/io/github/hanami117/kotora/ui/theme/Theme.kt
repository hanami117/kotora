package io.github.hanami117.kotora.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.R
import io.github.hanami117.kotora.core.ListStatus

/**
 * Kotora's own colours: the desktop's tokens (`app/src/theme.rs`, design-spec §1), dark and light.
 * No Material You dynamic colour: Kotora looks like Kotora on every phone.
 */
@Immutable
data class KotoraColors(
    val bg: Color,
    val side: Color,
    val surface: Color,
    val raised: Color,
    val field: Color,
    val border: Color,
    val borderStrong: Color,
    val text: Color,
    val dim: Color,
    val faint: Color,
    val accent: Color,
    val accentHi: Color,
    val accentLo: Color,
    val onAccent: Color,
    val tint: Color,
    val watching: Color,
    val completed: Color,
    val onHold: Color,
    val dropped: Color,
    val plan: Color,
    val air: Color,
    val snack: Color,
    val dark: Boolean,
) {
    fun status(s: ListStatus): Color = when (s) {
        ListStatus.WATCHING -> watching
        ListStatus.PLAN_TO_WATCH -> plan
        ListStatus.COMPLETED -> completed
        ListStatus.ON_HOLD -> onHold
        ListStatus.DROPPED -> dropped
    }
}

val DarkColors = KotoraColors(
    bg = Color(0xFF0F1626), side = Color(0xFF121B2E), surface = Color(0xFF182238), raised = Color(0xFF1F2B45),
    field = Color(0xFF141D31), border = Color(0xFF26324D), borderStrong = Color(0xFF34446A),
    text = Color(0xFFEDF0F7), dim = Color(0xFFB7C0D2), faint = Color(0xFF7D89A4),
    accent = Color(0xFFF28A22), accentHi = Color(0xFFFF9D3F), accentLo = Color(0xFFE57A18), onAccent = Color(0xFF1D1208),
    tint = Color(0x24F28A22), watching = Color(0xFF4CC38A), completed = Color(0xFF6EA8FF), onHold = Color(0xFFF2C94C),
    dropped = Color(0xFFF4898F), plan = Color(0xFFB29AFB), air = Color(0xFF4FD1E0), snack = Color(0xFF2A3654), dark = true,
)

val LightColors = KotoraColors(
    bg = Color(0xFFF6F7FB), side = Color(0xFFECEFF6), surface = Color(0xFFFFFFFF), raised = Color(0xFFE4E8F1),
    field = Color(0xFFFFFFFF), border = Color(0xFFDBE0EA), borderStrong = Color(0xFFB9C1D3),
    text = Color(0xFF131A2B), dim = Color(0xFF454F68), faint = Color(0xFF5F6982),
    accent = Color(0xFFF28A22), accentHi = Color(0xFFA64A00), accentLo = Color(0xFFE57A18), onAccent = Color(0xFF1D1208),
    tint = Color(0x29F28A22), watching = Color(0xFF13704A), completed = Color(0xFF2559C4), onHold = Color(0xFF8A5D00),
    dropped = Color(0xFFC02A35), plan = Color(0xFF6B45D1), air = Color(0xFF0A6D7A), snack = Color(0xFF131A2B), dark = false,
)

val LocalKotoraColors = staticCompositionLocalOf { DarkColors }

/** Inter (OFL), bundled: the desktop's font, three static weights. */
val Inter = FontFamily(
    Font(R.font.inter_regular, FontWeight.Normal),
    Font(R.font.inter_medium, FontWeight.Medium),
    Font(R.font.inter_semibold, FontWeight.SemiBold),
)

private fun style(size: Int, weight: FontWeight, line: Float = 1.35f) =
    TextStyle(fontFamily = Inter, fontWeight = weight, fontSize = size.sp, lineHeight = (size * line).sp)

private val KotoraTypography = Typography(
    headlineLarge = style(30, FontWeight.SemiBold, 1.2f),
    headlineSmall = style(22, FontWeight.SemiBold, 1.2f),
    titleLarge = style(21, FontWeight.SemiBold, 1.25f),
    titleMedium = style(15, FontWeight.SemiBold),
    titleSmall = style(15, FontWeight.Medium),
    bodyLarge = style(15, FontWeight.Normal, 1.5f),
    bodyMedium = style(14, FontWeight.Normal),
    bodySmall = style(13, FontWeight.Normal),
    labelLarge = style(14, FontWeight.Medium),
    labelMedium = style(12, FontWeight.Medium),
    labelSmall = style(11, FontWeight.Medium),
)

/** System, Light or Dark, as on the desktop. */
enum class ThemeMode { SYSTEM, LIGHT, DARK }

@Composable
fun KotoraTheme(mode: ThemeMode = ThemeMode.SYSTEM, content: @Composable () -> Unit) {
    val dark = when (mode) {
        ThemeMode.SYSTEM -> isSystemInDarkTheme()
        ThemeMode.LIGHT -> false
        ThemeMode.DARK -> true
    }
    val k = if (dark) DarkColors else LightColors
    val base = if (dark) darkColorScheme() else lightColorScheme()
    val scheme = base.copy(
        primary = k.accent, onPrimary = k.onAccent, primaryContainer = k.tint, onPrimaryContainer = k.text,
        secondary = k.accentHi, onSecondary = k.onAccent,
        background = k.bg, onBackground = k.text, surface = k.bg, onSurface = k.text,
        surfaceVariant = k.raised, onSurfaceVariant = k.dim,
        surfaceContainerLowest = k.bg, surfaceContainerLow = k.surface, surfaceContainer = k.surface,
        surfaceContainerHigh = k.raised, surfaceContainerHighest = k.raised,
        outline = k.borderStrong, outlineVariant = k.border, error = k.dropped,
        inverseSurface = k.snack, inverseOnSurface = Color(0xFFEDF0F7), inversePrimary = Color(0xFFFF9D3F),
    )
    CompositionLocalProvider(LocalKotoraColors provides k) {
        MaterialTheme(colorScheme = scheme, typography = KotoraTypography, content = content)
    }
}

object Kotora {
    val colors: KotoraColors @Composable get() = LocalKotoraColors.current
}
