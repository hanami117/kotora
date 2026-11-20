package io.github.hanami117.kotora.ui

import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.ScoreChoice
import io.github.hanami117.kotora.core.ServiceLink
import io.github.hanami117.kotora.core.plusOneLabel
import io.github.hanami117.kotora.ui.theme.Kotora
import io.github.hanami117.kotora.core.Row as ListRow

/** What the long-press menu does: the view model's Move to › and Score › (`KotoraApp` gives it). */
class QuickActions(
    val choices: List<ScoreChoice>,
    val move: (ListRow, ListStatus) -> Unit,
    val score: (ListRow, UInt?) -> Unit,
)

val LocalQuickActions = compositionLocalOf<QuickActions?> { null }

/** A manga read by volume (no chapter yet, some volumes): +1 steps volumes, as on the desktop. */
val ListRow.plusVolumes: Boolean get() = kind == io.github.hanami117.kotora.core.MediaKind.MANGA && progress == 0u && volumes > 0u

/** +1 is offered while watching (or rewatching) and before the last episode (volume). */
val ListRow.canPlusOne: Boolean
    get() {
        val atEnd = if (plusVolumes) totalVolumes?.let { volumes >= it } == true else total?.let { progress >= it } == true
        return !atEnd && (status == ListStatus.WATCHING || rewatching)
    }

/** "One more episode", "One more chapter", "One more volume" (the core's words). */
val ListRow.plusOneText: String
    get() = coreOr(if (kind == io.github.hanami117.kotora.core.MediaKind.ANIME) "One more episode" else if (plusVolumes) "One more volume" else "One more chapter") { plusOneLabel(this) }

/**
 * A core call for a label, or [fallback] when the native library isn't there (the JVM
 * screenshot tests run without it). The fallback is the core's text.
 */
inline fun coreOr(fallback: String, f: () -> String): String =
    try {
        f()
    } catch (_: LinkageError) {
        fallback
    }

/** The waiting change's tag: "Waiting to sync", "Conflict in queue", "Sync failed" (desktop `waiting_label`). */
val ListRow.waitingText: String get() = syncLabel ?: "Waiting to sync"

/** A conflict or a failure: the tag is red. */
val ListRow.waitingProblem: Boolean get() = syncLabel != null && syncLabel != "Waiting to sync"

private enum class Page { MAIN, MOVE, SCORE }

/**
 * The long-press menu of a row or poster, as the desktop's right-click menu (`ctxmenu.rs`):
 * Move to › the statuses, Score › the user's format and "No score", then the Open on links.
 */
@Composable
fun QuickMenu(row: ListRow, open: Boolean, links: List<ServiceLink>, onOpenUrl: (String) -> Unit, onDismiss: () -> Unit) {
    val k = Kotora.colors
    val actions = LocalQuickActions.current
    var page by remember(open) { mutableStateOf(Page.MAIN) }
    val close = {
        page = Page.MAIN
        onDismiss()
    }
    DropdownMenu(expanded = open, onDismissRequest = close, containerColor = k.raised) {
        when (page) {
            Page.MAIN -> {
                if (actions != null) {
                    DropdownMenuItem(text = { Text("Move to", color = k.text) }, trailingIcon = { Text("›", color = k.dim) }, onClick = { page = Page.MOVE })
                    DropdownMenuItem(text = { Text("Score", color = k.text) }, trailingIcon = { Text("›", color = k.dim) }, onClick = { page = Page.SCORE })
                    if (links.isNotEmpty()) HorizontalDivider(color = k.border)
                }
                for (l in links) {
                    DropdownMenuItem(
                        text = { Text(l.label, color = k.text) },
                        trailingIcon = { Icon(KIcons.External, null, tint = k.dim, modifier = Modifier.size(16.dp)) },
                        onClick = {
                            close()
                            onOpenUrl(l.url)
                        },
                    )
                }
            }
            Page.MOVE -> for (st in ListStatus.entries) {
                DropdownMenuItem(
                    text = { Text(statusLabel(st, row.kind), color = k.text) },
                    trailingIcon = { if (st == row.status) Text("✓", color = k.accentHi) },
                    onClick = {
                        close()
                        actions?.move?.invoke(row, st)
                    },
                )
            }
            Page.SCORE -> {
                for (c in actions?.choices.orEmpty()) {
                    DropdownMenuItem(
                        text = { Text(c.label, color = k.text) },
                        trailingIcon = { if (c.label == row.score) Text("✓", color = k.accentHi) },
                        onClick = {
                            close()
                            actions?.score?.invoke(row, c.points)
                        },
                    )
                }
                HorizontalDivider(color = k.border)
                DropdownMenuItem(
                    text = { Text("No score", color = k.text) },
                    trailingIcon = { if (row.score == null) Text("✓", color = k.accentHi) },
                    onClick = {
                        close()
                        actions?.score?.invoke(row, null)
                    },
                )
            }
        }
    }
}

/** "Mark anyway?" before a +1 of an episode not aired yet (the desktop's `ui.rs` question). */
@Composable
fun MarkAnywayDialog(m: MarkAnyway, onMark: () -> Unit, onCancel: () -> Unit) {
    val k = Kotora.colors
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text("Mark anyway?", color = k.text) },
        text = { Text(m.question, color = k.dim) },
        confirmButton = { TextButton(onClick = onMark) { Text("Mark anyway", color = k.accentHi) } },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel", color = k.dim) } },
        containerColor = k.raised,
    )
}
