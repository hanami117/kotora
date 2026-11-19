package io.github.hanami117.kotora.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.ui.theme.Kotora

/** What the profile menu needs: the signed-in accounts and what its items do. */
class Accounts(
    val accounts: List<Account>,
    val sample: Boolean,
    val canSignIn: (Service) -> Boolean,
    val onShow: (Service) -> Unit,
    val onSignIn: (Service) -> Unit,
) {
    /** The services not signed in yet, to sign in from the menu and Settings. */
    val others: List<Service> get() = if (sample) emptyList() else Service.entries.filter { s -> accounts.none { it.service == s } }
}

/** Provided by the signed-in shell; without it (screenshots of one screen) the avatar is a picture. */
val LocalAccounts = compositionLocalOf<Accounts?> { null }

/**
 * The profile button in the app bars: the active account's avatar, opening a menu to switch to
 * the other signed-in account or sign in to the other service (both can be signed in, as on the desktop).
 */
@Composable
fun AccountMenu(active: Account) {
    val a = LocalAccounts.current ?: return Avatar(active)
    val k = Kotora.colors
    var open by remember { mutableStateOf(false) }
    Box {
        Box(
            Modifier.size(44.dp).clip(CircleShape).clickable(role = Role.Button) { open = true }
                .semantics { contentDescription = "Accounts" },
            contentAlignment = Alignment.Center,
        ) { Avatar(active) }
        DropdownMenu(open, { open = false }, containerColor = k.raised) {
            Text("Accounts", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
            for (acc in a.accounts) {
                DropdownMenuItem(
                    text = { Text("${acc.userName} · ${serviceTitle(acc.service)}", color = k.text) },
                    leadingIcon = { if (acc.active) Icon(KIcons.Check, null, tint = k.accent) else Box(Modifier.size(24.dp)) },
                    onClick = { open = false; a.onShow(acc.service) },
                )
            }
            if (a.others.isNotEmpty()) HorizontalDivider(color = k.border)
            for (s in a.others) {
                DropdownMenuItem(
                    text = { Text("Sign in with ${serviceTitle(s)}", color = k.text) },
                    leadingIcon = { Icon(KIcons.Plus, null, tint = k.dim) },
                    enabled = a.canSignIn(s),
                    onClick = { open = false; a.onSignIn(s) },
                )
            }
        }
    }
}
