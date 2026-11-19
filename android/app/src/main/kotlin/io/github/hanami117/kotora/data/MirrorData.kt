package io.github.hanami117.kotora.data

import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.MirrorAlign
import io.github.hanami117.kotora.core.MirrorConflictCard
import io.github.hanami117.kotora.core.MirrorCount
import io.github.hanami117.kotora.core.MirrorEditCard
import io.github.hanami117.kotora.core.MirrorGroup
import io.github.hanami117.kotora.core.MirrorOpGroup
import io.github.hanami117.kotora.core.MirrorOpRow
import io.github.hanami117.kotora.core.MirrorPart
import io.github.hanami117.kotora.core.MirrorPick
import io.github.hanami117.kotora.core.MirrorPreview
import io.github.hanami117.kotora.core.MirrorPreviewView
import io.github.hanami117.kotora.core.MirrorQueue
import io.github.hanami117.kotora.core.MirrorSettle
import io.github.hanami117.kotora.core.MirrorState
import io.github.hanami117.kotora.core.MirrorWaitRow
import io.github.hanami117.kotora.core.Service
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Mirror edits (Beta), the desktop's mirror.rs through `kotora-mobile` (`mobile/src/mirror.rs`).
 * Next to [Repository], so other batches can grow that one without touching this. The words come
 * from the core, the same on both phones and the desktop.
 */
interface MirrorRepository {
    suspend fun state(): MirrorState?
    /** Network: reads the other service's lists and compares them. Nothing is saved yet. */
    suspend fun compare(mangaOnly: Boolean): MirrorPreview
    suspend fun choose(preview: MirrorPreview, align: MirrorAlign): String
    suspend fun stop(): String
    suspend fun queue(): MirrorQueue?
    /** Network for "Add to AniList" on an AniList main list. */
    suspend fun settle(kind: MediaKind, malId: UInt, how: MirrorSettle)
    suspend fun keepMine(id: Long)
    suspend fun takeTheirs(id: Long)
    suspend fun discard(id: Long)
}

/** The [MirrorRepository] of a repository. */
fun Repository.mirror(): MirrorRepository? = when (this) {
    is CoreRepository -> CoreMirror(this)
    is SampleRepository -> SampleMirror()
    else -> null
}

private class CoreMirror(private val repo: CoreRepository) : MirrorRepository {
    private val k get() = repo.kotora
    private suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    override suspend fun state() = io { k.mirrorState() }
    override suspend fun compare(mangaOnly: Boolean) = io { if (mangaOnly) k.mirrorCompareManga() else k.mirrorCompare() }
    override suspend fun choose(preview: MirrorPreview, align: MirrorAlign) = io { k.mirrorChoose(preview, align) }
    override suspend fun stop() = io { k.mirrorStop() }
    override suspend fun queue() = io { k.mirrorQueue() }
    override suspend fun settle(kind: MediaKind, malId: UInt, how: MirrorSettle) = io { k.mirrorSettle(kind, malId, how) }
    override suspend fun keepMine(id: Long) = io { k.mirrorKeepMine(id) }
    override suspend fun takeTheirs(id: Long) = io { k.mirrorTakeTheirs(id) }
    override suspend fun discard(id: Long) = io { k.mirrorDiscard(id) }
}

/** Sample data has one account: the row says to sign in to the other service, as the desktop. */
class SampleMirror : MirrorRepository {
    override suspend fun state() = sampleMirrorState(on = false)
    override suspend fun compare(mangaOnly: Boolean): MirrorPreview = throw IllegalStateException("Sample data: nothing is asked.")
    override suspend fun choose(preview: MirrorPreview, align: MirrorAlign): String = throw IllegalStateException("Sample data: nothing is saved.")
    override suspend fun stop() = "Off. Nothing more goes to the mirror."
    override suspend fun queue(): MirrorQueue? = null
    override suspend fun settle(kind: MediaKind, malId: UInt, how: MirrorSettle) {}
    override suspend fun keepMine(id: Long) {}
    override suspend fun takeTheirs(id: Long) {}
    override suspend fun discard(id: Long) {}
}

/** The core's words for a MyAnimeList main list and an AniList mirror (the screenshots, the sample). */
fun sampleMirrorState(on: Boolean, signedIn: Boolean = on): MirrorState = MirrorState(
    main = Service.MAL,
    to = Service.ANI_LIST,
    on = on,
    title = "Mirror edits to AniList",
    tag = "Beta",
    note = when {
        on -> "On. Every change you make here also goes to AniList. Changes made on its website are never overwritten: they show in the Queue. 2 changes waiting for AniList. 2 things to look at in the Queue."
        !signedIn -> "Sign in to AniList too: choose Use AniList above, sign in, then come back to MyAnimeList. Your MyAnimeList list stays the main one."
        else -> "Every change you make in Kotora also goes to your AniList list, through its own queue. Turning it on first compares the two lists and shows you what would change."
    },
    canTurnOn = signedIn,
    otherSignedIn = signedIn,
    alignManga = null,
    waiting = if (on) 2u else 0u,
    attention = if (on) 2u else 0u,
    badge = if (on) 3u else 0u,
    trouble = null,
    accountLine = if (on) "MyAnimeList ⇄ AniList" else "MyAnimeList",
    accountTip = if (on) "Changes also go to AniList. 3 in the Sync queue for AniList." else null,
    reading = "Reading your AniList list…",
    readingManga = "Reading your AniList manga list…",
    saving = "Saving…",
    turningOff = "Turning the mirror off…",
)

