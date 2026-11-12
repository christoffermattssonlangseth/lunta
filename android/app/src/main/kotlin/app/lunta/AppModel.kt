package app.lunta

import android.app.Application
import android.content.Context
import android.net.Uri
import android.os.Build
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.lunta.data.LibraryMode
import app.lunta.data.Positions
import app.lunta.data.Settings
import app.lunta.data.SmartFolder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.lunta_core.*
import java.io.File
import java.io.InputStream

/** A place in the library list (the iPhone app's SidebarItem). */
sealed interface Place {
    data object All : Place
    data object Starred : Place
    data object Unread : Place
    data object ForYou : Place
    data class Folder(val id: String) : Place
    data class Smart(val id: String) : Place
    data class Tag(val name: String) : Place
}

sealed interface Screen {
    data object Home : Screen
    data class Papers(val place: Place) : Screen
    data class Info(val id: String) : Screen
    data class Reader(val id: String, val pdf: Boolean = false) : Screen
}

/** One row of a paper list: the paper, and the matching text when it came from a search. */
data class Row(val paper: Paper, val snippet: String? = null)

/**
 * Everything the screens share: the open library, its sync, and the lists derived from it. Core calls
 * block, so they run on Dispatchers.IO; state that Compose reads is updated back on the main thread.
 */
class AppModel(val app: Application) {
    val settings = Settings(app)
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    var mode by mutableStateOf(settings.mode)
        private set
    var library: Library? = null
        private set
    var librarySync: LibrarySync? = null
        private set
    var driveAuth: DriveAuth? = null
        private set
    var positions: Positions? = null
        private set
    var opened by mutableStateOf(false)
        private set

    var papers by mutableStateOf<List<Paper>>(emptyList())
        private set
    var collections by mutableStateOf<List<PaperCollection>>(emptyList())
        private set
    var tags by mutableStateOf<List<String>>(emptyList())
        private set
    var smartFolders by mutableStateOf<List<SmartFolder>>(emptyList())
        private set

    var sort by mutableStateOf(runCatching { SortKey.valueOf(settings.sort) }.getOrDefault(SortKey.ADDED))
        private set
    var syncing by mutableStateOf(false)
        private set
    var syncError by mutableStateOf<String?>(null)
    var lastSyncMs by mutableStateOf(settings.lastSyncMs)
        private set
    /** A line shown at the bottom while something long runs ("Looking up 10.1038/…"). */
    var status by mutableStateOf<String?>(null)
    var error by mutableStateOf<String?>(null)
    var info by mutableStateOf<String?>(null)

    val stack = mutableStateListOf<Screen>(Screen.Home)
    val screen: Screen get() = stack.last()

    private val syncLock = Mutex()
    val progress = object : ProgressListener {
        override fun onProgress(message: String) { status = message }
    }

    val clientIdMissing: Boolean get() = BuildConfig.GOOGLE_CLIENT_ID.isBlank()

    // ---- navigation ----

    fun push(s: Screen) { stack.add(s) }
    fun pop(): Boolean { if (stack.size > 1) { stack.removeAt(stack.lastIndex); return true }; return false }
    fun resetTo(s: Screen) { stack.clear(); stack.add(s) }

    // ---- opening the library ----

    private fun rootDir(m: LibraryMode) = File(app.filesDir, if (m == LibraryMode.Drive) "library-drive" else "library-local")

    /** Opens the library for the chosen mode (blocking; call on IO). */
    private fun openBlocking(m: LibraryMode) {
        val root = rootDir(m).also { it.mkdirs() }
        val cache = File(app.cacheDir, "lunta-${m.name}").also { it.mkdirs() }
        val device = settings.deviceId
        val lib = Library.open(root.absolutePath, cache.absolutePath, device)
        val state = File(app.filesDir, "sync-state-${m.name}.json").absolutePath
        var s: LibrarySync? = null
        var auth: DriveAuth? = null
        if (m == LibraryMode.Drive) {
            auth = makeAuth()
            if (auth.isConnected()) {
                s = LibrarySync.googleDrive(root.absolutePath, device, state, auth, settings.driveFolder, true, progress)
            }
        } else if (settings.testRemoteDir.isNotBlank()) {
            s = LibrarySync.localDir(root.absolutePath, device, state, settings.testRemoteDir, true, progress)
        }
        library = lib; librarySync = s; driveAuth = auth
        positions = Positions(root, device, "${Build.MODEL} (Android)")
    }

    private fun makeAuth() = DriveAuth(
        BuildConfig.GOOGLE_CLIENT_ID, File(app.noBackupFilesDir, "google-tokens.json").absolutePath, true,
    )

