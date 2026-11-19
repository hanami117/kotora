package io.github.hanami117.kotora

import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.browser.customtabs.CustomTabsIntent
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import io.github.hanami117.kotora.data.SampleRepository
import io.github.hanami117.kotora.ui.AppViewModel
import io.github.hanami117.kotora.ui.KotoraApp
import io.github.hanami117.kotora.ui.theme.KotoraTheme
import io.github.hanami117.kotora.work.FlushWorker
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
                )
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        lifecycleScope.launch { vm.urlFlow.collect(::openInBrowser) }
        if (savedInstanceState == null) handleRedirect(intent)
        val lookAround = if (BuildConfig.DEBUG) ({ vm.lookAround(SampleRepository(System.currentTimeMillis() / 1000)) }) else null
        setContent {
            KotoraTheme {
                KotoraApp(vm, BuildConfig.VERSION_NAME, { System.currentTimeMillis() / 1000 }, lookAround)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleRedirect(intent)
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
