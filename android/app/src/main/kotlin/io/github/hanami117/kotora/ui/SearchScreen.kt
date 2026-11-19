package io.github.hanami117.kotora.ui

import io.github.hanami117.kotora.core.TitleStub
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.Hit
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.core.Sort
import io.github.hanami117.kotora.data.Editing
import io.github.hanami117.kotora.data.Repository
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * The search page's state, as the desktop's (`app/src/search.rs`) and iOS's: the user's own
 * entries that match (every status) at once, then the service's results that are not on the
 * list, asked after a pause in typing, for 3 letters or more, kept 10 minutes (the last 20).
 */
@Stable
class SearchModel {
    var text by mutableStateOf("")
    var kind by mutableStateOf(MediaKind.ANIME)
    var hits by mutableStateOf<List<Hit>>(emptyList())
        internal set
    var asked by mutableStateOf("")
        internal set
    var pending by mutableStateOf(false)
        internal set
    var error by mutableStateOf<String?>(null)
        internal set
    /** The whole list of [kind], for "On your list" and to leave out what is on it. */
    var mine by mutableStateOf<List<ListRow>>(emptyList())

    private data class Cached(val kind: MediaKind, val query: String, val at: Long, val hits: List<Hit>)
    private val cache = ArrayDeque<Cached>()
    private var job: Job? = null

    val query: String get() = norm(text)

    /** "On your list": every word in the title or the English title, as the desktop's filter. */
    val onList: List<ListRow>
        get() {
            val words = query.split(' ').filter { it.isNotEmpty() }
            if (words.isEmpty()) return emptyList()
            return mine.filter { r ->
                val t = (r.title + "\n" + (r.titleEn ?: "")).lowercase()
                words.all { t.contains(it) }
            }
        }

    /** Results that are not on the list (those are shown above them). */
    val newHits: List<Hit> get() = hits.filter { h -> mine.none { it.id == h.id } }

    suspend fun loadMine(repo: Repository?) {
        mine = runCatching { repo?.list(kind, null, Sort.TITLE) }.getOrNull() ?: emptyList()
    }

    /** Every edit of the box (and the Anime | Manga switch): the service after a pause. */
    fun changed(scope: CoroutineScope, editing: Editing?, now: Long, force: Boolean = false) {
        val q = query
        val k = kind
        if (!force && q == asked && (pending || error != null || hits.isNotEmpty())) return
        job?.cancel()
        asked = q
        error = null
        if (q.length < 3 || editing == null) {
            hits = emptyList()
            pending = false
            return
        }
        cache.removeAll { now - it.at >= CACHE_SECS }
        cache.firstOrNull { it.kind == k && it.query == q }?.let {
            hits = it.hits
            pending = false
            return
        }
        hits = emptyList()
        pending = true
        job = scope.launch {
            delay(DEBOUNCE_MS)
            try {
                val found = editing.search(k, q)
                cache.removeAll { it.kind == k && it.query == q }
                cache.addLast(Cached(k, q, now, found))
                if (cache.size > CACHE_MAX) cache.removeFirst()
                hits = found
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                error = messageOf(e)
            }
            pending = false
        }
    }

    companion object {
        const val DEBOUNCE_MS = 400L
        const val CACHE_SECS = 600L
        const val CACHE_MAX = 20

        fun norm(q: String) = q.split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ").lowercase()
    }
}

/**
 * Search: Anime | Manga (Manga on), the box, "On your list", then "On MyAnimeList" (AniList)
 * with "+ Watching" and the other statuses in a menu, as the desktop's search page.
 */
