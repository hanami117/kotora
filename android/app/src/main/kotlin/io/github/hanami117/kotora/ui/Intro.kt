package io.github.hanami117.kotora.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.pow
import kotlinx.coroutines.delay

/*
 * The startup intro, the desktop's (`app/src/intro.rs`) on a phone: the app opens under a
 * tiger-orange layer with 小虎 in the middle; three claw slashes rip across it about 90 ms apart,
 * the layer splits along them into four strips that slide away and fade, and the app is underneath.
 * About 0.8 s. A tap skips it.
 *
 * - The launch screen (Theme.Kotora.Starting) is this layer's start pose: the same orange and 小虎
 *   at the same place and size, so the hand-over has no seam. The clock starts at the first frame
 *   Compose draws, holds the solid orange LEAD, then runs.
 * - Same geometry and timeline as the desktop: one 1600×1000 box that covers the screen (one scale,
 *   centred), every piece a vector path, the desktop's tracks and easings (Rune's ease-in/out are
 *   CSS's). The strips travel in dp where the desktop's travel in points.
 * - Only on a cold start (MainActivity), and Settings → Startup animation turns it off.
 * - Remove animations (Android's accessibility setting, animator scale 0): the layer only fades.
 */

private const val VW = 1600f
private const val VH = 1000f

/** 小虎's em on the phone: the launch screen's icon (a 160dp circle shows) holds 2 × 72dp. */
val INTRO_NAME_EM = 72.dp

private val EASE_IN: Easing = CubicBezierEasing(0.42f, 0f, 1f, 1f)
private val EASE_OUT: Easing = CubicBezierEasing(0f, 0f, 0.58f, 1f)

private class Track(val at: Float, val dur: Float, val ease: Easing) {
    /** Eased progress (0 to 1) at time `t`. */
    fun at(t: Float): Float {
        if (dur <= 0f) return if (t >= at) 1f else 0f
        return ease.transform(((t - at) / dur).coerceIn(0f, 1f))
    }
}

// ---- the timeline (seconds from the first frame of the run), as on the desktop

private const val HOLD = 0.06f
private const val SLASH_GAP = 0.09f
private const val SLASH = 0.12f
private const val SPLIT_AT = 0.33f
private const val SPLIT = 0.45f

/** After the first drawn frame the solid orange holds this long before the tracks start. */
const val INTRO_LEAD_MS = 100L
const val INTRO_TOTAL = 0.8f
const val INTRO_REDUCED_TOTAL = 0.4f

private val REDUCED_FADE = Track(0.12f, 0.25f, EASE_OUT)
private fun slashTrack(k: Int) = Track(HOLD + SLASH_GAP * k, SLASH, CubicBezierEasing(0.3f, 0f, 0.2f, 1f))
private val CUT_FADE = Track(SPLIT_AT + 0.04f, 0.16f, EASE_IN)
private val STRIP_MOVE = Track(SPLIT_AT, SPLIT, CubicBezierEasing(0.35f, 0f, 0.55f, 1f))
private val STRIP_FADE = Track(SPLIT_AT + 0.22f, SPLIT - 0.22f, EASE_IN)
private val BASE_OFF = Track(SPLIT_AT, 0f, LinearEasing)
private val NAME_FADE = Track(SPLIT_AT - 0.04f, 0.14f, EASE_IN)

// ---- the geometry, in the 1600×1000 box

/** The three claw lines, upper right to lower left: start, control, end. */
private fun claw(k: Int): Array<Offset> {
    val dx = (k - 1f) * 210f
    val bow = floatArrayOf(52f, 70f, 58f)[k]
    val p0 = Offset(1250f + dx, -70f)
    val p2 = Offset(350f + dx, 1070f)
    val n = normal(p0, p2)
    val mid = Offset((p0.x + p2.x) / 2f + n.x * bow, (p0.y + p2.y) / 2f + n.y * bow)
    return arrayOf(p0, mid, p2)
}

