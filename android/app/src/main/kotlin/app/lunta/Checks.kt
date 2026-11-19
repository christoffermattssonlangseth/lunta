package app.lunta

import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.lunta_core.PaperUpdates
import uniffi.lunta_core.ProgressListener
import uniffi.lunta_core.RefCheckReport
import uniffi.lunta_core.UpdatesSummary
import uniffi.lunta_core.checkReferences
import java.io.File

/**
 * Checks against the published record (core/src/checks), as on the Mac (apple/Shared/Checks.swift):
 * Check a Reference List (pasted, or a .bib / .ris / text file picked through the Storage Access
 * Framework) with its report, and retractions, corrections and published versions of preprints,
 * checked weekly when the library opens and on demand (+ › Check for retractions…). Nothing in the
 * library changes until a choice is made (Switch to the published version, Link both, Keep the
 * preprint). ChecksDialogs (ui/ChecksUi.kt) shows them.
 */
class ReferenceChecks(private val model: AppModel) {
    /** "Check a reference list": the paste-or-pick dialog. */
    var asking by mutableStateOf(false)
    var running by mutableStateOf(false)
        private set
    var report by mutableStateOf<Pair<String, RefCheckReport>?>(null)
    var summary by mutableStateOf<UpdatesSummary?>(null)
    /** What the checks found, per paper (badges, the published version's choices). */
    var updates by mutableStateOf<Map<String, PaperUpdates>>(emptyMap())
        private set
    private var weeklyDone = false

    private val app get() = model.app
    private val listener = object : ProgressListener {
        override fun onProgress(message: String) { model.scope.launch { model.status = message } }
    }

    private fun cacheDir() = File(app.cacheDir, "checks").also { it.mkdirs() }.absolutePath

    fun checkText(text: String, title: String) {
        val lib = model.library ?: return
        if (running) return
        running = true
        model.status = "Checking the references…"
        model.scope.launch {
            try {
                val r = withContext(Dispatchers.IO) { lib.markReferencesInLibrary(checkReferences(text, cacheDir(), listener)) }
                report = title to r
            } catch (e: Exception) {
                model.error = "Couldn't check: ${e.message}"
            } finally {
                running = false
                model.status = null
            }
        }
    }

    fun checkUri(uri: Uri) {
        val text = runCatching { app.contentResolver.openInputStream(uri)?.use { it.readBytes().toString(Charsets.UTF_8) } }.getOrNull()
        if (text.isNullOrBlank()) { model.error = "Couldn't read that file."; return }
        checkText(text, uri.lastPathSegment?.substringAfterLast('/') ?: "the file")
    }

    fun loadUpdates() {
        val lib = model.library ?: return
        model.scope.launch { updates = withContext(Dispatchers.IO) { lib.allPaperUpdates() } }
    }

    /** Checks the library now and shows what was found. */
    fun checkLibrary() {
        val lib = model.library ?: return
        if (running) return
        running = true
        model.status = "Checking for retractions and published versions…"
        model.scope.launch {
            try {
                summary = withContext(Dispatchers.IO) { lib.checkPaperUpdates(emptyList(), listener) }
                updates = withContext(Dispatchers.IO) { lib.allPaperUpdates() }
            } catch (e: Exception) {
                model.error = "Couldn't check: ${e.message}"
            } finally {
                running = false
                model.status = null
            }
        }
    }

    /** The weekly check (once a week per device), quietly, when the library is open. */
    fun weekly() {
        val lib = model.library ?: return
        if (weeklyDone) return
        weeklyDone = true
        model.scope.launch {
            val s = withContext(Dispatchers.IO) { runCatching { lib.checkPaperUpdatesIfDue(null) }.getOrNull() }
            updates = withContext(Dispatchers.IO) { lib.allPaperUpdates() }
            if (s != null && s.changed.isNotEmpty()) summary = s
        }
    }

    /** A choice about a preprint's published version: "switch", "link" or "keep". */
    fun choose(id: String, what: String) {
        val lib = model.library ?: return
        model.scope.launch {
            try {
                withContext(Dispatchers.IO) {
                    when (what) {
                        "switch" -> lib.usePublishedVersion(id)
                        "link" -> lib.linkPublishedVersion(id)
                        else -> lib.keepPreprint(id)
                    }
                }
            } catch (e: Exception) {
                model.error = "Couldn't change it: ${e.message}"
            }
            updates = withContext(Dispatchers.IO) { lib.allPaperUpdates() }
            model.reload()
        }
    }
}
