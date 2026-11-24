package app.lunta.foryou

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.lunta.AppModel
import app.lunta.ai.OnDeviceAi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.lunta_core.*
import java.io.File
import java.util.TimeZone

/**
 * New for You on Android: the core's feed (core/src/foryou, `NewForYou`) on the open library. The
 * state is in the library folder (foryou.json, watches.json), so it is the same New for You as on
 * the Mac, iPhone and PCs: Drive sync carries it like the library's other shared files. What only
 * this phone keeps (when the feed was last opened, the last notification) is in the app's folder.
 *
 * Runs: in the app when one is due (at the morning time; a phone leaves the first two hours to a
 * computer) or on refresh; in the background by [ForYouWorker]. The on-device model writes the
 * searches, screens and summarizes only in the app, when it is installed and this phone may run it.
 */
class ForYouModel(private val model: AppModel) {
    var feed by mutableStateOf<FeedView?>(null)
        private set
    var settings by mutableStateOf<SettingsView?>(null)
        private set
    var running by mutableStateOf(false)
        private set
    var stage by mutableStateOf<String?>(null)
        private set
    var summarizing by mutableStateOf(false)
        private set
    /** keys of finds being added */
    var adding by mutableStateOf<Set<String>>(emptySet())
        private set

    private var core: NewForYou? = null
    private var coreLib: Library? = null

    // "From the journals you follow" (the core's JournalHighlights, journal-highlights.json)
    var highlights by mutableStateOf<HighlightsView?>(null)
        private set
    var journalChoices by mutableStateOf<List<JournalChoice>>(emptyList())
        private set
    var highlightsRunning by mutableStateOf(false)
        private set
    var highlightsWriting by mutableStateOf(false)
        private set
    private var hl: JournalHighlights? = null
    private var hlLib: Library? = null
    /** the digest last asked for (once per set of papers and run of the app) */
    private var digestTried: String? = null

    // "Across science" (the core's AcrossScience, across-science.json)
    var across by mutableStateOf<AcrossView?>(null)
        private set
    var acrossFields by mutableStateOf<List<FieldChoice>>(emptyList())
        private set
    var acrossRunning by mutableStateOf(false)
        private set
    var acrossWriting by mutableStateOf(false)
        private set
    private var asCore: AcrossScience? = null
    private var asLib: Library? = null
    private var acrossDigestTried: String? = null

    @Synchronized
    fun acrossCore(): AcrossScience? {
        val lib = model.library ?: return null
        if (asCore == null || asLib !== lib) {
            asCore = AcrossScience(lib, model.libraryDir().absolutePath, File(model.app.filesDir, "foryou").also { it.mkdirs() }.absolutePath)
            asLib = lib
        }
        return asCore
    }

    @Synchronized
    fun highlightsCore(): JournalHighlights? {
        val lib = model.library ?: return null
        if (hl == null || hlLib !== lib) {
            hl = JournalHighlights(lib, model.libraryDir().absolutePath, File(model.app.filesDir, "foryou").also { it.mkdirs() }.absolutePath)
            hlLib = lib
        }
        return hl
    }

    /** The core's New for You on the open library (made again when another library opens). */
    @Synchronized
    fun core(): NewForYou? {
        val lib = model.library ?: return null
        if (core == null || coreLib !== lib) {
            core = NewForYou(lib, model.libraryDir().absolutePath, File(model.app.filesDir, "foryou").also { it.mkdirs() }.absolutePath)
            coreLib = lib
        }
        return core
    }

    val newCount: Int get() = feed?.newCount?.toInt() ?: 0

    /** Reads the feed and settings again (after a sync, a change, a run). */
    fun refresh() {
        model.scope.launch { reload() }
    }

    suspend fun reload() {
        val fy = core() ?: return
        val now = System.currentTimeMillis()
        val (f, s) = withContext(Dispatchers.IO) { fy.feed(now, tz(now)) to fy.settings() }
        feed = f; settings = s
        highlightsCore()?.let { h ->
            val (v, c) = withContext(Dispatchers.IO) { h.view(now, tz(now)) to h.choices() }
            highlights = v; journalChoices = c
        }
        acrossCore()?.let { a ->
            val (v, c) = withContext(Dispatchers.IO) { a.view(now, tz(now)) to a.fields() }
            across = v; acrossFields = c
        }
    }

    /** The feed was looked at: the badge counts from now. */
    fun markOpened() {
        val fy = core() ?: return
        model.scope.launch { withContext(Dispatchers.IO) { runCatching { fy.markOpened(System.currentTimeMillis()) } }; reload() }
    }

