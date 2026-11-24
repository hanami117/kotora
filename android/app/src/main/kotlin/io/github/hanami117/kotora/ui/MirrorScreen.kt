package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.MirrorAlign
import io.github.hanami117.kotora.core.MirrorOpRow
import io.github.hanami117.kotora.core.MirrorPart
import io.github.hanami117.kotora.core.MirrorPreview
import io.github.hanami117.kotora.core.MirrorPreviewView
import io.github.hanami117.kotora.core.MirrorQueue
import io.github.hanami117.kotora.core.MirrorSettle
import io.github.hanami117.kotora.core.MirrorState
import io.github.hanami117.kotora.data.MirrorRepository
import io.github.hanami117.kotora.data.mirror
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.launch

/*
 * Mirror edits, as the desktop's Settings → Account row and its Queue part (mirror.rs):
 * - [MirrorRow]: "Mirror edits to AniList", its state line, the switch. On reads
 *   the other list and opens [MirrorScreen] (the preview and the three choices); nothing is saved
 *   until one is picked. Off drops what still waits for the mirror.
 * - [MirrorQueuePart]: the Sync queue's mirror part (website edits, conflicts, waiting, failed,
 *   sent today, adds still looking up their id).
 * The mirror's queue goes with the app's flush and sync, the FlushWorker and the NotifyWorker's
 * `background_refresh`: no WorkManager job of its own.
 */

val AppViewModel.mirror: MirrorRepository? get() = repo?.mirror()

/** Settings' row, with its state (the core) and the preview dialog. */
@Composable
fun MirrorSettings(vm: AppViewModel) {
    val m = vm.mirror ?: return
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<MirrorState?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }
    var note by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var preview by remember { mutableStateOf<MirrorPreview?>(null) }
    var view by remember { mutableStateOf<MirrorPreviewView?>(null) }
    LaunchedEffect(vm.list) { state = runCatching { m.state() }.getOrNull() }
    val s = state ?: return
    fun compare(mangaOnly: Boolean) = scope.launch {
        error = null; note = null
        busy = if (mangaOnly) s.readingManga else s.reading
        runCatching { m.compare(mangaOnly) }
            .onSuccess { preview = it; view = it.view() }
            .onFailure { error = message(it) }
        busy = null
    }
    MirrorRow(
        s, busy, note, error, previewOpen = preview != null,
        onSwitch = { on ->
            if (on && !s.on && preview == null) compare(false)
            if (!on && s.on) scope.launch {
                error = null
                busy = s.turningOff
                runCatching { m.stop() }.onSuccess { note = it }.onFailure { error = message(it) }
                busy = null
                state = runCatching { m.state() }.getOrNull()
                vm.listChanged()
            }
        },
        onAlignManga = { compare(true) },
    )
    val p = preview
    val v = view
    if (p != null && v != null) {
        val close = { preview = null; view = null; error = null }
        Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false)) {
            MirrorScreen(
                v,
                onPick = { kind, mal, theirs -> p.pickStatus(kind, mal, theirs); view = p.view() },
                onPickAll = { kind, theirs -> p.pickAll(kind, theirs); view = p.view() },
                onChoose = { align ->
                    preview = null; view = null
                    scope.launch {
                        busy = s.saving
                        runCatching { m.choose(p, align) }.onSuccess { note = it }.onFailure { error = message(it) }
                        busy = null
                        state = runCatching { m.state() }.getOrNull()
                        vm.listChanged()
                    }
                },
                onCancel = close,
            )
        }
    }
}

private fun message(e: Throwable): String = e.message?.takeIf { it.isNotBlank() } ?: "Something went wrong."

/** The row itself: title, the state line (or what is going on), the switch; then the note,
 *  the error, and "Align manga lists" when Manga was turned on after the mirror. */
