package app.lunta

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.lunta.ai.OnDeviceAi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.lunta_core.DoiReport
import uniffi.lunta_core.DoiSuggestion
import uniffi.lunta_core.DuplicateGroup
import uniffi.lunta_core.FileKind
import uniffi.lunta_core.GroupProposal
import uniffi.lunta_core.MissingDetails
import uniffi.lunta_core.HighlightSummary
import uniffi.lunta_core.Outline
import uniffi.lunta_core.SummaryLength
import uniffi.lunta_core.SummarySource
import uniffi.lunta_core.SummaryStyle
import uniffi.lunta_core.ProgressListener
import uniffi.lunta_core.TagSuggestion
import uniffi.lunta_core.TidyChange

/**
 * Tidy the Library (core/src/tidy): duplicates to merge, papers missing a DOI, a PDF or a reading
 * view, preprints with a published version, topics to make into tags or folders, and existing tags
 * for untagged papers. Lunta proposes, the researcher decides: nothing changes until a suggestion
 * is approved, and then through the core's change log (synced). Tags, folders and DOIs come back
 * with a TidyChange that Undo hands to the core. TidyDialog (ui/TidyUi.kt) shows it; the library's
 * + menu opens it.
 */
class LibraryTidy(private val model: AppModel) {
    var open by mutableStateOf(false)
    var loading by mutableStateOf(false)
        private set
    var papers by mutableStateOf(0)
        private set
    var duplicates by mutableStateOf<List<DuplicateGroup>>(emptyList())
        private set
    var missing by mutableStateOf<MissingDetails?>(null)
        private set
    var topics by mutableStateOf<List<GroupProposal>>(emptyList())
    /** The topics are the on-device model's (else found from words the papers share). */
    var topicsByModel by mutableStateOf(false)
        private set
    /** Why the model's topics aren't shown ("The on-device model gave nothing usable…"). */
    var topicsNote by mutableStateOf<String?>(null)
        private set
    var tagSuggestions by mutableStateOf<List<TagSuggestion>>(emptyList())
        private set
    /** DOIs looked up for papers without one (Find DOIs). */
    var dois by mutableStateOf<DoiReport?>(null)
        private set

    /** What runs now ("Looking up DOIs…", "Finding PDFs (3 of 12)…"), else null: one thing at a time. */
    var working by mutableStateOf<String?>(null)
        private set
    /** Which part runs: "doi", "pdf", "epub", "topics", "merge", "apply". */
    var workingOn by mutableStateOf<String?>(null)
        private set
    private var stop = false

    /** The last change applied, offered for Undo (the dialog's snackbar shows it once). */
    var undoable by mutableStateOf<TidyChange?>(null)
    /** A line for the snackbar without Undo ("Merged into …", "Found 4 PDFs."). */
    var message by mutableStateOf<String?>(null)

    private val listener = object : ProgressListener {
        override fun onProgress(message: String) { model.scope.launch { if (working != null) working = message } }
    }

    fun show() {
        open = true
        // an Undo or message left from the last time (the dialog closed while its snackbar was up)
        // is for an old change: not offered again
        undoable = null
        message = null
        dois = null
        topicsByModel = false
        topicsNote = null
        refresh(withTopics = true)
    }

    /** Reads the suggestions again (fast, offline). The model's topics stay unless [withTopics]. */
    fun refresh(withTopics: Boolean = !topicsByModel) {
        val lib = model.library ?: return
        loading = true
        model.scope.launch {
            try {
                val r = withContext(Dispatchers.IO) { lib.tidyReport() }
                papers = r.papers.toInt()
                duplicates = r.duplicates
                missing = r.missing
                tagSuggestions = r.tagSuggestions
                if (withTopics) { topics = r.topics; topicsByModel = false }
                // a DOI found for a paper that has one by now (or was merged away) is not offered
                dois = dois?.let { d -> d.copy(suggestions = d.suggestions.filter { it.paperId in r.missing.noDoi }) }
            } catch (e: Exception) {
                model.error = "Couldn't tidy the library: ${e.message}"
            } finally {
                loading = false
            }
        }
    }

