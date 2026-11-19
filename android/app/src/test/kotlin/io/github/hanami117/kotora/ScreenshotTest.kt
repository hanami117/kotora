package io.github.hanami117.kotora

import androidx.compose.runtime.Composable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.AiringMine
import io.github.hanami117.kotora.core.AiringType
import io.github.hanami117.kotora.core.NotifySettings
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.Sort
import io.github.hanami117.kotora.data.SampleEditing
import io.github.hanami117.kotora.core.ListQuery
import io.github.hanami117.kotora.core.ListSort
import io.github.hanami117.kotora.core.ListViewMode
import io.github.hanami117.kotora.core.QuickFilter
import io.github.hanami117.kotora.data.SampleRepository
import io.github.hanami117.kotora.ui.EntryActions
import io.github.hanami117.kotora.ui.EntryEditorScreen
import io.github.hanami117.kotora.ui.HomeScreen
import io.github.hanami117.kotora.ui.QueueScreen
import io.github.hanami117.kotora.ui.RemoveDialog
import io.github.hanami117.kotora.ui.SearchModel
import io.github.hanami117.kotora.ui.SearchScreen
import io.github.hanami117.kotora.ui.Shelves
import io.github.hanami117.kotora.ui.AiringScreen
import io.github.hanami117.kotora.ui.AiringUi
import io.github.hanami117.kotora.ui.DetailsScreen
import io.github.hanami117.kotora.ui.IntroOverlay
import io.github.hanami117.kotora.ui.ListScreen
import io.github.hanami117.kotora.ui.ProfileScreen
import io.github.hanami117.kotora.ui.ListUi
import io.github.hanami117.kotora.ui.SettingsScreen
import io.github.hanami117.kotora.ui.SignInScreen
import io.github.hanami117.kotora.ui.theme.KotoraTheme
import io.github.hanami117.kotora.ui.theme.ThemeMode
import kotlinx.coroutines.runBlocking
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Every screen and state with made-up data, dark and light: the Android version of the desktop's
 * `--snap --scene`. Pictures go to `app/build/outputs/roborazzi/` (`./gradlew recordRoborazziDebug`),
 * to compare by eye with the mockup. No network, no native library, no account.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w390dp-h844dp-xxhdpi")
class ScreenshotTest {
    @get:Rule val compose = createComposeRule()

    private val now = 1_795_000_000L
    private val repo = SampleRepository(now)
    private val account = Account(Service.MAL, "sample-user", null, true)

    private fun shot(name: String, dark: Boolean, content: @Composable () -> Unit) {
        compose.setContent { KotoraTheme(if (dark) ThemeMode.DARK else ThemeMode.LIGHT, content) }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/$name-${if (dark) "dark" else "light"}.png")
    }

    private fun listUi(
        kind: MediaKind = MediaKind.ANIME,
        status: ListStatus = ListStatus.WATCHING,
        waiting: Boolean = true,
        filter: QuickFilter = QuickFilter.ALL,
        query: String = "",
        sort: ListSort = ListSort.UPDATED,
        view: ListViewMode = ListViewMode.LIST,
    ) = runBlocking {
        val page = repo.listPage(kind, ListQuery(status, filter, query, sort, false))
        ListUi(
            kind = kind,
            status = status,
            sort = sort,
            filter = filter,
            query = query,
            view = view,
            rows = page.lines.map { it.row },
            lines = page.lines,
            empty = page.empty,
            emptyTitle = page.emptyTitle,
            emptyHelp = page.emptyHelp,
            counts = repo.counts(kind),
            syncedAt = now - 240,
            waiting = if (waiting) repo.waitingCount() else 0u,
            loaded = true,
        )
    }

    private fun list(name: String, dark: Boolean, ui: ListUi) = shot(name, dark) {
        ListScreen(ui, account, now, {}, {}, {}, {}, {}, {})
    }

