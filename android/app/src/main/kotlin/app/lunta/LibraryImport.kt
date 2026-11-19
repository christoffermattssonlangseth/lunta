package app.lunta

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.lunta_core.ImportProgress
import uniffi.lunta_core.ImportSource
import uniffi.lunta_core.discardImportSource
import uniffi.lunta_core.openImportSource
import java.io.File

/**
 * Import Library (Zotero, Mendeley, ReadCube, EndNote…) and Share as BibTeX / RIS. What was picked
 * through the Storage Access Framework (a .bib / .ris / .zip, or the folder Zotero's "Export Files"
 * makes, in Downloads or Google Drive) is copied into the app's cache, since content URIs aren't
 * paths; the core (core/src/import_source.rs, as on Windows and Linux) finds the export and its
 * PDFs, imports in batches and matches the folder's other PDFs. Reading views are made afterwards.
 */
class LibraryImport(private val model: AppModel) {
    /** Opened and waiting for "Put the imported papers in a folder?". */
    var pending by mutableStateOf<ImportSource?>(null)
        private set
    var running by mutableStateOf(false)
        private set
    /** "Import a library": what to pick (ImportDialogs shows the choice and the pickers). */
    var choosing by mutableStateOf(false)

    private val app get() = model.app
    /** The copy of what was picked, removed when the import is done or cancelled. */
    private var copy: File? = null
    private var makingViews = false

    /** Import Library…: the choice of a folder or a file, then the picker. */
    fun ask() {
        if (running || pending != null) model.error = "An import is already running." else choosing = true
    }

    /** A .bib, .ris (or EndNote .txt) or ZIP file. */
    fun pickedFile(uri: Uri) = open { dir ->
        val name = displayName(uri) ?: "export.bib"
        val f = File(dir, safe(name))
        app.contentResolver.openInputStream(uri)?.use { i -> f.outputStream().use { o -> i.copyTo(o) } }
            ?: error("Couldn't read “$name”.")
        f
    }

