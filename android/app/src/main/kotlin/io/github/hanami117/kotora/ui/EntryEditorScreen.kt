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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.EntryEditor
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.ScoreScale
import io.github.hanami117.kotora.core.editorSetStatus
import io.github.hanami117.kotora.core.editorStep
import io.github.hanami117.kotora.core.editorStepVolumes
import io.github.hanami117.kotora.ui.theme.Kotora

/**
 * The editor of one entry, as the desktop's (and iOS's sheet): status, progress (and a manga's
 * volumes), the score in the account's format, the rewatch (reread) count, then Cancel / Save
 * (one change, to the sync queue first). The − / + and status rules are the core's
 * (`editorStep`, `editorSetStatus`).
 */
@Composable
fun EntryEditorScreen(
    ed: EntryEditor?,
    scale: ScoreScale?,
    problem: String?,
    service: String,
    saving: Boolean,
    scoreLabel: (UInt?) -> String,
    scoreStep: (UInt?, Int) -> UInt?,
    onChange: (EntryEditor) -> Unit,
    onCancel: () -> Unit,
    onSave: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg)) {
        Row(
            Modifier.statusBarsPadding().fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            KButton("Cancel", onCancel, kind = ButtonKind.GHOST, height = 40.dp)
            Text(
                ed?.title ?: "",
                color = k.text,
                fontSize = 16.sp,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center,
                modifier = Modifier.weight(1f),
            )
            KButton("Save", onSave, kind = ButtonKind.PRIMARY, height = 36.dp, enabled = ed != null && !saving)
        }
        HorizontalDivider(color = k.border)
        when {
            ed != null -> Form(ed, scale, service, scoreLabel, scoreStep, onChange)
            problem != null -> Text(problem, color = k.dropped, modifier = Modifier.padding(24.dp))
            else -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = k.accent) }
        }
    }
}

@Composable
private fun Form(
    e: EntryEditor,
    scale: ScoreScale?,
    service: String,
    scoreLabel: (UInt?) -> String,
    scoreStep: (UInt?, Int) -> UInt?,
    onChange: (EntryEditor) -> Unit,
) {
    val k = Kotora.colors
    val manga = e.kind == MediaKind.MANGA
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        SectionLabel("Status")
        Card {
            Column {
                ListStatus.entries.forEachIndexed { i, st ->
                    if (i > 0) HorizontalDivider(color = k.border, modifier = Modifier.padding(start = 34.dp))
                    Row(
                        Modifier.fillMaxWidth().height(46.dp).clickable(role = Role.RadioButton) { onChange(editorSetStatus(e, st)) }.padding(horizontal = 16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        StatusDot(st)
                        Text(statusLabel(st, e.kind), color = k.text, fontSize = 15.sp, modifier = Modifier.weight(1f))
                        if (e.status == st) Icon(KIcons.Check, "Selected", tint = k.accent, modifier = Modifier.size(18.dp))
                    }
                }
            }
        }

        SectionLabel(if (manga) "Chapters" else "Progress")
        Card {
            Column {
                Stepper(if (manga) "Chapters" else "Episodes", e.progress, e.total, "progress", { onChange(editorStep(e, -1)) }, { onChange(editorStep(e, 1)) })
                if (manga) {
                    HorizontalDivider(color = k.border, modifier = Modifier.padding(start = 16.dp))
                    Stepper("Volumes", e.volumes, e.totalVolumes, "volumes", { onChange(editorStepVolumes(e, -1)) }, { onChange(editorStepVolumes(e, 1)) })
                }
            }
        }

        Row(verticalAlignment = Alignment.CenterVertically) {
            SectionLabel("Score", Modifier.weight(1f))
            Text(scoreLabel(e.score), color = k.dim, fontSize = 13.sp, modifier = Modifier.padding(end = 20.dp, top = 8.dp))
        }
        Card { ScoreCells(e, scale, scoreLabel, scoreStep, onChange) }

        // As the desktop's details: the count shows for a completed entry, or one counted before.
        if (e.status == ListStatus.COMPLETED || e.timesRewatched > 0u || e.rewatching) {
            SectionLabel(if (manga) "Reread" else "Rewatched")
            Card {
                Stepper(
                    if (manga) "Times reread" else "Times rewatched", e.timesRewatched, null, "times",
                    { if (e.timesRewatched > 0u) onChange(e.copy(timesRewatched = e.timesRewatched - 1u)) },
                    { onChange(e.copy(timesRewatched = e.timesRewatched + 1u)) },
                )
            }
            if (e.rewatching) {
                Text(
                    if (manga) "Rereading now: the last chapter ends the reread and counts it." else "Rewatching now: the last episode ends the rewatch and counts it.",
                    color = k.faint,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 6.dp),
                )
            }
        }
        Text(
            "It goes to the sync queue first, then to $service.",
            color = k.faint,
            fontSize = 12.sp,
            modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 14.dp),
        )
    }
}

