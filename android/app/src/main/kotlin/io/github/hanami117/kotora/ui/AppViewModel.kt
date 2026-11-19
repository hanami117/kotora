package io.github.hanami117.kotora.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.AiringMine
import io.github.hanami117.kotora.core.AiringType
import io.github.hanami117.kotora.core.AiringOrder
import io.github.hanami117.kotora.core.AiringPage
import io.github.hanami117.kotora.core.KotoraSettings
import io.github.hanami117.kotora.core.NoticeKind
import io.github.hanami117.kotora.core.SeasonGrid
import io.github.hanami117.kotora.core.SeasonPick
import io.github.hanami117.kotora.core.Setting
import io.github.hanami117.kotora.core.Edit
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.NotifySettings
import io.github.hanami117.kotora.core.PendingSignIn
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.ListEmptyKind
import io.github.hanami117.kotora.core.ListQuery
import io.github.hanami117.kotora.core.ListSort
import io.github.hanami117.kotora.core.ListViewMode
import io.github.hanami117.kotora.core.PageLine
import io.github.hanami117.kotora.core.QuickFilter
import io.github.hanami117.kotora.core.StatusCount
import io.github.hanami117.kotora.core.ProfileInfo
import io.github.hanami117.kotora.core.ServiceLink
import io.github.hanami117.kotora.data.AccountRepository
import io.github.hanami117.kotora.data.Repository
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch

sealed interface Session {
    data object Loading : Session
    /** `busy`: a sign-in is being completed. */
    data class SignedOut(val error: String? = null, val busy: Boolean = false) : Session
    data class SignedIn(val accounts: List<Account>) : Session {
        val active: Account? get() = accounts.firstOrNull { it.active } ?: accounts.firstOrNull()
    }
    /** The core couldn't start (the library didn't load, the database can't open). */
    data class Broken(val message: String) : Session
}

data class ListUi(
    val kind: MediaKind = MediaKind.ANIME,
    val status: ListStatus = ListStatus.WATCHING,
    val sort: ListSort = ListSort.UPDATED,
    /** The order's second direction (desktop `sort_rev`). */
    val reverse: Boolean = false,
    val filter: QuickFilter = QuickFilter.ALL,
    /** The "Filter your list" box. */
    val query: String = "",
    val view: ListViewMode = ListViewMode.LIST,
    val rows: List<Row> = emptyList(),
    /** The rows with their tags and score, and the empty state (`ListPage`). */
    val lines: List<PageLine> = emptyList(),
    val empty: ListEmptyKind = ListEmptyKind.NONE,
    val emptyTitle: String = "",
    val emptyHelp: String = "",
    val counts: List<StatusCount> = emptyList(),
    val syncedAt: Long? = null,
    val waiting: UInt = 0u,
    val syncing: Boolean = false,
    /** The last sync's problem ("Can't reach MyAnimeList."), shown in the sync line. */
    val problem: String? = null,
    val loaded: Boolean = false,
    /** The last sync couldn't reach the service (the queue page's offline notice). */
    val offline: Boolean = false,
)

/** The Airing page: the week and the season grid (local, from the saved seasons), filters, fetch. */
data class AiringUi(
    val week: AiringPage? = null,
    val type: AiringType = AiringType.ALL,
    val mine: AiringMine = AiringMine.ALL,
    /** This week, or the season grid. */
    val grid: Boolean = false,
    val order: AiringOrder = AiringOrder.POPULAR,
    /** The picked season, from the current one (0 now, 1 next, -1 the one before). */
    val offset: Int = 0,
    /** The picked season's grid when it isn't the current one. */
    val other: SeasonGrid? = null,
    val picks: List<SeasonPick> = emptyList(),
    /** The seasons are being asked for. */
    val loading: Boolean = false,
    /** The last fetch's problem ("Can't reach MyAnimeList."). */
    val error: String? = null,
)