    private fun run(what: String, first: String, block: suspend () -> Unit) {
        if (working != null) return
        working = first
        workingOn = what
        stop = false
        model.scope.launch {
            try { block() } catch (e: Exception) {
                model.error = e.message ?: e.toString()
            } finally {
                working = null
                workingOn = null
            }
        }
    }

    /** Stops a loop over papers (Find PDFs, Make reading views) after the paper it is on. */
    fun stopWorking() { stop = true }

    private fun applied(ch: TidyChange) {
        undoable = ch
        model.scope.launch { model.reload() }
        refresh()
    }

    // ---- duplicates ----

    /** Merges the group into [keep] (not undoable: the dialog asks first). */
    fun merge(group: DuplicateGroup, keep: String) {
        val lib = model.library ?: return
        run("merge", "Merging…") {
            val p = withContext(Dispatchers.IO) { lib.mergePapers(keep, group.paperIds.filter { it != keep }) }
            message = "Merged into “${p.meta.title}”."
            model.reload()
            refresh()
        }
    }

    /** "Not duplicates", "Doesn't need a DOI", a tag or topic not to suggest again (on this device). */
    fun dismiss(key: String) {
        val lib = model.library ?: return
        model.scope.launch {
            runCatching { withContext(Dispatchers.IO) { lib.tidyDismiss(key) } }.onFailure { model.error = "Couldn't dismiss it: ${it.message}" }
            refresh()
        }
    }

    fun dismissTopic(g: GroupProposal) {
        topics = topics.filter { it !== g }
        val lib = model.library ?: return
        model.scope.launch { runCatching { withContext(Dispatchers.IO) { lib.tidyDismiss("topic:${g.name}") } } }
    }

    // ---- missing details ----

    /** Looks up DOIs for the papers without one (network; about a second a paper). */
    fun findDois() {
        val lib = model.library ?: return
        val n = missing?.noDoi?.size ?: 0
        run("doi", "Looking up DOIs for $n ${if (n == 1) "paper" else "papers"}…") {
            dois = withContext(Dispatchers.IO) { lib.findMissingDois(emptyList(), listener) }
        }
    }

    fun applyDois(list: List<DoiSuggestion>) {
        val lib = model.library ?: return
        if (list.isEmpty()) return
        run("apply", "Adding DOIs…") {
            val ch = withContext(Dispatchers.IO) { lib.applyDoiSuggestions(list) }
            val done = list.map { it.paperId }.toSet()
            dois = dois?.let { d -> d.copy(suggestions = d.suggestions.filter { it.paperId !in done }) }
            applied(ch)
        }
    }

    /** Find PDF for each paper without one (and a reading view from a PDF found, as Find PDF does). */
    fun findPdfs(ids: List<String>) {
        val lib = model.library ?: return
        run("pdf", "Looking for open-access PDFs…") {
            var found = 0
            var tried = 0
            for ((i, id) in ids.withIndex()) {
                if (stop) break
                working = "Looking for open-access PDFs (${i + 1} of ${ids.size}, $found found)…"
                tried++
                val src = withContext(Dispatchers.IO) { runCatching { lib.findPdf(id) }.getOrNull() }
                if (src != null) {
                    found++
                    if (model.paper(id)?.hasEpub == false) {
                        withContext(Dispatchers.IO) { runCatching { lib.makeReadingView(id, null) } }
                    }
                }
            }
            model.reload()
            message = "Found ${plural(found, "PDF", "PDFs")} for the ${plural(tried, "paper", "papers")} looked up."
            refresh()
        }
    }

