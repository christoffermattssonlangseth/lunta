package app.lunta.data

import android.content.Context
import androidx.core.content.edit
import java.util.UUID

/** Where the library lives. */
enum class LibraryMode { None, Drive, Local }

/** Small preferences store (SharedPreferences). */
class Settings(context: Context) {
    private val p = context.getSharedPreferences("lunta", Context.MODE_PRIVATE)

    var mode: LibraryMode
        get() = runCatching { LibraryMode.valueOf(p.getString("mode", "None")!!) }.getOrDefault(LibraryMode.None)
        set(v) = p.edit { putString("mode", v.name) }

    val deviceId: String
        get() = p.getString("device", null) ?: ("android-" + UUID.randomUUID().toString().take(8)).also { id -> p.edit { putString("device", id) } }

    var driveFolder: String
        get() = p.getString("driveFolder", "Lunta")!!
        set(v) = p.edit { putString("driveFolder", v.ifBlank { "Lunta" }) }

    /** Testing: a plain folder used as the sync "remote" instead of Google Drive (set from a debug intent). */
    var testRemoteDir: String
        get() = p.getString("testRemoteDir", "")!!
        set(v) = p.edit { putString("testRemoteDir", v) }

    var sort: String
        get() = p.getString("sort", "Added")!!
        set(v) = p.edit { putString("sort", v) }

    var readerScale: Float
        get() = p.getFloat("readerScale", 1.0f)
        set(v) = p.edit { putFloat("readerScale", v) }

    var readerPaged: Boolean
        get() = p.getBoolean("readerPaged", false)
        set(v) = p.edit { putBoolean("readerPaged", v) }

    var syncMinutes: Int
        get() = p.getInt("syncMinutes", 30)
        set(v) = p.edit { putInt("syncMinutes", v) }

    var lastSyncMs: Long
        get() = p.getLong("lastSyncMs", 0)
        set(v) = p.edit { putLong("lastSyncMs", v) }

    // sign-in in progress (survives the app being killed while the browser is open)
    var pendingVerifier: String
        get() = p.getString("pendingVerifier", "")!!
        set(v) = p.edit { putString("pendingVerifier", v) }
    var pendingState: String
        get() = p.getString("pendingState", "")!!
        set(v) = p.edit { putString("pendingState", v) }
    var pendingRedirect: String
        get() = p.getString("pendingRedirect", "")!!
        set(v) = p.edit { putString("pendingRedirect", v) }
}
