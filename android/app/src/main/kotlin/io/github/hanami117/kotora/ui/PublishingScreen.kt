package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.Notice
import io.github.hanami117.kotora.core.PublishingLook
import io.github.hanami117.kotora.core.PublishingPage
import io.github.hanami117.kotora.core.PublishingRow
import io.github.hanami117.kotora.core.PublishingSort
import io.github.hanami117.kotora.data.PagesSource
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.launch

/*
 * Publishing (Manga mode, in Airing's place), the desktop's (`app/src/publishing.rs`) and iOS's:
 * for each Reading manga, the latest chapter out and when, the chapters left to read, an estimate
 * of the next release, from MangaUpdates and MangaDex (`kotora-mobile`, the desktop's words).
 * Opening the page asks only for what is older than four hours; Refresh asks again for all. A row
 * opens its panel: the matched series, recent releases, the series' page, a pasted link to fix it.
 */

private val SORTS = listOf(PublishingSort.NEXT to "Next release", PublishingSort.BEHIND to "Most to read", PublishingSort.LATEST to "Latest release")

private fun errorText(e: Throwable) = (e as? KotoraException)?.message?.takeIf { it.isNotBlank() } ?: e.message ?: "Something went wrong."

@Composable
fun PublishingScreen(
    pages: PagesSource,
    account: Account?,
    now: Long,
    /** Changes when the list changes (a +1, a sync): the page is read again. */
    listKey: Any?,
    onOpen: (UInt) -> Unit,
    onPlusOne: (UInt) -> Unit,
    onUrl: (String) -> Unit,
    onNotices: (List<Notice>) -> Unit,
    modifier: Modifier = Modifier,
    initial: PublishingPage? = null,
    refreshOnOpen: Boolean = true,
) {
    val k = Kotora.colors
    val scope = rememberCoroutineScope()
    var sort by remember { mutableStateOf(PublishingSort.NEXT) }
    var page by remember { mutableStateOf(initial) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var open by remember { mutableStateOf<PublishingRow?>(null) }

    suspend fun load() {
        try {
            page = pages.publishingPage(sort)
        } catch (e: Exception) {
            error = errorText(e)
        }
    }

    fun refresh(force: Boolean) {
        if (busy) return
        busy = true
        scope.launch {
            try {
                val r = pages.publishingRefresh(force)
                error = r.error
                if (r.notices.isNotEmpty()) onNotices(r.notices)
            } catch (e: Exception) {
                error = errorText(e)
            }
            load()
            busy = false
        }
    }

    LaunchedEffect(sort, listKey) { load() }
    if (refreshOnOpen) LaunchedEffect(Unit) {
        load()
        if (page?.stale == true) refresh(false)
    }

    Column(modifier.fillMaxSize().background(k.bg)) {
        TopBar("Publishing", account, busy, { refresh(true) })
        val status = when {
            busy -> "Checking MangaUpdates…"
            page?.checkedAt != null -> "Checked ${ago(page!!.checkedAt!!, now)}"
            else -> null
        }
        if (status != null) Text(status, color = k.faint, fontSize = 12.5.sp, modifier = Modifier.padding(start = 16.dp, bottom = 6.dp))
        error?.let { e ->
            PubAlert(if (page?.checkedAt != null) "$e Showing the saved releases." else e) { refresh(true) }
        }
        val p = page
        when {
            p == null -> Spacer(Modifier.weight(1f))
            p.rows.isEmpty() -> Column(Modifier.fillMaxSize().padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                Text("No manga in Reading", color = k.text, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(4.dp))
                Text("Manga you’re reading show their latest chapters here.", color = k.dim, fontSize = 13.sp, textAlign = TextAlign.Center)
                Spacer(Modifier.height(60.dp))
            }
            else -> {
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 10.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for ((s, label) in SORTS) PubChip(label, sort == s) { sort = s }
                }
                LazyColumn(Modifier.fillMaxSize()) {
                    items(p.rows, key = { it.id.toLong() }) { r -> PubRow(r, busy, { open = r }, { onOpen(r.id) }, { onPlusOne(r.id) }) }
                    item {
                        Text(
                            "${p.credit} · Next dates are estimates from past releases, not announcements.",
                            color = k.faint, fontSize = 12.sp, textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp),
                        )
                    }
                }
            }
        }
    }
    open?.let { r ->
        PublishingPanel(r, pages, onUrl, onClose = { open = null }, onChanged = { scope.launch { load(); open = page?.rows?.firstOrNull { it.id == r.id } } })
    }
}

