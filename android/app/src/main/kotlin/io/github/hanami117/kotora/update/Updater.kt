package io.github.hanami117.kotora.update

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/** A newer Android release on GitHub: its tag `android-v0.5.1`, the APK and the release page. */
data class AndroidRelease(val version: String, val code: Int, val apkUrl: String, val apkName: String, val size: Long, val pageUrl: String)

/** What Settings → Updates shows. */
sealed interface UpdateStep {
    data object Idle : UpdateStep
    data object Checking : UpdateStep
    data class Downloading(val percent: Int) : UpdateStep
    /** Android asks the user to let Kotora install apps first. */
    data object NeedsPermission : UpdateStep
}

/**
 * The update check: once a day at most (and on Check now), asks GitHub for the repo's releases
 * and keeps the newest `android-v<version>` one with an APK. Nothing is downloaded until the
 * user taps Download; then the APK goes to the cache and Android's package installer opens it
 * (it only installs over this app when the signature is the same).
 */
class Updater(context: Context, private val versionName: String, private val versionCode: Int) {
    private val app = context.applicationContext
    private val p = app.getSharedPreferences("kotora_update", Context.MODE_PRIVATE)

    var enabled by mutableStateOf(p.getBoolean("check", true))
        private set
    var lastCheck by mutableStateOf(p.getLong("last_check", 0L).takeIf { it > 0 })
        private set
    /** The last check's answer ("Kotora is up to date.", …), shown under Check now. */
    var result by mutableStateOf<String?>(null)
        private set
    var available by mutableStateOf<AndroidRelease?>(null)
        private set
    var step by mutableStateOf<UpdateStep>(UpdateStep.Idle)
        private set

    fun changeEnabled(on: Boolean) {
        enabled = on
        p.edit().putBoolean("check", on).apply()
    }

    /** At start: a quiet check when it's on and the last one is a day old. Returns a newer release, once. */
    suspend fun quietCheck(nowSeconds: Long): AndroidRelease? {
        if (!enabled || step != UpdateStep.Idle) return null
        val last = lastCheck
        if (last != null && nowSeconds - last < DAY && nowSeconds >= last) return null
        val found = check(nowSeconds, manual = false) ?: return null
        if (p.getString("skipped", null) == found.version) return null
        return found
    }

    /** Settings → Check now. */
    fun checkNow(scope: CoroutineScope, nowSeconds: Long) {
        if (step != UpdateStep.Idle) return
        scope.launch { check(nowSeconds, manual = true) }
    }

    fun skip(release: AndroidRelease) {
        p.edit().putString("skipped", release.version).apply()
        available = null
    }

    private suspend fun check(nowSeconds: Long, manual: Boolean): AndroidRelease? {
        step = UpdateStep.Checking
        val found = withContext(Dispatchers.IO) { runCatching { newest(fetch(RELEASES_URL)) } }
        step = UpdateStep.Idle
        lastCheck = nowSeconds
        p.edit().putLong("last_check", nowSeconds).apply()
        val release = found.getOrElse {
            if (manual) result = CHECK_FAILED
            return null
        }
        val newer = release?.takeIf { it.code > versionCode }
        available = newer
        result = if (newer == null) UP_TO_DATE else null
        return newer
    }

    /** Download: the APK to `cache/updates`, then the package installer. */
    fun download(scope: CoroutineScope, release: AndroidRelease) {
        if (step is UpdateStep.Downloading) return
        if (!canInstall()) {
            step = UpdateStep.NeedsPermission
            return
        }
        scope.launch {
            step = UpdateStep.Downloading(0)
            val file = withContext(Dispatchers.IO) { runCatching { save(release) } }
            step = UpdateStep.Idle
            file.fold(
                onSuccess = { install(it) },
                onFailure = { result = "Couldn’t download Kotora ${release.version}: ${it.message ?: "network error"}" },
            )
        }
    }

