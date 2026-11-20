package io.github.hanami117.kotora

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.activity.viewModels
import androidx.browser.customtabs.CustomTabsIntent
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import io.github.hanami117.kotora.data.SampleRepository
import io.github.hanami117.kotora.data.UiPrefs
import io.github.hanami117.kotora.notify.Notifier
import io.github.hanami117.kotora.ui.AppPrefs
import io.github.hanami117.kotora.ui.AppViewModel
import io.github.hanami117.kotora.ui.IntroOverlay
import io.github.hanami117.kotora.ui.KotoraApp
import io.github.hanami117.kotora.ui.Session
import io.github.hanami117.kotora.ui.theme.KotoraTheme
import io.github.hanami117.kotora.ui.theme.ThemeMode
import io.github.hanami117.kotora.work.FlushWorker
import io.github.hanami117.kotora.work.NotifyWorker
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val vm: AppViewModel by viewModels {
        viewModelFactory {
            initializer {
                val app = application as KotoraApplication
                AppViewModel(
                    repoSource = { app.repository },
                    brokenMessage = { app.brokenMessage },
                    requestFlush = { FlushWorker.enqueue(app) },
                    saved = createSavedStateHandle(),
                    requestNotes = { NotifyWorker.runNow(app) },
                    onSignedIn = { on -> if (on) NotifyWorker.schedule(app) else NotifyWorker.cancel(app) },
                )
            }
        }
    }

    private val uiPrefs by lazy { UiPrefs(this) }
    private var notificationsOff by mutableStateOf(false)
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        notificationsOff = !Notifier.allowed(this)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // The launch screen is the intro's start pose; it goes as soon as the app draws.
        val splash = installSplashScreen()
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val app = application as KotoraApplication
        // Only on a cold start (not a rotation, not coming back to a running Kotora), when on.
        val playIntro = savedInstanceState == null && !app.introPlayed && uiPrefs.intro
        app.introPlayed = true
        if (playIntro) splash.setOnExitAnimationListener { it.remove() }
        Notifier.ensureChannels(this)
        lifecycleScope.launch { vm.urlFlow.collect(::openInBrowser) }
        if (savedInstanceState == null) {
            handleRedirect(intent)
            handleNotification(intent)
        }
        // Signed in for the first time on Android 13+: ask once for notifications (both switches
        // that need nothing more are on by default, as on the desktop).
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                notificationsOff = !Notifier.allowed(this@MainActivity)
                snapshotSession { signedIn ->
                    if (signedIn && Build.VERSION.SDK_INT >= 33 && !uiPrefs.askedNotifications && !Notifier.allowed(this@MainActivity)) {
                        uiPrefs.askedNotifications = true
                        askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                    }
                }
            }
        }
        val lookAround = if (BuildConfig.DEBUG) ({ vm.lookAround(SampleRepository(System.currentTimeMillis() / 1000)) }) else null
        val reduced = Settings.Global.getFloat(contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
        setContent {
            var intro by remember { mutableStateOf(uiPrefs.intro) }
            var introUp by remember { mutableStateOf(playIntro) }
            // Settings → Appearance → Theme (the desktop's `[display] theme`).
            val mode = when (vm.settings?.theme) { "light" -> ThemeMode.LIGHT; "dark" -> ThemeMode.DARK; else -> ThemeMode.SYSTEM }
            KotoraTheme(mode) {
                Box(Modifier.fillMaxSize()) {
                    val prefs = AppPrefs(
                        intro = intro,
                        onIntro = { intro = it; uiPrefs.intro = it },
                        notificationsOff = notificationsOff,
                        openSystemSettings = ::openNotificationSettings,
                        updater = (application as KotoraApplication).updater,
                    )
                    KotoraApp(vm, BuildConfig.VERSION_NAME, { System.currentTimeMillis() / 1000 }, lookAround, prefs)
                    if (introUp) IntroOverlay(reduced, onDone = { introUp = false })
                }
            }
        }
    }

    /** Calls `f` with whether the session is signed in, each time it changes, while started. */
    private suspend fun snapshotSession(f: (Boolean) -> Unit) {
        androidx.compose.runtime.snapshotFlow { vm.session is Session.SignedIn }.collect(f)
    }

    private fun openNotificationSettings() {
        val i = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, packageName)
        } else {
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName"))
        }
        try {
            startActivity(i)
        } catch (_: ActivityNotFoundException) {
        }
    }

    /** A tapped notification: its page. */
    private fun handleNotification(intent: Intent?) {
        val (kind, id) = Notifier.target(intent) ?: return
        vm.open(kind, id)
        intent?.removeExtra(Notifier.EXTRA_KIND)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleRedirect(intent)
        handleNotification(intent)
    }

    /** `io.github.hanami117.kotora://oauth/mal?code=…` or `…/oauth/anilist#access_token=…`. */
    private fun handleRedirect(intent: Intent?) {
        val uri = intent?.data ?: return
        if (intent.action != Intent.ACTION_VIEW || uri.scheme != "io.github.hanami117.kotora" || uri.host != "oauth") return
        vm.finishSignIn(uri.toString())
        setIntent(Intent(this, MainActivity::class.java))
    }

    /** A Custom Tab: the user's browser with its saved sessions and password manager. Never a WebView. */
    private fun openInBrowser(url: String) {
        val uri = Uri.parse(url)
        try {
            CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(this, uri)
        } catch (_: ActivityNotFoundException) {
            try {
                startActivity(Intent(Intent.ACTION_VIEW, uri))
            } catch (_: ActivityNotFoundException) {
                android.widget.Toast.makeText(this, "No browser to open the sign-in page.", android.widget.Toast.LENGTH_LONG).show()
            }
        }
    }
}
