package io.github.hanami117.kotora.ui

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewModelScope
import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.ServiceDetails
import io.github.hanami117.kotora.core.ServiceLink
import io.github.hanami117.kotora.core.TitleStub
import io.github.hanami117.kotora.core.Row as ListRow
import io.github.hanami117.kotora.data.details
import io.github.hanami117.kotora.ui.theme.Kotora
import kotlinx.coroutines.launch

// The details page's service information, as the desktop's (`app/src/details.rs`): the stats row,
// the other titles and synonyms, Synopsis and Information, the loading and offline states, and the
// "Your list" card of a title that is not on the list, with its add buttons.

/** The service's information as the page has it: the saved copy, then the service's answer. */
data class DetailsInfoUi(
    val info: ServiceDetails? = null,
    val loading: Boolean = false,
    val error: String? = null,
)

/** The saved copy at once, then the service's answer; a failed refresh of a shown copy only ends the loading. */
suspend fun AppViewModel.loadDetails(kind: MediaKind, id: UInt, now: DetailsInfoUi, set: (DetailsInfoUi) -> Unit) {
    val d = repo?.details() ?: return
    var ui = now.copy(error = null)
    if (ui.info == null) {
        runCatching { d.savedDetails(kind, id) }.getOrNull()?.let { saved ->
            ui = ui.copy(info = saved)
            if (saved.fresh) return set(ui)
        }
    }
    set(ui.copy(loading = true))
    ui = try {
        ui.copy(info = d.fetchDetails(kind, id), loading = false, error = null)
    } catch (e: Exception) {
        ui.copy(loading = false, error = if (ui.info == null) messageOf(e) else null)
    }
    set(ui)
}

/** "Add as …" for a title not on the list (details page, Airing): saved at once, sent through the queue. */
fun AppViewModel.addTitle(stub: TitleStub, status: ListStatus) {
    val d = repo?.details() ?: return
    viewModelScope.launch {
        try {
            d.addTitle(stub, status)
            reload()
            flushSoon()
        } catch (e: Exception) {
            snack(Snack(messageOf(e)))
        }
    }
}

/** What a title not on the list is known by before its details come (Search, Airing). */
object Stubs {
    private val known = mutableMapOf<Pair<MediaKind, UInt>, TitleStub>()
    fun put(stub: TitleStub) { known[stub.kind to stub.id] = stub }
    fun get(kind: MediaKind, id: UInt): TitleStub? = known[kind to id]
}

/** Score, rank, popularity, members. */
@Composable
fun DetailsStats(info: ServiceDetails, modifier: Modifier = Modifier) {
    val k = Kotora.colors
    Row(modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for (s in info.stats) {
            val shape = RoundedCornerShape(10.dp)
            Column(Modifier.background(k.surface, shape).border(1.dp, k.border, shape).padding(horizontal = 12.dp, vertical = 8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (s.star) Text("★", color = k.accent, fontSize = 13.sp)
                    Text(s.value, color = k.text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                }
                Text(s.label, color = k.faint, fontSize = 11.5.sp)
            }
        }
    }
}

/** The other titles (English, Japanese) and "Also: synonyms". */
@Composable
fun DetailsTitles(info: ServiceDetails, shown: List<String>) {
    val k = Kotora.colors
    val others = info.otherTitles.filter { it !in shown }
    if (others.isEmpty() && info.also == null) return
    SelectionContainer {
        Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            for (t in others) Text(t, color = k.dim, fontSize = 13.5.sp, fontWeight = FontWeight.Medium)
            info.also?.let { Text(it, color = k.dim, fontSize = 12.5.sp) }
        }
    }
}

/** The service's tags: type, episodes, airing (lit), season. */
@Composable
fun DetailsTags(info: ServiceDetails) {
    FlowRow(Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for (t in info.tags) Tag(t, if (info.running && (t == "Airing" || t == "Publishing")) TagKind.AIR else TagKind.PLAIN)
    }
}

/** Synopsis and Information, or the error with Try again, or the skeleton while loading. */
@Composable
fun DetailsBody(ui: DetailsInfoUi, service: String, onRetry: () -> Unit) {
    val k = Kotora.colors
    val i = ui.info
    Column(Modifier.fillMaxWidth().padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        when {
            i != null -> {
                if (ui.loading) Text("Updating…", color = k.faint, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 16.dp))
                var more by remember { mutableStateOf(false) }
                val long = i.synopsis.length > 320
                Card {
                    Column(Modifier.animateContentSize()) {
                        CardTitle("Synopsis")
                        SelectionContainer {
                            Text(
                                i.synopsis,
                                color = k.dim,
                                fontSize = 14.sp,
                                lineHeight = 20.sp,
                                maxLines = if (more) Int.MAX_VALUE else 6,
                                modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = if (long) 6.dp else 14.dp),
                            )
                        }
                        if (long) KButton(if (more) "Less" else "More", { more = !more }, Modifier.padding(start = 4.dp, bottom = 6.dp), ButtonKind.GHOST, 32.dp, fontSize = 13.sp)
                    }
                }
                Card {
                    Column {
                        CardTitle("Information")
                        Column(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            i.facts.forEachIndexed { n, f ->
                                if (n == 1 || (i.kind == MediaKind.MANGA && f.label == "Chapters")) Genres(i.genres)
                                Fact(f.label) { Text(f.value, color = k.text, fontSize = 14.sp) }
                            }
                            if (i.facts.size < 2) Genres(i.genres)
                        }
                    }
                }
            }
            ui.error != null -> {
                val shape = RoundedCornerShape(10.dp)
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp).background(k.dropped.copy(alpha = .08f), shape).border(1.dp, k.dropped.copy(alpha = .3f), shape)
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(ui.error, color = k.text, fontSize = 14.sp, modifier = Modifier.weight(1f))
                    KButton("Try again", onRetry, kind = ButtonKind.GHOST, height = 36.dp)
                }
            }
            else -> Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Loading from $service…", color = k.faint, fontSize = 12.sp)
                for (n in 0 until 3) Box(Modifier.fillMaxWidth().height(if (n == 2) 90.dp else 36.dp).background(k.raised, RoundedCornerShape(10.dp)))
            }
        }
    }
}

