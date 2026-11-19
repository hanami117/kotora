package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.ui.theme.Kotora

/**
 * The first page: one button per service. The sign-in opens in the browser (a Custom Tab) and
 * comes back to `io.github.hanami117.kotora://oauth/…`.
 */
@Composable
fun SignInScreen(
    error: String?,
    busy: Boolean,
    hasClientId: (Service) -> Boolean,
    onSignIn: (Service) -> Unit,
    onLookAround: (() -> Unit)?,
    onToken: (suspend (String) -> String?)? = null,
) {
    val k = Kotora.colors
    Column(
        Modifier.fillMaxSize().background(k.bg).safeDrawingPadding().verticalScroll(rememberScrollState()).padding(horizontal = 28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.weight(1f).height(48.dp))
        KotoraLogo(132.dp)
        Text("Kotora", style = MaterialTheme.typography.headlineLarge, color = k.text, modifier = Modifier.padding(top = 18.dp, bottom = 6.dp))
        Text(
            "Your anime and manga list, kept up to date on MyAnimeList or AniList.",
            style = MaterialTheme.typography.bodyLarge,
            color = k.dim,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(28.dp))
        if (busy) {
            Row(Modifier.height(110.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                CircularProgressIndicator(Modifier.size(22.dp), color = k.accent, strokeWidth = 2.dp)
                Text("Signing in…", color = k.dim)
            }
        } else {
            val noIds = Service.entries.filter { !hasClientId(it) }
            KButton("Sign in with MyAnimeList", { onSignIn(Service.MAL) }, Modifier.fillMaxWidth(), ButtonKind.PRIMARY, 50.dp, hasClientId(Service.MAL), 15.sp)
            Spacer(Modifier.height(10.dp))
            KButton("Sign in with AniList", { onSignIn(Service.ANI_LIST) }, Modifier.fillMaxWidth(), ButtonKind.SECONDARY, 50.dp, hasClientId(Service.ANI_LIST), 15.sp)
            if (noIds.isNotEmpty()) {
                Text(
                    "This build has no Client ID for " + noIds.joinToString(" or ") { if (it == Service.MAL) "MyAnimeList" else "AniList" } + " yet.",
                    color = k.onHold,
                    fontSize = 13.sp,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(top = 12.dp),
                )
            }
        }
        if (error != null) {
            Text(error, color = k.dropped, fontSize = 13.sp, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 14.dp))
            // AniList's fallback after a problem (desktop setup.rs `setup/token`).
            if (onToken != null && !busy) TokenPasteField(onToken, Modifier.padding(top = 12.dp))
        }
        Row(Modifier.padding(top = 18.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(KIcons.Lock, null, tint = k.faint, modifier = Modifier.size(18.dp).padding(top = 1.dp))
            Text(
                "The sign-in opens in your browser. Kotora never sees your password and has no server: your list goes straight to the service.",
                color = k.faint,
                fontSize = 13.sp,
                lineHeight = 19.sp,
            )
        }
        if (onLookAround != null) {
            KButton("Look around with sample data", onLookAround, Modifier.padding(top = 12.dp), ButtonKind.GHOST)
        }
        Spacer(Modifier.weight(1f).height(24.dp))
        Box(Modifier.fillMaxWidth().padding(bottom = 18.dp), contentAlignment = Alignment.Center) {
            Text("No account with Kotora · no ads · no tracking", color = k.faint, fontSize = 12.sp)
        }
    }
}