    @Test fun listDark() = list("list", true, listUi())
    @Test fun listLight() = list("list", false, listUi())
    @Test fun listMangaDark() = list("list-manga", true, listUi(MediaKind.MANGA, waiting = false))
    @Test fun listOfflineDark() = list("list-offline", true, listUi().copy(problem = "Can't reach MyAnimeList. Showing your saved list."))
    @Test fun listEmptyLight() = list("list-empty", false, listUi(query = "zzz"))
    @Test fun listGridDark() = list("list-grid", true, listUi(view = ListViewMode.GRID))
    @Test fun listLargeLight() = list("list-large", false, listUi(view = ListViewMode.LARGE, sort = ListSort.PROGRESS))
    @Test fun listNewEpisodeDark() = list("list-new-episode", true, listUi(filter = QuickFilter.NEW_EPISODE))
    @Test fun listFilterLight() = list("list-filter", false, listUi(query = "ka"))
    @Test fun listSyncingDark() = list("list-syncing", true, listUi().copy(syncing = true))

    private fun details(name: String, dark: Boolean, id: UInt) = shot(name, dark) {
        val row = runBlocking { repo.entry(MediaKind.ANIME, id) }
        DetailsScreen(row, account, {}, {})
    }

    @Test fun detailsDark() = details("details", true, 7u)
    @Test fun detailsLight() = details("details", false, 7u)
    @Test fun detailsRewatchDark() = details("details-rewatch", true, 19u)

    @Test fun signInDark() = shot("signin", true) { SignInScreen(null, false, { true }, {}, null) }
    @Test fun signInLight() = shot("signin", false) { SignInScreen(null, false, { true }, {}, null) }
    @Test fun signInNoClientIdDark() = shot("signin-no-id", true) { SignInScreen(null, false, { false }, {}, {}) }
    @Test fun signInErrorLight() = shot("signin-error", false) {
        SignInScreen("You cancelled the sign-in on MyAnimeList.", false, { true }, {}, null)
    }
    @Test fun signInBusyDark() = shot("signin-busy", true) { SignInScreen(null, true, { true }, {}, null) }

    @Test fun settingsDark() = shot("settings", true) { SettingsScreen(listOf(account), true, 2u, "0.5.0", false, {}, {}, {}) }
    @Test fun settingsLight() = shot("settings", false) { SettingsScreen(listOf(account), false, 0u, "0.5.0", false, {}, {}, {}) }

    // ---- Airing

    private fun airing(name: String, dark: Boolean, ui: AiringUi) = shot(name, dark) {
        AiringScreen(ui, account, {}, {}, {}, { _, _ -> }, {}, ticking = false)
    }

    private fun week(type: AiringType = AiringType.ALL, mine: AiringMine = AiringMine.ALL) = runBlocking { repo.airingWeek(type, mine) }

    @Test fun airingDark() = airing("airing", true, AiringUi(week()))
    @Test fun airingLight() = airing("airing", false, AiringUi(week()))
    @Test fun airingMineDark() = airing("airing-mine", true, AiringUi(week(mine = AiringMine.ON), mine = AiringMine.ON))
    @Test fun airingUpdatingErrorLight() = airing("airing-error", false, AiringUi(week(), loading = false, error = "Can\u2019t reach MyAnimeList."))
    @Test fun airingLoadingDark() = airing("airing-loading", true, AiringUi(week().copy(shows = 0u, fetchedAt = null, days = emptyList(), untimed = emptyList()), loading = true))
    @Test fun airingNothingFilteredLight() = airing("airing-filtered", false, AiringUi(week().copy(days = week().days.map { it.copy(slots = emptyList(), onList = 0u) }, untimed = emptyList()), type = AiringType.MOVIE))

    // ---- the startup intro, held at points of its run (the desktop's `Intro::frozen` scenes)

    private fun intro(name: String, dark: Boolean, at: Float, reduced: Boolean = false) = shot(name, dark) {
        androidx.compose.foundation.layout.Box {
            ListScreen(listUi(), account, now, {}, {}, {}, {}, {}, {})
            IntroOverlay(reduced, onDone = {}, frozenAt = at)
        }
    }

