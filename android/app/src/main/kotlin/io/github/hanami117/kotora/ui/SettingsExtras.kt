package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.FillPreview
import io.github.hanami117.kotora.core.FillProgress
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.KotoraSettings
import io.github.hanami117.kotora.core.ReleaseSources
import io.github.hanami117.kotora.core.Setting
import io.github.hanami117.kotora.core.VolumeFill
import io.github.hanami117.kotora.data.PagesSource
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/*
 * The desktop's settings rows Android didn't have (`app/src/settings.rs`, its words), saved in
 * `config.toml` under the desktop's keys: Statistics, Theme, Titles, Startup (Open on, Mode),
 * Release data (Manga on), Fill volumes from chapters… (Manga on, not with sample data).
 */

/** The Theme and Titles rows, inside Settings' Appearance card. */
@Composable
fun AppearanceExtras(settings: KotoraSettings?, titleLanguage: String, onSetting: (Setting) -> Unit, onTitles: (String) -> Unit) {
    val k = Kotora.colors
    Column {
        ExtraItem("Theme", "System follows your phone's light or dark setting.") {
            Picker(settings?.theme ?: "system", listOf("system" to "System", "light" to "Light", "dark" to "Dark")) { onSetting(Setting.Theme(it)) }
        }
        HorizontalDivider(color = k.border)
        ExtraItem("Titles", "Romaji, English or Japanese titles everywhere.") {
            Picker(titleLanguage, listOf("romaji" to "Romaji", "english" to "English", "japanese" to "Japanese"), onTitles)
        }
        HorizontalDivider(color = k.border)
    }
}

/** The Statistics link, Startup, Release data and Manga sections (after Notifications). */
@Composable
fun SettingsExtras(
    pages: PagesSource,
    settings: KotoraSettings?,
    mangaEnabled: Boolean,
    sample: Boolean,
    onSetting: (Setting) -> Unit,
    onStats: () -> Unit,
    onFill: () -> Unit,
) {
    val k = Kotora.colors
    val scope = rememberCoroutineScope()
    var sources by remember { mutableStateOf<ReleaseSources?>(null) }
    var sourceNote by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(pages) { sources = runCatching { pages.releaseSources() }.getOrNull() }

    SectionLabel("Kotora")
    Card {
        ExtraItem("Statistics", "What you watched and read, by Kotora's count.", Modifier.clickable(role = Role.Button, onClick = onStats)) {
            Icon(KIcons.Chevron, null, tint = k.faint, modifier = Modifier.size(18.dp))
        }
    }
    SectionLabel("Startup")
    Card {
        Column {
            ExtraItem("Open on", "Home, or the page Kotora was on when it was last closed or hidden.") {
                Picker(settings?.openOn ?: "home", listOf("home" to "Home", "last" to "Last page")) { onSetting(Setting.OpenOn(it)) }
            }
            HorizontalDivider(color = k.border)
            ExtraItem("Mode", "Last used, Anime or Manga. Manga only when Manga is on.") {
                Picker(settings?.startMode ?: "last", listOf("last" to "Last used", "anime" to "Anime", "manga" to "Manga")) { onSetting(Setting.StartMode(it)) }
            }
        }
    }
    if (mangaEnabled) {
        SectionLabel("Release data")
        Card {
            Column {
                val s = sources ?: ReleaseSources(mangaupdates = true, mangadex = true)
                fun set(n: ReleaseSources) {
                    if (!n.mangaupdates && !n.mangadex) { sourceNote = "At least one release source stays on."; return }
                    sources = n
                    sourceNote = null
                    scope.launch { runCatching { pages.setReleaseSources(n) }.onFailure { sourceNote = it.message } }
                }
                ExtraItem("MangaUpdates", "Release data for Publishing from MangaUpdates.") { ExtraSwitch(s.mangaupdates) { set(s.copy(mangaupdates = it)) } }
                HorizontalDivider(color = k.border)
                ExtraItem("MangaDex", "Release data for Publishing from MangaDex, official chapters included.") { ExtraSwitch(s.mangadex) { set(s.copy(mangadex = it)) } }
            }
        }
        sourceNote?.let { Text(it, color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 6.dp)) }
        if (!sample) {
            SectionLabel("Manga")
            Card {
                ExtraItem("Fill volumes from chapters…", "Volumes read from chapters read, with MangaDex’s volume tables; never lowered.", Modifier.clickable(role = Role.Button, onClick = onFill)) {
                    Icon(KIcons.Chevron, null, tint = k.faint, modifier = Modifier.size(18.dp))
                }
            }
        }
    }
}

@Composable
private fun ExtraItem(title: String, sub: String?, modifier: Modifier = Modifier, trailing: @Composable () -> Unit) {
    val k = Kotora.colors
    Row(
        modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, color = k.text, fontWeight = FontWeight.Medium, fontSize = 14.sp)
            if (sub != null) Text(sub, color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
        }
        trailing()
    }
}