private fun normal(a: Offset, b: Offset): Offset {
    val dx = b.x - a.x
    val dy = b.y - a.y
    val len = max(hypot(dx, dy), 1e-3f)
    return Offset(-dy / len, dx / len)
}

private fun bezier(c: Array<Offset>, t: Float): Offset {
    val u = 1f - t
    return c[0] * (u * u) + c[1] * (2f * u * t) + c[2] * (t * t)
}

private fun bezierNormal(c: Array<Offset>, t: Float): Offset {
    val d = (c[1] - c[0]) * (2f * (1f - t)) + (c[2] - c[1]) * (2f * t)
    return normal(Offset.Zero, d)
}

private const val SAMPLES = 28

/** A claw mark's outline around line `k`: sharp at the start, widest a third in, a long tail. */
private fun mark(k: Int, width: Float, shift: Float): List<Offset> {
    val c = claw(k)
    val a = 0.55f
    val b = 1.15f
    val peak = (a / (a + b)).pow(a) * (b / (a + b)).pow(b)
    val left = ArrayList<Offset>(SAMPLES + 1)
    val right = ArrayList<Offset>(SAMPLES + 1)
    for (i in 0..SAMPLES) {
        val t = i / SAMPLES.toFloat()
        val h = width / 2f * t.pow(a) * (1f - t).pow(b) / peak
        val p = bezier(c, t)
        val n = bezierNormal(c, t)
        val o = shift * width
        left += p + n * (o + h)
        right += p + n * (o - h)
    }
    return left + right.reversed()
}

private const val REACH = 1400f

/** Claw line `k` carried on straight past both ends (the strips' edges). */
private fun edge(k: Int): List<Offset> {
    val c = claw(k)
    fun unit(a: Offset, b: Offset): Offset {
        val d = b - a
        return d / max(d.getDistance(), 1e-3f)
    }
    val d0 = unit(c[1], c[0])
    val d2 = unit(c[1], c[2])
    return listOf(c[0] + d0 * REACH) + (0..SAMPLES).map { bezier(c, it / SAMPLES.toFloat()) } + listOf(c[2] + d2 * REACH)
}

/** The orange strip `k` (0 to 3, left to right). */
private fun strip(k: Int): List<Offset> = when (k) {
    0 -> edge(0).let { e -> e + listOf(Offset(-REACH, e.last().y), Offset(-REACH, e.first().y)) }
    3 -> edge(2).let { e -> e + listOf(Offset(VW + REACH, e.last().y), Offset(VW + REACH, e.first().y)) }
    else -> edge(k - 1) + edge(k).reversed()
}

/** Where strip `k` goes (dp): across the claws away from the middle, and along them. */
private fun stripAway(k: Int): Offset {
    val c = claw(1)
    val n = normal(c[0], c[2])
    val d = Offset(n.y, -n.x)
    val across = floatArrayOf(360f, 140f, -140f, -360f)[k]
    val along = floatArrayOf(700f, -560f, 600f, -680f)[k]
    return n * across + d * along
}

/** The box laid over a `w` × `h` canvas: one scale that covers it, centred. */
private class Fit(val w: Float, val h: Float) {
    val k = max(w / VW, h / VH)
    fun pt(p: Offset) = Offset((p.x - VW / 2f) * k + w / 2f, (p.y - VH / 2f) * k + h / 2f)
    fun path(pts: List<Offset>) = Path().apply {
        pts.forEachIndexed { i, p0 ->
            val p = pt(p0)
            if (i == 0) moveTo(p.x, p.y) else lineTo(p.x, p.y)
        }
        close()
    }
}

private val RIM = Color(0xFFFFB866)
private val GLINT = Color(0xFFFFF6E6).copy(alpha = 0.95f)
val INTRO_ORANGE = Color(0xFFF28A22)

