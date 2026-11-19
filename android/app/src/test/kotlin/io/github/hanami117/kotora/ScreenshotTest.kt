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
import io.github.hanami117.kotora.data.SampleRepository
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

    private fun listUi(kind: MediaKind = MediaKind.ANIME, status: ListStatus = ListStatus.WATCHING, waiting: Boolean = true) = runBlocking {
        ListUi(
            kind = kind,
            status = status,
            sort = Sort.UPDATED,
            rows = repo.list(kind, status, Sort.UPDATED),
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
    @Test fun listEmptyLight() = list("list-empty", false, listUi().copy(rows = emptyList(), syncedAt = null, waiting = 0u))
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
}