    /** Makes the reading view of each paper that has a PDF and none (the PDF fetched first if it is on Drive only). */
    fun makeReadingViews(ids: List<String>) {
        val lib = model.library ?: return
        run("epub", "Making reading views…") {
            var made = 0
            var failed = 0
            for ((i, id) in ids.withIndex()) {
                if (stop) break
                working = "Making reading views (${i + 1} of ${ids.size})…"
                val ok = withContext(Dispatchers.IO) {
                    runCatching {
                        if (lib.filePath(id, FileKind.PDF) == null) model.librarySync?.fetchPaper(id, listOf(FileKind.PDF))
                        lib.makeReadingView(id, null)
                    }.isSuccess
                }
                if (ok) made++ else failed++
            }
            withContext(Dispatchers.IO) { runCatching { lib.reindexQuick() } }
            model.reload()
            message = "Made ${plural(made, "reading view", "reading views")}" + (if (failed > 0) "; ${plural(failed, "PDF", "PDFs")} couldn't be converted." else ".")
            refresh()
        }
    }

    /** Switch to the Published Version: the checks' own choice (ReferenceChecks.choose), after the dialog's confirmation. */
    fun switchToPublished(id: String) {
        missing = missing?.let { m -> m.copy(published = m.published.filter { it.paperId != id }) }
        model.checks.choose(id, "switch")
    }

    // ---- topics and tags ----

    /** Topics from the on-device model (Qwen3 1.7B: about 70 titles fit its 4,096 tokens). */
    fun suggestTopicsWithModel() {
        val lib = model.library ?: return
        run("topics", "The on-device model is reading your latest titles…") {
            OnDeviceAi.init(model.app)
            val engine = OnDeviceAi.engine()
            if (engine == null) {
                topicsNote = "The on-device model couldn't be loaded on this phone."
                return@run
            }
            val found = withContext(Dispatchers.IO) { lib.tidyTopicsWithModel(engine, MODEL_TITLES.toUInt()) }
            if (found.isEmpty()) {
                topicsNote = "The on-device model gave nothing usable; these are found from words your papers share."
                topics = withContext(Dispatchers.IO) { lib.tidyTopics() }
                topicsByModel = false
            } else {
                topics = found
                topicsByModel = true
                topicsNote = null
            }
        }
    }

    /** Topics found from shared words again (after the model's). */
    fun heuristicTopics() {
        topicsNote = null
        refresh(withTopics = true)
    }

    fun createGroups(list: List<GroupProposal>, asFolders: Boolean) {
        val lib = model.library ?: return
        if (list.isEmpty()) return
        run("apply", if (asFolders) "Making folders…" else "Adding tags…") {
            val ch = withContext(Dispatchers.IO) { lib.createGroups(list, asFolders) }
            topics = topics.filter { t -> list.none { it.paperIds == t.paperIds } }
            applied(ch)
        }
    }

    fun applyTags(list: List<TagSuggestion>) {
        val lib = model.library ?: return
        if (list.isEmpty()) return
        run("apply", "Adding tags…") {
            val ch = withContext(Dispatchers.IO) { lib.applyTagSuggestions(list) }
            applied(ch)
        }
    }

    fun undo(ch: TidyChange) {
        val lib = model.library ?: return
        model.scope.launch {
            try {
                withContext(Dispatchers.IO) { lib.undoTidyChange(ch) }
                message = "Undone."
            } catch (e: Exception) {
                model.error = "Couldn't undo: ${e.message}"
            }
            model.reload()
            refresh()
        }
    }

    companion object {
        /** Titles given to the phone's model for topics, and highlights for an outline (4,096 tokens of context). */
        const val MODEL_TITLES = 70
        const val MODEL_HIGHLIGHTS = 45
        fun plural(n: Int, one: String, many: String) = "$n ${if (n == 1) one else many}"
    }
}

/**
 * Outline from Highlights (core/src/outline.rs) for a set of papers: a folder, a tag, a list or a
 * search. Every point is a highlight word for word or the researcher's note; the on-device model,
 * when used, only groups and names the themes. OutlineDialog (ui/OutlineUi.kt) shows it.
 * Summarize Highlights: the model condenses the same highlights, every sentence citing those it is
 * based on and checked by the core (core/src/outline/summary.rs); when none holds up, the outline.
 */