    /** At start-up: the day's papers if they are due. */
    fun runIfDue() {
        val fy = core() ?: return
        model.scope.launch {
            reload()
            val now = System.currentTimeMillis()
            if (withContext(Dispatchers.IO) { fy.isDue(now, tz(now), true) }) runNow() else summarizeIfNeeded()
            val h = highlightsCore()
            if (h != null && withContext(Dispatchers.IO) { h.isDue(now, tz(now), true) }) runHighlights() else writeHighlightsDigest()
            val a = acrossCore()
            if (a != null && withContext(Dispatchers.IO) { a.isDue(now, tz(now), true) }) runAcross() else writeAcrossDigest()
        }
    }

    // ---- Across science ----

    /** Looks for today's papers across science now (one OpenAlex request), or picks again from today's. */
    fun runAcross() {
        val a = acrossCore() ?: return
        if (acrossRunning) return
        acrossRunning = true
        model.scope.launch {
            try {
                val now = System.currentTimeMillis()
                withContext(Dispatchers.IO) { a.run(now, tz(now)) }
                reload()
                model.syncNow()
            } catch (e: Exception) {
                model.error = "Couldn't get today's papers across science: ${e.message ?: e}"
            } finally {
                acrossRunning = false
            }
            writeAcrossDigest()
        }
    }

    /** One sentence a field by the on-device model, when it may run here; the core keeps only checked sentences. */
    fun writeAcrossDigest() {
        val a = acrossCore() ?: return
        OnDeviceAi.init(model.app)
        if (acrossWriting || !OnDeviceAi.usable) return
        model.scope.launch {
            val key = withContext(Dispatchers.IO) { a.digestRequest()?.key } ?: return@launch
            if (key == acrossDigestTried) return@launch
            acrossDigestTried = key
            acrossWriting = true
            try {
                val engine = OnDeviceAi.engine() ?: return@launch
                withContext(Dispatchers.IO) { a.writeDigest(engine) }
            } finally {
                acrossWriting = false
            }
            reload()
        }
    }

    fun setAcrossOn(on: Boolean) = acrossSetting { it.setOn(on) }
    fun setAcrossField(id: String, on: Boolean) = acrossSetting { it.setField(id, on) }
    fun setAcrossPerField(n: Int) = acrossSetting { it.setPerField(n.toUInt()) }

    private fun acrossSetting(change: (AcrossScience) -> Unit) {
        val a = acrossCore() ?: return
        model.scope.launch {
            withContext(Dispatchers.IO) { runCatching { change(a) } }
            reload()
            if (across?.on == true) runAcross()
        }
    }

    fun dismissAcross(key: String) {
        val a = acrossCore() ?: return
        model.scope.launch { withContext(Dispatchers.IO) { runCatching { a.dismiss(key) } }; reload() }
    }

    /** Add to Library, as for a find. */
    fun addAcross(p: AcrossPaper, mustRead: Boolean) {
        val a = acrossCore() ?: return
        val lib = model.library ?: return
        val busy = "as:" + p.key
        if (busy in adding) return
        adding = adding + busy
        model.scope.launch {
            try {
                val id = withContext(Dispatchers.IO) {
                    p.inLibrary
                        ?: p.doi.takeIf { it.isNotBlank() }?.let { lib.findByDoi(it)?.id }
                        ?: p.doi.takeIf { it.isNotBlank() }?.let { runCatching { lib.addFromInput(it, model.progress).paper.id }.getOrNull() }
                        ?: a.findMeta(p.key)?.let { lib.addPaper(it, null, null, null).id }
                } ?: throw IllegalStateException("Couldn't add “${p.title}”.")
                withContext(Dispatchers.IO) {
                    a.added(p.key, id)
                    if (mustRead) lib.setMustRead(id, true)
                }
                model.status = null
                model.reload()
                reload()
            } catch (e: Exception) {
                model.status = null
                model.error = e.message ?: e.toString()
            } finally {
                adding = adding - busy
            }
        }
    }

    // ---- journal highlights ----

    /** Looks for the period's papers in the chosen journals now (Crossref, Europe PMC: seconds). */
    fun runHighlights() {
        val h = highlightsCore() ?: return
        if (highlightsRunning) return
        highlightsRunning = true
        model.scope.launch {
            try {
                val now = System.currentTimeMillis()
                withContext(Dispatchers.IO) { h.run(now, tz(now)) }
                reload()
                model.syncNow()
            } catch (e: Exception) {
                model.error = "Couldn't look in your journals: ${e.message ?: e}"
            } finally {
                highlightsRunning = false
            }
            writeHighlightsDigest()
        }
    }

