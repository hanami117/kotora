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
import androidx.compose.runtime.CompositionLocalProvider
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
import io.github.hanami117.kotora.core.EntryEditor
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.ProfileInfo
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.QueueItem
import io.github.hanami117.kotora.core.ScoreScale
import io.github.hanami117.kotora.core.Sort
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.launch

/** The phone's settings the Activity keeps (the startup animation, the system's notification switch). */
data class AppPrefs(
    val intro: Boolean = true,
    val onIntro: (Boolean) -> Unit = {},
    val notificationsOff: Boolean = false,
    val openSystemSettings: () -> Unit = {},
)

private enum class Tab(val route: String, val label: String) { HOME("home", "Home"), LIST("list", "List"), AIRING("airing", "Airing"), SEARCH("search", "Search"), SETTINGS("settings", "Settings") }

@Composable
fun KotoraApp(vm: AppViewModel, version: String, nowSeconds: () -> Long, onLookAround: (() -> Unit)?, prefs: AppPrefs = AppPrefs()) {
    val k = Kotora.colors
    when (val s = vm.session) {
        Session.Loading -> Box(Modifier.fillMaxSize().background(k.bg))
        is Session.Broken -> Box(Modifier.fillMaxSize().background(k.bg).padding(32.dp), contentAlignment = Alignment.Center) {
            Text(s.message, color = k.dropped, textAlign = TextAlign.Center)
        }
        is Session.SignedOut -> SignInScreen(
            s.error, s.busy, vm::hasClientId, vm::signIn, onLookAround,
            onToken = if (vm.lastSignIn == Service.ANI_LIST) vm::signInWithToken else null,
        )
        is Session.SignedIn -> SignedIn(vm, s, version, nowSeconds, prefs)
    }
}

