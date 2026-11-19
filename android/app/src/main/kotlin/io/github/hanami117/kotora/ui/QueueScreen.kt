package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.QueueItem
import io.github.hanami117.kotora.core.QueueState
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.ui.theme.Kotora

/**
 * The sync queue, as the desktop's: conflicts as cards (Keep mine / Take theirs), then Waiting,
 * Failed and Sent today. Every signed-in account's changes.
 */
@Composable
fun QueueScreen(
    items: List<QueueItem>?,
    service: String,
    now: Long,
    problem: String?,
    syncing: Boolean,
    onBack: () -> Unit,
    onSyncNow: () -> Unit,
    onAction: (QueueAction, QueueItem) -> Unit,
    modifier: Modifier = Modifier,
) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg)) {
        Row(
            Modifier.statusBarsPadding().fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(KIcons.Back, "Back", tint = k.text) }
            Text("Sync queue", color = k.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
            KButton("Sync now", onSyncNow, kind = ButtonKind.GHOST, height = 40.dp, enabled = !syncing)
        }
        HorizontalDivider(color = k.border)
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
            if (problem != null) {
                Text(
                    problem,
                    color = k.dropped,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp).fillMaxWidth()
                        .background(k.dropped.copy(alpha = .1f), RoundedCornerShape(10.dp)).padding(12.dp),
                )
            }
            if (items != null && items.isEmpty()) {
                Column(Modifier.fillMaxWidth().padding(top = 80.dp, start = 24.dp, end = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Nothing waiting", color = k.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    Text("Your changes go here first, then to $service.", color = k.dim, fontSize = 13.sp, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 4.dp))
                }
            }
            val all = items ?: emptyList()
            for (item in all.filter { it.state == QueueState.CONFLICT }) {
                ConflictCard(item, now, onAction, Modifier.padding(top = 12.dp))
            }
            Group("Waiting", all.filter { it.state == QueueState.WAITING || it.state == QueueState.SENDING }, true, now, onAction)
            Group("Failed", all.filter { it.state == QueueState.FAILED }, true, now, onAction)
            Group("Sent today", all.filter { it.state == QueueState.SENT }, false, now, onAction)
        }
    }
}

@Composable
private fun Group(title: String, list: List<QueueItem>, count: Boolean, now: Long, onAction: (QueueAction, QueueItem) -> Unit) {
    if (list.isEmpty()) return
    val k = Kotora.colors
    SectionLabel(if (count) "$title ${list.size}" else title)
    Card {
        Column {
            list.forEachIndexed { i, item ->
                if (i > 0) HorizontalDivider(color = k.border, modifier = Modifier.padding(start = 64.dp))
                QueueLine(item, now, onAction)
            }
        }
    }
}

/** A cover for a queue line: only the id, the title and the address are known. */
@Composable
private fun QueueCover(item: QueueItem) {
    val row = ListRow(item.kind, item.mediaId, item.title, null, ListStatus.WATCHING, 0u, null, 0u, null, null, "", "", null, item.coverUrl, false, 0u, false, 0)
    Cover(row, 36.dp, 52.dp, 6.dp, 12.sp)
}

@Composable
private fun QueueLine(item: QueueItem, now: Long, onAction: (QueueAction, QueueItem) -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp).alpha(if (item.state == QueueState.SENT) .6f else 1f),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        QueueCover(item)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(item.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(item.what, color = k.dim, fontSize = 13.sp)
                if (item.edits > 1u && item.state != QueueState.SENT) Text("(${item.edits} edits merged)", color = k.faint, fontSize = 12.sp)
            }
            item.error?.let { e ->
                val failed = item.state == QueueState.FAILED
                Text(if (failed) e else "$e; Kotora tries again by itself", color = if (failed) k.dropped else k.faint, fontSize = 12.sp, maxLines = 2)
            }
            Text(ago(item.sentAt ?: item.editedAt, now), color = k.faint, fontSize = 12.sp)
        }
        Box(Modifier.align(Alignment.CenterVertically)) {
            when {
                item.removed && (item.state == QueueState.WAITING || item.state == QueueState.FAILED) -> QueueButton("Undo") { onAction(QueueAction.DISCARD, item) }
                item.state == QueueState.FAILED -> QueueButton("Discard") { onAction(QueueAction.DISCARD, item) }
                item.state == QueueState.SENT -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    Icon(KIcons.Check, null, tint = k.watching, modifier = Modifier.size(14.dp))
                    Text("Sent", color = k.watching, fontSize = 12.sp, fontWeight = FontWeight.Medium)
                }
                item.state == QueueState.SENDING -> QueuePill("Sending", k.dim)
                else -> QueuePill("Waiting", k.onHold)
            }
        }
    }
}

