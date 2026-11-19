package io.github.hanami117.kotora

import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.hanami117.kotora.ui.lastChecked
import io.github.hanami117.kotora.update.Updater
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** GitHub's release list -> the newest Android release (org.json needs Robolectric). */
@RunWith(AndroidJUnit4::class)
class UpdaterTest {
    private fun release(tag: String, apk: String? = "kotora-android-${tag.removePrefix("android-v")}.apk", draft: Boolean = false, pre: Boolean = false) =
        """{"tag_name":"$tag","draft":$draft,"prerelease":$pre,"html_url":"https://github.com/o/r/releases/tag/$tag","assets":[""" +
            (apk?.let { """{"name":"$it","size":123,"browser_download_url":"https://github.com/o/r/releases/download/$tag/$it"}""" } ?: "") + "]}"

    @Test fun versionCodeIsTheBuildsFormula() {
        assertEquals(500, Updater.versionCode("0.5.0"))
        assertEquals(10213, Updater.versionCode("1.2.13"))
        assertNull(Updater.versionCode("0.5"))
        assertNull(Updater.versionCode("x.y.z"))
    }

    @Test fun picksTheNewestPublishedAndroidReleaseWithAnApk() {
        val json = "[" + listOf(
            release("v0.9.0"), // desktop: ignored
            release("android-v0.7.0", draft = true),
            release("android-v0.6.1", pre = true),
            release("android-v0.6.0", apk = null),
            release("android-v0.5.2"),
            release("android-v0.5.10"),
        ).joinToString(",") + "]"
        val r = Updater.newest(json)!!
        assertEquals("0.5.10", r.version)
        assertEquals(510, r.code)
        assertEquals("kotora-android-0.5.10.apk", r.apkName)
        assertEquals(123L, r.size)
        assertTrue(r.apkUrl.endsWith("/android-v0.5.10/kotora-android-0.5.10.apk"))
    }

    @Test fun noAndroidReleaseIsNull() {
        assertNull(Updater.newest("[" + release("v0.5.0") + "]"))
        assertNull(Updater.newest("[]"))
    }

    @Test fun lastCheckedReadsLikeTheDesktop() {
        assertEquals("Not checked yet", lastChecked(null, 0))
        val now = System.currentTimeMillis() / 1000
        assertTrue(lastChecked(now, now).startsWith("Last checked: today at "))
    }
}