/**
 * The layer over the whole app while the intro runs. `frozenAt` (0 to 1 of the run): a held
 * frame for the screenshot tests, no clock. `onDone`: it is over (or skipped); drop it.
 */
@Composable
fun IntroOverlay(reduced: Boolean, onDone: () -> Unit, frozenAt: Float? = null) {
    val k = Kotora.colors
    val nameColor = if (k.dark) Color(0xFFFFF7EC) else Color(0xFF1D1208)
    val cutColor = k.bg
    val total = if (reduced) INTRO_REDUCED_TOTAL else INTRO_TOTAL
    var t by remember { mutableFloatStateOf((frozenAt ?: 0f) * total) }
    var skipped by remember { mutableStateOf(false) }
    val fade = remember { Animatable(1f) }
    val done by rememberUpdatedState(onDone)
    if (frozenAt == null) {
        LaunchedEffect(Unit) {
            withFrameNanos { }
            delay(INTRO_LEAD_MS)
            val start = withFrameNanos { it }
            while (t < total) {
                val now = withFrameNanos { it }
                t = (now - start) / 1e9f
            }
            done()
        }
    }
    LaunchedEffect(skipped) {
        if (skipped) {
            fade.animateTo(0f, tween(150))
            done()
        }
    }
    val ko = remember { PathParser().parsePathString(Kanji.KO).toPath() }
    val tora = remember { PathParser().parsePathString(Kanji.TORA).toPath() }
    Box(
        Modifier
            .fillMaxSize()
            .graphicsLayer { alpha = fade.value }
            .semantics { contentDescription = "Kotora" }
            .pointerInput(Unit) { detectTapGestures { skipped = true } }
            .drawWithCache {
                val fit = Fit(size.width, size.height)
                val strips = (0..3).map { fit.path(strip(it)) }
                val widths = floatArrayOf(30f, 38f, 32f)
                val slashes = (0..2).map { s ->
                    val w = widths[s]
                    listOf(fit.path(mark(s, w * 1.5f, 0f)) to RIM, fit.path(mark(s, w, 0f)) to cutColor, fit.path(mark(s, w * 0.26f, 0.24f)) to GLINT)
                }
                val origins = (0..2).map { fit.pt(claw(it)[0]) }
                val em = INTRO_NAME_EM.toPx()
                val dp = density
                onDrawBehind {
                    val now = t
                    fun name(alpha: Float) {
                        if (alpha <= 0f) return
                        for ((i, glyph) in listOf(ko, tora).withIndex()) {
                            translate(size.width / 2f - em + i * em, size.height / 2f - em / 2f) {
                                scale(em / 100f, em / 100f, Offset.Zero) { drawPath(glyph, nameColor, alpha = alpha) }
                            }
                        }
                    }
                    if (reduced) {
                        val a = 1f - REDUCED_FADE.at(now)
                        drawRect(INTRO_ORANGE, alpha = a)
                        name(a)
                        return@onDrawBehind
                    }
                    val base = 1f - BASE_OFF.at(now)
                    if (base > 0f) drawRect(INTRO_ORANGE, alpha = base)
                    val m = STRIP_MOVE.at(now)
                    val stripAlpha = 1f - STRIP_FADE.at(now)
                    if (stripAlpha > 0f) {
                        strips.forEachIndexed { i, p ->
                            val away = stripAway(i) * (m * dp)
                            translate(away.x, away.y) { drawPath(p, INTRO_ORANGE, alpha = stripAlpha) }
                        }
                    }
                    name(1f - NAME_FADE.at(now))
                    val cutAlpha = 1f - CUT_FADE.at(now)
                    if (cutAlpha > 0f) {
                        slashes.forEachIndexed { s, parts ->
                            val sc = max(slashTrack(s).at(now), 0.001f)
                            scale(sc, sc, origins[s]) {
                                for ((p, c) in parts) drawPath(p, c, alpha = cutAlpha)
                            }
                        }
                    }
                }
            },
    )
}
