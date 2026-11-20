package io.github.hanami117.kotora.data

import io.github.hanami117.kotora.core.Hit
import io.github.hanami117.kotora.core.InfoFact
import io.github.hanami117.kotora.core.InfoStat
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.PastedId
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.ServiceDetails
import io.github.hanami117.kotora.core.TitleStub
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/**
 * The details page's service information (the desktop's details.rs) and the add of a title that is
 * not on the list (details page, Airing, a notification, a pasted id). Next to [Repository], so
 * other batches can grow that one without touching this.
 */
interface DetailsRepository {
    /** Local: the saved copy, shown at once. */
    suspend fun savedDetails(kind: MediaKind, id: UInt): ServiceDetails?
    /** Network. */
    suspend fun fetchDetails(kind: MediaKind, id: UInt): ServiceDetails
    suspend fun addTitle(stub: TitleStub, status: ListStatus)
    /** A pasted MyAnimeList/AniList link (of the active service) or a bare id. */
    fun pastedId(kind: MediaKind, text: String): PastedId?
}

/** The [DetailsRepository] of a repository. */
fun Repository.details(): DetailsRepository? = when (this) {
    is CoreRepository -> CoreDetails(this)
    is SampleRepository -> SampleDetails(this)
    else -> null
}

private class CoreDetails(private val repo: CoreRepository) : DetailsRepository {
    private val k get() = repo.kotora
    private suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { block() }

    override suspend fun savedDetails(kind: MediaKind, id: UInt) = io { k.savedDetails(kind, id) }
    override suspend fun fetchDetails(kind: MediaKind, id: UInt) = io { k.fetchDetails(kind, id) }
    override suspend fun addTitle(stub: TitleStub, status: ListStatus) = io { k.addTitle(stub, status) }
    override fun pastedId(kind: MediaKind, text: String) = k.pastedId(kind, text)
}

/** Made-up information for the sample data and the screenshot tests. */
class SampleDetails(private val repo: SampleRepository, private val pause: Long = 400) : DetailsRepository {
    override suspend fun savedDetails(kind: MediaKind, id: UInt): ServiceDetails? = null

    override suspend fun fetchDetails(kind: MediaKind, id: UInt): ServiceDetails {
        if (pause > 0) delay(pause)
        return sampleDetails(kind, id, repo.rows.firstOrNull { it.kind == kind && it.id == id }?.let { it.title to it.coverUrl }
            ?: SampleEditing.sampleHits(kind).firstOrNull { it.id == id }?.let { it.title to it.coverUrl }, repo.rows.any { it.kind == kind && it.id == id })
    }

    override suspend fun addTitle(stub: TitleStub, status: ListStatus) {
        val hit = Hit(
            stub.kind, stub.id, stub.title, null, null, emptyList(), stub.format, stub.episodes, null, "", null, null, null,
            stub.coverUrl, emptyList(), null, "", "",
        )
        repo.editing()?.add(hit, status)
    }

    override fun pastedId(kind: MediaKind, text: String): PastedId? {
        val t = text.trim()
        t.toUIntOrNull()?.takeIf { it > 0u && t.length <= 9 }?.let { return PastedId(it, false) }
        val part = "/${if (kind == MediaKind.MANGA) "manga" else "anime"}/"
        if (!t.contains("myanimelist.net$part")) return null
        return t.substringAfter(part).takeWhile { it.isDigit() }.toUIntOrNull()?.takeIf { it > 0u }?.let { PastedId(it, true) }
    }
}

/** The desktop's made-up details (`details::fake_details`), in the page's words. */
fun sampleDetails(kind: MediaKind, id: UInt, known: Pair<String, String?>?, onList: Boolean): ServiceDetails {
    val title = known?.first ?: "Lanterns of Kirigaoka"
    val manga = kind == MediaKind.MANGA
    val facts = if (manga) {
        listOf(
            InfoFact("Published", "Since Apr 4, 2021"), InfoFact("Authors", "Aoi Mori (Story & Art)"), InfoFact("Serialization", "Monthly Lantern"),
            InfoFact("Chapters", "Unknown while publishing"), InfoFact("Volumes", "Unknown while publishing"),
        )
    } else {
        listOf(InfoFact("Aired", "Since Oct 3, 2026"), InfoFact("Studios", "Studio Hoshi"), InfoFact("Source", "Light novel"), InfoFact("Length", "24 min per episode"))
    }
    return ServiceDetails(
        kind, id, Service.MAL, title, null, "霧ヶ丘の提灯", listOf("霧ヶ丘の提灯"), "Also: Kirigaoka Lanterns", if (manga) "manga" else "tv",
        if (manga) listOf("Manga", "Publishing") else listOf("TV", "12 episodes", "Airing", "Fall 2026"), true,
        if (manga) null else 12u, null, if (manga) "currently_publishing" else "currently_airing", 8.31f,
        listOf(InfoStat("8.31", "MAL", true), InfoStat("#412", "ranked", false), InfoStat("#1,208", "popular", false), InfoStat("182,340", "members", false)),
        "Every autumn the hillside town of Kirigaoka hangs a thousand paper lanterns for the festival of returning lights. This year Hana, the lantern maker's daughter, finds one that glows on its own.",
        facts, listOf("Drama", "Slice of Life", "Supernatural"), known?.second, null, 1_795_000_000L, true, onList,
    )
}