@Composable
private fun ExtraSwitch(checked: Boolean, onChange: (Boolean) -> Unit) {
    val k = Kotora.colors
    Switch(checked, onChange, colors = SwitchDefaults.colors(checkedTrackColor = k.accent, checkedThumbColor = Color.White, uncheckedTrackColor = k.borderStrong, uncheckedBorderColor = k.borderStrong, uncheckedThumbColor = k.text))
}

@Composable
private fun Picker(value: String, items: List<Pair<String, String>>, onPick: (String) -> Unit) {
    val k = Kotora.colors
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier.clip(RoundedCornerShape(8.dp)).clickable(role = Role.DropdownList) { open = true }.padding(horizontal = 10.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(items.firstOrNull { it.first == value }?.second ?: value, color = k.accentHi, fontSize = 14.sp, fontWeight = FontWeight.Medium)
            Icon(KIcons.Down, null, tint = k.accentHi, modifier = Modifier.size(14.dp))
        }
        DropdownMenu(open, { open = false }, containerColor = k.raised) {
            for ((key, label) in items) DropdownMenuItem(
                text = { Text(label, color = if (key == value) k.accentHi else k.text, fontSize = 14.sp) },
                trailingIcon = if (key == value) ({ Icon(KIcons.Check, null, tint = k.accentHi, modifier = Modifier.size(16.dp)) }) else null,
                onClick = { open = false; if (key != value) onPick(key) },
            )
        }
    }
}

// --------------------------------------------------------------------------- fill volumes

private sealed interface FillPhase {
    data class Idle(val candidates: UInt) : FillPhase
    data class Busy(val progress: FillProgress) : FillPhase
    data class Preview(val preview: FillPreview) : FillPhase
    data class Done(val queued: UInt, val undone: UInt?) : FillPhase
}

private fun plural(n: UInt, one: String, many: String) = if (n == 1u) "1 $one" else "$n $many"

