package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.ui.theme.Kotora

/** M2's settings: the account (sign out), Manga lists, About. The rest comes in M5. */
@Composable
fun SettingsScreen(
    accounts: List<Account>,
    mangaEnabled: Boolean,
    waiting: UInt,
    version: String,
    sample: Boolean,
    onMangaEnabled: (Boolean) -> Unit,
    onSignOut: (Service) -> Unit,
    onLeaveSample: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val k = Kotora.colors
    Column(modifier.fillMaxSize().background(k.bg).verticalScroll(rememberScrollState())) {
        TopBar("Settings", accounts.firstOrNull { it.active }, false, null)
        SectionLabel("Account")
        Card {
            Column {
                accounts.forEachIndexed { i, a ->
                    if (i > 0) HorizontalDivider(color = k.border)
                    Item(serviceTitle(a.service), if (a.active) "${a.userName} · shown" else a.userName) {
                        if (sample) KButton("Leave", onLeaveSample, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
                        else KButton("Sign out", { onSignOut(a.service) }, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
                    }
                }
                HorizontalDivider(color = k.border)
                Item("Waiting changes", if (waiting == 0u) "Everything is sent." else "$waiting waiting to sync") {}
            }
        }
        Text(
            "Sign out removes this phone's copy of the list, its waiting changes and the sign-in.",
            color = k.faint,
            fontSize = 12.sp,
            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 6.dp, bottom = 4.dp),
        )
        SectionLabel("Lists")
        Card {
            Item("Manga lists", "Also sync your manga list") {
                Switch(
                    checked = mangaEnabled,
                    onCheckedChange = onMangaEnabled,
                    colors = SwitchDefaults.colors(checkedTrackColor = k.accent, checkedThumbColor = androidx.compose.ui.graphics.Color.White, uncheckedTrackColor = k.borderStrong, uncheckedBorderColor = k.borderStrong, uncheckedThumbColor = k.text),
                )
            }
        }
        SectionLabel("About")
        Card {
            Column {
                Item("Kotora $version", "A tribute to Taiga. GPL-3.0-or-later.") {}
                HorizontalDivider(color = k.border)
                Item("Data", "From MyAnimeList and AniList, for your own account. No Kotora server.") {}
                HorizontalDivider(color = k.border)
                Item("Font", "Inter, SIL Open Font License 1.1") {}
            }
        }
    }
}

@Composable
private fun Item(title: String, sub: String?, trailing: @Composable () -> Unit) {
    val k = Kotora.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 12.dp),
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