@Composable
fun MirrorRow(
    s: MirrorState,
    busy: String?,
    note: String?,
    error: String?,
    previewOpen: Boolean,
    onSwitch: (Boolean) -> Unit,
    onAlignManga: () -> Unit,
) {
    val k = Kotora.colors
    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(s.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                Text(busy ?: s.note, color = k.faint, fontSize = 12.5.sp, modifier = Modifier.padding(top = 2.dp))
            }
            Switch(
                checked = s.on || previewOpen,
                onCheckedChange = onSwitch,
                enabled = busy == null && (s.on || s.canTurnOn),
                modifier = Modifier.testTag("settings/mirror"),
                colors = SwitchDefaults.colors(checkedTrackColor = k.accent, checkedThumbColor = Color.White, uncheckedTrackColor = k.borderStrong, uncheckedBorderColor = k.borderStrong, uncheckedThumbColor = k.text),
            )
        }
        if (note != null) Text(note, color = k.text, fontSize = 13.sp, modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp))
        if (error != null) Text(error, color = k.dropped, fontSize = 13.sp, modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp))
        val a = s.alignManga
        if (a != null && busy == null && !previewOpen) {
            HorizontalDivider(color = k.border)
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Column(Modifier.weight(1f)) {
                    Text(a.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    Text(a.note, color = k.faint, fontSize = 12.5.sp, modifier = Modifier.padding(top = 2.dp))
                }
                KButton(a.button, onAlignManga, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
            }
        }
    }
}