@Composable
private fun CardTitle(t: String) {
    Text(t, color = Kotora.colors.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 8.dp))
}

@Composable
private fun Genres(genres: List<String>) {
    if (genres.isEmpty()) return
    Fact("Genres") {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) { for (g in genres) Tag(g) }
    }
}

@Composable
private fun Fact(label: String, value: @Composable () -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(label, color = Kotora.colors.faint, fontSize = 13.sp, modifier = Modifier.width(96.dp))
        Box(Modifier.weight(1f)) { value() }
    }
}

/** "Your list" for a title that is not on it: "Add as Watching", then "Add as" the other statuses, as the desktop's details page. */
@Composable
fun AddToListCard(kind: MediaKind, service: String, onAdd: (ListStatus) -> Unit) {
    val k = Kotora.colors
    Card {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Your list", color = k.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                Text(service, color = k.faint, fontSize = 12.sp)
            }
            Text("Not on your list.", color = k.dim, fontSize = 14.sp)
            KButton("Add as ${statusLabel(ListStatus.WATCHING, kind)}", { onAdd(ListStatus.WATCHING) }, Modifier.fillMaxWidth(), ButtonKind.PRIMARY, 44.dp)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                for (st in listOf(ListStatus.PLAN_TO_WATCH, ListStatus.COMPLETED, ListStatus.ON_HOLD, ListStatus.DROPPED)) {
                    val label = statusLabel(st, kind)
                    Row(
                        Modifier.height(32.dp).clip(CircleShape).border(1.dp, k.borderStrong, CircleShape)
                            .clickable(role = Role.Button) { onAdd(st) }.semantics { contentDescription = "Add as $label" }.padding(horizontal = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        StatusDot(st)
                        Text(label, color = k.dim, fontSize = 13.sp)
                    }
                }
            }
            Text("It goes to the sync queue first, then to $service.", color = k.faint, fontSize = 12.sp)
        }
    }
}

/** The details of a title not on the list (Search, Airing, a notification, a pasted id): the service's information and the add. */
@Composable
fun NotOnListScreen(
    kind: MediaKind,
    id: UInt,
    stub: TitleStub?,
    ui: DetailsInfoUi,
    account: Account?,
    onBack: () -> Unit,
    onAdd: (TitleStub, ListStatus) -> Unit,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    links: List<ServiceLink> = emptyList(),
    onOpenUrl: (String) -> Unit = {},
) {
    val k = Kotora.colors
    val i = ui.info
    val title = i?.title ?: stub?.title ?: ""
    val cover = i?.coverUrl ?: stub?.coverUrl
    val service = account?.let { serviceTitle(it.service) } ?: "MyAnimeList"
    val asRow = ListRow(kind, id, title, null, ListStatus.PLAN_TO_WATCH, 0u, i?.episodes ?: stub?.episodes, 0u, i?.volumes, null, i?.format ?: stub?.format ?: "", i?.airStatus ?: "", null, cover, false, 0u, false, 0)
    val newStub = TitleStub(kind, id, title, cover, i?.format ?: stub?.format ?: "", i?.episodes ?: stub?.episodes)
    Column(modifier.fillMaxSize().background(k.bg).verticalScroll(rememberScrollState())) {
        Box(Modifier.fillMaxWidth().height(250.dp)) {
            Box(Modifier.fillMaxSize().blur(30.dp).alpha(.55f).background(tileBrush(id.toInt())))
            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(.3f to Color.Transparent, 1f to k.bg)))
            Row(Modifier.statusBarsPadding().fillMaxWidth().height(56.dp).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack, modifier = Modifier.clip(CircleShape).background(k.bg.copy(alpha = .6f))) { Icon(KIcons.Back, "Back", tint = k.text) }
            }
        }
        Row(Modifier.padding(horizontal = 16.dp).offset(y = (-170).dp).height(170.dp), horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.Bottom) {
            Cover(asRow, 120.dp, 170.dp, 12.dp, 30.sp)
            Text(title, color = k.text, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, lineHeight = 26.sp, modifier = Modifier.padding(bottom = 4.dp))
        }
        Column(Modifier.offset(y = (-170).dp)) {
            if (i != null) {
                DetailsTags(i)
                DetailsStats(i, Modifier.padding(bottom = 12.dp))
                DetailsTitles(i, listOf(title))
            }
            if (links.isNotEmpty()) {
                Row(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (l in links) OpenOnButton(l.label, { onOpenUrl(l.url) })
                }
            }
            AddToListCard(kind, service) { onAdd(newStub, it) }
            DetailsBody(ui, service, onRetry)
            Box(Modifier.size(1.dp, 40.dp))
        }
    }
}