class HighlightsOutline(private val model: AppModel) {
    /** The papers and what they are ("Spatial transcriptomics", "the search “microglia”"). */
    var request by mutableStateOf<Pair<String, List<String>>?>(null)
    var topic by mutableStateOf("")
    var running by mutableStateOf(false)
        private set
    var result by mutableStateOf<Outline?>(null)
        private set
    var summary by mutableStateOf<HighlightSummary?>(null)
        private set
    /** Why the summary isn't shown (none of its sentences held up): said over the outline. */
    var summaryFailed by mutableStateOf<String?>(null)
        private set
    /** The dialog's Summary side, and its length and style (kept for Back and the next time). */
    var asSummary by mutableStateOf(false)
    var longer by mutableStateOf(false)
    var bullets by mutableStateOf(false)
    /** A summary's Open in Paper: (paper, highlight) for the reader to show once its page is ready. */
    var jump by mutableStateOf<Pair<String, String>?>(null)
    private var generation = 0

    fun ask(title: String, ids: List<String>) {
        generation++
        running = false
        result = null
        summary = null
        summaryFailed = null
        topic = ""
        request = title to ids
    }

    fun close() {
        generation++
        request = null
        result = null
        summary = null
        summaryFailed = null
        running = false
    }

    fun back() { generation++; result = null; summary = null; summaryFailed = null; running = false }

    /** The summary by the on-device model (in parts when there are many highlights, then combined). */
    fun summarize(topic: String, length: SummaryLength, style: SummaryStyle) {
        val lib = model.library ?: return
        val ids = request?.second ?: return
        val gen = ++generation
        running = true
        summaryFailed = null
        model.scope.launch {
            try {
                OnDeviceAi.init(model.app)
                val engine = OnDeviceAi.engine() ?: error("download the on-device model first (Settings › On-device AI)")
                val s = withContext(Dispatchers.IO) { lib.summaryWithModel(ids, topic, length, style, engine, LibraryTidy.MODEL_HIGHLIGHTS.toUInt()) }
                if (gen != generation) return@launch
                if (s.ok) summary = s
                else {
                    // nothing held up: the outline instead
                    val o = withContext(Dispatchers.IO) { lib.outlineFromHighlights(ids, topic, null, null) }
                    if (gen == generation) { summaryFailed = s.note; result = o }
                }
            } catch (e: Exception) {
                if (gen == generation) model.error = "Couldn't summarize the highlights: ${e.message}"
            } finally {
                if (gen == generation) running = false
            }
        }
    }

    /** Open in Paper: the paper's reading view at the highlight. */
    fun openAt(s: SummarySource) {
        close()
        jump = s.paperId to s.annotationId
        val under = model.screen
        if (!(under is Screen.Reader && under.id == s.paperId && !under.pdf)) model.push(Screen.Reader(s.paperId, false))
    }

    fun make(topic: String, withModel: Boolean) {
        val lib = model.library ?: return
        val ids = request?.second ?: return
        val gen = ++generation
        running = true
        model.scope.launch {
            try {
                val engine = if (withModel) { OnDeviceAi.init(model.app); OnDeviceAi.engine() } else null
                // the reading views are needed for the sections (Drive syncs them lazily; without them, the PDF page or nothing)
                val o = withContext(Dispatchers.IO) {
                    if (engine != null) lib.outlineWithModel(ids, topic, engine, LibraryTidy.MODEL_HIGHLIGHTS.toUInt())
                    else lib.outlineFromHighlights(ids, topic, null, null)
                }
                if (gen == generation) result = o
            } catch (e: Exception) {
                if (gen == generation) model.error = "Couldn't make the outline: ${e.message}"
            } finally {
                if (gen == generation) running = false
            }
        }
    }
}
