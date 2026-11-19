package io.github.hanami117.kotora.ui

import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.ExportRow
import io.github.hanami117.kotora.core.ImportConflict
import io.github.hanami117.kotora.core.ImportDone
import io.github.hanami117.kotora.core.ImportGroup
import io.github.hanami117.kotora.core.ImportPreview
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.Kotora as Core
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.XmlExport
import io.github.hanami117.kotora.core.XmlImport
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/*
 * Settings → Your list: Export anime/manga list… (a MyAnimeList XML file, saved where the system's
 * file picker says: SAF ACTION_CREATE_DOCUMENT) and Import anime/manga list… (ACTION_OPEN_DOCUMENT,
 * a .xml / .xml.gz, previewed, then queued). The desktop's rows and words (`app/src/transfer.rs`);
 * the work is `mobile/src/xml.rs`.
 */

private fun msg(e: Throwable) = (e as? KotoraException)?.message?.takeIf { it.isNotBlank() } ?: e.message ?: "Something went wrong."

/** The section, with its pickers and the import sheet. `core`: null with sample data (no section). */
@Composable
fun TransferSection(core: Core?, mangaEnabled: Boolean, onChanged: () -> Unit, onQueue: () -> Unit) {
    if (core == null) return
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val kinds = if (mangaEnabled) listOf(MediaKind.ANIME, MediaKind.MANGA) else listOf(MediaKind.ANIME)
    var notes by remember { mutableStateOf(mapOf<MediaKind, String>()) }
    var pending by remember { mutableStateOf<Pair<MediaKind, XmlExport>?>(null) }
    var importKind by remember { mutableStateOf(MediaKind.ANIME) }
    var importing by remember { mutableStateOf<Pair<String, ByteArray?>?>(null) }

    fun nameOf(uri: Uri): String = runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
    }.getOrNull() ?: uri.lastPathSegment ?: "the file"

    val create = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/xml")) { uri ->
        val (kind, ex) = pending ?: return@rememberLauncherForActivityResult
        pending = null
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val r = runCatching {
                withContext(Dispatchers.IO) {
                    context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(ex.bytes()) } ?: error("Can’t write the file.")
                    ex.savedNote(nameOf(uri))
                }
            }
            notes = notes + (kind to r.getOrElse { "Can’t write the file: ${msg(it)}" })
        }
    }
    val open = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val bytes = withContext(Dispatchers.IO) { runCatching { context.contentResolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull() }
            importing = nameOf(uri) to bytes
        }
    }

    val rows = kinds.map { kind ->
        val ex = runCatching { core.xmlExportRow(kind) }.getOrNull()
        val im = runCatching { core.xmlImport(kind).row() }.getOrNull()
        kind to (ex?.let { it.copy(note = notes[kind] ?: it.note) } to im)
    }
    TransferRows(
        rows.map { (kind, r) -> Triple(kind, r.first, r.second) },
        onExport = { kind ->
            scope.launch {
                runCatching { withContext(Dispatchers.IO) { core.xmlExport(kind) } }
                    .onSuccess { pending = kind to it; create.launch(it.fileName()) }
                    .onFailure { notes = notes + (kind to msg(it)) }
            }
        },
        onImport = { kind ->
            importKind = kind
            open.launch(arrayOf("text/xml", "application/xml", "application/gzip", "application/x-gzip", "application/octet-stream"))
        },
    )
    importing?.let { (name, bytes) ->
        ImportSheet(core, importKind, name, bytes, onChanged, onQueue = { importing = null; onQueue() }) { importing = null }
    }
}

/** The rows of the section (stateless: the screenshot tests draw it). */
@Composable
fun TransferRows(rows: List<Triple<MediaKind, ExportRow?, ExportRow?>>, onExport: (MediaKind) -> Unit, onImport: (MediaKind) -> Unit) {
    val k = Kotora.colors
    SectionLabel("Your list")
    Card {
        Column {
            rows.forEachIndexed { i, (kind, ex, im) ->
                if (ex != null) {
                    TransferLine(ex.title, ex.note, "Export…", ex.enabled) { onExport(kind) }
                    HorizontalDivider(color = k.border)
                }
                if (im != null) {
                    TransferLine(im.title, im.note, "Import…", im.enabled) { onImport(kind) }
                    if (i < rows.size - 1) HorizontalDivider(color = k.border)
                }
            }
        }
    }
}

