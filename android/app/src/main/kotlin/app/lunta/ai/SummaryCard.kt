package app.lunta.ai

import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import org.json.JSONTokener
import uniffi.lunta_core.AiTextListener
import uniffi.lunta_core.Library
import uniffi.lunta_core.PaperSummary
import uniffi.lunta_core.aiSourceHash
import uniffi.lunta_core.aiSummaryNote
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The Summary card of one open paper (SummaryMaker in apple/Shared/Summary.swift): the page says what
 * the summary is made from (luntaSummarySource) and draws the card (luntaSummaryShow / Hide, from
 * the Apple apps' SummaryCard.swift); this keeps it with the paper (summary.json, which syncs, so a
 * summary made on the Mac or a PC shows here at once) or writes it with the on-device model.
 * [pageReady] when the page has loaded, [action] for the card's ⋯ menu, [close] when the reader goes.
 */
class SummaryCard(
    private val scope: CoroutineScope,
    private val lib: Library,
    private val paperId: String,
    private val title: String,
    private val abstract: String,
) {
    private var js: (suspend (String) -> String?)? = null
    private var job: Job? = null
    private var stop = AtomicBoolean(false)
    /** what the card shows (drawn again when the page reloads) */
    private var shown: JSONObject? = null

    private suspend fun source(): String? {
        val raw = js?.invoke("window.luntaSummarySource ? luntaSummarySource(1900, ${JSONObject.quote(abstract)}) : ''") ?: return null
        val json = runCatching { JSONTokener(raw).nextValue() as? String }.getOrNull() ?: return null
        return runCatching { JSONObject(json).optString("text") }.getOrNull()?.takeIf { it.isNotBlank() }
    }

    private suspend fun show(state: String, text: String, note: String = "") {
        val o = JSONObject().put("state", state).put("text", text).put("note", note)
        shown = o
        js?.invoke("window.luntaSummaryShow && luntaSummaryShow(${JSONObject.quote(o.toString())})")
    }

    private suspend fun hide() {
        shown = null
        js?.invoke("window.luntaSummaryHide && luntaSummaryHide()")
    }

    fun pageReady(run: suspend (String) -> String?) {
        js = run
        if (!OnDeviceAi.summaries) { scope.launch { hide() }; return }
        shown?.let { o -> scope.launch { js?.invoke("window.luntaSummaryShow && luntaSummaryShow(${JSONObject.quote(o.toString())})") } }
        if (job == null) start(force = false)
    }

    fun action(a: String) {
        when (a) {
            "regenerate" -> { stopJob(); start(force = true) }
            "hide" -> { stopJob(); OnDeviceAi.showSummaries(false); scope.launch { hide() } }
        }
    }

    fun close() { stopJob(); js = null }

    private fun stopJob() { stop.set(true); job?.cancel(); job = null }

    private fun start(force: Boolean) {
        val flag = AtomicBoolean(false)
        stop = flag
        job = scope.launch {
            try { run(force, flag) } catch (e: Exception) { Log.w("Lunta", "summary: $e") }
            if (stop === flag) job = null
        }
    }

    private suspend fun run(force: Boolean, flag: AtomicBoolean) {
        val text = source()
        if (text == null || text.split(Regex("\\s+")).size < 80) { hide(); return }
        val hash = aiSourceHash(text)
        val saved = withContext(Dispatchers.IO) { runCatching { lib.summary(paperId) }.getOrNull() }
        if (!force && saved != null && saved.source == hash && saved.text.isNotBlank()) {
            show("done", saved.text, aiSummaryNote(saved.model))
            return
        }
        // no model here (a summary never starts a download): no card, or the old one stays on Regenerate
        if (!OnDeviceAi.usable) { if (!force) hide(); return }
        show("loading", "Summarizing…")
        val engine = OnDeviceAi.engine() ?: run { hide(); return }
        if (flag.get()) return
        var last = 0L
        val started = System.currentTimeMillis()
        val summary = withContext(Dispatchers.IO) {
            runCatching {
                engine.summarize(title, text, object : AiTextListener {
                    override fun onText(text: String): Boolean {
                        if (flag.get()) return false
                        val now = System.currentTimeMillis()
                        // a few updates a second (each lays the page out again)
                        if (now - last > 250) { last = now; scope.launch { if (!flag.get()) show("writing", text) } }
                        return true
                    }
                })
            }.getOrNull()
        }
        if (flag.get()) return
        if (summary == null) { hide(); return }
        val s = PaperSummary(summary, engine.modelId(), System.currentTimeMillis(), hash)
        withContext(Dispatchers.IO) { runCatching { lib.saveSummary(paperId, s) } }
        show("done", summary, aiSummaryNote(s.model))
        Log.i("Lunta", "summary of $paperId in ${(System.currentTimeMillis() - started) / 1000.0} s")
    }
}