    /** The digest by the on-device model, when it may run here; the core keeps only checked sentences. */
    fun writeHighlightsDigest() {
        val h = highlightsCore() ?: return
        OnDeviceAi.init(model.app)
        if (highlightsWriting || !OnDeviceAi.usable) return
        model.scope.launch {
            val key = withContext(Dispatchers.IO) { h.digestRequest()?.key } ?: return@launch
            if (key == digestTried) return@launch
            digestTried = key
            highlightsWriting = true
            try {
                val engine = OnDeviceAi.engine() ?: return@launch
                withContext(Dispatchers.IO) { h.writeDigest(engine) }
            } finally {
                highlightsWriting = false
            }
            reload()
        }
    }

    /** A journal in the highlights or out; the problem to show, or null. */
    suspend fun setHighlightJournal(name: String, on: Boolean): String? {
        val h = highlightsCore() ?: return "The library isn't open yet."
        return try {
            withContext(Dispatchers.IO) { h.setJournal(name, on) }
            reload()
            runHighlights()
            null
        } catch (e: Exception) {
            (e.message ?: e.toString()).removePrefix("Invalid: ").removePrefix("invalid: ")
        }
    }

    fun highlightsDefaults() {
        val h = highlightsCore() ?: return
        model.scope.launch {
            try { withContext(Dispatchers.IO) { h.turnOnDefaults() } } catch (e: Exception) { model.error = e.message ?: e.toString() }
            reload()
            runHighlights()
        }
    }

    fun setHighlightCadence(cadence: String) {
        val h = highlightsCore() ?: return
        model.scope.launch {
            withContext(Dispatchers.IO) { runCatching { h.setCadence(cadence) } }
            reload()
            runHighlights()
        }
    }

    fun dismissHighlight(key: String) {
        val h = highlightsCore() ?: return
        model.scope.launch { withContext(Dispatchers.IO) { runCatching { h.dismiss(key) } }; reload() }
    }

    /** Add to Library, as for a find. */
    fun addHighlight(p: HighlightPaper, mustRead: Boolean) {
        val h = highlightsCore() ?: return
        val lib = model.library ?: return
        val busy = "hl:" + p.key
        if (busy in adding) return
        adding = adding + busy
        model.scope.launch {
            try {
                val id = withContext(Dispatchers.IO) {
                    p.inLibrary
                        ?: p.doi.takeIf { it.isNotBlank() }?.let { lib.findByDoi(it)?.id }
                        ?: p.doi.takeIf { it.isNotBlank() }?.let { runCatching { lib.addFromInput(it, model.progress).paper.id }.getOrNull() }
                        ?: h.findMeta(p.key)?.let { lib.addPaper(it, null, null, null).id }
                } ?: throw IllegalStateException("Couldn't add “${p.title}”.")
                withContext(Dispatchers.IO) {
                    h.added(p.key, id)
                    if (mustRead) lib.setMustRead(id, true)
                }
                model.status = null
                model.reload()
                reload()
            } catch (e: Exception) {
                model.status = null
                model.error = e.message ?: e.toString()
            } finally {
                adding = adding - busy
            }
        }
    }

    /** Looks for new papers now (with the model when it may run here). */
    fun runNow() {
        val fy = core() ?: return
        if (running) return
        running = true
        stage = "Looking for new papers…"
        model.scope.launch {
            try {
                val engine = if (OnDeviceAi.usable) OnDeviceAi.engine() else null
                val listener = object : ForYouListener {
                    override fun onStage(stage: String) { model.scope.launch { this@ForYouModel.stage = stage } }
                }
                val now = System.currentTimeMillis()
                withContext(Dispatchers.IO) { fy.run(now, tz(now), engine, listener) }
                reload()
                // foryou.json goes to the other devices
                model.syncNow()
            } catch (e: Exception) {
                model.error = "Couldn't look for new papers: ${e.message ?: e}"
            } finally {
                running = false
                stage = null
            }
            reload()
            summarizeIfNeeded()
        }
    }

    fun stop() { core()?.stop() }

    /** One sentence for each paper without one, and the digest, when the model may run here. */
    fun summarizeIfNeeded() {
        val fy = core() ?: return
        OnDeviceAi.init(model.app)
        if (summarizing || running || !OnDeviceAi.usable) return
        model.scope.launch {
            if (!withContext(Dispatchers.IO) { fy.needsSummaries(tz(System.currentTimeMillis())) }) return@launch
            summarizing = true
            try {
                val engine = OnDeviceAi.engine() ?: return@launch
                withContext(Dispatchers.IO) { fy.summarize(engine, tz(System.currentTimeMillis())) }
            } finally {
                summarizing = false
            }
            reload()
        }
    }

