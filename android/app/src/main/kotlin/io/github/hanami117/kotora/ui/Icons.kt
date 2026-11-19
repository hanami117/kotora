package io.github.hanami117.kotora.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/** Kotora's line icons (design-spec §5): a 24 grid, 1.6 stroke, round ends; tinted by `Icon`. */
object KIcons {
    private fun circle(cx: Float, cy: Float, r: Float) =
        "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0"

    private fun icon(name: String, vararg paths: String): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
            for (d in paths) {
                addPath(
                    pathData = addPathNodes(d),
                    fill = null,
                    stroke = SolidColor(Color.Black),
                    strokeLineWidth = 1.6f,
                    strokeLineCap = StrokeCap.Round,
                    strokeLineJoin = StrokeJoin.Round,
                )
            }
        }.build()

    val List = icon("list", "M8 6h12M8 12h12M8 18h12", circle(4f, 6f, .6f), circle(4f, 12f, .6f), circle(4f, 18f, .6f))
    val Calendar = icon(
        "cal",
        "M6 5h12a2.5 2.5 0 0 1 2.5 2.5v10a2.5 2.5 0 0 1-2.5 2.5H6a2.5 2.5 0 0 1-2.5-2.5v-10A2.5 2.5 0 0 1 6 5z",
        "M3.5 10h17M8 3v4M16 3v4M8 14h2M14 14h2M8 17h2",
    )
    val Book = icon(
        "book",
        "M4 5.5C4 4.7 4.7 4 5.5 4H11v16H5.5c-.8 0-1.5-.7-1.5-1.5z",
        "M20 5.5c0-.8-.7-1.5-1.5-1.5H13v16h5.5c.8 0 1.5-.7 1.5-1.5z",
    )
    val Close = icon("close", "M6 6l12 12M18 6L6 18")
    val Home = icon("home", "M4 11l8-6.5 8 6.5", "M6 9.5V20h12V9.5", "M10 20v-5h4v5")
    val Search = icon("search", circle(11f, 11f, 6.5f), "M16 16l4.5 4.5")
    val Settings = icon("set", "M4 7h9M17 7h3M4 17h3M11 17h9", circle(15f, 7f, 2f), circle(9f, 17f, 2f))
    val Back = icon("back", "M15 5l-7 7 7 7")
    val Chevron = icon("chev", "M9 6l6 6-6 6")
    val Down = icon("down", "M6 9l6 6 6-6")
    val Sync = icon("sync", "M19.5 12a7.5 7.5 0 0 1-13.3 4.8M4.5 12a7.5 7.5 0 0 1 13.3-4.8", "M18 3.5v3.8h-3.8M6 20.5v-3.8h3.8")
    val Plus = icon("plus", "M12 5v14M5 12h14")
    val Minus = icon("minus", "M5 12h14")
    val Check = icon("check", "M5 12.5l4.5 4.5L19 7.5")
    val More = icon("more", circle(12f, 5.5f, 1f), circle(12f, 12f, 1f), circle(12f, 18.5f, 1f))
    val External = icon(
        "ext",
        "M14 4h6v6M20 4l-9 9",
        "M18 14v4.5c0 .8-.7 1.5-1.5 1.5h-11c-.8 0-1.5-.7-1.5-1.5v-11C4 6.7 4.7 6 5.5 6H10",
    )
    val Lock = icon(
        "lock",
        "M7 10.5h10a2 2 0 0 1 2 2v5.5a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2v-5.5a2 2 0 0 1 2-2z",
        "M8 10.5V8a4 4 0 0 1 8 0v2.5",
    )
    val Clock = icon("clock", circle(12f, 12f, 8f), "M12 7.5V12l3 2")
    val Offline = icon(
        "off",
        "M7 18h10.5a4 4 0 0 0 .7-7.9A6 6 0 0 0 7.1 8.6 4.8 4.8 0 0 0 7 18z",
        "M3 3l18 18",
    )
    val Alert = icon("alert", "M12 4l9 16H3z", "M12 10v4.5M12 17.5v.01")
}