/** A conflict: the user's change and the service's entry side by side, Keep mine / Take theirs. */
@Composable
fun ConflictCard(item: QueueItem, now: Long, onAction: (QueueAction, QueueItem) -> Unit, modifier: Modifier = Modifier) {
    val k = Kotora.colors
    val service = serviceTitle(item.service)
    val theirs = item.theirs
    val (theirsHead, theirsValue, take) = when {
        theirs == null -> Triple(service, "Removed from your list", "Remove here too")
        item.removed -> Triple("$service · changed ${ago(item.theirsChangedAt ?: now, now)}", theirs, "Keep it")
        else -> Triple("$service · changed ${ago(item.theirsChangedAt ?: now, now)}", theirs, "Take theirs")
    }
    val shape = RoundedCornerShape(14.dp)
    Column(
        modifier.fillMaxWidth().padding(horizontal = 16.dp).clip(shape).background(k.surface, shape)
            .border(1.dp, k.accent.copy(alpha = .45f), shape).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            QueueCover(item)
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(KIcons.Alert, null, tint = k.accent, modifier = Modifier.size(15.dp))
                    Text("Conflict: ${item.title}", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, maxLines = 2)
                }
                Text(item.theirsEntry?.let { "$service has $it" } ?: "removed on another device", color = k.faint, fontSize = 12.sp, maxLines = 2)
            }
        }
        Text(
            if (item.removed) "$service changed after you removed it. Kotora did not delete it. Pick one."
            else "$service changed after your edit. Kotora did not overwrite it. Pick one.",
            color = k.dim,
            fontSize = 13.sp,
        )
        Side("Yours · ${if (item.removed) "removed" else "edited"} here, ${ago(item.editedAt, now)}", item.mine, true) {
            QueueButton(if (item.removed) "Remove anyway" else "Keep mine", primary = true) { onAction(QueueAction.KEEP_MINE, item) }
        }
        Side(theirsHead, theirsValue, false) {
            QueueButton(take) { onAction(QueueAction.TAKE_THEIRS, item) }
        }
        if (!item.removed) {
            Text("When only the progress differs and yours is higher, Kotora sends yours by itself.", color = k.faint, fontSize = 12.sp)
        }
    }
}

@Composable
private fun Side(head: String, value: String, mine: Boolean, button: @Composable () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    Column(
        Modifier.fillMaxWidth().clip(shape).background(k.field, shape).border(1.dp, if (mine) k.accent.copy(alpha = .5f) else k.border, shape).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(head, color = k.faint, fontSize = 12.sp)
        Text(value, color = k.text, fontSize = 16.sp, fontWeight = FontWeight.Medium)
        Box(Modifier.padding(top = 2.dp)) { button() }
    }
}

@Composable
private fun QueueButton(title: String, primary: Boolean = false, onClick: () -> Unit) {
    KButton(title, onClick, kind = if (primary) ButtonKind.PRIMARY else ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
}

@Composable
fun QueuePill(text: String, color: Color) {
    Text(
        text,
        color = color,
        fontSize = 11.5.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.background(color.copy(alpha = .12f), CircleShape).padding(horizontal = 8.dp, vertical = 2.dp),
    )
}

/** Settings' line to the queue: what waits, and the conflicts that want a pick. */
@Composable
fun QueueLink(waiting: UInt, conflicts: UInt, onOpen: () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(role = Role.Button, onClick = onOpen).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text("Sync queue", color = k.text, fontWeight = FontWeight.Medium, fontSize = 14.sp)
            Text(if (waiting == 0u) "Everything is sent." else "$waiting waiting to sync", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
        }
        if (conflicts > 0u) QueuePill(if (conflicts == 1u) "1 conflict" else "$conflicts conflicts", k.accent)
        Icon(KIcons.Chevron, null, tint = k.faint, modifier = Modifier.size(18.dp))
    }
}