    /** Called at start-up. */
    fun start() {
        if (mode == LibraryMode.None || opened) return
        scope.launch {
            try {
                withContext(Dispatchers.IO) { openBlocking(mode) }
                opened = true
                reload()
                SyncWorker.schedule(app, settings.syncMinutes, librarySync != null)
                syncNow()
            } catch (e: Exception) {
                error = "Couldn't open the library: ${e.message}"
            }
        }
    }

    /** Waits until the library is open (for the background worker). */
    suspend fun ensureOpen() {
        if (opened || mode == LibraryMode.None) return
        withContext(Dispatchers.IO) { synchronized(this@AppModel) { if (library == null) openBlocking(mode) } }
        opened = true
        reload()
    }

    fun chooseLocal(remoteDir: String = "") {
        scope.launch {
            try {
                settings.testRemoteDir = remoteDir
                settings.mode = LibraryMode.Local
                mode = LibraryMode.Local
                withContext(Dispatchers.IO) { openBlocking(LibraryMode.Local) }
                opened = true
                reload()
                resetTo(Screen.Home)
                SyncWorker.schedule(app, settings.syncMinutes, librarySync != null)
                if (librarySync != null) syncNow()
            } catch (e: Exception) {
                error = "Couldn't create the library: ${e.message}"
            }
        }
    }

    /** Forgets the choice of library (the files stay on the device) and returns to the setup screen. */
    fun leaveLibrary(disconnect: Boolean) {
        scope.launch {
            withContext(Dispatchers.IO) { if (disconnect) runCatching { driveAuth?.disconnect() } }
            SyncWorker.cancel(app)
            library = null; librarySync = null; driveAuth = null; positions = null
            opened = false
            papers = emptyList(); collections = emptyList(); tags = emptyList()
            settings.mode = LibraryMode.None
            mode = LibraryMode.None
            resetTo(Screen.Home)
        }
    }

    // ---- Google sign-in (Custom Tab + PKCE; the redirect comes back to MainActivity) ----