/**
 * "Fill volumes from chapters…" (the desktop's `volfill.rs`): looks every manga with chapters read
 * and volumes short of their total up on MangaDex, shows what would change, then queues it as one
 * change set; Undo takes back what hasn't been sent.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FillVolumesSheet(pages: PagesSource, serviceTitle: String, onChanged: () -> Unit, onClose: () -> Unit) {
    val k = Kotora.colors
    val scope = rememberCoroutineScope()
    var job by remember { mutableStateOf<VolumeFill?>(null) }
    var phase by remember { mutableStateOf<FillPhase>(FillPhase.Idle(0u)) }
    var error by remember { mutableStateOf<String?>(null) }
    var working by remember { mutableStateOf(false) }
    fun msg(e: Throwable) = (e as? KotoraException)?.message?.takeIf { it.isNotBlank() } ?: e.message ?: "Something went wrong."

    LaunchedEffect(Unit) {
        try {
            val j = pages.volumeFill()
            job = j
            phase = FillPhase.Idle(j?.let { withContext(Dispatchers.IO) { it.candidates() } } ?: 0u)
        } catch (e: Exception) {
            error = msg(e)
        }
    }
    DisposableEffect(Unit) { onDispose { job?.cancel() } }

    fun lookUp(refresh: Boolean) {
        val j = job ?: return
        error = null
        phase = FillPhase.Busy(FillProgress(0u, 0u))
        scope.launch {
            val poll = launch { while (isActive) { phase = FillPhase.Busy(j.progress()); delay(150) } }
            val r = runCatching { withContext(Dispatchers.IO) { j.lookUp(refresh) } }
            poll.cancel()
            r.onSuccess { pv -> phase = if (pv.stopped) FillPhase.Idle(withContext(Dispatchers.IO) { j.candidates() }) else FillPhase.Preview(pv) }
            r.onFailure { e -> error = msg(e); phase = FillPhase.Idle(runCatching { withContext(Dispatchers.IO) { j.candidates() } }.getOrDefault(0u)) }
        }
    }

    fun apply() {
        val j = job ?: return
        working = true
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { j.apply() } }
                .onSuccess { phase = FillPhase.Done(it, null); onChanged() }
                .onFailure { error = msg(it) }
            working = false
        }
    }

    fun undo() {
        val j = job ?: return
        working = true
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { j.undo() } }
                .onSuccess { phase = FillPhase.Done(0u, it); onChanged() }
                .onFailure { error = msg(it) }
            working = false
        }
    }

    ModalBottomSheet(onDismissRequest = { job?.cancel(); onClose() }, containerColor = k.bg) {
        FillBody(phase, error, working, serviceTitle, ::lookUp, { job?.cancel() }, ::apply, ::undo)
    }
}

@Composable
private fun FillBody(
    phase: FillPhase,
    error: String?,
    working: Boolean,
    serviceTitle: String,
    onLookUp: (Boolean) -> Unit,
    onStop: () -> Unit,
    onApply: () -> Unit,
    onUndo: () -> Unit,
) {
    val k = Kotora.colors
    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 24.dp).navigationBarsPadding(), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Text("Fill volumes", color = k.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        when (phase) {
            is FillPhase.Idle -> {
                Text(
                    if (phase.candidates == 0u) "No manga to look up: none has chapters read with volumes short of its total."
                    else "Sets volumes read from chapters read, with MangaDex’s volume tables. You see every change first; volumes are never lowered.",
                    color = k.dim, fontSize = 15.sp,
                )
                if (phase.candidates > 0u) KButton("Look up ${phase.candidates} manga", { onLookUp(false) }, kind = ButtonKind.PRIMARY, height = 36.dp)
            }
            is FillPhase.Busy -> {
                val p = phase.progress
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    CircularProgressIndicator(Modifier.size(18.dp), color = k.accent, strokeWidth = 2.dp)
                    Text("Looking up ${p.of} manga on MangaDex… (${p.done} of ${p.of})", color = k.dim, fontSize = 15.sp)
                }
                LinearProgressIndicator({ if (p.of == 0u) 0f else p.done.toFloat() / p.of.toFloat() }, Modifier.fillMaxWidth(), color = k.accent, trackColor = k.border)
                KButton("Stop", onStop, height = 36.dp)
            }
            is FillPhase.Preview -> {
                val pv = phase.preview
                Counts(listOf(
                    Triple(pv.fill.size, "volumes go up", k.watching) to true,
                    Triple(pv.same.size, "already right", k.completed) to false,
                    Triple(pv.higher.size, "higher than MangaDex (left alone)", k.onHold) to false,
                    Triple(pv.notFound.size, "not found on MangaDex", k.faint) to false,
                ))
                Group("Volumes that go up", pv.fill.map { "${it.title}: Ch ${it.chapters} → Vol ${it.from} → ${it.to}" })
                Group("Not found on MangaDex", pv.notFound)
                Group("Higher than MangaDex (left alone)", pv.higher.map { "${it.title}: Vol ${it.from} (MangaDex: ${it.to})" })
                Group("Already right", pv.same.map { "${it.title}: Vol ${it.from}" })
                if (pv.fill.isEmpty()) {
                    Text("Nothing to change.", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    KButton("Look up again", { onLookUp(true) }, height = 36.dp)
                } else {
                    Text("Only volumes change, through the sync queue to $serviceTitle at its pace; Undo takes back what hasn’t been sent.", color = k.dim, fontSize = 13.sp)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        KButton("Fill ${plural(pv.fill.size.toUInt(), "volume count", "volume counts")}", onApply, kind = ButtonKind.PRIMARY, height = 36.dp, enabled = !working)
                        KButton("Look up again", { onLookUp(true) }, height = 36.dp)
                    }
                }
            }
            is FillPhase.Done -> {
                val u = phase.undone
                if (u != null) {
                    Text("Took back ${plural(u, "change", "changes")} that hadn’t been sent yet.", color = k.text, fontSize = 15.sp)
                } else {
                    Text("${plural(phase.queued, "volume change", "volume changes")} queued: they go to $serviceTitle at its pace.", color = k.text, fontSize = 15.sp)
                    KButton("Undo", onUndo, height = 36.dp, enabled = !working)
                }
            }
        }
        error?.let { Text(it, color = k.dropped, fontSize = 13.sp) }
        Text(StatsText.mangadex() + ". Saved answers are used for 30 days; “Look up again” asks MangaDex anew.", color = k.faint, fontSize = 12.sp)
    }
}

@Composable
private fun Counts(items: List<Pair<Triple<Int, String, Color>, Boolean>>) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for ((t, always) in items) {
            if (t.first == 0 && !always) continue
            Row(
                Modifier.background(t.third.copy(alpha = .14f), RoundedCornerShape(13.dp)).padding(horizontal = 10.dp, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text("${t.first}", color = t.third, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                Text(t.second, color = t.third, fontSize = 13.sp)
            }
        }
    }
}

@Composable
private fun Group(title: String, lines: List<String>) {
    if (lines.isEmpty()) return
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    Column(Modifier.fillMaxWidth().background(k.field, shape).border(1.dp, k.border, shape).padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title.uppercase(), color = k.faint, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = .4.sp)
        for (l in lines) Text(l, color = k.text, fontSize = 13.5.sp)
    }
}

/** For the screenshot tests: the sheet's body at a phase. */
@Composable
fun FillVolumesPreview(preview: FillPreview?, queued: UInt? = null) {
    val phase = when {
        queued != null -> FillPhase.Done(queued, null)
        preview != null -> FillPhase.Preview(preview)
        else -> FillPhase.Idle(12u)
    }
    Box(Modifier.background(Kotora.colors.bg)) { FillBody(phase, null, false, "MyAnimeList", {}, {}, {}, {}) }
}
