package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.material3.Icon
import androidx.compose.ui.semantics.Role
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
import io.github.hanami117.kotora.core.NotifySettings
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.ui.theme.Kotora

/** Settings: the account (sign out), Manga lists, Notifications, the startup animation, About. */
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
    notify: NotifySettings = NotifySettings(startedAiring = true, newEpisode = true, newChapter = false),
    onNotify: (NotifySettings) -> Unit = {},
    intro: Boolean = true,
    onIntro: (Boolean) -> Unit = {},
    systemNotificationsOff: Boolean = false,
    onOpenSystemSettings: () -> Unit = {},
    onProfile: (() -> Unit)? = null,
    onOpenUrl: (String) -> Unit = {},
    conflicts: UInt = 0u,
    onQueue: (() -> Unit)? = null,
    /** Theme and Titles rows (in Appearance). */
    appearanceExtras: @Composable () -> Unit = {},
    /** Statistics, Startup, Release data, Manga (after Notifications). */
    extras: @Composable () -> Unit = {},
    /** Mirror edits to the other service, Beta (`MirrorScreen.kt`), in the Account card. */
    mirror: @Composable () -> Unit = {},
) {
    val k = Kotora.colors
    val menu = LocalAccounts.current
    Column(modifier.fillMaxSize().background(k.bg).verticalScroll(rememberScrollState())) {
        TopBar("Settings", accounts.firstOrNull { it.active }, false, null)
        SectionLabel("Account")
        Card {
            Column {
                accounts.forEachIndexed { i, a ->
                    if (i > 0) HorizontalDivider(color = k.border)
                    Item(serviceTitle(a.service), if (a.active) "${a.userName} · shown" else a.userName) {
                        if (sample) KButton("Leave", onLeaveSample, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
                        else Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (!a.active && menu != null) KButton("Show", { menu.onShow(a.service) }, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
                            KButton("Sign out", { onSignOut(a.service) }, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
                        }
                    }
                }
                for (other in menu?.others.orEmpty()) {
                    HorizontalDivider(color = k.border)
                    Item(serviceTitle(other), "Not signed in") {
                        KButton("Sign in", { menu?.onSignIn(other) }, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp, enabled = menu?.canSignIn(other) == true)
                    }
                }
                if (onProfile != null && accounts.isNotEmpty()) {
                    HorizontalDivider(color = k.border)
                    Row(Modifier.clickable(role = Role.Button, onClick = onProfile)) {
                        Item("Profile", "Your numbers on ${serviceTitle((accounts.firstOrNull { it.active } ?: accounts.first()).service)}") {
                            Icon(KIcons.Chevron, null, tint = k.faint)
                        }
                    }
                }
                HorizontalDivider(color = k.border)
                if (onQueue != null) QueueLink(waiting, conflicts, onQueue)
                else Item("Waiting changes", if (waiting == 0u) "Everything is sent." else "$waiting waiting to sync") {}
                if (!sample) {
                    HorizontalDivider(color = k.border)
                    mirror()
                }
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
                KSwitch(mangaEnabled, onCheckedChange = onMangaEnabled)
            }
        }
        SectionLabel("Notifications")
        Card {
            Column {
                if (systemNotificationsOff) {
                    Item("Notifications are off", "Android doesn't let Kotora show them. Turn them on in Android's settings.") {
                        KButton("Open", onOpenSystemSettings, kind = ButtonKind.SECONDARY, height = 34.dp, fontSize = 13.sp)
                    }
                    HorizontalDivider(color = k.border)
                }
                Item("Started airing", "When a show on Plan to watch starts airing: \u201cTitle started airing\u201d.") {
                    KSwitch(notify.startedAiring) { onNotify(notify.copy(startedAiring = it)) }
                }
                HorizontalDivider(color = k.border)
                Item("New episode", "When the next episode of a show you\u2019re watching is out, by the estimated broadcast time: \u201cTitle \u00b7 Ep 7 is out\u201d.") {
                    KSwitch(notify.newEpisode) { onNotify(notify.copy(newEpisode = it)) }
                }
                HorizontalDivider(color = k.border)
                val chapterNote = if (mangaEnabled) {
                    "When a manga you\u2019re reading gets a new chapter: \u201cNew chapter of Title: Ch 75\u201d. Kotora checks MangaUpdates every few hours while this is on. Off by default."
                } else {
                    "For manga you\u2019re reading, from MangaUpdates. Turn Manga lists on first."
                }
                Item("New chapter", chapterNote) {
                    KSwitch(notify.newChapter && mangaEnabled, enabled = mangaEnabled) { onNotify(notify.copy(newChapter = it)) }
                }
            }
        }
        extras()
        SectionLabel("Appearance")
        Card {
          Column {
            appearanceExtras()
            Item("Startup animation", "Claw marks when Kotora opens. Coming back to it doesn\u2019t play it; with Remove animations it is a short fade.") {
                KSwitch(intro, onCheckedChange = onIntro)
            }
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
                HorizontalDivider(color = k.border)
                Item("小虎", "Noto Sans JP outlines, SIL Open Font License 1.1") {}
                AboutLinks(onOpenUrl)
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

@Composable
private fun KSwitch(checked: Boolean, enabled: Boolean = true, onCheckedChange: (Boolean) -> Unit) {
    val k = Kotora.colors
    Switch(
        checked = checked,
        onCheckedChange = onCheckedChange,
        enabled = enabled,
        colors = SwitchDefaults.colors(checkedTrackColor = k.accent, checkedThumbColor = androidx.compose.ui.graphics.Color.White, uncheckedTrackColor = k.borderStrong, uncheckedBorderColor = k.borderStrong, uncheckedThumbColor = k.text),
    )
}