/** The two lists compared: counts, the lists, the status conflicts to pick, what can't be
 *  mirrored, what each choice queues, and the choices ({main} wins / Merge / Don't align / Cancel). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MirrorScreen(
    view: MirrorPreviewView,
    onPick: (MediaKind, UInt, Boolean) -> Unit,
    onPickAll: (MediaKind, Boolean) -> Unit,
    onChoose: (MirrorAlign) -> Unit,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg).statusBarsPadding().navigationBarsPadding()) {
        Row(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onCancel) { Icon(KIcons.Back, view.cancel, tint = k.text) }
            Text("Mirror edits", color = k.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        }
        HorizontalDivider(color = k.border)
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            for (part in view.parts) PreviewPart(part, onPick, onPickAll)
            view.explain.forEachIndexed { i, t -> Text(t, color = if (i == 3) k.faint else k.dim, fontSize = 13.sp) }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                KButton(view.mainWins, { onChoose(MirrorAlign.MAIN_WINS) }, height = 36.dp, fontSize = 13.sp)
                KButton(view.merge, { onChoose(MirrorAlign.MERGE) }, kind = ButtonKind.PRIMARY, height = 36.dp, fontSize = 13.sp)
                KButton(view.dontAlign, { onChoose(MirrorAlign.DONT_ALIGN) }, height = 36.dp, fontSize = 13.sp)
                KButton(view.cancel, onCancel, height = 36.dp, fontSize = 13.sp)
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PreviewPart(part: MirrorPart, onPick: (MediaKind, UInt, Boolean) -> Unit, onPickAll: (MediaKind, Boolean) -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(12.dp)
    Column(
        Modifier.fillMaxWidth().background(k.field, shape).border(1.dp, k.border, shape).padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        part.heading?.let { Text(it, color = k.faint, fontSize = 12.sp, fontWeight = FontWeight.Medium, letterSpacing = .5.sp) }
        Text(part.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            for (c in part.counts) {
                val col = when (c.key) {
                    "only-main" -> k.watching
                    "only-mirror" -> k.plan
                    "different" -> k.onHold
                    "same" -> k.completed
                    else -> k.faint
                }
                Text(
                    "${c.count} ${c.label}", color = col, fontSize = 12.sp, fontWeight = FontWeight.Medium,
                    modifier = Modifier.background(col.copy(alpha = .12f), RoundedCornerShape(11.dp)).padding(horizontal = 8.dp, vertical = 3.dp),
                )
            }
        }
        for (g in part.groups) {
            var open by remember(g.key) { mutableStateOf(false) }
            Column {
                Text(
                    "${if (open) "▾" else "▸"} ${g.title} (${g.lines.size})", color = k.text, fontSize = 13.sp, fontWeight = FontWeight.Medium,
                    modifier = Modifier.fillMaxWidth().clickable { open = !open }.padding(vertical = 4.dp),
                )
                if (open) for (l in g.lines) Text(l, color = k.dim, fontSize = 12.5.sp, modifier = Modifier.padding(start = 14.dp, top = 2.dp))
            }
        }
        part.picksHeading?.let { h ->
            Text(h, color = k.faint, fontSize = 11.5.sp, fontWeight = FontWeight.Medium, letterSpacing = .5.sp, modifier = Modifier.padding(top = 4.dp))
            for (r in part.picks) {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(r.title, color = k.text, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Chip(r.mainLabel, !r.mirrorPicked) { onPick(part.kind, r.malId, false) }
                        Chip(r.mirrorLabel, r.mirrorPicked) { onPick(part.kind, r.malId, true) }
                    }
                }
            }
            part.allLine?.let { all ->
                Text(all, color = k.faint, fontSize = 13.sp)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Chip(part.allMainLabel, part.allMainOn) { onPickAll(part.kind, false) }
                    Chip(part.allMirrorLabel, part.allMirrorOn) { onPickAll(part.kind, true) }
                }
            }
        }
    }
}

@Composable
private fun Chip(text: String, on: Boolean, onClick: () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(8.dp)
    Text(
        text, color = k.text, fontSize = 12.5.sp, fontWeight = FontWeight.Medium, maxLines = 1,
        modifier = Modifier.height(30.dp).background(if (on) k.accent.copy(alpha = .18f) else k.raised, shape)
            .border(1.dp, if (on) k.accent else k.borderStrong, shape).clickable(onClick = onClick).padding(horizontal = 10.dp, vertical = 6.dp),
    )
}

/** The Sync queue's mirror part, read from the core, with its actions. */
@Composable
fun MirrorQueue(vm: AppViewModel) {
    val m = vm.mirror ?: return
    val scope = rememberCoroutineScope()
    var q by remember { mutableStateOf<MirrorQueue?>(null) }
    LaunchedEffect(vm.list) { q = runCatching { m.queue() }.getOrNull() }
    val shown = q ?: return
    fun act(f: suspend (MirrorRepository) -> Unit) = scope.launch {
        runCatching { f(m) }.onFailure { vm.snack(Snack(message(it))) }
        q = runCatching { m.queue() }.getOrNull()
        vm.listChanged()
    }
    MirrorQueuePart(
        shown,
        onSettle = { kind, mal, how -> act { it.settle(kind, mal, how) } },
        onKeepMine = { id -> act { it.keepMine(id) } },
        onTakeTheirs = { id -> act { it.takeTheirs(id) } },
        onDiscard = { id -> act { it.discard(id) } },
    )
}

/** The desktop's `queue_section`: heading and when it was read, the trouble, website edits,
 *  conflicts, then Waiting / Failed / Sent today, and "Not on AniList yet". */
