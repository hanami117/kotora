package io.github.hanami117.kotora.data

import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.Edit
import io.github.hanami117.kotora.core.FlushReport
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.PendingSignIn
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.Sort
import io.github.hanami117.kotora.core.StatusCount
import io.github.hanami117.kotora.core.SyncReport

/**
 * What the screens need: the core ([CoreRepository]) or made-up data ([SampleRepository], for the
 * screenshot tests and "Look around" in debug builds). Every call is blocking-safe to call from
 * a coroutine: the implementations switch to [kotlinx.coroutines.Dispatchers.IO] themselves.
 *
 * Errors come as [io.github.hanami117.kotora.core.KotoraException], whose message is the text to show.
 */
interface Repository {
    val isSample: Boolean

    fun hasClientId(service: Service): Boolean

    suspend fun accounts(): List<Account>
    suspend fun list(kind: MediaKind, status: ListStatus?, sort: Sort): List<Row>
    suspend fun counts(kind: MediaKind): List<StatusCount>
    suspend fun entry(kind: MediaKind, id: UInt): Row?
    suspend fun syncedAt(kind: MediaKind): Long?
    suspend fun waitingCount(): UInt
    suspend fun mangaEnabled(): Boolean
    suspend fun setMangaEnabled(on: Boolean)

    suspend fun plusOne(kind: MediaKind, id: UInt): Edit
    suspend fun undo(edit: Edit)

    /** Network. */
    suspend fun sync(): SyncReport
    /** Network. */
    suspend fun flush(): FlushReport

    suspend fun signInStart(service: Service): PendingSignIn
    /** Network. */
    suspend fun signInFinish(pending: PendingSignIn, redirect: String): Account
    suspend fun signOut(service: Service)
}