/** A message at the bottom, with Undo when it is an edit (or `onUndo`: a removal's Discard). */
data class Snack(val text: String, val undo: Edit? = null, val onUndo: (() -> Unit)? = null)

/**
 * The app's state: the session, the list page, sign-in. One for the Activity. Talks to a
 * [Repository]; the UI never calls the core itself.
 */
class AppViewModel(
    private val repoSource: () -> Repository?,
    private val brokenMessage: () -> String?,
    private val requestFlush: () -> Unit,
    private val saved: SavedStateHandle,
    /** The saved list changed (a sync): the notifications look again. */
    private val requestNotes: () -> Unit = {},
    /** Signed in (true: the notifications' periodic look starts) or out. */
    private val onSignedIn: (Boolean) -> Unit = {},
) : ViewModel() {
    var session by mutableStateOf<Session>(Session.Loading)
        private set
    var list by mutableStateOf(ListUi())
        private set
    var mangaEnabled by mutableStateOf(false)
        private set
    var airing by mutableStateOf(AiringUi())
        private set
    var notify by mutableStateOf(NotifySettings(startedAiring = true, newEpisode = true, newChapter = false))
        private set
    /** The desktop's settings in `config.toml` (theme, startup, …). */
    var settings by mutableStateOf<KotoraSettings?>(null)
        private set
    /** `romaji`, `english`, `japanese`; the list's titles follow it. */
    var titleLanguage by mutableStateOf("romaji")
        private set
    private var shownTitles: Map<UInt, String> = emptyMap()

    /** A notification was tapped: the page to open (kind, id). */
    private val opens = Channel<Pair<MediaKind, UInt>>(Channel.CONFLATED)
    val openFlow = opens.receiveAsFlow()
    /** The service that asked to sign in again (the "Sign in again" banner); the list stays. */
    var expired by mutableStateOf<Service?>(null)
        private set
    /** The service of the last sign-in started (AniList shows the token paste after a problem). */
    var lastSignIn by mutableStateOf<Service?>(null)
        private set

    private val snacks = Channel<Snack>(Channel.BUFFERED)
    val snackFlow = snacks.receiveAsFlow()

    /** Pages ask for the browser through this: the Activity opens a Custom Tab. */
    private val urls = Channel<String>(Channel.BUFFERED)
    val urlFlow = urls.receiveAsFlow()

    private var repoOverride: Repository? = null
    val repo: Repository? get() = repoOverride ?: repoSource()
    val isSample: Boolean get() = repo?.isSample == true

    init {
        viewModelScope.launch { start() }
    }

    private suspend fun start() {
        val r = repo ?: run {
            session = Session.Broken(brokenMessage() ?: "Kotora couldn't start.")
            return
        }
        val accounts = runCatching { r.accounts() }.getOrElse { emptyList() }
        if (accounts.isEmpty()) {
            session = Session.SignedOut()
            if (!r.isSample) onSignedIn(false)
            return
        }
        session = Session.SignedIn(accounts)
        mangaEnabled = runCatching { r.mangaEnabled() }.getOrDefault(false)
        notify = runCatching { r.pages.notifySettings() }.getOrDefault(notify)
        settings = runCatching { r.pages.settings() }.getOrNull()
        titleLanguage = runCatching { r.pages.titleLanguage() }.getOrDefault("romaji")
        settings?.let { st ->
            // Settings → Startup → Mode: the list opens on Anime, Manga or the last one.
            val kind = when (st.startMode) {
                "anime" -> MediaKind.ANIME
                "manga" -> if (mangaEnabled) MediaKind.MANGA else MediaKind.ANIME
                else -> if (st.listKind == MediaKind.MANGA && !mangaEnabled) MediaKind.ANIME else st.listKind
            }
            if (!list.loaded) list = list.copy(kind = kind)
        }
        if (!r.isSample) onSignedIn(true)
        reload()
        if (list.syncedAt == null && !r.isSample) sync() else if (!r.isSample) requestFlush()
    }

    private fun message(e: Throwable): String = when (e) {
        is KotoraException -> e.message?.takeIf { it.isNotBlank() } ?: "Something went wrong."
        else -> e.message ?: e.javaClass.simpleName
    }

    // ------------------------------------------------------------------------------- the list

    suspend fun reload() {
        val r = repo ?: return
        val l = list
        try {
            shownTitles = if (titleLanguage == "romaji") emptyMap() else runCatching { r.pages.shownTitles(l.kind) }.getOrDefault(emptyList()).associate { it.mediaId to it.title }
            val src = r as? ListPageSource
            val page = src?.listPage(l.kind, ListQuery(l.status, l.filter, l.query, l.sort, l.reverse))
            val view = src?.listView(l.kind, l.status) ?: ListViewMode.LIST
            val now = list
            // Keep what changed meanwhile (another page, order, chip or filter picked).
            if (now.kind != l.kind || now.status != l.status || now.sort != l.sort || now.reverse != l.reverse || now.filter != l.filter || now.query != l.query) return
            list = now.copy(
                rows = page?.lines?.map { shown(it.row) } ?: emptyList(),
                lines = page?.lines?.map { it.copy(row = shown(it.row)) } ?: emptyList(),
                empty = page?.empty ?: ListEmptyKind.NONE,
                emptyTitle = page?.emptyTitle ?: "",
                emptyHelp = page?.emptyHelp ?: "",
                view = view,
                counts = r.counts(l.kind),
                syncedAt = r.syncedAt(l.kind),
                waiting = r.waitingCount(),
                loaded = true,
            )
        } catch (e: KotoraException.NotSignedIn) {
            session = Session.SignedOut()
        } catch (e: Exception) {
            list = l.copy(problem = message(e), loaded = true)
        }
    }

    /** A row with its title in the chosen language. */
    private fun shown(row: Row): Row = shownTitles[row.id]?.takeIf { it != row.title }?.let { row.copy(title = it) } ?: row

    fun select(
        kind: MediaKind = list.kind,
        status: ListStatus = list.status,
        sort: ListSort = list.sort,
        reverse: Boolean? = null,
        filter: QuickFilter? = null,
        query: String = list.query,
    ) {
        val kindChanged = kind != list.kind
        val newStatus = if (kindChanged) ListStatus.WATCHING else status
        // A new page starts on All (desktop `go`); a new order starts in its own direction.
        val pageChanged = kindChanged || newStatus != list.status
        list = list.copy(
            kind = kind,
            status = newStatus,
            sort = sort,
            reverse = reverse ?: (if (sort != list.sort) false else list.reverse),
            filter = filter ?: (if (pageChanged) QuickFilter.ALL else list.filter),
            query = query,
        )
        viewModelScope.launch {
            if (kindChanged) runCatching { repo?.pages?.setSetting(Setting.ListKind(kind)) }
            if (kindChanged && kind == MediaKind.MANGA && !mangaEnabled) switchManga(true) else reload()
        }
    }

    /** List, Grid or Large for the page's tab (saved per tab). */
    fun setView(v: ListViewMode) {
        val l = list
        list = l.copy(view = v)
        val src = repo as? ListPageSource ?: return
        viewModelScope.launch { src.setListView(l.kind, l.status, v) }
    }

    fun sync() {
        val r = repo ?: return
        if (list.syncing) return
        list = list.copy(syncing = true)
        viewModelScope.launch {
            try {
                r.flush()
                r.sync()
                list = list.copy(problem = null, offline = false)
                expired = null
                (session as? Session.SignedIn)?.let { session = Session.SignedIn(r.accounts()) }
            } catch (e: KotoraException.SignInAgain) {
                expired = (session as? Session.SignedIn)?.active?.service
                list = list.copy(problem = "Sign in again: the service no longer accepts Kotora's sign-in.")
            } catch (e: KotoraException.Offline) {
                list = list.copy(problem = message(e), offline = true)
            } catch (e: Exception) {
                list = list.copy(problem = message(e))
            }
            list = list.copy(syncing = false)
            reload()
            if (airing.week != null) reloadAiring()
            if (!r.isSample) requestNotes()
        }
    }

    fun plusOne(row: Row) {
        val r = repo ?: return
        viewModelScope.launch {
            try {
                val edit = r.plusOne(row.kind, row.id)
                reload()
                requestFlush()
                snacks.send(Snack(edit.message, edit))
            } catch (e: Exception) {
                snacks.send(Snack(message(e)))
            }
        }
    }

    fun undo(edit: Edit) {
        val r = repo ?: return
        viewModelScope.launch {
            try {
                r.undo(edit)
                reload()
                requestFlush()
            } catch (e: Exception) {
                snacks.send(Snack(message(e)))
            }
        }
    }

    /** A message at the bottom (the editor's and the queue's pages, `EditActions.kt`). */
    fun snack(s: Snack) {
        snacks.trySend(s)
    }

    /** The queue may go now (an edit was saved). */
    fun flushSoon() = requestFlush()

    suspend fun entry(kind: MediaKind, id: UInt): Row? = runCatching { repo?.entry(kind, id) }.getOrNull()?.let { if (kind == list.kind) shown(it) else it }

    fun switchManga(on: Boolean) {
        val r = repo ?: return
        viewModelScope.launch {
            runCatching { r.setMangaEnabled(on) }
            mangaEnabled = on
            if (on && !r.isSample) sync() else reload()
        }
    }

    // ------------------------------------------------------------------------------- airing

    /** The page opened: the saved week at once, then the seasons when they are missing or old. */
    fun openAiring() {
        val r = repo ?: return
        if (airing.picks.isEmpty()) airing = airing.copy(picks = runCatching { r.pages.seasonPicks(PICKER_BACK) }.getOrDefault(emptyList()))
        viewModelScope.launch {
            reloadAiring()
            if (airing.week?.stale != false) fetchAiring(false)
            else if (airing.offset != 0 && airing.other?.stale != false) fetchAiring(false)
        }
    }

    /** The Refresh button (and Retry): asks the service again. */
    fun refreshAiring() = fetchAiring(true)

    private fun fetchAiring(force: Boolean) {
        val r = repo ?: return
        if (airing.loading) return
        airing = airing.copy(loading = true, error = null)
        val offset = airing.offset
        viewModelScope.launch {
            val error = try {
                r.pages.airingRefresh(force)
                if (offset != 0) r.pages.seasonFetch(offset, force)
                null
            } catch (e: Exception) {
                message(e)
            }
            airing = airing.copy(loading = false, error = error)
            reloadAiring()
        }
    }

    /** The week and grid again from what is saved (a filter, the minute, the list changed). Local. */
    suspend fun reloadAiring() {
        val r = repo ?: return
        try {
            val a = airing
            val week = r.pages.airingPage(a.type, a.mine, a.order)
            val other = if (a.offset != 0) r.pages.seasonGrid(a.offset, a.type, a.mine, a.order) else null
            airing = airing.copy(week = week, other = other)
        } catch (e: KotoraException.NotSignedIn) {
            session = Session.SignedOut()
        } catch (e: Exception) {
            airing = airing.copy(error = message(e))
        }
    }

    fun airingFilter(type: AiringType = airing.type, mine: AiringMine = airing.mine, order: AiringOrder = airing.order, grid: Boolean = airing.grid) {
        airing = airing.copy(type = type, mine = mine, order = order, grid = grid)
        viewModelScope.launch { reloadAiring() }
    }

    /** The season picker: a season's grid (asked for when it isn't saved or is old). */
    fun pickSeason(offset: Int) {
        airing = airing.copy(offset = offset, grid = true, other = null, error = null)
        viewModelScope.launch {
            reloadAiring()
            if (offset != 0 && airing.other?.stale != false) fetchAiring(false)
        }
    }

    // ------------------------------------------------------------------------- notifications

    fun changeNotify(settings: NotifySettings) {
        val r = repo ?: return
        val old = notify
        notify = settings
        viewModelScope.launch {
            runCatching {
                if (settings.startedAiring != old.startedAiring) r.pages.setNotify(NoticeKind.STARTED_AIRING, settings.startedAiring)
                if (settings.newEpisode != old.newEpisode) r.pages.setNotify(NoticeKind.NEW_EPISODE, settings.newEpisode)
                if (settings.newChapter != old.newChapter) r.pages.setNotify(NoticeKind.NEW_CHAPTER, settings.newChapter)
            }
            this@AppViewModel.settings = runCatching { r.pages.settings() }.getOrDefault(this@AppViewModel.settings)
            if (!r.isSample) requestNotes()
        }
    }

    // ----------------------------------------------------------------------- desktop settings

    /** One setting of `config.toml` (Theme, Open on, Mode, …). */
    fun changeSetting(setting: Setting) {
        val r = repo ?: return
        viewModelScope.launch {
            try {
                settings = r.pages.setSetting(setting)
            } catch (e: Exception) {
                snacks.send(Snack(message(e)))
            }
        }
    }

    /** Settings → Titles: the list (and the pages) show this language from now on. */
    fun changeTitleLanguage(value: String) {
        val r = repo ?: return
        viewModelScope.launch {
            try {
                r.pages.setTitleLanguage(value)
                titleLanguage = value
                reload()
            } catch (e: Exception) {
                snacks.send(Snack(message(e)))
            }
        }
    }

    /** The page left (for Open on: Last page). */
    fun leftOn(page: String) {
        val r = repo ?: return
        if (settings?.lastPage == page) return
        settings = settings?.copy(lastPage = page)
        viewModelScope.launch { runCatching { r.pages.setSetting(Setting.LastPage(page)) } }
    }

    /** After Fill volumes or its Undo: the list and the queue changed. */
    fun listChanged() {
        viewModelScope.launch { reload() }
        requestFlush()
    }

    /** A notification was tapped (the Activity's intent). */
    fun open(kind: MediaKind, id: UInt) {
        opens.trySend(kind to id)
    }

    // ------------------------------------------------------------------------------- sign-in

    fun hasClientId(service: Service) = repo?.hasClientId(service) == true

    /** Opens the service's sign-in page; the redirect comes back through [finishSignIn]. */
    fun signIn(service: Service) {
        val r = repo ?: return
        lastSignIn = service
        viewModelScope.launch {
            try {
                val p = r.signInStart(service)
                saved[PENDING_SERVICE] = p.service.name
                saved[PENDING_URL] = p.url
                saved[PENDING_STATE] = p.state
                saved[PENDING_VERIFIER] = p.verifier
                if (!signedIn) session = Session.SignedOut()
                urls.send(p.url)
            } catch (e: Exception) {
                fail(message(e))
            }
        }
    }

    private fun pending(): PendingSignIn? {
        val service = saved.get<String>(PENDING_SERVICE)?.let { n -> Service.entries.find { it.name == n } } ?: return null
        return PendingSignIn(service, saved[PENDING_URL] ?: "", saved[PENDING_STATE] ?: "", saved[PENDING_VERIFIER] ?: "")
    }

    private fun clearPending() {
        for (k in listOf(PENDING_SERVICE, PENDING_URL, PENDING_STATE, PENDING_VERIFIER)) saved.remove<String>(k)
    }

    /** The browser came back to `io.github.hanami117.kotora://oauth/…`. */
    fun finishSignIn(redirect: String) {
        val r = repo ?: return
        val service = when {
            redirect.contains("://oauth/mal") -> Service.MAL
            redirect.contains("://oauth/anilist") -> Service.ANI_LIST
            else -> return
        }
        // AniList's implicit grant needs nothing kept; MyAnimeList's verifier and state must be ours.
        val p = pending()?.takeIf { it.service == service }
            ?: if (service == Service.ANI_LIST) PendingSignIn(service, "", "", "") else null
        if (p == null) {
            fail("That sign-in was started before Kotora restarted. Sign in again.")
            return
        }
        clearPending()
        val adding = signedIn
        if (adding) viewModelScope.launch { snacks.send(Snack("Signing in to ${serviceTitle(service)}…")) } else session = Session.SignedOut(busy = true)
        viewModelScope.launch {
            try {
                val a = r.signInFinish(p, redirect)
                if (expired == service) expired = null
                if (adding) snacks.send(Snack("Signed in to ${serviceTitle(service)} as ${a.userName}."))
                start()
            } catch (e: Exception) {
                fail(message(e))
            }
        }
    }

    private val signedIn: Boolean get() = session is Session.SignedIn

    /** A sign-in problem: on the sign-in page when signed out, at the bottom when adding an account or signing in again. */
    private fun fail(text: String) {
        if (signedIn) viewModelScope.launch { snacks.send(Snack(text)) } else session = Session.SignedOut(error = text)
    }

    /** Show the lists of another signed-in account (MyAnimeList and AniList can both be signed in). */
    fun setActive(service: Service) {
        val r = repo ?: return
        if (service == (session as? Session.SignedIn)?.active?.service) return
        viewModelScope.launch {
            try {
                r.setActive(service)
                list = ListUi(kind = if (list.kind == MediaKind.MANGA && mangaEnabled) MediaKind.MANGA else MediaKind.ANIME)
                start()
            } catch (e: Exception) {
                snacks.send(Snack(message(e)))
            }
        }
    }

    fun signOut(service: Service) {
        val r = repo ?: return
        viewModelScope.launch {
            runCatching { r.signOut(service) }
            list = ListUi()
            airing = AiringUi()
            start()
        }
    }

    // ---------------------------------------------------------------- links, profile, token paste

    /** Opens a web page (a Custom Tab), as the sign-in does. */
    fun openUrl(url: String) {
        viewModelScope.launch { urls.send(url) }
    }

    /** The entry's "Open on …" links (the active service's page). */
    fun serviceLinks(kind: MediaKind, id: UInt): List<ServiceLink> =
        runCatching { (repo as? AccountRepository)?.serviceLinks(kind, id) }.getOrNull() ?: emptyList()

    suspend fun profile(service: Service): ProfileInfo? = runCatching { (repo as? AccountRepository)?.profile(service) }.getOrNull()

    /** Signs in with a pasted AniList token or address; the problem's words, or null when it worked. */
    suspend fun signInWithToken(raw: String): String? {
        val r = repo as? AccountRepository ?: return "Kotora couldn't start."
        return try {
            val a = r.signInWithToken(raw)
            if (session is Session.SignedIn) snacks.send(Snack("Signed in to ${serviceTitle(a.service)} as ${a.userName}."))
            expired = null
            start()
            null
        } catch (e: Exception) {
            message(e)
        }
    }

    /** Debug builds: the made-up lists, without signing in. */
    fun lookAround(sample: Repository) {
        repoOverride = sample
        airing = AiringUi()
        viewModelScope.launch { start() }
    }

    fun leaveSample() {
        repoOverride = null
        list = ListUi()
        airing = AiringUi()
        viewModelScope.launch { start() }
    }

    private companion object {
        const val PENDING_SERVICE = "signin.service"
        const val PENDING_URL = "signin.url"
        const val PENDING_STATE = "signin.state"
        const val PENDING_VERIFIER = "signin.verifier"
        /** How far back the season picker goes (about ten years). */
        const val PICKER_BACK = 40u
    }
}