@Composable
fun MirrorQueuePart(
    q: MirrorQueue,
    onSettle: (MediaKind, UInt, MirrorSettle) -> Unit,
    onKeepMine: (Long) -> Unit,
    onTakeTheirs: (Long) -> Unit,
    onDiscard: (Long) -> Unit,
) {
    val k = Kotora.colors
    Column(Modifier.fillMaxWidth().testTag("queue/mirror")) {
        Row(Modifier.padding(start = 16.dp, end = 16.dp, top = 22.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(q.heading, color = k.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
            q.read?.let { Text(it, color = k.faint, fontSize = 12.sp) }
        }
        q.trouble?.let { t ->
            val shape = RoundedCornerShape(10.dp)
            Text(
                t, color = k.text, fontSize = 13.sp,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 10.dp).fillMaxWidth()
                    .background(k.onHold.copy(alpha = .10f), shape).border(1.dp, k.onHold.copy(alpha = .4f), shape).padding(12.dp),
            )
        }
        for (e in q.edits) {
            MirrorCard(
                e.title, e.why,
                Side(e.mainHead, e.mainText, e.useMain?.let { l -> l to { onSettle(e.kind, e.malId, MirrorSettle.USE_MAIN) } }),
                Side(e.mirrorHead, e.mirrorText, e.useMirror?.let { l -> l to { onSettle(e.kind, e.malId, MirrorSettle.USE_MIRROR) } }),
                e.leave to { onSettle(e.kind, e.malId, MirrorSettle.LEAVE) },
            )
        }
        for (c in q.conflicts) {
            MirrorCard(
                c.title, c.why,
                Side(c.mineHead, c.mineText, c.keep to { onKeepMine(c.id) }),
                Side(c.theirsHead, c.theirsText, c.take to { onTakeTheirs(c.id) }),
                null,
            )
        }
        for (g in q.groups) {
            SectionLabel(g.label)
            Card {
                Column {
                    g.rows.forEachIndexed { i, r ->
                        if (i > 0) HorizontalDivider(color = k.border)
                        OpLine(r) { onDiscard(r.id) }
                    }
                }
            }
        }
        if (q.waits.isNotEmpty()) {
            SectionLabel(q.waitsHeading)
            Card {
                Column {
                    q.waits.forEachIndexed { i, w ->
                        if (i > 0) HorizontalDivider(color = k.border)
                        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp)) {
                            Text(w.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(w.why, color = k.dim, fontSize = 13.sp)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun OpLine(r: MirrorOpRow, onDiscard: () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.Top,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        val fade = if (r.sent) .6f else 1f
        Column(Modifier.weight(1f)) {
            Text(r.title, color = k.text.copy(alpha = fade), fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(r.what, color = k.dim.copy(alpha = fade), fontSize = 13.sp)
            r.note?.let { Text(it, color = if (r.noteIsError) k.dropped else k.faint, fontSize = 12.sp, maxLines = 2) }
            Text(r.age, color = k.faint, fontSize = 12.sp)
        }
        when {
            r.canDiscard -> KButton("Discard", onDiscard, height = 34.dp, fontSize = 13.sp)
            r.sent -> Text(r.state, color = k.watching.copy(alpha = fade), fontSize = 12.sp, fontWeight = FontWeight.Medium)
            else -> {
                val col = if (r.state == "Sending") k.dim else k.onHold
                Text(r.state, color = col, fontSize = 11.5.sp, fontWeight = FontWeight.Medium, modifier = Modifier.background(col.copy(alpha = .12f), RoundedCornerShape(10.dp)).padding(horizontal = 8.dp, vertical = 2.dp))
            }
        }
    }
}

private class Side(val head: String, val body: String, val act: Pair<String, () -> Unit>?)

@Composable
private fun MirrorCard(title: String, why: String, a: Side, b: Side, foot: Pair<String, () -> Unit>?) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(14.dp)
    Column(
        Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp).fillMaxWidth()
            .background(k.surface, shape).border(1.dp, k.accent.copy(alpha = .45f), shape).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("⚠", color = k.accent, fontSize = 13.sp)
            Text(title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, maxLines = 2)
        }
        Text(why, color = k.dim, fontSize = 13.sp)
        SideBox(a, true)
        SideBox(b, false)
        foot?.let { KButton(it.first, it.second, height = 34.dp, fontSize = 13.sp) }
    }
}

@Composable
private fun SideBox(s: Side, on: Boolean) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    Column(
        Modifier.fillMaxWidth().background(k.field, shape).border(1.dp, if (on) k.accent.copy(alpha = .5f) else k.border, shape).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(s.head, color = k.faint, fontSize = 12.sp)
        Text(s.body, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        s.act?.let { KButton(it.first, it.second, kind = if (on) ButtonKind.PRIMARY else ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp) }
    }
}
