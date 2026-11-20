package io.github.hanami117.kotora.data

import android.content.Context

/** The phone's own settings that the core doesn't keep: the startup animation, and whether
 *  Kotora already asked for the notification permission (Android 13+ asks once, then Settings). */
class UiPrefs(context: Context) {
    private val p = context.applicationContext.getSharedPreferences("kotora_ui", Context.MODE_PRIVATE)

    /** Settings → Startup animation (the desktop's `[display] intro`): on by default. */
    var intro: Boolean
        get() = p.getBoolean("intro", true)
        set(v) = p.edit().putBoolean("intro", v).apply()

    var askedNotifications: Boolean
        get() = p.getBoolean("asked_notifications", false)
        set(v) = p.edit().putBoolean("asked_notifications", v).apply()
}
