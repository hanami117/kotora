package io.github.hanami117.kotora.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.update.AndroidRelease
import io.github.hanami117.kotora.update.UpdateStep
import io.github.hanami117.kotora.update.Updater
import io.github.hanami117.kotora.ui.theme.Kotora
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale

/** What Settings → About → Updates shows; plain values, so screenshots need no [Updater]. */
data class UpdateUi(
    val enabled: Boolean,
    val lastCheck: Long?,
    val result: String?,
    val available: AndroidRelease?,
    val step: UpdateStep,
    val onEnabled: (Boolean) -> Unit = {},
    val onCheckNow: () -> Unit = {},
    val onDownload: (AndroidRelease) -> Unit = {},
    val onSkip: (AndroidRelease) -> Unit = {},
    val onNotes: (AndroidRelease) -> Unit = {},
    val onAllowInstall: () -> Unit = {},
)

/** The desktop's wording (`app/src/update.rs`): "Kotora 0.5.1 is available", "Last checked: today at 14:05". */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun UpdateRows(u: UpdateUi, now: Long) {
    val k = Kotora.colors
    HorizontalDivider(color = k.border)
    SettingsItem("Check for updates", Updater.CHECK_NOTE) {
        KSwitch(u.enabled, onCheckedChange = u.onEnabled)
    }
    HorizontalDivider(color = k.border)
    val note = (u.result?.let { "$it " } ?: "") + lastChecked(u.lastCheck, now) + "."
    SettingsItem("Check now", note) {
        val checking = u.step == UpdateStep.Checking
        KButton(if (checking) "Checking…" else "Check now", u.onCheckNow, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp, enabled = !checking)
    }
    val r = u.available ?: return
    HorizontalDivider(color = k.border)
    when (val step = u.step) {
        is UpdateStep.Downloading -> SettingsItem("Kotora ${r.version} is available", "Downloading… ${step.percent}%") {}
        UpdateStep.NeedsPermission -> SettingsItem("Allow Kotora to install apps", "Android asks once. Turn on “Allow from this source”, come back and tap Download again.") {
            KButton("Open", u.onAllowInstall, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
        }
        else -> {
            SettingsItem("Kotora ${r.version} is available", "Nothing is downloaded until you tap Download. Android asks before it installs.") {}
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp)) {
                KButton("Download", { u.onDownload(r) }, height = 34.dp, fontSize = 13.sp)
                KButton("What’s new", { u.onNotes(r) }, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
                KButton("Skip this version", { u.onSkip(r) }, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
            }
        }
    }
}

/** "Last checked: today at 14:05", "yesterday at 09:30", "3 Nov at 18:00" (core `update::last_checked`). */
fun lastChecked(last: Long?, now: Long): String {
    if (last == null) return "Not checked yet"
    val at = Calendar.getInstance().apply { timeInMillis = last * 1000 }
    val today = Calendar.getInstance().apply { timeInMillis = now * 1000 }
    val time = SimpleDateFormat("HH:mm", Locale.ROOT).format(Date(last * 1000))
    val dayDiff = run {
        val a = at.clone() as Calendar
        val b = today.clone() as Calendar
        for (c in listOf(a, b)) { c.set(Calendar.HOUR_OF_DAY, 12); c.set(Calendar.MINUTE, 0); c.set(Calendar.SECOND, 0); c.set(Calendar.MILLISECOND, 0) }
        ((b.timeInMillis - a.timeInMillis) / 86_400_000L).toInt()
    }
    val day = when {
        dayDiff == 0 -> "today"
        dayDiff == 1 -> "yesterday"
        at.get(Calendar.YEAR) == today.get(Calendar.YEAR) -> SimpleDateFormat("d MMM", Locale.ENGLISH).format(Date(last * 1000))
        else -> SimpleDateFormat("d MMM yyyy", Locale.ENGLISH).format(Date(last * 1000))
    }
    return "Last checked: $day at $time"
}
