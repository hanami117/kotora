package io.github.hanami117.kotora.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.launch

private enum class Tab(val route: String, val label: String) { LIST("list", "List"), AIRING("airing", "Airing"), SEARCH("search", "Search"), SETTINGS("settings", "Settings") }

@Composable
fun KotoraApp(vm: AppViewModel, version: String, nowSeconds: () -> Long, onLookAround: (() -> Unit)?) {
    val k = Kotora.colors
    when (val s = vm.session) {
        Session.Loading -> Box(Modifier.fillMaxSize().background(k.bg))
        is Session.Broken -> Box(Modifier.fillMaxSize().background(k.bg).padding(32.dp), contentAlignment = Alignment.Center) {
            Text(s.message, color = k.dropped, textAlign = TextAlign.Center)
        }
        is Session.SignedOut -> SignInScreen(s.error, s.busy, vm::hasClientId, vm::signIn, onLookAround)
        is Session.SignedIn -> SignedIn(vm, s, version, nowSeconds)
    }
}

@Composable
private fun SignedIn(vm: AppViewModel, s: Session.SignedIn, version: String, nowSeconds: () -> Long) {
    val k = Kotora.colors
    val nav = rememberNavController()
    val snackHost = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    LaunchedEffect(vm) {
        vm.snackFlow.collect { snack ->
            snackHost.currentSnackbarData?.dismiss()
            scope.launch {
                val r = snackHost.showSnackbar(snack.text, actionLabel = snack.undo?.let { "Undo" }, duration = SnackbarDuration.Long)
                if (r == SnackbarResult.ActionPerformed) snack.undo?.let(vm::undo)
            }
        }
    }
    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route
    val topLevel = Tab.entries.any { it.route == route } || route == null
    Scaffold(
        containerColor = k.bg,
        snackbarHost = {
            SnackbarHost(snackHost) { data ->
                Snackbar(
                    data,
                    shape = RoundedCornerShape(10.dp),
                    containerColor = k.snack,
                    contentColor = Color(0xFFEDF0F7),
                    actionColor = Color(0xFFFF9D3F),
                )
            }
        },
        bottomBar = { if (topLevel) BottomNav(nav, route ?: Tab.LIST.route, vm.list.kind, vm.list.waiting) },
    ) { pad ->
        NavHost(nav, startDestination = Tab.LIST.route, modifier = Modifier.padding(bottom = pad.calculateBottomPadding())) {
            composable(Tab.LIST.route) {
                ListScreen(
                    ui = vm.list,
                    account = s.active,
                    now = nowSeconds(),
                    onKind = { vm.select(kind = it) },
                    onStatus = { vm.select(status = it) },
                    onSort = { vm.select(sort = it) },
                    onSync = vm::sync,
                    onPlusOne = vm::plusOne,
                    onOpen = { r -> nav.navigate("details/${r.kind.name}/${r.id}") },
                    modifier = Modifier.statusBarsPadding(),
                )
            }
            composable(Tab.AIRING.route) {
                val manga = vm.list.kind == MediaKind.MANGA
                Placeholder(
                    if (manga) "Publishing" else "Airing",
                    if (manga) "New chapters of what you read, from MangaUpdates: in a later build (M4)."
                    else "This week's episodes of what you watch, with notifications: in a later build (M4).",
                    s.active,
                    Modifier.statusBarsPadding(),
                )
            }
            composable(Tab.SEARCH.route) {
                Placeholder("Search", "Search and add anime and manga: in a later build (M3).", s.active, Modifier.statusBarsPadding())
            }
            composable(Tab.SETTINGS.route) {
                SettingsScreen(
                    accounts = s.accounts,
                    mangaEnabled = vm.mangaEnabled,
                    waiting = vm.list.waiting,
                    version = version,
                    sample = vm.isSample,
                    onMangaEnabled = vm::switchManga,
                    onSignOut = vm::signOut,
                    onLeaveSample = vm::leaveSample,
                    modifier = Modifier.statusBarsPadding(),
                )
            }
            composable("details/{kind}/{id}") { back ->
                val kind = MediaKind.valueOf(back.arguments?.getString("kind") ?: "ANIME")
                val id = back.arguments?.getString("id")?.toUIntOrNull() ?: 0u
                var row by remember { mutableStateOf<ListRow?>(null) }
                var loaded by remember { mutableStateOf(false) }
                // Read again whenever the list changes (a +1 here or a sync).
                LaunchedEffect(vm.list) {
                    row = vm.entry(kind, id)
                    loaded = true
                }
                if (loaded) {
                    DetailsScreen(row, s.active, onBack = { nav.popBackStack() }, onPlusOne = vm::plusOne, modifier = Modifier.navigationBarsPadding())
                } else {
                    Box(Modifier.fillMaxSize().background(k.bg), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(color = k.accent)
                    }
                }
            }
        }
    }
}

@Composable
private fun BottomNav(nav: NavHostController, route: String, kind: MediaKind, waiting: UInt) {
    val k = Kotora.colors
    Column(Modifier.fillMaxWidth().background(k.side)) {
        HorizontalDivider(color = k.border)
        Row(Modifier.fillMaxWidth().height(68.dp).padding(top = 8.dp)) {
            for (tab in Tab.entries) {
                val (icon, label) = when (tab) {
                    Tab.LIST -> KIcons.List to tab.label
                    Tab.AIRING -> if (kind == MediaKind.MANGA) KIcons.Book to "Publishing" else KIcons.Calendar to "Airing"
                    Tab.SEARCH -> KIcons.Search to tab.label
                    Tab.SETTINGS -> KIcons.Settings to tab.label
                }
                NavItem(icon, label, route == tab.route, if (tab == Tab.SETTINGS && waiting > 0u) waiting.toString() else null, Modifier.weight(1f)) {
                    if (route != tab.route) {
                        nav.navigate(tab.route) {
                            popUpTo(Tab.LIST.route) { saveState = true }
                            launchSingleTop = true
                            restoreState = true
                        }
                    }
                }
            }
        }
        Box(Modifier.navigationBarsPadding())
    }
}

@Composable
private fun NavItem(icon: ImageVector, label: String, on: Boolean, badge: String?, modifier: Modifier, onClick: () -> Unit) {
    val k = Kotora.colors
    Column(
        modifier.clickable(role = Role.Tab, onClick = onClick).semantics { selected = on },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Box {
            Box(
                Modifier.size(60.dp, 32.dp).clip(RoundedCornerShape(16.dp)).background(if (on) k.tint else Color.Transparent),
                contentAlignment = Alignment.Center,
            ) {
                Icon(icon, null, tint = if (on) k.accentHi else k.faint, modifier = Modifier.size(22.dp))
            }
            if (badge != null) {
                Text(
                    badge,
                    color = k.onAccent,
                    fontSize = 10.sp,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.align(Alignment.TopEnd).background(k.accent, RoundedCornerShape(8.dp)).padding(horizontal = 5.dp, vertical = 1.dp),
                )
            }
        }
        Text(label, color = if (on) k.text else k.faint, fontSize = 12.sp, fontWeight = FontWeight.Medium)
    }
}
