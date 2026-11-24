package io.github.hanami117.kotora.ui

import androidx.lifecycle.viewModelScope
import io.github.hanami117.kotora.core.EntryEditor
import io.github.hanami117.kotora.core.Hit
import io.github.hanami117.kotora.core.KotoraException
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.QueueItem
import io.github.hanami117.kotora.core.Row
import io.github.hanami117.kotora.data.Editing
import io.github.hanami117.kotora.data.editing
import kotlinx.coroutines.launch

/**
 * The editor's, the details page's and the queue's actions (`mobile/src/edit.rs`, `remove.rs`,
 * `search.rs`), next to [AppViewModel] rather than in it: each one saves at once, lets the queue
 * go, reads the list again, and says what it did with Undo where the desktop has one.
 */
val AppViewModel.editing: Editing? get() = repo?.editing()

fun messageOf(e: Throwable): String = when (e) {
    is KotoraException -> e.message?.takeIf { it.isNotBlank() } ?: "Something went wrong."
    else -> e.message ?: e.javaClass.simpleName
}

private fun AppViewModel.act(block: suspend (Editing) -> Snack?) {
    val e = editing ?: return
    viewModelScope.launch {
        try {
            val s = block(e)
            reload()
            flushSoon()
            s?.let(::snack)
        } catch (ex: Exception) {
            snack(Snack(messageOf(ex)))
        }
    }
}

/** Rewatch from episode 1 (Reread from chapter 1), or Stop rewatching; Undo puts it back. */
fun AppViewModel.rewatch(row: Row, start: Boolean) = act {
    val edit = if (start) it.startRewatch(row.kind, row.id) else it.stopRewatch(row.kind, row.id)
    Snack(edit.message, edit)
}

/** −1 ("One less"), with Undo. */
fun AppViewModel.minusOne(row: Row) = act {
    val edit = it.minusOne(row.kind, row.id)
    Snack(edit.message, edit)
}

/** Move to › (the desktop's right-click menu): at once, with Undo. */
fun AppViewModel.moveTo(row: Row, status: ListStatus) = act { e ->
    e.moveTo(row.kind, row.id, status)?.let { Snack(it.message, it) }
}

/** Score › a score or "No score": at once, with Undo. */
fun AppViewModel.setScore(row: Row, points: UInt?) = act { e ->
    e.setScore(row.kind, row.id, points)?.let { Snack(it.message, it) }
}

/** Remove from list (asked first by the page): Undo discards the waiting removal. */
fun AppViewModel.remove(row: Row) = act { e ->
    val r = e.remove(row.kind, row.id)
    Snack(r.message, onUndo = { act { it.discard(r.service, r.opId); null } })
}

/** The editor's Save: one change in the queue. `true`: the page may close. */
suspend fun AppViewModel.saveEditor(ed: EntryEditor): Boolean {
    val e = editing ?: return false
    return try {
        val said = e.saveEditor(ed)
        reload()
        if (said != null) {
            flushSoon()
            snack(Snack(said))
        }
        true
    } catch (ex: Exception) {
        snack(Snack(messageOf(ex)))
        false
    }
}

/** Search's "+ Watching" and "Add as …": saved at once, sent through the queue. */
fun AppViewModel.add(hit: Hit, status: ListStatus) = act {
    it.add(hit, status)
    null
}

enum class QueueAction { KEEP_MINE, TAKE_THEIRS, DISCARD }

/** A queue card's button. */
fun AppViewModel.queue(action: QueueAction, item: QueueItem) = act {
    when (action) {
        QueueAction.KEEP_MINE -> it.keepMine(item)
        QueueAction.TAKE_THEIRS -> it.takeTheirs(item)
        QueueAction.DISCARD -> it.discard(item.service, item.id)
    }
    null
}

/** The queue page's "Sync now": waiting changes may go at once, then the list is fetched. */
fun AppViewModel.syncNow() {
    val e = editing ?: return
    viewModelScope.launch {
        runCatching { e.retryNow() }
        sync()
    }
}