@Composable
private fun SignedIn(vm: AppViewModel, s: Session.SignedIn, version: String, nowSeconds: () -> Long, prefs: AppPrefs) {
    val k = Kotora.colors
    val nav = rememberNavController()
    val snackHost = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    LaunchedEffect(vm) {
        vm.snackFlow.collect { snack ->
            snackHost.currentSnackbarData?.dismiss()
            scope.launch {
                val canUndo = snack.undo != null || snack.onUndo != null
                val r = snackHost.showSnackbar(snack.text, actionLabel = if (canUndo) "Undo" else null, duration = SnackbarDuration.Long)
                if (r == SnackbarResult.ActionPerformed) snack.undo?.let(vm::undo) ?: snack.onUndo?.invoke()
            }
        }
    }
    // A notification was tapped: its anime's or manga's page.
    LaunchedEffect(vm, nav) {
        vm.openFlow.collect { (kind, id) ->
            if (kind != vm.list.kind) vm.select(kind = kind)
            nav.navigate("details/${kind.name}/$id") { launchSingleTop = true }
        }
    }
    val search = remember { SearchModel() }
    var conflicts by remember { mutableStateOf(0u) }
    LaunchedEffect(vm.list) { conflicts = runCatching { vm.editing?.conflictCount() }.getOrNull() ?: 0u }
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
        bottomBar = { if (topLevel) BottomNav(nav, route ?: Tab.HOME.route, vm.list.kind, vm.list.waiting) },
    ) { pad ->
        val accounts = Accounts(s.accounts, vm.isSample, vm::hasClientId, vm::setActive, vm::signIn)
        CompositionLocalProvider(LocalAccounts provides accounts) {
        NavHost(nav, startDestination = Tab.HOME.route, modifier = Modifier.padding(bottom = pad.calculateBottomPadding())) {
            composable(Tab.HOME.route) {
                var shelves by remember { mutableStateOf(Shelves()) }
                LaunchedEffect(vm.list, vm.mangaEnabled) {
                    val r = vm.repo ?: return@LaunchedEffect
                    val anime = runCatching { r.list(MediaKind.ANIME, null, Sort.UPDATED) }.getOrDefault(emptyList())
                    val manga = if (vm.mangaEnabled) runCatching { r.list(MediaKind.MANGA, null, Sort.UPDATED) }.getOrDefault(emptyList()) else emptyList()
                    shelves = Shelves.of(anime, manga)
                }
                HomeScreen(
                    shelves = shelves,
                    account = s.active,
                    mangaEnabled = vm.mangaEnabled,
                    syncing = vm.list.syncing,
                    onSync = vm::sync,
                    onPlusOne = vm::plusOne,
                    onOpen = { r -> nav.navigate("details/${r.kind.name}/${r.id}") },
                    onSeeAll = { kind, status ->
                        vm.select(kind = kind)
                        vm.select(status = status)
                        nav.navigate(Tab.LIST.route) {
                            popUpTo(Tab.HOME.route) { saveState = true }
                            launchSingleTop = true
                        }
                    },
                    modifier = Modifier.statusBarsPadding(),
                )
            }
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
                    expired = vm.expired,
                    onSignIn = vm::signIn,
                    linksFor = { r -> vm.serviceLinks(r.kind, r.id) },
                    onOpenUrl = vm::openUrl,
                    onReverse = { vm.select(reverse = it) },
                    onFilter = { vm.select(filter = it) },
                    onQuery = { vm.select(query = it) },
                    onView = vm::setView,
                )
            }
            composable(Tab.AIRING.route) {
                if (vm.list.kind == MediaKind.MANGA) {
                    Placeholder(
                        "Publishing",
                        "New chapters of what you read, from MangaUpdates: in a later build (M4). New chapter notifications already work (Settings).",
                        s.active,
                        Modifier.statusBarsPadding(),
                    )
                } else {
                    AiringScreen(
                        ui = vm.airing,
                        account = s.active,
                        onOpen = vm::openAiring,
                        onRefresh = vm::refreshAiring,
                        onTick = vm::reloadAiring,
                        onFilter = { t, m -> vm.airingFilter(type = t ?: vm.airing.type, mine = m ?: vm.airing.mine) },
                        onShow = { id -> nav.navigate("details/${MediaKind.ANIME.name}/$id") },
                        modifier = Modifier.statusBarsPadding(),
                    )
                }
            }
            composable(Tab.SEARCH.route) {
                val now = nowSeconds()
                LaunchedEffect(search.kind, vm.list) { search.loadMine(vm.repo) }
                LaunchedEffect(vm.mangaEnabled) { if (!vm.mangaEnabled) search.kind = MediaKind.ANIME }
                SearchScreen(
                    s = search,
                    account = s.active,
                    mangaEnabled = vm.mangaEnabled,
                    onKind = {
                        search.kind = it
                        search.changed(scope, vm.editing, now, force = true)
                    },
                    onText = {
                        search.text = it
                        search.changed(scope, vm.editing, now)
                    },
                    onAgain = { search.changed(scope, vm.editing, now, force = true) },
                    onPlusOne = vm::plusOne,
                    onOpen = { r -> nav.navigate("details/${r.kind.name}/${r.id}") },
                    onAdd = { hit, status -> vm.add(hit, status) },
                    modifier = Modifier.statusBarsPadding(),
                )
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
                    notify = vm.notify,
                    onNotify = vm::changeNotify,
                    intro = prefs.intro,
                    onIntro = prefs.onIntro,
                    systemNotificationsOff = prefs.notificationsOff,
                    onOpenSystemSettings = prefs.openSystemSettings,
                    onProfile = { nav.navigate("profile") },
                    onOpenUrl = vm::openUrl,
                    conflicts = conflicts,
                    onQueue = { nav.navigate("queue") },
                )
            }
            composable("queue") {
                var items by remember { mutableStateOf<List<QueueItem>?>(null) }
                LaunchedEffect(vm.list) { items = vm.editing?.queueItems() ?: emptyList() }
                QueueScreen(
                    items = items,
                    service = s.active?.let { serviceTitle(it.service) } ?: "the service",
                    now = nowSeconds(),
                    problem = vm.list.problem,
                    syncing = vm.list.syncing,
                    onBack = { nav.popBackStack() },
                    onSyncNow = vm::syncNow,
                    onAction = { a, item -> vm.queue(a, item) },
                    modifier = Modifier.navigationBarsPadding(),
                    expired = vm.expired,
                    offline = vm.list.offline,
                    waiting = vm.list.waiting,
                    onSignIn = vm::signIn,
                )
            }
            composable("edit/{kind}/{id}") { back ->
                val kind = MediaKind.valueOf(back.arguments?.getString("kind") ?: "ANIME")
                val id = back.arguments?.getString("id")?.toUIntOrNull() ?: 0u
                val editing = vm.editing
                var ed by remember { mutableStateOf<EntryEditor?>(null) }
                var scale by remember { mutableStateOf<ScoreScale?>(null) }
                var problem by remember { mutableStateOf<String?>(null) }
                var saving by remember { mutableStateOf(false) }
                LaunchedEffect(kind, id) {
                    try {
                        ed = editing?.editor(kind, id)
                        scale = editing?.scoreScale()
                    } catch (e: Exception) {
                        problem = messageOf(e)
                    }
                }
                EntryEditorScreen(
                    ed = ed,
                    scale = scale,
                    problem = problem,
                    service = s.active?.let { serviceTitle(it.service) } ?: "the service",
                    saving = saving,
                    scoreLabel = { editing?.scoreLabel(it) ?: "" },
                    scoreStep = { p, d -> editing?.scoreStep(p, d) },
                    onChange = { ed = it },
                    onCancel = { nav.popBackStack() },
                    onSave = {
                        val e = ed ?: return@EntryEditorScreen
                        saving = true
                        scope.launch {
                            if (vm.saveEditor(e)) nav.popBackStack()
                            saving = false
                        }
                    },
                    modifier = Modifier.navigationBarsPadding(),
                )
            }
            composable("profile") {
                var info by remember { mutableStateOf<ProfileInfo?>(null) }
                LaunchedEffect(s.active?.service) { info = s.active?.service?.let { vm.profile(it) } }
                ProfileScreen(info, vm.isSample, onBack = { nav.popBackStack() }, onOpenUrl = vm::openUrl, onSignOut = {
                    nav.popBackStack()
                    vm.signOut(it)
                }, modifier = Modifier.navigationBarsPadding())
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
                    var asking by remember { mutableStateOf<String?>(null) }
                    DetailsScreen(
                        row,
                        s.active,
                        onBack = { nav.popBackStack() },
                        onPlusOne = vm::plusOne,
                        modifier = Modifier.navigationBarsPadding(),
                        links = vm.serviceLinks(kind, id),
                        onOpenUrl = vm::openUrl,
                        actions = { r ->
                            EntryActions(
                                r,
                                onEdit = { nav.navigate("edit/${r.kind.name}/${r.id}") },
                                onRewatch = { start -> vm.rewatch(r, start) },
                                onMinusOne = { vm.minusOne(r) },
                                onRemove = { scope.launch { asking = runCatching { vm.editing?.removeQuestion(r.kind, r.id) }.getOrNull() ?: "Remove ${r.title} from your list?" } },
                            )
                        },
                    )
                    asking?.let { q ->
                        RemoveDialog(q, onRemove = {
                            asking = null
                            row?.let(vm::remove)
                            nav.popBackStack()
                        }, onDismiss = { asking = null })
                    }
                } else {
                    Box(Modifier.fillMaxSize().background(k.bg), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(color = k.accent)
                    }
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
                    Tab.HOME -> KIcons.Home to tab.label
                    Tab.LIST -> KIcons.List to tab.label
                    Tab.AIRING -> if (kind == MediaKind.MANGA) KIcons.Book to "Publishing" else KIcons.Calendar to "Airing"
                    Tab.SEARCH -> KIcons.Search to tab.label
                    Tab.SETTINGS -> KIcons.Settings to tab.label
                }
                NavItem(icon, label, route == tab.route, if (tab == Tab.SETTINGS && waiting > 0u) waiting.toString() else null, Modifier.weight(1f)) {
                    if (route != tab.route) {
                        nav.navigate(tab.route) {
                            popUpTo(Tab.HOME.route) { saveState = true }
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