    @Test fun introStartDark() = intro("intro-0-start", true, 0f)
    @Test fun introSlashDark() = intro("intro-1-slash", true, 0.3f)
    @Test fun introSplitDark() = intro("intro-2-split", true, 0.55f)
    @Test fun introAwayDark() = intro("intro-3-away", true, 0.8f)
    @Test fun introSlashLight() = intro("intro-1-slash", false, 0.3f)
    @Test fun introReducedLight() = intro("intro-reduced", false, 0.5f, reduced = true)

    @Test fun settingsNotificationsLight() = shot("settings-notifications", false) {
        SettingsScreen(
            listOf(account), false, 0u, "0.5.0", false, {}, {}, {},
            notify = NotifySettings(startedAiring = true, newEpisode = false, newChapter = false),
            systemNotificationsOff = true,
        )
    }

    // Batches A, G, H: links, Profile, AniList token paste, "Sign in again".
    @Test fun profileDark() = shot("profile", true) { ProfileScreen(io.github.hanami117.kotora.data.sampleProfile(), false, {}, {}, {}) }
    @Test fun profileLight() = shot("profile", false) { ProfileScreen(io.github.hanami117.kotora.data.sampleProfile(), false, {}, {}, {}) }
    @Test fun detailsLinksDark() = shot("details-links", true) {
        val row = runBlocking { repo.entry(MediaKind.ANIME, 7u) }
        DetailsScreen(row, account, {}, {}, links = repo.serviceLinks(MediaKind.ANIME, 7u))
    }
    @Test fun listExpiredDark() = shot("list-expired", true) {
        ListScreen(listUi(), account, now, {}, {}, {}, {}, {}, {}, expired = Service.MAL)
    }
    @Test fun signInTokenLight() = shot("signin-token", false) {
        SignInScreen("AniList refused the token. Sign in again.", false, { true }, {}, null, onToken = { null })
    }
    @Test fun settingsLinksDark() = shot("settings-links", true) {
        SettingsScreen(listOf(account), true, 0u, "0.5.0", false, {}, {}, {}, onProfile = {})
    }
    // ---- the editor, the sync queue, Search and add, Home (AN-1, AN-2)

    private val editing = io.github.hanami117.kotora.data.SampleEditing(repo, now)

    private fun editor(name: String, dark: Boolean, kind: MediaKind, id: UInt, change: (io.github.hanami117.kotora.core.EntryEditor) -> io.github.hanami117.kotora.core.EntryEditor = { it }) = shot(name, dark) {
        val ed = runBlocking { change(editing.editor(kind, id)) }
        val scale = runBlocking { editing.scoreScale() }
        EntryEditorScreen(ed, scale, null, "MyAnimeList", false, editing::scoreLabel, editing::scoreStep, {}, {}, {})
    }

    @Test fun editorDark() = editor("editor", true, MediaKind.ANIME, 7u)
    @Test fun editorLight() = editor("editor", false, MediaKind.ANIME, 7u)
    @Test fun editorRewatchDark() = editor("editor-rewatch", true, MediaKind.ANIME, 19u)
    @Test fun editorMangaLight() = editor("editor-manga", false, MediaKind.MANGA, 102u)

    private fun detailsActions(name: String, dark: Boolean, id: UInt) = shot(name, dark) {
        val row = runBlocking { repo.entry(MediaKind.ANIME, id) }
        DetailsScreen(row, account, {}, {}, actions = { EntryActions(it, {}, {}, {}, {}) })
    }

    @Test fun detailsActionsDark() = detailsActions("details-actions", true, 7u)
    @Test fun detailsActionsRewatchLight() = detailsActions("details-actions-rewatch", false, 19u)
    @Test fun removeDialogDark() = shot("remove-dialog", true) {
        RemoveDialog("Remove Hoshikuzu Relay from your list? It goes from your MyAnimeList list too, with its progress and score.", {}, {})
    }

