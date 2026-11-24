package io.github.hanami117.kotora.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.ui.theme.Kotora

/**
 * The bottom of the details page's "Your list" card, as the desktop's: the rewatch count, then
 * Edit, −1, Rewatch for a completed entry (Stop rewatching during one; manga: Reread), and
 * Remove from list….
 */
@Composable
fun EntryActions(
    row: ListRow,
    onEdit: () -> Unit,
    onRewatch: (Boolean) -> Unit,
    onMinusOne: () -> Unit,
    onRemove: () -> Unit,
) {
    val k = Kotora.colors
    val manga = row.kind == MediaKind.MANGA
    HorizontalDivider(color = k.border)
    Column(Modifier.fillMaxWidth().padding(16.dp)) {
        if (row.status == ListStatus.COMPLETED || row.timesRewatched > 0u || row.rewatching) {
            Row(Modifier.fillMaxWidth().padding(bottom = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(if (manga) "Reread" else "Rewatched", color = k.faint, fontSize = 14.sp, modifier = Modifier.weight(1f))
                if (row.rewatching) Tag(if (manga) "Rereading" else "Rewatching")
                Text(times(row.timesRewatched), color = k.text, fontSize = 14.sp, fontWeight = FontWeight.Medium)
            }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            KButton("Edit", onEdit, kind = ButtonKind.PRIMARY)
            if (row.progress > 0u) {
                KButton("−1", onMinusOne, Modifier.semantics { contentDescription = "One less ${if (manga) "chapter" else "episode"}" })
            }
            if (row.status == ListStatus.COMPLETED && !row.rewatching) {
                KButton(if (manga) "Reread" else "Rewatch", { onRewatch(true) }, Modifier.semantics { contentDescription = if (manga) "Reread from chapter 1" else "Rewatch from episode 1" })
            } else if (row.rewatching) {
                KButton(if (manga) "Stop rereading" else "Stop rewatching", { onRewatch(false) })
            }
        }
        KButton("Remove from list…", onRemove, Modifier.padding(top = 4.dp), kind = ButtonKind.GHOST, height = 36.dp, fontSize = 13.sp)
    }
}

/** "Not yet", "Once", "Twice", "3 times" (the core's words). */
fun times(n: UInt): String = when (n) {
    0u -> "Not yet"
    1u -> "Once"
    2u -> "Twice"
    else -> "$n times"
}

/** Remove from list asks first, in the desktop's words. */
@Composable
fun RemoveDialog(question: String, onRemove: () -> Unit, onDismiss: () -> Unit) {
    val k = Kotora.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = k.surface,
        title = { Text("Remove from list", color = k.text) },
        text = { Text(question, color = k.dim) },
        confirmButton = { TextButton(onClick = onRemove) { Text("Remove", color = k.dropped, fontWeight = FontWeight.SemiBold) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel", color = k.dim) } },
    )
}