@Composable
fun SearchScreen(
    s: SearchModel,
    account: Account?,
    mangaEnabled: Boolean,
    onKind: (MediaKind) -> Unit,
    onText: (String) -> Unit,
    onAgain: () -> Unit,
    onPlusOne: (ListRow) -> Unit,
    onOpen: (ListRow) -> Unit,
    onAdd: (Hit, ListStatus) -> Unit,
    /** A service result: its details page. */
    onOpenHit: (Hit) -> Unit = {},
    /** A pasted MyAnimeList/AniList link or a bare id in the box (the active service's). */
    pastedId: (MediaKind, String) -> UInt? = { _, _ -> null },
    onOpenId: (MediaKind, UInt) -> Unit = { _, _ -> },
    /** Why the service isn't searched now (signed out of it, offline): the desktop's `blocked`. */
    blocked: String? = null,
    modifier: Modifier = Modifier,
) {
    val k = Kotora.colors
    val service = account?.let { serviceTitle(it.service) } ?: "MyAnimeList"
    Column(modifier.fillMaxSize().background(k.bg)) {
        TopBar("Search", account, false, null)
        if (mangaEnabled) {
            Segmented(
                listOf("Anime", "Manga"),
                if (s.kind == MediaKind.ANIME) 0 else 1,
                { onKind(if (it == 0) MediaKind.ANIME else MediaKind.MANGA) },
                Modifier.padding(start = 16.dp, end = 16.dp, bottom = 10.dp),
            )
        }
        SearchField(s.text, if (s.kind == MediaKind.MANGA) "Search manga" else "Search anime", onText)
        val q = s.text.trim()
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 80.dp)) {
            if (q.isEmpty()) {
                item {
                    Text(
                        if (s.kind == MediaKind.MANGA) "Find a manga on your list or on $service, and add it." else "Find an anime on your list or on $service, and add it.",
                        color = k.faint,
                        fontSize = 14.sp,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 40.dp),
                    )
                }
                return@LazyColumn
            }
            pastedId(s.kind, q)?.let { id ->
                item(key = "id-line") {
                    val known = s.mine.firstOrNull { it.id == id }?.title ?: s.hits.firstOrNull { it.id == id }?.title
                    val what = known?.let { "$service #$id: $it" } ?: "$service #$id"
                    Row(
                        Modifier.fillMaxWidth().height(44.dp).clickable {
                            s.hits.firstOrNull { it.id == id }?.let { Stubs.put(it.stub()) }
                            onOpenId(s.kind, id)
                        }.semantics { contentDescription = "Open $service ${if (s.kind == MediaKind.MANGA) "manga" else "anime"} $id" }.padding(horizontal = 16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Text("→", color = k.accentHi, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                        Text("Open $what", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            val mine = s.onList
            item { Head("On your list · ${mine.size}", null) }
            if (mine.isEmpty()) item { Note("Nothing on your list matches “$q”.") }
            items(mine, key = { "mine-${it.kind}-${it.id}" }) { row ->
                ListRowItem(row, onPlusOne = { onPlusOne(row) }, onOpen = { onOpen(row) })
                HorizontalDivider(color = k.border)
            }
            val fresh = s.newHits
            val end = when {
                s.pending -> "Searching…"
                blocked == null && s.error == null && s.asked.length >= 3 && s.hits.isNotEmpty() -> "${s.hits.size} found"
                else -> null
            }
            item { Head("On $service", end) }
            when {
                blocked != null -> item { Note(blocked) }
                s.query.length < 3 -> item { Note("Type 3 letters or more to search $service too.") }
                s.pending -> item { Note("Searching $service…") }
                s.error != null -> item {
                    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(s.error ?: "", color = k.dropped, fontSize = 14.sp, modifier = Modifier.weight(1f))
                        KButton("Try again", onAgain, kind = ButtonKind.GHOST, height = 36.dp)
                    }
                }
                fresh.isEmpty() -> item {
                    Note(if (s.hits.isEmpty()) "$service found nothing for “$q”." else "Everything $service found is on your list already.")
                }
                else -> items(fresh, key = { "hit-${it.kind}-${it.id}" }) { h ->
                    Box(Modifier.clickable { onOpenHit(h) }) { HitRow(h) { onAdd(h, it) } }
                    HorizontalDivider(color = k.border)
                }
            }
        }
    }
}

@Composable
private fun SearchField(text: String, hint: String, onText: (String) -> Unit) {
    val k = Kotora.colors
    val shape = RoundedCornerShape(10.dp)
    var focused by remember { mutableStateOf(false) }
    Row(
        Modifier.padding(start = 16.dp, end = 16.dp, bottom = 6.dp).fillMaxWidth().height(44.dp)
            .background(k.field, shape).border(1.dp, if (focused) k.accent else k.borderStrong, shape).padding(start = 12.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(KIcons.Search, null, tint = k.faint, modifier = Modifier.size(18.dp))
        Box(Modifier.weight(1f)) {
            if (text.isEmpty()) Text(hint, color = k.faint, fontSize = 15.sp)
            BasicTextField(
                text,
                onText,
                singleLine = true,
                textStyle = TextStyle(color = k.text, fontSize = 15.sp),
                cursorBrush = SolidColor(k.accent),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, imeAction = ImeAction.Search),
                modifier = Modifier.fillMaxWidth().semantics { contentDescription = hint }
                    .onFocusChanged { focused = it.isFocused },
            )
        }
        if (text.isNotEmpty()) {
            IconButton(onClick = { onText("") }, modifier = Modifier.size(36.dp)) { Icon(KIcons.Close, "Clear", tint = k.faint, modifier = Modifier.size(16.dp)) }
        }
    }
}

@Composable
private fun Head(title: String, end: String?) {
    val k = Kotora.colors
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 4.dp)) {
        Text(title, color = k.dim, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
        if (end != null) Text(end, color = k.faint, fontSize = 12.sp)
    }
}

@Composable
private fun Note(text: String) {
    Text(text, color = Kotora.colors.faint, fontSize = 14.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
}

/** A service's result: cover, title, type and what it is, the second title, and the add buttons. */
@Composable
fun HitRow(hit: Hit, add: (ListStatus) -> Unit) {
    val k = Kotora.colors
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 84.dp).padding(start = 16.dp, end = 12.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        val asRow = ListRow(hit.kind, hit.id, hit.title, hit.titleEn, ListStatus.PLAN_TO_WATCH, 0u, hit.episodes, 0u, hit.volumes, null, hit.format, hit.airStatus, hit.startDate, hit.coverUrl, false, 0u, false, 0)
        Cover(asRow, 44.dp, 64.dp, 5.dp, 14.sp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Text(hit.title, color = k.text, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                if (hit.typeLabel.isNotEmpty()) Tag(hit.typeLabel)
                Text(hit.meta, color = k.faint, fontSize = 12.5.sp, maxLines = 2)
            }
            hit.titleEn?.takeIf { it.isNotEmpty() && !it.equals(hit.title, ignoreCase = true) }?.let {
                Text(it, color = k.dim, fontSize = 12.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        val shape = RoundedCornerShape(8.dp)
        val watching = statusLabel(ListStatus.WATCHING, hit.kind)
        Row(Modifier.height(36.dp).clip(shape).border(1.dp, k.accent.copy(alpha = .6f), shape), verticalAlignment = Alignment.CenterVertically) {
            Row(
                Modifier.height(36.dp).clickable(role = Role.Button) { add(ListStatus.WATCHING) }.padding(horizontal = 10.dp)
                    .semantics { contentDescription = "Add as $watching" },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Icon(KIcons.Plus, null, tint = k.accentHi, modifier = Modifier.size(14.dp))
                Text(watching, color = k.accentHi, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            }
            Box(Modifier.width(1.dp).height(20.dp).background(k.accent.copy(alpha = .4f)))
            Box {
                Box(
                    Modifier.size(30.dp, 36.dp).clickable(role = Role.DropdownList) { menu = true }.semantics { contentDescription = "Add with another status" },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(KIcons.Down, null, tint = k.accentHi, modifier = Modifier.size(14.dp))
                }
                DropdownMenu(menu, { menu = false }, containerColor = k.surface) {
                    Text("Add as", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
                    for (st in listOf(ListStatus.PLAN_TO_WATCH, ListStatus.COMPLETED, ListStatus.ON_HOLD, ListStatus.DROPPED)) {
                        DropdownMenuItem(
                            text = { Text(statusLabel(st, hit.kind), color = k.text) },
                            leadingIcon = { StatusDot(st) },
                            onClick = {
                                menu = false
                                add(st)
                            },
                        )
                    }
                }
            }
        }
    }
}

/** What the details page shows before the service answers. */
fun Hit.stub() = TitleStub(kind, id, title, coverUrl, format, episodes)
