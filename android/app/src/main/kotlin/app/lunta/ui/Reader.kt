package app.lunta.ui

import android.content.Intent
import android.net.Uri
import android.view.ViewGroup
import android.webkit.WebView
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.FormatListBulleted
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import app.lunta.*
import kotlinx.coroutines.*
import org.json.JSONObject
import org.json.JSONTokener
import uniffi.lunta_core.*
import uniffi.lunta_core.Annotation
import java.io.File
import kotlin.coroutines.resume

private val COLORS = listOf("yellow", "green", "blue", "pink")

/** The reader: the reading view (EPUB in a web view with the Apple apps' scripts) or the PDF. */
@Composable
fun ReaderScreen(model: AppModel, id: String, pdf: Boolean) {
    val p = model.paper(id)
    if (p == null) { LaunchedEffect(Unit) { model.pop() }; return }
    if (pdf) PdfReader(model, p) else EpubReader(model, p)
}

private data class TocItem(val title: String, val level: Int, val anchor: String)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun EpubReader(model: AppModel, paper: Paper) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val id = paper.id
    var book by remember { mutableStateOf<EpubBook?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var web by remember { mutableStateOf<LuntaWebView?>(null) }
    var chrome by remember { mutableStateOf(true) }
    var hasSelection by remember { mutableStateOf(false) }
    var showToc by remember { mutableStateOf(false) }
    var showList by remember { mutableStateOf(false) }
    var showText by remember { mutableStateOf(false) }
    var overflow by remember { mutableStateOf(false) }
    var scale by remember { mutableFloatStateOf(model.settings.readerScale) }
    var paged by remember { mutableStateOf(model.settings.readerPaged) }
    var annotations by remember { mutableStateOf<List<Annotation>>(emptyList()) }
    var editing by remember { mutableStateOf<Annotation?>(null) }
    var figure by remember { mutableStateOf<Pair<String, String>?>(null) }
    var resumeNote by remember { mutableStateOf<String?>(null) }
    var restored by remember { mutableStateOf(false) }
    val lib = model.library!!

    LaunchedEffect(id) {
        try {
            book = withContext(Dispatchers.IO) { lib.openEpub(id) }
        } catch (e: Exception) { failure = e.message ?: "Couldn't open the reading view" }
    }
    LaunchedEffect(Unit) { delay(2500); chrome = false }

    suspend fun js(code: String): String? = suspendCancellableCoroutine { c ->
        val w = web
        if (w == null) c.resume(null) else w.evaluateJavascript(code) { c.resume(it) }
    }
    fun jsString(code: String): suspend () -> String? = {
        js(code)?.takeIf { it != "null" }?.let { (JSONTokener(it).nextValue() as? String) }
    }

    suspend fun refreshHighlights() {
        val list = withContext(Dispatchers.IO) { lib.annotations(id) }
        annotations = list
        val items = org.json.JSONArray()
        for (a in list.filter { it.location == "epub" }) {
            items.put(JSONObject().put("id", a.id).put("quote", a.quote).put("prefix", a.prefix).put("suffix", a.suffix)
                .put("color", a.color).put("note", a.note).put("start", a.position.toIntOrNull() ?: 0))
        }
        js("luntaApplyAnnotations(${JSONObject.quote(items.toString())})")
    }

    suspend fun savePosition() {
        if (!restored) return
        val o = js("luntaReadingOffset()")?.toDoubleOrNull()?.toInt() ?: return
        withContext(Dispatchers.IO) { model.positions?.save(id, o) }
    }

    suspend fun pageReady() {
        refreshHighlights()
        if (restored) return
        val spot = withContext(Dispatchers.IO) { model.positions?.latest(id) }
        if (spot != null && spot.offset > 300) {
            delay(350)
            js("luntaScrollToOffset(${spot.offset})")
            resumeNote = if (spot.mine) "Continuing where you left off" else "Continuing where you left off on your ${spot.device}"
            delay(300)
        }
        restored = true
        scope.launch { delay(3000); resumeNote = null }
    }

    suspend fun addHighlight(color: String, thenEdit: Boolean = false) {
        val sel = jsString("luntaSelection()")() ?: run { model.info = "Select some text first, then highlight it."; return }
        val o = JSONObject(sel)
        val a = Annotation(
            "", id, AnnotationKind.HIGHLIGHT, color, "epub", o.optInt("start").toString(),
            o.optString("quote"), o.optString("prefix"), o.optString("suffix"), "", 0, 0,
        )
        val saved = withContext(Dispatchers.IO) { lib.saveAnnotation(a) }
        js("window.getSelection().removeAllRanges()")
        hasSelection = false
        model.reload()
        refreshHighlights()
        if (thenEdit) editing = saved
    }

    val bridge = remember {
        LuntaBridge { name, body ->
            val v: Any? = runCatching { JSONTokener(body).nextValue() }.getOrNull()
            when (name) {
                "ann" -> scope.launch {
                    val s = v as? String ?: return@launch
                    if (s == "__ready__") pageReady()
                    else withContext(Dispatchers.IO) { lib.annotations(id) }.firstOrNull { it.id == s }?.let { editing = it }
                }
                "page" -> if (v == "hello") scope.launch { js("luntaSetPaged(${model.settings.readerPaged}, 30, 40, false)") }
                "fig" -> (v as? JSONObject)?.let { figure = it.optString("src") to it.optString("caption") }
                "__selection" -> hasSelection = v == true
                "__chrome" -> chrome = !chrome
                "__link" -> (v as? String)?.let { runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(it)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } }
            }
        }
    }

    fun close() { scope.launch { savePosition(); model.pop() } }
    BackHandler { if (showToc || showList || showText) { showToc = false; showList = false; showText = false } else close() }
    LifecycleEventEffect(Lifecycle.Event.ON_PAUSE) { scope.launch { savePosition() } }
    LaunchedEffect(restored) { while (restored) { delay(6000); savePosition() } }

    val toc = remember(book) {
        val b = book ?: return@remember emptyList<TocItem>()
        b.toc.map { t ->
            val anchor = if (t.target.contains('#')) t.target.substringAfter('#') else "section-" + b.chapters.indexOfFirst { it.path == t.target }.coerceAtLeast(0)
            TocItem(t.title, t.level.toInt(), anchor)
        }
    }

    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        val b = book
        if (failure != null) {
            Column(Modifier.align(Alignment.Center).padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text("No reading view: $failure")
                Spacer(Modifier.height(12.dp))
                Button({ model.pop() }) { Text("Back") }
            }
        } else if (b != null) {
            val bg = MaterialTheme.colorScheme.background
            val dir = remember(b) { File(b.dir) }
            val url = remember(b) {
                val rel = File(b.combined).relativeTo(dir).path.split(File.separatorChar).joinToString("/") { Uri.encode(it) }
                "https://$READER_HOST/epub/$rel?scale=${model.settings.readerScale}&paged=${if (model.settings.readerPaged) 1 else 0}"
            }
            AndroidView(
                modifier = Modifier.fillMaxSize().systemBarsPadding(),
                factory = { c ->
                    LuntaWebView(c, dir, bridge).apply {
                        layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                        setBackgroundColor(bg.toArgb())
                        web = this
                        loadUrl(url)
                    }
                },
                onRelease = { it.destroy(); if (web === it) web = null },
            )
        } else {
            CircularProgressIndicator(Modifier.align(Alignment.Center))
        }

        AnimatedVisibility(chrome, Modifier.align(Alignment.TopCenter), enter = fadeIn(), exit = fadeOut()) {
            Surface(color = MaterialTheme.colorScheme.surface.copy(alpha = .96f), shadowElevation = 3.dp) {
                Row(Modifier.statusBarsPadding().height(56.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    IconButton({ close() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
                    Text(paper.meta.title, Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    IconButton({ showToc = true }) { Icon(Icons.AutoMirrored.Filled.FormatListBulleted, "Contents") }
                    IconButton({ showList = true; scope.launch { refreshHighlights() } }) {
                        BadgedBox({ if (annotations.isNotEmpty()) Badge { Text(annotations.size.toString()) } }) { Icon(Icons.Outlined.Highlight, "Highlights") }
                    }
                    IconButton({ showText = true }) { Icon(Icons.Filled.TextFields, "Text size and layout") }
                    Box {
                        IconButton({ overflow = true }) { Icon(Icons.Filled.MoreVert, "More") }
                        DropdownMenu(overflow, { overflow = false }) {
                            DropdownMenuItem({ Text("Paper info") }, { overflow = false; close() })
                            if (paper.hasPdf || model.librarySync?.paperFiles(id)?.any { it.name == "paper.pdf" } == true) {
                                DropdownMenuItem({ Text("Open the PDF") }, {
                                    overflow = false
                                    scope.launch { savePosition(); model.ensureFiles(id, FileKind.PDF); model.pop(); model.push(Screen.Reader(id, true)) }
                                })
                            }
                            DropdownMenuItem({ Text(if (paper.read) "Mark as unread" else "Mark as read") }, { overflow = false; model.toggleRead(paper) })
                        }
                    }
                }
            }
        }

        resumeNote?.let { n ->
            Surface(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(24.dp), shape = MaterialTheme.shapes.large, tonalElevation = 6.dp, shadowElevation = 3.dp) {
                Text("🔖  $n", Modifier.padding(horizontal = 16.dp, vertical = 10.dp), style = MaterialTheme.typography.bodySmall)
            }
        }

        // the highlight bar, while text is selected
        AnimatedVisibility(hasSelection, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(16.dp), enter = fadeIn(), exit = fadeOut()) {
            Surface(shape = CircleShape, tonalElevation = 8.dp, shadowElevation = 6.dp) {
                Row(Modifier.padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    for (c in COLORS) {
                        Box(Modifier.size(34.dp).clip(CircleShape).background(HighlightColors.getValue(c)).clickable { scope.launch { addHighlight(c) } }.semantics { contentDescription = "Highlight $c" })
                    }
                    TextButton({ scope.launch { addHighlight("yellow", thenEdit = true) } }) { Icon(Icons.Outlined.EditNote, null); Spacer(Modifier.width(4.dp)); Text("Note") }
                }
            }
        }
    }

    // contents
    if (showToc) ModalBottomSheet({ showToc = false }) {
        LazyColumn(Modifier.padding(bottom = 24.dp)) {
            item { Text("Contents", Modifier.padding(horizontal = 24.dp, vertical = 8.dp), style = MaterialTheme.typography.titleLarge) }
            items(toc) { t ->
                Text(
                    t.title, Modifier.fillMaxWidth().clickable {
                        showToc = false; chrome = false
                        val safe = t.anchor.replace("\\", "").replace("'", "\\'")
                        scope.launch { js("var e=document.getElementById('$safe'); if(e) e.scrollIntoView({behavior:'smooth', block:'start'});") }
                    }.padding(start = 24.dp + 16.dp * (t.level - 1).coerceAtLeast(0), end = 24.dp, top = 10.dp, bottom = 10.dp),
                    style = if (t.level <= 1) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }

    // highlights and notes
    if (showList) ModalBottomSheet({ showList = false }) {
        LazyColumn(Modifier.padding(bottom = 24.dp)) {
            item { Text("Highlights & Notes", Modifier.padding(horizontal = 24.dp, vertical = 8.dp), style = MaterialTheme.typography.titleLarge) }
            if (annotations.isEmpty()) item {
                Text("No highlights yet. Select text in the reading view, then pick a colour.", Modifier.padding(horizontal = 24.dp, vertical = 8.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            val sorted = annotations.sortedBy { it.position.toIntOrNull() ?: 0 }
            items(sorted, key = { it.id }) { a ->
                Row(
                    Modifier.fillMaxWidth().clickable {
                        if (a.location != "epub") model.info = "This highlight was made in the PDF on another device; PDF highlights show on the Mac and iPhone for now."
                        else { showList = false; chrome = false; scope.launch { js("luntaShowAnnotation('${a.id.filter { it.isLetterOrDigit() || it == '-' }}')") } }
                    }.padding(horizontal = 24.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.Top,
                ) {
                    Box(Modifier.padding(top = 2.dp).width(4.dp).height(40.dp).clip(CircleShape).background(HighlightColors[a.color] ?: HighlightColors.getValue("yellow")))
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(a.quote.trim(), style = MaterialTheme.typography.bodyMedium, maxLines = 4, overflow = TextOverflow.Ellipsis)
                        if (a.note.isNotBlank()) Text("✎ ${a.note.trim()}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
                        if (a.location != "epub") Text("in the PDF", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.outline)
                    }
                    IconButton({ editing = a }) { Icon(Icons.Outlined.Edit, "Edit") }
                }
            }
            if (annotations.isNotEmpty()) item {
                TextButton({
                    val md = "## ${paper.meta.title}\n\n" + sorted.joinToString("") { "> ${it.quote.trim()}\n${if (it.note.isNotBlank()) "\n${it.note.trim()}\n" else ""}\n" }
                    copyText(ctx, "highlights as Markdown", md)
                }, Modifier.padding(horizontal = 16.dp)) { Text("Copy all as Markdown") }
            }
        }
    }

    // text size and layout
    if (showText) ModalBottomSheet({ showText = false }) {
        Column(Modifier.padding(horizontal = 24.dp).padding(bottom = 32.dp)) {
            Text("Text & Layout", style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("A", style = MaterialTheme.typography.bodySmall)
                Slider(
                    scale, {
                        scale = (Math.round(it * 20f) / 20f)
                    }, Modifier.weight(1f).padding(horizontal = 12.dp), valueRange = 0.7f..1.8f,
                    onValueChangeFinished = {
                        model.settings.readerScale = scale
                        scope.launch { js("luntaSetTextScale($scale)") }
                    },
                )
                Text("A", style = MaterialTheme.typography.headlineSmall)
            }
            Spacer(Modifier.height(8.dp))
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                SegmentedButton(!paged, { paged = false; model.settings.readerPaged = false; scope.launch { js("luntaSetPaged(false, 30, 40, false)") } }, SegmentedButtonDefaults.itemShape(0, 2)) { Text("Scroll") }
                SegmentedButton(paged, { paged = true; model.settings.readerPaged = true; scope.launch { js("luntaSetPaged(true, 30, 40, false)") } }, SegmentedButtonDefaults.itemShape(1, 2)) { Text("Pages") }
            }
            Text("In page mode, swipe or tap the edges of the screen to turn pages.", Modifier.padding(top = 8.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }

    editing?.let { a ->
        AnnotationDialog(a, ctx, { editing = null }, onDelete = {
            scope.launch { withContext(Dispatchers.IO) { lib.deleteAnnotation(a.paperId, a.id) }; editing = null; model.reload(); refreshHighlights() }
        }, onSave = { color, note ->
            scope.launch {
                withContext(Dispatchers.IO) {
                    lib.saveAnnotation(a.copy(color = color, note = note.trim(), kind = if (note.isBlank()) AnnotationKind.HIGHLIGHT else AnnotationKind.NOTE))
                }
                editing = null; model.reload(); refreshHighlights()
            }
        })
    }

    figure?.let { (src, caption) -> FigureDialog(src, caption, book?.let { File(it.dir) }) { figure = null } }
}

@Composable
private fun AnnotationDialog(a: Annotation, ctx: android.content.Context, onDismiss: () -> Unit, onDelete: () -> Unit, onSave: (String, String) -> Unit) {
    var color by remember { mutableStateOf(a.color.ifBlank { "yellow" }) }
    var note by remember { mutableStateOf(a.note.trim()) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (a.note.isNotBlank()) "Note" else "Highlight") },
        text = {
            Column {
                Text("“${a.quote.trim()}”", style = MaterialTheme.typography.bodySmall, maxLines = 6, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.height(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    for (c in COLORS) {
                        Box(
                            Modifier.size(36.dp).clip(CircleShape).background(HighlightColors.getValue(c))
                                .border(if (c == color) 3.dp else 0.dp, MaterialTheme.colorScheme.onSurface, CircleShape).clickable { color = c },
                        )
                    }
                }
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(note, { note = it }, Modifier.fillMaxWidth(), label = { Text("Note") }, minLines = 3)
                Row {
                    TextButton({ copyText(ctx, "quote", a.quote.trim()) }) { Text("Copy quote") }
                    TextButton(onDelete, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Delete") }
                }
            }
        },
        confirmButton = { TextButton({ onSave(color, note) }) { Text("Save") } },
        dismissButton = { TextButton(onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun FigureDialog(src: String, caption: String, dir: File?, onDismiss: () -> Unit) {
    val ctx = LocalContext.current
    androidx.compose.ui.window.Dialog(onDismiss, androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize(), color = Color.Black.copy(alpha = .94f)) {
            Column(Modifier.systemBarsPadding()) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) { IconButton(onDismiss) { Icon(Icons.Filled.Close, "Close", tint = Color.White) } }
                if (dir != null) AndroidView(modifier = Modifier.weight(1f).fillMaxWidth(), factory = { c ->
                    LuntaWebView(c, dir, LuntaBridge { _, _ -> }).apply {
                        settings.setSupportZoom(true); settings.builtInZoomControls = true; settings.displayZoomControls = false
                        settings.useWideViewPort = true; settings.loadWithOverviewMode = true
                        setBackgroundColor(android.graphics.Color.TRANSPARENT)
                        val safe = android.text.TextUtils.htmlEncode(src)
                        loadDataWithBaseURL("https://$READER_HOST/", "<html><body style='margin:0;background:#fff'><img src=\"$safe\" style='width:100%;height:auto'></body></html>", "text/html", "utf-8", null)
                    }
                })
                if (caption.isNotBlank()) Text(caption, Modifier.padding(16.dp), color = Color.White, style = MaterialTheme.typography.bodySmall, maxLines = 4)
            }
        }
    }
}