    fun beginGoogleSignIn(context: Context) {
        if (clientIdMissing) {
            error = "Google Drive sign-in isn't set up in this build: there is no Google OAuth client id. " +
                "Create an \"Android\" OAuth client in Google Cloud Console (package app.lunta) and put " +
                "lunta.googleClientId=<id>.apps.googleusercontent.com in android/local.properties, then rebuild. " +
                "Meanwhile you can use a library on this device."
            return
        }
        try {
            val auth = driveAuth ?: makeAuth().also { driveAuth = it }
            val redirect = "${BuildConfig.REDIRECT_SCHEME}:/oauth2redirect"
            val req = auth.beginSignIn(redirect)
            settings.pendingVerifier = req.verifier
            settings.pendingState = req.state
            settings.pendingRedirect = redirect
            CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(req.url))
        } catch (e: Exception) {
            error = "Couldn't start Google sign-in: ${e.message}"
        }
    }

    fun finishGoogleSignIn(redirectUrl: String) {
        scope.launch {
            try {
                status = "Signing in to Google…"
                withContext(Dispatchers.IO) {
                    val auth = driveAuth ?: makeAuth().also { driveAuth = it }
                    val code = auth.codeFromRedirect(redirectUrl, settings.pendingState)
                    auth.completeSignIn(code, settings.pendingVerifier, settings.pendingRedirect)
                }
                settings.mode = LibraryMode.Drive
                mode = LibraryMode.Drive
                withContext(Dispatchers.IO) { openBlocking(LibraryMode.Drive) }
                opened = true
                reload()
                resetTo(Screen.Home)
                SyncWorker.schedule(app, settings.syncMinutes, true)
                status = null
                syncNow()
            } catch (e: Exception) {
                status = null
                error = e.message ?: "Sign-in failed"
            }
        }
    }

    // ---- reading the library ----

    suspend fun reload() {
        val lib = library ?: return
        val (ps, cols, tg, sf) = withContext(Dispatchers.IO) {
            Quad(
                lib.papers(PaperFilter(null, null, false, false), sort),
                lib.collections(),
                lib.tags(),
                SmartFolder.load(rootDir(mode)),
            )
        }
        papers = ps; collections = cols; tags = tg; smartFolders = sf
    }

    private data class Quad<A, B, C, D>(val a: A, val b: B, val c: C, val d: D)

    fun changeSort(s: SortKey) {
        sort = s
        settings.sort = s.name
        scope.launch { reload() }
    }

    fun paper(id: String): Paper? = papers.firstOrNull { it.id == id }

    fun placeTitle(p: Place): String = when (p) {
        Place.All -> "All Papers"
        Place.Starred -> "Starred"
        Place.Unread -> "Unread"
        Place.ForYou -> "New for You"
        is Place.Folder -> collections.firstOrNull { it.id == p.id }?.name ?: "Folder"
        is Place.Smart -> smartFolders.firstOrNull { it.id == p.id }?.name ?: "Smart Folder"
        is Place.Tag -> p.name
    }

    /** The papers of a place, optionally narrowed by a full-text search, in the chosen order. */
    suspend fun rows(place: Place, query: String): List<Row> {
        val lib = library ?: return emptyList()
        val q = query.trim()
        return withContext(Dispatchers.IO) {
            val base: List<Paper> = when (place) {
                Place.All -> papers
                Place.Starred -> papers.filter { it.starred }
                Place.Unread -> papers.filter { !it.read }
                Place.ForYou -> emptyList()
                is Place.Folder -> {
                    val ids = descendants(place.id)
                    papers.filter { p -> p.collections.any { it in ids } }
                }
                is Place.Smart -> {
                    val f = smartFolders.firstOrNull { it.id == place.id }
                    if (f == null) emptyList() else {
                        val hits = f.text.trim().takeIf { it.isNotEmpty() }?.let { t -> runCatching { lib.search(t, 2000u).map { it.paper.id }.toSet() }.getOrDefault(emptySet()) }
                        val now = System.currentTimeMillis()
                        papers.filter { f.matches(it, hits, now) }
                    }
                }
                is Place.Tag -> papers.filter { place.name in it.tags }
            }
            if (q.isEmpty()) base.map { Row(it) } else {
                val inBase = base.associateBy { it.id }
                val hits = runCatching { lib.search(toFtsQuery(q), 300u) }.getOrDefault(emptyList())
                hits.mapNotNull { h -> inBase[h.paper.id]?.let { Row(it, h.snippet) } }
            }
        }
    }

    private fun toFtsQuery(q: String): String = q

    fun descendants(id: String): Set<String> {
        val out = mutableSetOf(id)
        var grew = true
        while (grew) {
            grew = false
            for (c in collections) if (c.parent != null && c.parent in out && out.add(c.id)) grew = true
        }
        return out
    }

    fun count(place: Place): Int = when (place) {
        Place.All -> papers.size
        Place.Starred -> papers.count { it.starred }
        Place.Unread -> papers.count { !it.read }
        is Place.Tag -> papers.count { place.name in it.tags }
        is Place.Folder -> { val ids = descendants(place.id); papers.count { p -> p.collections.any { it in ids } } }
        else -> 0
    }

    // ---- changes ----

    private fun launchLib(failure: String? = null, block: suspend (Library) -> Unit) {
        val lib = library ?: return
        scope.launch {
            try { block(lib); reload() } catch (e: Exception) { error = (failure?.let { "$it: " } ?: "") + (e.message ?: e.toString()) }
        }
    }

    fun toggleStar(p: Paper) = launchLib { withContext(Dispatchers.IO) { it.setStarred(p.id, !p.starred) } }
    fun toggleRead(p: Paper) = launchLib { withContext(Dispatchers.IO) { it.setRead(p.id, !p.read) } }
    fun markRead(id: String) { if (paper(id)?.read == false) launchLib { withContext(Dispatchers.IO) { it.setRead(id, true) } } }
    fun setTag(p: Paper, tag: String, on: Boolean) = launchLib { withContext(Dispatchers.IO) { it.setTag(p.id, tag, on) } }
    fun setMember(p: Paper, collection: String, on: Boolean) = launchLib { withContext(Dispatchers.IO) { it.setMembership(collection, p.id, on) } }
    fun newFolder(name: String, parent: String? = null, then: ((PaperCollection) -> Unit)? = null) = launchLib {
        val c = withContext(Dispatchers.IO) { it.createCollection(name.trim(), parent) }
        then?.invoke(c)
    }
    fun renameFolder(id: String, name: String) = launchLib { withContext(Dispatchers.IO) { it.renameCollection(id, name.trim()) } }
    fun deleteFolder(id: String) = launchLib { withContext(Dispatchers.IO) { it.deleteCollection(id) } }
    fun deletePaper(id: String) = launchLib { withContext(Dispatchers.IO) { it.deletePaper(id) } }

    fun cite(p: Paper, style: CitationStyle): String? = library?.citation(p.id, style)

    // ---- adding ----

    private fun <T> busy(first: String, work: suspend () -> T, done: suspend (T) -> Unit) {
        scope.launch {
            status = first
            try { done(work()) } catch (e: Exception) { error = e.message ?: e.toString() } finally { status = null }
        }
    }

    private suspend fun added(out: AddOutcome) {
        reload()
        resetTo(Screen.Home)
        push(Screen.Papers(Place.All))
        push(Screen.Info(out.paper.id))
        out.note?.let { info = "Added “${out.paper.meta.title}”, but no reading view could be made: $it" }
    }

    /** DOI, PMCID, arXiv id or a link. */
    fun addByInput(text: String) {
        val lib = library ?: return
        busy("Looking up…", { withContext(Dispatchers.IO) { lib.addFromInput(text, progress) } }) { added(it) }
    }

    fun addPdf(stream: InputStream, name: String) {
        val lib = library ?: return
        busy("Reading the PDF…", {
            withContext(Dispatchers.IO) {
                val f = File(app.cacheDir, "import-${System.nanoTime()}.pdf")
                stream.use { i -> f.outputStream().use { o -> i.copyTo(o) } }
                try { lib.addFromPdf(f.absolutePath, progress) } finally { f.delete() }
            }
        }) { added(it) }
    }

    /** A paper that has none gets this PDF (and, if it has no reading view, one made from it). */
    fun attachPdf(id: String, stream: InputStream) {
        val lib = library ?: return
        busy("Adding the PDF…", {
            withContext(Dispatchers.IO) {
                val f = File(app.cacheDir, "attach-${System.nanoTime()}.pdf")
                stream.use { i -> f.outputStream().use { o -> i.copyTo(o) } }
                try { lib.attachPdfFile(id, f.absolutePath, progress) } finally { f.delete() }
            }
        }) { note ->
            reload()
            note?.let { info = "The PDF was added, but no reading view could be made: $it" }
        }
    }

    fun findPdf(id: String) {
        val lib = library ?: return
        busy("Looking for an open-access PDF…", { withContext(Dispatchers.IO) { lib.findPdf(id) } }) { src ->
            reload()
            info = if (src != null) "Found the PDF at $src." else "No open-access PDF found for this paper."
            if (src != null && paper(id)?.hasEpub == false) makeReadingView(id)
        }
    }

    fun makeReadingView(id: String) {
        val lib = library ?: return
        busy("Making the reading view…", { withContext(Dispatchers.IO) { lib.makeReadingView(id, progress) } }) { reload() }
    }

    /** Handles text shared from another app: a DOI or a link. */
    fun handleSharedText(text: String) {
        val doi = extractDoi(text)
        val url = Regex("https?://\\S+").find(text)?.value
        val input = if (doi.isNotEmpty()) doi else url ?: text.trim()
        if (input.isBlank()) { error = "Nothing to add in what was shared."; return }
        addByInput(input)
    }

    // ---- files, fetched on demand when the library is synced lazily ----

    /** Names of the files (book.epub, paper.pdf, …) of a paper that exist on the remote or here. */
    suspend fun availableFiles(id: String): Set<String> = withContext(Dispatchers.IO) {
        val names = librarySync?.paperFiles(id)?.map { it.name }?.toMutableSet() ?: mutableSetOf()
        val lib = library
        if (lib != null) {
            if (lib.filePath(id, FileKind.EPUB) != null) names += "book.epub"
            if (lib.filePath(id, FileKind.PDF) != null) names += "paper.pdf"
        }
        names
    }

    /** Makes sure the given kinds are on the device; returns the path of the first one that is. */
    suspend fun ensureFiles(id: String, vararg kinds: FileKind) {
        val lib = library ?: return
        withContext(Dispatchers.IO) {
            val missing = kinds.filter { lib.filePath(id, it) == null }
            if (missing.isNotEmpty()) {
                librarySync?.fetchPaper(id, missing)
                lib.reindexQuick()
            }
        }
        status = null
        reload()
    }

    // ---- syncing ----

    suspend fun syncNow() {
        val s = librarySync ?: run { refreshOnly(); return }
        if (!syncLock.tryLock()) return
        syncing = true
        try {
            val lib = library
            val r = withContext(Dispatchers.IO) {
                val r = s.syncNow()
                if (r.changed && lib != null) lib.refresh()
                r
            }
            settings.lastSyncMs = r.finishedMs
            lastSyncMs = r.finishedMs
            syncError = r.errors.firstOrNull()
            reload()
            withContext(Dispatchers.IO) { library?.let { while (it.indexFulltext(25u) > 0u) { /* indexed a batch */ } } }
        } catch (e: Exception) {
            syncError = e.message ?: e.toString()
        } finally {
            syncing = false
            status = null
            syncLock.unlock()
        }
    }

    private suspend fun refreshOnly() {
        val lib = library ?: return
        withContext(Dispatchers.IO) { lib.refresh() }
        reload()
        withContext(Dispatchers.IO) { while (lib.indexFulltext(25u) > 0u) { /* batch */ } }
    }

    fun refreshNow() { scope.launch { syncNow() } }
}

/** Runs [block] on the main thread once the library is open. */
fun kotlinx.coroutines.CoroutineScope.launchWhenOpen(model: AppModel, block: () -> Unit) {
    launch {
        var waited = 0
        while (!model.opened && waited < 200) { kotlinx.coroutines.delay(50); waited++ }
        if (model.opened) block() else model.error = "The library isn't open yet. Try again."
    }
}
