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
import io.github.hanami117.kotora.core.AiringWeek
import io.github.hanami117.kotora.core.Edit
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.NotifySettings
import io.github.hanami117.kotora.core.PendingSignIn
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.Sort
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
    val sort: Sort = Sort.UPDATED,
    val rows: List<Row> = emptyList(),
    val counts: List<StatusCount> = emptyList(),
    val syncedAt: Long? = null,
    val waiting: UInt = 0u,
    val syncing: Boolean = false,
    /** The last sync's problem ("Can't reach MyAnimeList."), shown in the sync line. */
    val problem: String? = null,
    val loaded: Boolean = false,
)

/** The Airing page: the week (local, from the saved seasons), its filters, the fetch. */
data class AiringUi(
    val week: AiringWeek? = null,
    val type: AiringType = AiringType.ALL,
    val mine: AiringMine = AiringMine.ALL,
    /** The seasons are being asked for. */
    val loading: Boolean = false,
    /** The last fetch's problem ("Can't reach MyAnimeList."). */
    val error: String? = null,
)

/** A message at the bottom, with Undo when it is an edit. */
data class Snack(val text: String, val undo: Edit? = null)

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
        notify = runCatching { r.notifySettings() }.getOrDefault(notify)
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
            list = l.copy(
                rows = r.list(l.kind, l.status, l.sort),
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

    fun select(kind: MediaKind = list.kind, status: ListStatus = list.status, sort: Sort = list.sort) {
        val kindChanged = kind != list.kind
        list = list.copy(kind = kind, status = if (kindChanged) ListStatus.WATCHING else status, sort = sort)
        viewModelScope.launch {
            if (kindChanged && kind == MediaKind.MANGA && !mangaEnabled) switchManga(true) else reload()
        }
    }

    fun sync() {
        val r = repo ?: return
        if (list.syncing) return
        list = list.copy(syncing = true)
        viewModelScope.launch {
            try {
                r.flush()
                r.sync()
                list = list.copy(problem = null)
                expired = null
                (session as? Session.SignedIn)?.let { session = Session.SignedIn(r.accounts()) }
            } catch (e: KotoraException.SignInAgain) {
                expired = (session as? Session.SignedIn)?.active?.service
                list = list.copy(problem = "Sign in again: the service no longer accepts Kotora's sign-in.")
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

    suspend fun entry(kind: MediaKind, id: UInt): Row? = runCatching { repo?.entry(kind, id) }.getOrNull()

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
        viewModelScope.launch {
            reloadAiring()
            if (airing.week?.stale != false) fetchAiring(false)
        }
    }

    /** The Refresh button (and Retry): asks the service again. */
    fun refreshAiring() = fetchAiring(true)

    private fun fetchAiring(force: Boolean) {
        val r = repo ?: return
        if (airing.loading) return
        airing = airing.copy(loading = true, error = null)
        viewModelScope.launch {
            val error = try {
                r.airingFetch(force)
                null
            } catch (e: Exception) {
                message(e)
            }
            airing = airing.copy(loading = false, error = error)
            reloadAiring()
        }
    }

    /** The week again from what is saved (a filter, the minute, the list changed). Local. */
    suspend fun reloadAiring() {
        val r = repo ?: return
        try {
            airing = airing.copy(week = r.airingWeek(airing.type, airing.mine))
        } catch (e: KotoraException.NotSignedIn) {
            session = Session.SignedOut()
        } catch (e: Exception) {
            airing = airing.copy(error = message(e))
        }
    }

    fun airingFilter(type: AiringType = airing.type, mine: AiringMine = airing.mine) {
        airing = airing.copy(type = type, mine = mine)
        viewModelScope.launch { reloadAiring() }
    }

    // ------------------------------------------------------------------------- notifications

    fun changeNotify(settings: NotifySettings) {
        val r = repo ?: return
        notify = settings
        viewModelScope.launch {
            runCatching { r.setNotifySettings(settings) }
            if (!r.isSample) requestNotes()
        }
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
                session = Session.SignedOut()
                urls.send(p.url)
            } catch (e: Exception) {
                session = Session.SignedOut(error = message(e))
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
            session = Session.SignedOut(error = "That sign-in was started before Kotora restarted. Sign in again.")
            return
        }
        clearPending()
        session = Session.SignedOut(busy = true)
        viewModelScope.launch {
            try {
                r.signInFinish(p, redirect)
                if (expired == service) expired = null
                start()
            } catch (e: Exception) {
                session = Session.SignedOut(error = message(e))
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
    }
}