    // ---- feedback ----

    private fun change(block: (NewForYou) -> Unit) {
        val fy = core() ?: return
        model.scope.launch {
            try { withContext(Dispatchers.IO) { block(fy) } } catch (e: Exception) { model.error = e.message ?: e.toString() }
            reload()
        }
    }

    fun dismiss(key: String) = change { it.dismiss(key) }
    fun boost(key: String) = change { it.boost(key) }

    /**
     * Add to Library: the paper by its DOI as "Add Paper" does (reading view and all), else (no DOI, or
     * the lookup failed) with the details the feed has; then it counts as a paper you wanted.
     */
    fun add(f: FindView, mustRead: Boolean, then: (String) -> Unit = {}) {
        val fy = core() ?: return
        val lib = model.library ?: return
        if (f.key in adding) return
        adding = adding + f.key
        model.scope.launch {
            try {
                val id = withContext(Dispatchers.IO) {
                    f.inLibrary
                        ?: f.doi.takeIf { it.isNotBlank() }?.let { lib.findByDoi(it)?.id }
                        ?: f.doi.takeIf { it.isNotBlank() }?.let { runCatching { lib.addFromInput(it, model.progress).paper.id }.getOrNull() }
                        ?: fy.findMeta(f.key)?.let { lib.addPaper(it, null, null, null).id }
                } ?: throw IllegalStateException("Couldn't add “${f.title}”.")
                withContext(Dispatchers.IO) {
                    fy.added(f.key, id)
                    if (mustRead) lib.setMustRead(id, true)
                }
                model.status = null
                model.reload()
                reload()
                then(id)
            } catch (e: Exception) {
                model.status = null
                model.error = e.message ?: e.toString()
            } finally {
                adding = adding - f.key
            }
        }
    }

    // ---- settings ----

    /** Saves the settings; a changed paragraph (or a newly connected ORCID iD) starts a run. */
    fun save(interests: String, perDay: Int, morning: Int, preprints: String, runAnyway: Boolean, openalexKey: String? = null) {
        val fy = core() ?: return
        model.scope.launch {
            val changed = try {
                withContext(Dispatchers.IO) {
                    if (openalexKey != null) fy.setOpenalexKey(openalexKey.trim())
                    fy.setSettings(interests, perDay.toUInt(), morning.toUInt(), preprints)
                }
            } catch (e: Exception) { model.error = e.message; false }
            reload()
            if (changed || runAnyway) runNow()
        }
    }

    /** Looks you up on OpenAlex; the problem to show, or null. */
    suspend fun connectOrcid(input: String): String? {
        val fy = core() ?: return "The library isn't open yet."
        return try {
            withContext(Dispatchers.IO) { fy.connectOrcid(input, System.currentTimeMillis()) }
            reload()
            null
        } catch (e: Exception) {
            (e.message ?: e.toString()).removePrefix("Invalid: ").removePrefix("invalid: ")
        }
    }

    fun disconnectOrcid() = change { it.disconnectOrcid() }
    fun follow(author: String) = change { it.follow(author, System.currentTimeMillis()) }
    fun removeWatch(id: String) = change { it.removeWatch(id) }

    /**
     * Follows a journal (its name as OpenAlex has it: "nature neuro" → "Nature Neuroscience"), an
     * author, a paper (a DOI: papers citing it) or a topic. The problem to show, or null.
     */
    suspend fun addWatch(kind: String, value: String): String? {
        val fy = core() ?: return "The library isn't open yet."
        val v = value.trim()
        return try {
            val name = if (kind == "journal") withContext(Dispatchers.IO) { journalName(v) } ?: return "No journal called “$v” found." else v
            withContext(Dispatchers.IO) { fy.addWatch(kind, name, name, System.currentTimeMillis()) }
            reload()
            null
        } catch (e: Exception) {
            e.message ?: e.toString()
        }
    }

    /** The model drafts the paragraph from your papers and library (null without the model). */
    suspend fun draftInterests(): String? {
        val fy = core() ?: return null
        OnDeviceAi.init(model.app)
        val engine = OnDeviceAi.engine() ?: return null
        return withContext(Dispatchers.IO) { fy.draftInterests(engine) }
    }

    val canDraft: Boolean get() { OnDeviceAi.init(model.app); return OnDeviceAi.usable }

    companion object {
        /** Minutes east of UTC now (the core works out the local morning with it). */
        fun tz(now: Long): Int = TimeZone.getDefault().getOffset(now) / 60000
    }
}