@Composable
private fun Stepper(label: String, value: UInt, total: UInt?, id: String, minus: () -> Unit, plus: () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().height(58.dp).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(label, color = k.text, fontSize = 15.sp, modifier = Modifier.weight(1f))
        RoundButton(KIcons.Minus, "One less", value > 0u, minus)
        Text(
            total?.let { "$value / $it" } ?: "$value",
            color = k.text,
            fontSize = 16.sp,
            fontWeight = FontWeight.Medium,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(min = 72.dp).semantics { contentDescription = "$label $value" + (total?.let { " of $it" } ?: "") + " ($id)" },
        )
        RoundButton(KIcons.Plus, "One more", total?.let { value < it } ?: true, plus)
    }
}

@Composable
fun RoundButton(icon: ImageVector, label: String, enabled: Boolean, onClick: () -> Unit) {
    val k = Kotora.colors
    Box(
        Modifier.size(40.dp).clip(CircleShape).background(k.raised, CircleShape).border(1.dp, k.borderStrong, CircleShape)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick).alpha(if (enabled) 1f else .4f)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, tint = k.text, modifier = Modifier.size(18.dp))
    }
}

@Composable
private fun ScoreCells(e: EntryEditor, scale: ScoreScale?, scoreLabel: (UInt?) -> String, scoreStep: (UInt?, Int) -> UInt?, onChange: (EntryEditor) -> Unit) {
    val k = Kotora.colors
    val choices = scale?.choices ?: emptyList()
    val shown = scoreLabel(e.score)
    val columns = if (choices.size <= 5) choices.size + 1 else 6
    val cells = listOf<Pair<String, UInt?>>("–" to null) + choices.map { it.label to it.points }
    Column {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            for (line in cells.chunked(columns)) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for ((label, points) in line) {
                        val on = if (points == null) e.score == null else e.score != null && shown == label
                        ScoreCell(label, on, Modifier.weight(1f)) { onChange(e.copy(score = points)) }
                    }
                    repeat(columns - line.size) { Box(Modifier.weight(1f)) }
                }
            }
        }
        if (scale?.fine == true) {
            HorizontalDivider(color = k.border, modifier = Modifier.padding(start = 16.dp))
            Row(
                Modifier.fillMaxWidth().height(58.dp).padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text("Fine", color = k.text, fontSize = 15.sp, modifier = Modifier.weight(1f))
                RoundButton(KIcons.Minus, "Lower score", true) { onChange(e.copy(score = scoreStep(e.score, -1))) }
                Text(shown, color = k.text, fontSize = 16.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center, modifier = Modifier.widthIn(min = 72.dp))
                RoundButton(KIcons.Plus, "Higher score", true) { onChange(e.copy(score = scoreStep(e.score, 1))) }
            }
        }
    }
}

@Composable
private fun ScoreCell(label: String, on: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(8.dp)
    Box(
        modifier.height(40.dp).clip(shape).background(if (on) k.accent else k.field, shape)
            .border(1.dp, if (on) k.accent else k.borderStrong, shape)
            .clickable(role = Role.RadioButton, onClick = onClick)
            .semantics { contentDescription = if (label == "–") "No score" else "Score $label" },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (on) k.onAccent else k.text, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}
