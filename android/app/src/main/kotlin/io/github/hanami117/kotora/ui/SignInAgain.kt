package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.launch

/**
 * "Sign in again" (desktop ui.rs `alert/expired`, and the queue's `queue/expired` with [waiting]):
 * the service no longer accepts Kotora's sign-in. The saved list and the waiting changes stay.
 */
@Composable
fun SignInAgainBanner(service: Service, onSignIn: (Service) -> Unit, modifier: Modifier = Modifier, waiting: UInt? = null) {
    val k = Kotora.colors
    val svc = serviceTitle(service)
    val text = if (waiting == null) "$svc needs you to sign in again. Your saved list still works."
    else "Sign in again. Your $svc sign-in expired. ${if (waiting == 1u) "1 change" else "$waiting changes"} wait${if (waiting == 1u) "s" else ""} in the queue; nothing is lost."
    val shape = RoundedCornerShape(10.dp)
    Row(
        modifier.padding(horizontal = 16.dp, vertical = 6.dp).fillMaxWidth()
            .background(k.onHold.copy(alpha = .10f), shape).border(1.dp, k.onHold.copy(alpha = .4f), shape)
            .padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(text, color = k.text, fontSize = 14.sp, modifier = Modifier.weight(1f))
        KButton("Sign in", { onSignIn(service) }, kind = ButtonKind.PRIMARY, height = 34.dp, fontSize = 13.sp)
    }
}

/**
 * AniList's fallback (desktop setup.rs `setup/token`): the address the browser ended on, or the
 * token AniList showed, pasted by hand. "Use token" checks it with AniList; [onUse] returns the
 * problem's words, or null when it worked.
 */
@Composable
fun TokenPasteField(onUse: suspend (String) -> String?, modifier: Modifier = Modifier, lead: String = "Or paste the address your browser ended on, or the token AniList showed:") {
    val k = Kotora.colors
    var text by rememberSaveable { mutableStateOf("") }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    var busy by rememberSaveable { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val use = {
        if (!busy && text.isNotBlank()) {
            busy = true
            val raw = text
            scope.launch {
                error = onUse(raw)
                busy = false
                if (error == null) text = ""
            }
        }
    }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(lead, color = k.dim, fontSize = 13.sp)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it; error = null },
                modifier = Modifier.weight(1f),
                singleLine = true,
                textStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 12.sp, color = k.text),
                placeholder = { Text("Paste the token from AniList’s page", color = k.faint, fontSize = 12.sp, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis) },
                isError = error != null,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { use() }),
                shape = RoundedCornerShape(8.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedContainerColor = k.field,
                    unfocusedContainerColor = k.field,
                    focusedBorderColor = k.accent,
                    unfocusedBorderColor = k.borderStrong,
                    errorBorderColor = k.dropped.copy(alpha = .7f),
                    cursorColor = k.accent,
                ),
            )
            if (busy) CircularProgressIndicator(Modifier.size(20.dp).padding(horizontal = 4.dp), color = k.dim, strokeWidth = 2.dp)
            else KButton("Use token", { use() }, kind = ButtonKind.SECONDARY, height = 48.dp, enabled = text.isNotBlank(), fontSize = 13.sp)
        }
        error?.let { Text(it, color = k.dropped, fontSize = 12.sp) }
    }
}