/** The desktop's preview scene (mirror.rs scenes), in the core's words. */
fun sampleMirrorPreview(): MirrorPreviewView = MirrorPreviewView(
    main = Service.MAL,
    to = Service.ANI_LIST,
    parts = listOf(
        MirrorPart(
            kind = MediaKind.ANIME,
            heading = null,
            title = "Your MyAnimeList list and your AniList list, matched by MyAnimeList id",
            counts = listOf(
                MirrorCount("only-main", 3u, "only on MyAnimeList"),
                MirrorCount("only-mirror", 1u, "only on AniList"),
                MirrorCount("different", 2u, "different"),
                MirrorCount("same", 41u, "the same"),
                MirrorCount("unmatched", 1u, "not mirrored"),
            ),
            groups = listOf(
                MirrorGroup("only-main", "Only on MyAnimeList", listOf("Hoshikuzu Relay: Watching, episode 7, score 8", "Paper Kite Diaries: Plan to watch", "Rust & Rain: Completed, episode 12")),
                MirrorGroup("only-mirror", "Only on AniList", listOf("Moonlit Harbor: Plan to watch")),
                MirrorGroup("different", "On both, different", listOf("Lantern Post Office: MyAnimeList Watching, episode 5 · AniList Completed, episode 12", "Inkwell Cartographers: MyAnimeList On hold, episode 3 · AniList Watching, episode 4")),
                MirrorGroup("unmatched", "Not mirrored", listOf("Web Short Specials: on AniList, no MyAnimeList id")),
            ),
            picksHeading = "STATUS TO PICK (WITH “MERGE”)",
            picks = listOf(
                MirrorPick(9301u, "Lantern Post Office", "MyAnimeList: Watching", "AniList: Completed", false),
                MirrorPick(9302u, "Inkwell Cartographers", "MyAnimeList: On hold", "AniList: Watching", true),
            ),
            allLine = "All of them:",
            allMainLabel = "All MyAnimeList’s",
            allMirrorLabel = "All AniList’s",
            allMainOn = false,
            allMirrorOn = false,
        ),
    ),
    explain = listOf(
        "MyAnimeList wins: AniList becomes a copy of your MyAnimeList list (5 changes). Anime only on AniList stay there.",
        "Merge: what is missing on each side is added; where they differ, the one further along wins and an empty score is filled (5 changes for AniList, 2 changes for MyAnimeList).",
        "Don’t align: nothing changes now; only edits you make from now on are mirrored.",
        "Nothing is removed from either list, and every change goes through the Queue at each service’s pace.",
    ),
    mainWins = "MyAnimeList wins",
    merge = "Merge",
    dontAlign = "Don’t align",
    cancel = "Cancel",
)

/** The desktop's queue scene: a website edit, a conflict, waiting, failed, sent, a lookup. */
fun sampleMirrorQueue(): MirrorQueue = MirrorQueue(
    heading = "AniList mirror",
    read = "read 3 min ago",
    trouble = null,
    edits = listOf(
        MirrorEditCard(
            MediaKind.ANIME, 9101u, "AniList: Moonlit Harbor", "Added on AniList; it isn’t on your MyAnimeList list.",
            "MyAnimeList", "Not on the list", null, "AniList", "Plan to watch", "Add to MyAnimeList", "Leave both as they are",
        ),
    ),
    conflicts = listOf(
        MirrorConflictCard(
            41L, "AniList: Lantern Post Office", "AniList changed after this edit. Kotora didn’t overwrite it. Pick one.",
            "From your MyAnimeList list", "Watching, episode 6", "Send MyAnimeList’s", "AniList", "Completed, episode 12, score 9", "Keep AniList’s",
        ),
    ),
    groups = listOf(
        MirrorOpGroup("waiting", "WAITING", listOf(MirrorOpRow(42L, "Hoshikuzu Relay", "Progress 6 → 7", null, false, "Waiting", false, false, "now"))),
        MirrorOpGroup("failed", "FAILED", listOf(MirrorOpRow(43L, "Paper Kite Diaries", "Score → 8", "Validation error: score", true, "Failed", true, false, "2 h"))),
        MirrorOpGroup("sent", "SENT TODAY", listOf(MirrorOpRow(44L, "Inkwell Cartographers", "Status → Watching", null, false, "Sent", false, true, "3 h"))),
    ),
    waitsHeading = "NOT ON ANILIST YET",
    waits = listOf(MirrorWaitRow(MediaKind.ANIME, 9200u, "Rust & Rain", "Looking up its AniList id")),
)
