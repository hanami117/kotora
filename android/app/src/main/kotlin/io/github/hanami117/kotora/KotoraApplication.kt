package io.github.hanami117.kotora

import android.app.Application
import android.util.Log
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.disk.DiskCache
import coil3.disk.directory
import io.github.hanami117.kotora.data.CoreRepository
import io.github.hanami117.kotora.data.Repository

/**
 * One [Repository] (one core, one database) for the process: the Activity and the workers share
 * it, so the core's rate budgets cover both.
 */
class KotoraApplication : Application(), SingletonImageLoader.Factory {
    /** Why the core couldn't start, when it couldn't. */
    var brokenMessage: String? = null
        private set

    val repository: Repository? by lazy {
        try {
            CoreRepository(this)
        } catch (e: Throwable) {
            Log.e("kotora", "the core didn't start", e)
            brokenMessage = "Kotora couldn't start: ${e.message ?: e.javaClass.simpleName}"
            null
        }
    }

    /** Covers: Coil's cache in `cacheDir/covers`, 100 MB at most (the OS may clear it any time). */
    override fun newImageLoader(context: PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .diskCache { DiskCache.Builder().directory(cacheDir.resolve("covers")).maxSizeBytes(100L * 1024 * 1024).build() }
            .build()
}