    private fun queue(name: String, dark: Boolean, empty: Boolean = false) = shot(name, dark) {
        val items = if (empty) emptyList() else runBlocking { editing.queueItems() }
        QueueScreen(items, "MyAnimeList", now, null, false, {}, {}, { _, _ -> })
    }

    @Test fun queueDark() = queue("queue", true)
    @Test fun queueLight() = queue("queue", false)
    @Test fun queueEmptyDark() = queue("queue-empty", true, empty = true)
    @Test fun queueOfflineDark() = shot("queue-offline", true) {
        QueueScreen(runBlocking { editing.queueItems() }, "MyAnimeList", now, "Can't reach MyAnimeList. Showing your saved list.", false, {}, {}, { _, _ -> }, offline = true, waiting = 2u)
    }
    @Test fun queueExpiredLight() = shot("queue-expired", false) {
        QueueScreen(runBlocking { editing.queueItems() }, "MyAnimeList", now, "Sign in again.", false, {}, {}, { _, _ -> }, expired = Service.MAL, waiting = 2u)
    }

    // AN-5: both accounts signed in, Show / Sign in in Settings.
    private val two = listOf(account, Account(Service.ANI_LIST, "sample-ani", null, false))
    @Test fun settingsTwoAccountsDark() = shot("settings-accounts", true) {
        androidx.compose.runtime.CompositionLocalProvider(io.github.hanami117.kotora.ui.LocalAccounts provides io.github.hanami117.kotora.ui.Accounts(two, false, { true }, {}, {})) {
            SettingsScreen(two, true, 3u, "0.5.0", false, {}, {}, {})
        }
    }
    @Test fun settingsOneAccountLight() = shot("settings-signin", false) {
        androidx.compose.runtime.CompositionLocalProvider(io.github.hanami117.kotora.ui.LocalAccounts provides io.github.hanami117.kotora.ui.Accounts(listOf(account), false, { true }, {}, {})) {
            SettingsScreen(listOf(account), true, 0u, "0.5.0", false, {}, {}, {})
        }
    }
    @Test fun settingsQueueDark() = shot("settings-queue", true) { SettingsScreen(listOf(account), true, 2u, "0.5.0", false, {}, {}, {}, conflicts = 1u, onQueue = {}) }

    private fun search(name: String, dark: Boolean, text: String, kind: MediaKind = MediaKind.ANIME, pending: Boolean = false) = shot(name, dark) {
        val s = SearchModel()
        s.kind = kind
        s.text = text
        s.mine = runBlocking { repo.list(kind, null, Sort.TITLE) }
        if (SearchModel.norm(text).length >= 3) {
            s.asked = SearchModel.norm(text)
            if (pending) s.pending = true else s.hits = SampleEditing.sampleHits(kind)
        }
        SearchScreen(s, account, true, {}, {}, {}, {}, {}, { _, _ -> })
    }

    @Test fun searchEmptyDark() = search("search-empty", true, "")
    @Test fun searchDark() = search("search", true, "lantern")
    @Test fun searchLight() = search("search", false, "lantern")
    @Test fun searchShortLight() = search("search-short", false, "la")
    @Test fun searchPendingDark() = search("search-pending", true, "lantern", pending = true)
    @Test fun searchMangaDark() = search("search-manga", true, "lantern", MediaKind.MANGA)

    private fun home(name: String, dark: Boolean, manga: Boolean = true, empty: Boolean = false) = shot(name, dark) {
        val shelves = if (empty) Shelves(loaded = true) else runBlocking {
            Shelves.of(repo.list(MediaKind.ANIME, null, Sort.UPDATED), repo.list(MediaKind.MANGA, null, Sort.UPDATED))
        }
        HomeScreen(shelves, account, manga, false, {}, {}, {}, { _, _ -> })
    }

    @Test fun homeDark() = home("home", true)
    @Test fun homeLight() = home("home", false)
    @Test fun homeEmptyLight() = home("home-empty", false, manga = false, empty = true)
}