@Composable
private fun PubChip(label: String, on: Boolean, onClick: () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(16.dp)
    Text(
        label, color = if (on) k.accentHi else k.dim, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 1,
        modifier = Modifier.clip(shape).background(if (on) k.tint else k.field, shape).border(1.dp, if (on) k.accent.copy(alpha = .45f) else k.border, shape)
            .clickable(onClick = onClick).padding(horizontal = 12.dp, vertical = 6.dp),
    )
}

@Composable
private fun PubAlert(text: String, onRetry: () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 10.dp)
            .background(k.dropped.copy(alpha = .10f), RoundedCornerShape(10.dp)).border(1.dp, k.dropped.copy(alpha = .35f), RoundedCornerShape(10.dp))
            .padding(start = 12.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, color = k.text, fontSize = 13.sp, modifier = Modifier.weight(1f))
        KButton("Retry", onRetry, kind = ButtonKind.SECONDARY, height = 32.dp, fontSize = 13.sp)
    }
}

@Composable
private fun PubRow(r: PublishingRow, busy: Boolean, onPanel: () -> Unit, onOpen: () -> Unit, onPlusOne: () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onPanel).padding(start = 16.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        val shape = RoundedCornerShape(5.dp)
        Box(Modifier.size(44.dp, 64.dp).clip(shape).background(tileBrush(r.id.toInt()), shape).clickable(onClick = onOpen), contentAlignment = Alignment.Center) {
            Text(initials(r.title), color = Color.White.copy(alpha = .92f), fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            if (r.coverUrl != null) AsyncImage(model = r.coverUrl, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(r.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                if (r.look == PublishingLook.GUESS) Tag("Check match", TagKind.WAIT)
                if (r.error != null) Tag("Couldn’t check", TagKind.WAIT)
            }
            if (r.latest.isEmpty()) {
                val t = when (r.look) {
                    PublishingLook.PENDING -> if (busy) "Looking…" else "–"
                    PublishingLook.NOT_FOUND -> "Not found on the release sources"
                    else -> "–"
                }
                Text(t, color = k.faint, fontSize = 12.5.sp)
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(r.latest, color = if (r.fresh) k.accentHi else k.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
                    Text(r.`when`, color = if (r.ahead) k.onHold else k.dim, fontSize = 12.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (r.pace.isNotEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(r.pace, color = if (r.hiatus) k.dropped else if (r.finished) k.dim else k.text, fontSize = 12.5.sp, maxLines = 1)
                    if (r.next.isNotEmpty()) Text(r.next, color = when (r.next) { "late" -> k.onHold; "due today" -> k.accentHi; else -> k.dim }, fontSize = 12.5.sp, maxLines = 1)
                }
            }
        }
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(6.dp)) {
            when (val b = r.behind) {
                null -> Text("–", color = k.faint, fontSize = 12.5.sp)
                0u -> Text("Caught up", color = k.dim, fontSize = 12.5.sp)
                else -> Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text("$b", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                    Text("to read", color = k.dim, fontSize = 12.sp)
                }
            }
            val shape2 = RoundedCornerShape(8.dp)
            Box(
                Modifier.size(44.dp, 30.dp).clip(shape2).background(k.raised, shape2).border(1.dp, k.borderStrong, shape2).clickable(onClick = onPlusOne)
                    .semantics { contentDescription = "Mark chapter ${r.progress + 1u} of ${r.title} read" },
                contentAlignment = Alignment.Center,
            ) { Text("+1", color = k.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold) }
        }
    }
    Box(Modifier.fillMaxWidth().padding(start = 72.dp).height(1.dp).background(k.border.copy(alpha = .6f)))
}

/** A row's panel: the matched series, recent releases, the series' page, a pasted link (and Reset). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PublishingPanel(row: PublishingRow, pages: PagesSource, onUrl: (String) -> Unit, onClose: () -> Unit, onChanged: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onClose, containerColor = Kotora.colors.bg) {
        PublishingPanelBody(row, pages, onUrl, onChanged)
    }
}

@Composable
fun PublishingPanelBody(row: PublishingRow, pages: PagesSource, onUrl: (String) -> Unit, onChanged: () -> Unit) {
    val k = Kotora.colors
    val scope = rememberCoroutineScope()
    var link by remember { mutableStateOf("") }
    var note by remember { mutableStateOf<Pair<Boolean, String>?>(null) }
    var working by remember { mutableStateOf(false) }
    fun act(f: suspend () -> Unit) {
        working = true
        scope.launch {
            note = try {
                f()
                onChanged()
                false to "Saved. The releases come from that series now."
            } catch (e: Exception) {
                true to errorText(e)
            }
            working = false
        }
    }
    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).padding(bottom = 24.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(row.title, color = k.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        val t = row.seriesTitle
        if (t != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("$t on ${row.source ?: "MangaUpdates"}", color = k.dim, fontSize = 13.5.sp, modifier = Modifier.weight(1f, fill = false))
                if (row.look == PublishingLook.GUESS) Tag("Check match", TagKind.WAIT)
                if (row.look == PublishingLook.PICKED) Tag("Your link")
            }
            row.seriesUrl?.let { u -> KButton("Open the series page", { onUrl(u) }, kind = ButtonKind.GHOST, height = 32.dp, fontSize = 13.5.sp) }
        } else {
            Text(if (row.look == PublishingLook.NOT_FOUND) "No series found on MangaUpdates or MangaDex." else "Not looked up yet.", color = k.dim, fontSize = 13.5.sp)
        }
        row.error?.let { Text(it, color = k.dropped, fontSize = 12.5.sp) }
        if (row.recent.isNotEmpty()) {
            Text("RECENT RELEASES", color = k.faint, fontSize = 12.sp, fontWeight = FontWeight.Medium, letterSpacing = .5.sp, modifier = Modifier.padding(top = 6.dp))
            for (r in row.recent) Row(Modifier.fillMaxWidth()) {
                Text(r.label, color = k.text, fontSize = 13.5.sp, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                Text(r.`when`, color = k.dim, fontSize = 12.5.sp)
            }
        }
        Text("WRONG SERIES?", color = k.faint, fontSize = 12.sp, fontWeight = FontWeight.Medium, letterSpacing = .5.sp, modifier = Modifier.padding(top = 6.dp))
        Text("Paste its mangaupdates.com/series/… or mangadex.org/title/… address.", color = k.faint, fontSize = 12.5.sp)
        OutlinedTextField(
            link, { link = it }, Modifier.fillMaxWidth(), singleLine = true,
            placeholder = { Text("Series link", color = k.faint) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            colors = OutlinedTextFieldDefaults.colors(focusedBorderColor = k.accent, unfocusedBorderColor = k.borderStrong, focusedTextColor = k.text, unfocusedTextColor = k.text, cursorColor = k.accent),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            KButton("Use", { act { pages.publishingLink(row.id, link.trim()) } }, kind = ButtonKind.PRIMARY, height = 36.dp, enabled = link.isNotBlank() && !working)
            if (row.look == PublishingLook.PICKED) KButton("Reset", { act { pages.publishingReset(row.id) } }, height = 36.dp, enabled = !working)
            if (working) CircularProgressIndicator(Modifier.size(18.dp), color = k.accent, strokeWidth = 2.dp)
        }
        note?.let { (failed, text) -> Text(text, color = if (failed) k.dropped else k.dim, fontSize = 12.5.sp) }
    }
}
