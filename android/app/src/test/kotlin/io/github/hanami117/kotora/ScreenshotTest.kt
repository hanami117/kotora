package io.github.hanami117.kotora

import androidx.compose.runtime.Composable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.Sort
import io.github.hanami117.kotora.data.SampleRepository
import io.github.hanami117.kotora.ui.DetailsScreen
import io.github.hanami117.kotora.ui.ListScreen
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
}
