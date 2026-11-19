package app.lunta.ai

import android.app.ActivityManager
import android.content.Context
import android.os.Build
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.edit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.lunta_core.AiDownloadListener
import uniffi.lunta_core.AiEngine
import uniffi.lunta_core.AiException
import uniffi.lunta_core.AiModel
import uniffi.lunta_core.aiDeleteModel
import uniffi.lunta_core.aiDownloadModel
import uniffi.lunta_core.aiModelFile
import uniffi.lunta_core.aiPartialBytes
import uniffi.lunta_core.aiPhoneModel
import uniffi.lunta_core.aiUnsupportedReason
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.ceil

/**
 * On-device AI on Android: the open model Qwen3 1.7B (GGUF, 1.3 GB) run by the core's llama.cpp,
 * for the reader's Summary card and Ask. Downloaded once, when the user turns it on in Settings,
 * into the app's no-backup folder; checked against its SHA-256; deletable.
 *
 * The memory rule: the model takes about 1.3 GB while it works, plus ~0.5 GB for its context, on top
 * of the app and the system. Phones with 6 GB or more run it; with 4–6 GB it may work (the system
 * may close other apps, or Lunta) and is allowed after a warning; with less, or without the
 * processor features llama.cpp is built for, it is not offered, and Settings says why.
 */
object OnDeviceAi {
    private lateinit var app: Context
    private val prefs by lazy { app.getSharedPreferences("lunta-ai", Context.MODE_PRIVATE) }
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    fun init(context: Context) {
        if (::app.isInitialized) return
        app = context.applicationContext
        refresh()
    }

    val model: AiModel by lazy { aiPhoneModel() }
    private val dir: File get() = File(app.noBackupFilesDir, "models")

    enum class Support { Ok, LowMemory, TooLittleMemory, Unsupported }

    /** The phone's memory as sold (6, 8, 12 GB…): the system reports less than that (some is reserved). */
    val memoryGb: Int by lazy {
        val mi = ActivityManager.MemoryInfo()
        (app.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(mi)
        val gib = mi.totalMem / (1024.0 * 1024 * 1024)
        listOf(1, 2, 3, 4, 6, 8, 10, 12, 16, 18, 24, 32).firstOrNull { it >= gib - 0.05 } ?: ceil(gib).toInt()
    }
    private val totalGib: Double by lazy {
        val mi = ActivityManager.MemoryInfo()
        (app.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(mi)
        mi.totalMem / (1024.0 * 1024 * 1024)
    }

    /** Why the model can't run here at all (an ABI or processor llama.cpp isn't built for). */
    val unsupportedReason: String? by lazy {
        val abi = Build.SUPPORTED_ABIS.firstOrNull() ?: ""
        if (abi != "arm64-v8a" && abi != "x86_64") "On-device AI needs a 64-bit ARM phone (arm64-v8a); this one is $abi."
        else runCatching { aiUnsupportedReason() }.getOrElse { "On-device AI isn't part of this build of Lunta." }
    }

    val support: Support by lazy {
        when {
            unsupportedReason != null -> Support.Unsupported
            // a 6 GB phone reports about 5.3–5.6 GiB, a 4 GB one about 3.5–3.7
            totalGib >= 5.0 -> Support.Ok
            totalGib >= 3.3 -> Support.LowMemory
            else -> Support.TooLittleMemory
        }
    }

    /** What Settings says when the phone isn't a good fit. */
    val supportNote: String?
        get() = when (support) {
            Support.Ok -> null
            Support.LowMemory -> "On-device AI needs a phone with at least 6 GB of memory; this phone has $memoryGb GB. You can try it anyway: it may be slow, and Android may close other apps (or Lunta) to make room."
            Support.TooLittleMemory -> "On-device AI needs a phone with at least 6 GB of memory; this phone has $memoryGb GB."
            Support.Unsupported -> unsupportedReason
        }

    /** The user accepted the warning on a 4–6 GB phone. */
    var tryAnyway: Boolean
        get() = prefs.getBoolean("tryAnyway", false)
        set(v) = prefs.edit { putBoolean("tryAnyway", v) }

    /** Settings › "Show summaries in the reader" */
    var summaries by mutableStateOf(true)
        private set

    fun showSummaries(on: Boolean) { summaries = on; prefs.edit { putBoolean("summaries", on) } }

    // ---- the model file ----

    var installed by mutableStateOf(false)
        private set
    /** bytes done / total while downloading, else null */
    var downloading by mutableStateOf<Pair<Long, Long>?>(null)
        private set
    var partial by mutableStateOf(0L)
        private set
    var error by mutableStateOf<String?>(null)
        private set
    private var stop: AtomicBoolean? = null

    /** Reads the state of the files (call when Settings opens). */
    fun refresh() {
        summaries = prefs.getBoolean("summaries", true)
        if (support == Support.Unsupported) return
        installed = runCatching { aiModelFile(model, dir.path) != null }.getOrDefault(false)
        partial = runCatching { aiPartialBytes(model, dir.path).toLong() }.getOrDefault(0)
    }

    /** Can it write summaries and answers now? */
    val usable: Boolean
        get() = installed && (support == Support.Ok || (support == Support.LowMemory && tryAnyway))

    fun download() {
        if (downloading != null || support == Support.Unsupported || support == Support.TooLittleMemory) return
        val flag = AtomicBoolean(false)
        stop = flag
        error = null
        downloading = partial to model.size.toLong()
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                var last = 0L
                runCatching {
                    aiDownloadModel(model, dir.path, object : AiDownloadListener {
                        override fun onProgress(done: ULong, total: ULong): Boolean {
                            val now = System.currentTimeMillis()
                            if (now - last > 250) {
                                last = now
                                scope.launch { if (downloading != null) downloading = done.toLong() to total.toLong() }
                            }
                            return !flag.get()
                        }
                    })
                }
            }
            downloading = null
            stop = null
            r.exceptionOrNull()?.let { e -> if (e !is AiException.Cancelled) error = e.message ?: e.toString() }
            refresh()
        }
    }

    fun cancelDownload() { stop?.set(true) }

    fun delete() {
        cancelDownload()
        scope.launch {
            engineLock.withLock { engine?.destroy(); engine = null }
            withContext(Dispatchers.IO) { runCatching { aiDeleteModel(model, dir.path) } }
            refresh()
        }
    }

    // ---- the engine ----

    private val engineLock = Mutex()
    private var engine: AiEngine? = null

    /** The loaded model (loaded now if needed: a few seconds), or null if it can't be used. */
    suspend fun engine(): AiEngine? = engineLock.withLock {
        engine?.let { return@withLock it }
        if (!usable) return@withLock null
        val path = withContext(Dispatchers.IO) { runCatching { aiModelFile(model, dir.path) }.getOrNull() } ?: return@withLock null
        withContext(Dispatchers.IO) { runCatching { AiEngine.load(path, model.id, model.context, 0) }.getOrNull() }.also { engine = it }
    }

    /** Lets the model's memory go (Android asked for memory back). */
    fun release() {
        scope.launch { engineLock.withLock { engine?.destroy(); engine = null } }
    }

    const val NOTE = "Written on this phone by an open model (Qwen3 1.7B). It can be wrong: check what matters against the paper."
}