    /** A folder: the export and the PDFs in it are copied (a Zotero export: the .bib and files/…). */
    fun pickedFolder(tree: Uri) = open { dir ->
        val rootId = DocumentsContract.getTreeDocumentId(tree)
        val name = displayName(DocumentsContract.buildDocumentUriUsingTree(tree, rootId)) ?: "Export"
        val top = File(dir, safe(name)).also { it.mkdirs() }
        var n = 0
        suspend fun walk(docId: String, into: File) {
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
            val cols = arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE)
            app.contentResolver.query(children, cols, null, null, null)?.use { c ->
                while (c.moveToNext()) {
                    val id = c.getString(0)
                    val child = c.getString(1) ?: continue
                    if (child.startsWith(".")) continue
                    if (c.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) {
                        walk(id, File(into, safe(child)).also { it.mkdirs() })
                    } else if (child.substringAfterLast('.', "").lowercase() in setOf("bib", "bibtex", "ris", "txt", "pdf")) {
                        n++
                        withMain { model.status = "Copying files from “$name”: $n" }
                        val out = File(into, safe(child))
                        app.contentResolver.openInputStream(DocumentsContract.buildDocumentUriUsingTree(tree, id))?.use { i ->
                            out.outputStream().use { o -> i.copyTo(o) }
                        }
                    }
                }
            }
        }
        walk(rootId, top)
        top
    }

    /** Copies what was picked (on IO), opens it with the core, then asks for the folder. */
    private fun open(copyIn: suspend (File) -> File) {
        if (running || pending != null) { model.error = "An import is already running."; return }
        running = true
        model.scope.launch {
            model.status = "Opening the export…"
            val dir = File(app.cacheDir, "import-copy-${System.nanoTime()}").also { it.mkdirs() }
            copy = dir
            try {
                pending = withContext(Dispatchers.IO) {
                    val picked = copyIn(dir)
                    openImportSource(picked.absolutePath, app.cacheDir.absolutePath)
                }
            } catch (e: Exception) {
                cleanUp()
                running = false
                model.error = e.message ?: e.toString()
            } finally {
                model.status = null
            }
        }
    }

    fun cancel() {
        pending?.let { discardImportSource(it) }
        pending = null
        cleanUp()
        running = false
    }

    /** The answer to the folder question: a name, or empty for none. */
    fun start(folder: String) {
        val source = pending ?: return
        val lib = model.library ?: return cancel()
        pending = null
        model.scope.launch {
            val progress = object : ImportProgress {
                override fun onImportProgress(message: String, done: UInt, total: UInt) {
                    model.scope.launch { if (running) model.status = message }
                }
            }
            try {
                val s = withContext(Dispatchers.IO) { lib.importLibrary(source, folder.trim().ifEmpty { null }, progress) }
                model.reload()
                val views = s.paperIds.isNotEmpty()
                model.info = s.text + if (views) "\n\nReading views are made now, one paper after another, while Lunta is open." else ""
                if (views) makeReadingViews(s.paperIds)
            } catch (e: Exception) {
                model.error = "Import failed: ${e.message ?: e}"
                model.reload()
            } finally {
                cleanUp()
                running = false
                model.status = null
            }
        }
    }

    /** Reading views for the papers that have none, one after another (network and the converter). */
    fun makeReadingViews(ids: List<String>) {
        val lib = model.library ?: return
        if (makingViews) return
        makingViews = true
        model.scope.launch {
            val todo = ids.filter { model.paper(it)?.hasEpub == false }
            for ((i, id) in todo.withIndex()) {
                if (!running) model.status = "Making reading views: ${i + 1} of ${todo.size}"
                withContext(Dispatchers.IO) { runCatching { lib.makeReadingView(id, null) } }
                if ((i + 1) % 10 == 0) model.reload()
            }
            makingViews = false
            if (todo.isNotEmpty()) {
                model.status = null
                model.reload()
            }
        }
    }

    private fun cleanUp() {
        copy?.deleteRecursively()
        copy = null
    }

    private suspend fun withMain(block: () -> Unit) = withContext(Dispatchers.Main) { block() }

    private fun displayName(uri: Uri): String? = runCatching {
        app.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull()

    private fun safe(name: String) = name.replace('/', '_').replace('\\', '_').ifBlank { "file" }

    companion object {
        /**
         * Shares papers as a .bib or .ris file through the share sheet (mail, Drive, Overleaf…):
         * the core's BibTeX / RIS, the same as the desktop's Copy BibTeX.
         */
        fun share(model: AppModel, context: Context, ids: List<String>, ris: Boolean, name: String) {
            val lib = model.library ?: return
            if (ids.isEmpty()) return
            model.scope.launch {
                val file = withContext(Dispatchers.IO) {
                    val text = if (ris) lib.exportRis(ids) else lib.exportBibtex(ids)
                    val dir = File(context.cacheDir, "exports").also { it.deleteRecursively(); it.mkdirs() }
                    val base = name.replace(Regex("[^\\p{L}\\p{N} ._-]+"), " ").trim().take(60).ifBlank { "Lunta" }
                    File(dir, base + if (ris) ".ris" else ".bib").also { it.writeText(text) }
                }
                val uri = FileProvider.getUriForFile(context, "${context.packageName}.exports", file)
                val mime = if (ris) "application/x-research-info-systems" else "application/x-bibtex"
                val send = Intent(Intent.ACTION_SEND).apply {
                    type = mime
                    putExtra(Intent.EXTRA_STREAM, uri)
                    putExtra(Intent.EXTRA_SUBJECT, file.name)
                    clipData = ClipData.newUri(context.contentResolver, file.name, uri)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                runCatching {
                    context.startActivity(Intent.createChooser(send, if (ris) "Share as RIS" else "Share as BibTeX"))
                }.onFailure { model.error = "Couldn't share: ${it.message}" }
            }
        }
    }
}

/** The FileProvider for shared .bib / .ris files (cache/exports/); its own class, so it doesn't
 *  clash with the updater's FileProvider in the GitHub build's manifest. */
class ExportFileProvider : FileProvider()
