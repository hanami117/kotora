package io.github.hanami117.kotora.data

import android.content.Context
import android.util.Log
import io.github.hanami117.kotora.BuildConfig
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.AiringMine
import io.github.hanami117.kotora.core.AiringType
import io.github.hanami117.kotora.core.AiringWeek
import io.github.hanami117.kotora.core.ClientIds
import io.github.hanami117.kotora.core.Edit
import io.github.hanami117.kotora.core.FlushReport
import io.github.hanami117.kotora.core.Kotora
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.LogLevel
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Note
import io.github.hanami117.kotora.core.NotifySettings
import io.github.hanami117.kotora.core.Paths
import io.github.hanami117.kotora.core.PendingSignIn
import io.github.hanami117.kotora.core.Platform
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.Sort
import io.github.hanami117.kotora.core.StatusCount
import io.github.hanami117.kotora.core.SyncReport
import java.io.File
import java.time.Instant
import java.time.ZoneId
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The real thing: `kotora-mobile` (Rust) through UniFFI. One per process (the Application's).
 *
 * Files: the database and the tokens in `noBackupFilesDir` (never backed up), `config.toml` in
 * `filesDir/config`, caches in `cacheDir/kotora`.
 */
class CoreRepository(context: Context) : Repository, AccountRepository {
    private val app = context.applicationContext
    private val vault = KeystoreVault(File(app.noBackupFilesDir, "tokens"))
    private val kotora: Kotora = Kotora(
        Paths(
            configDir = File(app.filesDir, "config").path,
            dataDir = app.noBackupFilesDir.path,
            cacheDir = File(app.cacheDir, "kotora").path,
        ),
        ClientIds(mal = BuildConfig.MAL_CLIENT_ID, anilist = BuildConfig.ANILIST_CLIENT_ID),
        AndroidPlatform(vault),
    )

    override val isSample = false

    override fun hasClientId(service: Service) = kotora.hasClientId(service)

    private suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    override suspend fun accounts(): List<Account> = io { kotora.accounts() }
    override suspend fun list(kind: MediaKind, status: ListStatus?, sort: Sort): List<Row> = io { kotora.list(kind, status, sort) }
    override suspend fun counts(kind: MediaKind): List<StatusCount> = io { kotora.counts(kind) }
    override suspend fun entry(kind: MediaKind, id: UInt): Row? = io { kotora.entry(kind, id) }
    override suspend fun syncedAt(kind: MediaKind): Long? = io { kotora.syncedAt(kind) }
    override suspend fun waitingCount(): UInt = io { kotora.waitingCount() }
    override suspend fun mangaEnabled(): Boolean = io { kotora.mangaEnabled() }
    override suspend fun setMangaEnabled(on: Boolean) = io { kotora.setMangaEnabled(on) }
    override suspend fun plusOne(kind: MediaKind, id: UInt): Edit = io { kotora.plusOne(kind, id) }
    override suspend fun undo(edit: Edit) = io { kotora.undo(edit) }
    override suspend fun sync(): SyncReport = io { kotora.sync() }
    override suspend fun flush(): FlushReport = io { kotora.flushQueue() }
    override suspend fun signInStart(service: Service): PendingSignIn = io { kotora.signInStart(service) }
    override suspend fun signInFinish(pending: PendingSignIn, redirect: String): Account = io { kotora.signInFinish(pending, redirect) }
    override suspend fun signOut(service: Service) = io { kotora.signOut(service) }
    override suspend fun airingWeek(type: AiringType, mine: AiringMine): AiringWeek = io { kotora.airingWeek(type, mine) }
    override suspend fun airingFetch(force: Boolean): Boolean = io { kotora.airingFetch(force) }
    override suspend fun notifySettings(): NotifySettings = io { kotora.notifySettings() }
    override suspend fun setNotifySettings(settings: NotifySettings) = io { kotora.setNotifySettings(settings) }
    override suspend fun airNotes(): List<Note> = io { kotora.airNotes() }
    override suspend fun chapterNotes(): List<Note> = io { kotora.chapterNotes() }

    override suspend fun profile(service: Service) = io { kotora.profile(service) }
    override fun serviceLinks(kind: MediaKind, id: UInt) = kotora.serviceLinks(kind, id)
    override suspend fun signInWithToken(raw: String): Account = io { kotora.signInWithToken(raw) }
}

/** The core's window on Android: the user's time zone, the log, the Keystore-wrapped tokens. */
private class AndroidPlatform(private val vault: KeystoreVault) : Platform {
    override fun utcOffset(atUnix: Long): Int =
        ZoneId.systemDefault().rules.getOffset(Instant.ofEpochSecond(atUnix)).totalSeconds

    override fun log(level: LogLevel, message: String) {
        val priority = when (level) {
            LogLevel.DEBUG -> Log.DEBUG
            LogLevel.INFO -> Log.INFO
            LogLevel.WARN -> Log.WARN
            LogLevel.ERROR -> Log.ERROR
        }
        Log.println(priority, "kotora", message)
    }

    override fun tokenLoad(service: String): ByteArray? = vault.load(service)
    override fun tokenSave(service: String, bytes: ByteArray): Boolean = vault.save(service, bytes)
    override fun tokenDelete(service: String) = vault.delete(service)
}