@Composable
private fun TransferLine(title: String, note: String, action: String, enabled: Boolean, onClick: () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().clickable(enabled = enabled, role = Role.Button, onClick = onClick).heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, color = k.text, fontWeight = FontWeight.Medium, fontSize = 14.sp)
            Text(note, color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
        }
        Text(action, color = if (enabled) k.accentHi else k.faint, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}

// ---------------------------------------------------------------------------------------- import

/** What the import sheet shows (stateless, for [ImportBody]). */
data class ImportUi(
    val busy: String? = null,
    val error: String? = null,
    val preview: ImportPreview? = null,
    val useFile: Set<UInt> = emptySet(),
    val applying: Boolean = false,
    val done: ImportDone? = null,
    val undone: String? = null,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ImportSheet(core: Core, kind: MediaKind, name: String, bytes: ByteArray?, onChanged: () -> Unit, onQueue: () -> Unit, onClose: () -> Unit) {
    val k = Kotora.colors
    val scope = rememberCoroutineScope()
    var job by remember { mutableStateOf<XmlImport?>(null) }
    var ui by remember { mutableStateOf(ImportUi(busy = "Reading the file…")) }

    LaunchedEffect(Unit) {
        if (bytes == null) { ui = ImportUi(error = "Can’t read the file: $name"); return@LaunchedEffect }
        try {
            val j = withContext(Dispatchers.IO) { core.xmlImport(kind) }
            job = j
            val poll = launch { while (isActive) { j.busy().takeIf { it.isNotEmpty() }?.let { ui = ui.copy(busy = it) }; delay(150) } }
            val r = runCatching { withContext(Dispatchers.IO) { j.read(bytes, name) } }
            poll.cancel()
            ui = r.fold({ ImportUi(preview = it) }, { ImportUi(error = msg(it)) })
        } catch (e: Exception) {
            ui = ImportUi(error = msg(e))
        }
    }
    DisposableEffect(Unit) { onDispose { job?.cancel() } }

    fun apply(newOnly: Boolean) {
        val j = job ?: return
        ui = ui.copy(applying = true)
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { j.apply(newOnly, ui.useFile.toList()) } }
                .onSuccess { ui = ImportUi(done = it); if (it.queued > 0u) onChanged() }
                .onFailure { ui = ui.copy(applying = false, error = msg(it)) }
        }
    }

    fun undo() {
        val j = job ?: return
        ui = ui.copy(applying = true)
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { j.undo() } }
                .onSuccess { ui = ui.copy(applying = false, undone = it); onChanged() }
                .onFailure { ui = ui.copy(applying = false, error = msg(it)) }
        }
    }

    ModalBottomSheet(onDismissRequest = { job?.cancel(); onClose() }, containerColor = k.bg) {
        ImportBody(
            kind, ui,
            onPick = { id, file -> ui = ui.copy(useFile = if (file) ui.useFile + id else ui.useFile - id) },
            onPickAll = { file -> ui = ui.copy(useFile = if (file) ui.preview?.conflicts?.map { it.id }?.toSet().orEmpty() else emptySet()) },
            onApply = ::apply,
            onUndo = ::undo,
            onQueue = onQueue,
            onClose = { job?.cancel(); onClose() },
        )
    }
}

/** The most rows listed per group, and conflicts one by one (the desktop's). */
private const val SHOWN = 6
private const val CONFLICTS_SHOWN = 8

