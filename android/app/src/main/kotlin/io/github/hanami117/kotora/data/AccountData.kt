package io.github.hanami117.kotora.data

import io.github.hanami117.kotora.core.Account
import io.github.hanami117.kotora.core.ListStatus
import io.github.hanami117.kotora.core.MediaKind
import io.github.hanami117.kotora.core.ProfileCount
import io.github.hanami117.kotora.core.ProfileInfo
import io.github.hanami117.kotora.core.Service
import io.github.hanami117.kotora.core.ServiceLink

/**
 * The Profile page, the "Open on …" links and the AniList token paste (the desktop's profile.rs,
 * ctxmenu.rs/details.rs links and setup.rs token field). Next to [Repository], so other batches
 * can grow that one without touching this.
 */
interface AccountRepository {
    suspend fun profile(service: Service): ProfileInfo?
    fun serviceLinks(kind: MediaKind, id: UInt): List<ServiceLink>
    /** Network. AniList: the address the browser ended on, or the token AniList showed. */
    suspend fun signInWithToken(raw: String): Account
}

/**
 * About's links, the desktop's (settings.rs `Row::Links`, `SUPPORT_URL`); the same as the core's
 * `site_url()`/`source_url()`/`support_url()`, kept here so the screenshot tests need no native library.
 */
object Links {
    const val SITE = "https://kotora.moe"
    const val SOURCE = "https://github.com/hanami117/kotora"
    const val SUPPORT = "https://ko-fi.com/hanami117"

    /** As the core's `media_url` (for the sample data). */
    fun media(service: Service, kind: MediaKind, id: UInt): String {
        val k = if (kind == MediaKind.MANGA) "manga" else "anime"
        return if (service == Service.MAL) "https://myanimelist.net/$k/$id" else "https://anilist.co/$k/$id"
    }
}

/** The desktop's made-up profile (`profile::fake`), for the sample data and the screenshots. */
fun sampleProfile(): ProfileInfo = ProfileInfo(
    service = Service.MAL,
    userName = "sample-user",
    pictureUrl = null,
    joinedLine = "MyAnimeList · Joined Mar 2, 2019",
    openLabel = "Open on MyAnimeList",
    profileUrl = "https://myanimelist.net/profile/sample-user",
    days = 63.1,
    episodes = 3790u,
    meanScore = 7.62,
    counts = listOf(
        ProfileCount(ListStatus.WATCHING, 11u),
        ProfileCount(ListStatus.COMPLETED, 236u),
        ProfileCount(ListStatus.ON_HOLD, 7u),
        ProfileCount(ListStatus.DROPPED, 2u),
        ProfileCount(ListStatus.PLAN_TO_WATCH, 48u),
    ),
    total = 304u,
    rewatchedLine = "Rewatched 4 times",
    pendingLine = null,
    scoreSpread = listOf(0u, 0u, 1u, 2u, 5u, 14u, 38u, 51u, 27u, 9u),
    scoredLine = "147 scored · mean 7.62",
    signedInLine = "Signed in as sample-user on MyAnimeList.",
)