    /** "Allow from this source" in Android's settings, for Kotora. */
    fun openInstallPermission() {
        step = UpdateStep.Idle
        val i = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${app.packageName}"))
        app.startActivity(i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private fun canInstall(): Boolean = app.packageManager.canRequestPackageInstalls()

    private fun save(release: AndroidRelease): File {
        val dir = app.cacheDir.resolve("updates").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val part = dir.resolve("${release.apkName}.part")
        val conn = open(release.apkUrl, "application/octet-stream")
        try {
            if (conn.responseCode != 200) error("HTTP ${conn.responseCode}")
            val total = conn.contentLengthLong.takeIf { it > 0 } ?: release.size
            conn.inputStream.use { input ->
                part.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    var done = 0L
                    var shown = -1
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        done += n
                        val pct = if (total > 0) (done * 100 / total).toInt().coerceIn(0, 100) else 0
                        if (pct != shown) { shown = pct; step = UpdateStep.Downloading(pct) }
                    }
                }
            }
        } finally {
            conn.disconnect()
        }
        if (release.size > 0 && part.length() != release.size) error("the file is incomplete")
        // Only an APK of this app, and newer than this one.
        val info = @Suppress("DEPRECATION") app.packageManager.getPackageArchiveInfo(part.path, 0)
            ?: error("the file isn’t an app")
        val code = if (Build.VERSION.SDK_INT >= 28) info.longVersionCode else @Suppress("DEPRECATION") info.versionCode.toLong()
        if (info.packageName != app.packageName) error("the file is another app")
        if (code <= versionCode) error("the file isn’t newer")
        val apk = dir.resolve(release.apkName)
        if (!part.renameTo(apk)) error("couldn’t save the file")
        return apk
    }

    private fun install(apk: File) {
        val uri = FileProvider.getUriForFile(app, "${app.packageName}.updates", apk)
        val i = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        app.startActivity(i)
    }

    private fun open(url: String, accept: String): HttpURLConnection =
        (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 15_000
            readTimeout = 30_000
            instanceFollowRedirects = true
            setRequestProperty("Accept", accept)
            setRequestProperty("User-Agent", "Kotora-Android/$versionName")
        }

    private fun fetch(url: String): String {
        val conn = open(url, "application/vnd.github+json")
        try {
            if (conn.responseCode != 200) error("HTTP ${conn.responseCode}")
            return conn.inputStream.bufferedReader().use { it.readText() }
        } finally {
            conn.disconnect()
        }
    }

    companion object {
        const val DAY = 86_400L
        const val UP_TO_DATE = "Kotora is up to date."
        const val CHECK_FAILED = "Couldn’t check for updates."
        const val TAG_PREFIX = "android-v"
        const val RELEASES_URL = "https://api.github.com/repos/hanami117/kotora/releases?per_page=30"
        const val CHECK_NOTE = "Once a day, asks GitHub whether a new version of Kotora for Android is out, and says so. It sends only the Kotora version. Nothing is downloaded until you tap Download."

        /** 0.5.0 -> 500, 1.2.13 -> 10213: the versionCode the build gives that version (build.gradle.kts). */
        fun versionCode(version: String): Int? {
            val parts = version.split('.', '-').take(3).map { it.toIntOrNull() ?: return null }
            if (parts.size != 3) return null
            return parts[0] * 10000 + parts[1] * 100 + parts[2]
        }

        /** The newest published `android-v…` release with an APK, from GitHub's release list. */
        fun newest(json: String): AndroidRelease? {
            val arr = JSONArray(json)
            var best: AndroidRelease? = null
            for (i in 0 until arr.length()) {
                val r = arr.getJSONObject(i)
                if (r.optBoolean("draft") || r.optBoolean("prerelease")) continue
                val tag = r.optString("tag_name")
                if (!tag.startsWith(TAG_PREFIX)) continue
                val version = tag.removePrefix(TAG_PREFIX)
                val code = versionCode(version) ?: continue
                val assets = r.optJSONArray("assets") ?: continue
                val apk = (0 until assets.length()).map { assets.getJSONObject(it) }
                    .firstOrNull { it.optString("name").endsWith(".apk") } ?: continue
                if (best == null || code > best.code) {
                    best = AndroidRelease(version, code, apk.optString("browser_download_url"), apk.optString("name"), apk.optLong("size"), r.optString("html_url"))
                }
            }
            return best
        }
    }
}