/** The desktop's import panel: reading, the preview (counts, groups, status to pick), the result. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ImportBody(
    kind: MediaKind,
    ui: ImportUi,
    onPick: (UInt, Boolean) -> Unit = { _, _ -> },
    onPickAll: (Boolean) -> Unit = {},
    onApply: (Boolean) -> Unit = {},
    onUndo: () -> Unit = {},
    onQueue: () -> Unit = {},
    onClose: () -> Unit = {},
) {
    val k = Kotora.colors
    Column(Modifier.fillMaxWidth().background(k.bg).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Text(if (kind == MediaKind.MANGA) "Import manga list" else "Import anime list", color = k.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        ui.busy?.let {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                CircularProgressIndicator(Modifier.size(18.dp), color = k.accent, strokeWidth = 2.dp)
                Text(it, color = k.dim, fontSize = 15.sp)
            }
        }
        ui.error?.let { Text(it, color = k.dropped, fontSize = 13.sp) }
        ui.preview?.let { pv ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(pv.file, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                Text(pv.total, color = k.faint, fontSize = 13.sp)
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Pill(pv.new, "new", k.watching)
                Pill(pv.different, "different", k.onHold)
                Pill(pv.same, "same", k.completed)
                Pill(pv.unknown, "unknown", k.faint)
            }
            for (g in pv.groups.filter { it.heading != "Left out" }) ImportGroupBox(g)
            if (pv.conflicts.isNotEmpty()) Conflicts(pv.conflicts, ui.useFile, onPick, onPickAll)
            for (g in pv.groups.filter { it.heading == "Left out" }) ImportGroupBox(g)
            Text(pv.note, color = k.faint, fontSize = 12.sp)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                KButton(pv.addNew, { onApply(true) }, height = 36.dp, enabled = pv.addNewEnabled && !ui.applying)
                KButton(pv.addAndUpdate, { onApply(false) }, kind = ButtonKind.PRIMARY, height = 36.dp, enabled = pv.addAndUpdateEnabled && !ui.applying)
                KButton("Cancel", onClose, height = 36.dp, enabled = !ui.applying)
            }
        }
        ui.done?.let { d ->
            Text(ui.undone ?: d.message, color = k.text, fontSize = 15.sp)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (d.canUndo && ui.undone == null) {
                    KButton("Open Queue", onQueue, height = 36.dp)
                    KButton("Undo import", onUndo, height = 36.dp, enabled = !ui.applying)
                }
                KButton("Close", onClose, height = 36.dp)
            }
        }
    }
}

@Composable
private fun Pill(n: UInt, word: String, color: Color) {
    val k = Kotora.colors
    Row(
        Modifier.background(color.copy(alpha = .14f), RoundedCornerShape(11.dp)).padding(horizontal = 8.dp, vertical = 3.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(Modifier.size(6.dp).background(color, CircleShape)) {}
        Text("$n $word", color = k.text, fontSize = 12.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun ImportGroupBox(g: ImportGroup) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    Column(Modifier.fillMaxWidth().background(k.field, shape).border(1.dp, k.border, shape).padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(g.heading.uppercase(), color = k.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = .4.sp)
        for (l in g.rows.take(SHOWN)) Text(l, color = k.dim, fontSize = 13.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (g.rows.size > SHOWN) Text("and ${g.rows.size - SHOWN} more", color = k.faint, fontSize = 13.sp)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Conflicts(cs: List<ImportConflict>, useFile: Set<UInt>, onPick: (UInt, Boolean) -> Unit, onPickAll: (Boolean) -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    Column(Modifier.fillMaxWidth().background(k.field, shape).border(1.dp, k.border, shape).padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("STATUS TO PICK (WITH “UPDATE”)", color = k.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = .4.sp)
        for (c in cs.take(CONFLICTS_SHOWN)) {
            Text(c.title, color = k.text, fontSize = 13.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Choice(c.keep, c.id !in useFile) { onPick(c.id, false) }
                Choice(c.useFile, c.id in useFile) { onPick(c.id, true) }
            }
        }
        if (cs.size > 1) {
            val more = if (cs.size > CONFLICTS_SHOWN) "and ${cs.size - CONFLICTS_SHOWN} more. " else ""
            Text("${more}All of them:", color = k.faint, fontSize = 13.sp)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Choice("Keep all yours", useFile.isEmpty()) { onPickAll(false) }
                Choice("Use all from the file", cs.all { it.id in useFile }) { onPickAll(true) }
            }
        }
    }
}

@Composable
private fun Choice(title: String, on: Boolean, onClick: () -> Unit) {
    val k = Kotora.colors
    Text(
        title,
        color = if (on) Color.White else k.text,
        fontSize = 12.5.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.background(if (on) k.accent else k.raised, RoundedCornerShape(7.dp)).clickable(role = Role.RadioButton, onClick = onClick).padding(horizontal = 10.dp, vertical = 6.dp),
    )
}
