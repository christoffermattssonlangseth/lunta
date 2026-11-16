package app.lunta.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.RectF
import com.caverock.androidsvg.SVG
import android.net.Uri
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Subtitles
import androidx.compose.material.icons.outlined.BrokenImage
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.SubtitlesOff
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import kotlin.math.max
import kotlin.math.min

// Figures in the reading view (apple/Shared/FigureViewer.swift). The page's figure script (in the
// generated style.js) links figure mentions, shows figures as cards and posts "fig" messages; this is
// the app's side: the figure on its own (image large and zoomable, label, "Panel B · 3 of 16", the
// caption below; swipe for the paper's other figures) and the grid of all figures. Both are drawn
// over the reader, which is left as it is, so closing returns to exactly the same place.

/** A highlight in a figure's caption: its text and colour. */
data class FigMark(val text: String, val color: String)

data class ReaderFigure(
    val index: Int, val images: List<String>, val label: String, val title: String, val caption: String, val marks: List<FigMark>,
)

/** What the viewer opens: figure [index] of [set], at image [start], pointing to [panel] ("B" in "Fig. 2B"). */
data class FigureOpen(val set: List<ReaderFigure>, val index: Int, val start: Int, val panel: String)

private fun figure(o: JSONObject, i: Int): ReaderFigure {
    val srcs = o.optJSONArray("srcs")?.let { a -> (0 until a.length()).map { a.optString(it) }.filter { it.isNotEmpty() } }
        ?: listOfNotNull(o.optString("src").takeIf { it.isNotEmpty() })
    val marks = o.optJSONArray("marks")?.let { a ->
        (0 until a.length()).mapNotNull { a.optJSONObject(it) }.mapNotNull { m ->
            m.optString("t").takeIf { it.isNotEmpty() }?.let { FigMark(it, m.optString("c").takeIf { c -> c in HighlightColors } ?: "yellow") }
        }
    } ?: emptyList()
    fun s(k: String) = o.optString(k).trim()
    return ReaderFigure(o.optInt("index", i), srcs, s("label"), s("title"), s("caption"), marks)
}

/** The paper's figures, from the page's {list: […]} message (sent as it loads). */
fun figureList(v: Any?): List<ReaderFigure>? {
    val a = (v as? JSONObject)?.optJSONArray("list") ?: return null
    return (0 until a.length()).mapNotNull { a.optJSONObject(it) }.mapIndexed { i, o -> figure(o, i) }
}

/** A tapped mention, card or image ({all, index, start, panel}), or a lone image ({srcs, label, …}). */
fun figureOpen(v: Any?): FigureOpen? {
    val o = v as? JSONObject ?: return null
    val all: JSONArray? = o.optJSONArray("all")
    if (all != null && o.has("index")) {
        val set = (0 until all.length()).mapNotNull { all.optJSONObject(it) }.mapIndexed { i, x -> figure(x, i) }
        val i = o.optInt("index")
        if (i in set.indices) {
            val start = o.optInt("start").coerceIn(0, max(0, set[i].images.size - 1))
            return FigureOpen(set, i, start, o.optString("panel").trim())
        }
    }
    val f = figure(o, 0)
    if (f.images.isEmpty() && f.caption.isEmpty() && f.title.isEmpty()) return null
    return FigureOpen(listOf(f), 0, 0, "")
}

/** The file behind an image URL of the page (https://lunta.local/epub/…), inside the book's folder. */
fun figureFile(bookDir: File, src: String): File? {
    val u = runCatching { Uri.parse(src) }.getOrNull() ?: return null
    if (u.host != READER_HOST) return null
    val path = u.path ?: return null
    if (!path.startsWith("/epub/")) return null
    val f = File(bookDir, path.removePrefix("/epub/")).canonicalFile
    return f.takeIf { it.path.startsWith(bookDir.canonicalPath + File.separator) && it.isFile }
}

/** An image, made smaller than [maxSide] pixels on its longest side (off the main thread). SVG
 *  figures (which BitmapFactory can't read) are drawn with AndroidSVG at that size. */
private suspend fun decodeImage(f: File?, maxSide: Int): ImageBitmap? = if (f == null) null else withContext(Dispatchers.IO) {
    runCatching {
        if (isSvg(f)) return@runCatching renderSvg(f, maxSide)
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(f.path, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return@runCatching null
        var sample = 1
        while (max(bounds.outWidth, bounds.outHeight) / sample > maxSide) sample *= 2
        BitmapFactory.decodeFile(f.path, BitmapFactory.Options().apply { inSampleSize = sample })?.asImageBitmap()
    }.getOrNull()
}

private fun isSvg(f: File): Boolean {
    val ext = f.extension.lowercase()
    if (ext == "svg") return true
    if (ext in setOf("png", "jpg", "jpeg", "gif", "webp", "bmp")) return false
    // no telling extension: look at the start of the file
    val head = runCatching { f.inputStream().use { i -> ByteArray(512).let { b -> String(b, 0, maxOf(0, i.read(b)), Charsets.UTF_8) } } }.getOrDefault("")
    return head.contains("<svg")
}

/** An SVG drawn into a bitmap [maxSide] pixels on its longer side (at most 2400: it is sharp at any size). */
private fun renderSvg(f: File, maxSide: Int): ImageBitmap? {
    val svg = f.inputStream().use { SVG.getFromInputStream(it) }
    val vb = svg.documentViewBox
    var w = svg.documentWidth
    var h = svg.documentHeight
    if ((w <= 0f || h <= 0f) && vb != null) { w = vb.width(); h = vb.height() }
    if (w <= 0f || h <= 0f) { w = 1000f; h = 750f }
    if (vb == null) svg.setDocumentViewBox(0f, 0f, w, h)
    val side = min(maxSide, 2400).toFloat()
    val k = side / max(w, h)
    val bw = (w * k).toInt().coerceAtLeast(1)
    val bh = (h * k).toInt().coerceAtLeast(1)
    svg.setDocumentWidth("100%")
    svg.setDocumentHeight("100%")
    val bmp = Bitmap.createBitmap(bw, bh, Bitmap.Config.ARGB_8888)
    svg.renderToCanvas(Canvas(bmp), RectF(0f, 0f, bw.toFloat(), bh.toFloat()))
    return bmp.asImageBitmap()
}

private val Paper = Color(0xFFFFFDF8)
private val DarkText = Color(0xFFD8D5CE)

/** The figure on its own: swipe for the others, pinch or double-tap to zoom. Black in dark mode. */
@Composable
fun FigureViewer(open: FigureOpen, bookDir: File, dark: Boolean = isSystemInDarkTheme(), onDismiss: () -> Unit) {
    val figs = open.set
    Dialog(onDismiss, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        val pager = rememberPagerState(initialPage = open.index.coerceIn(0, figs.size - 1)) { figs.size }
        var showCaption by rememberSaveable { mutableStateOf(true) }
        var zoomed by remember { mutableStateOf(false) }
        LaunchedEffect(pager.currentPage) { zoomed = false }
        Surface(Modifier.fillMaxSize(), color = if (dark) Color.Black else Paper, contentColor = if (dark) DarkText else Color(0xFF1D1D1F)) {
            Column(Modifier.systemBarsPadding()) {
                val shown = figs[pager.currentPage]
                Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(shown.label.ifEmpty { "Figure" }, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        val panel = if (pager.currentPage == open.index) open.panel else ""
                        val sub = listOfNotNull(panel.takeIf { it.isNotEmpty() }?.let { "Panel $it" }, if (figs.size > 1) "${pager.currentPage + 1} of ${figs.size}" else null)
                        if (sub.isNotEmpty()) Text(sub.joinToString(" · "), style = MaterialTheme.typography.labelMedium, color = LocalContentColor.current.copy(alpha = .65f))
                    }
                    if (figs.any { it.caption.isNotEmpty() || it.title.isNotEmpty() }) {
                        IconButton({ showCaption = !showCaption }) {
                            Icon(if (showCaption) Icons.Filled.Subtitles else Icons.Outlined.SubtitlesOff, if (showCaption) "Hide the caption" else "Show the caption")
                        }
                    }
                    IconButton(onDismiss) { Icon(Icons.Filled.Close, "Back to the text") }
                }
                HorizontalPager(pager, Modifier.weight(1f).fillMaxWidth(), userScrollEnabled = !zoomed, beyondViewportPageCount = 1) { i ->
                    FigurePage(figs[i], if (i == open.index) open.start else 0, showCaption, dark, bookDir) { z -> if (i == pager.currentPage) zoomed = z }
                }
            }
        }
    }
}

@Composable
private fun FigurePage(f: ReaderFigure, start: Int, showCaption: Boolean, dark: Boolean, bookDir: File, onZoom: (Boolean) -> Unit) {
    var image by remember { mutableIntStateOf(start.coerceIn(0, max(0, f.images.size - 1))) }
    val hasCaption = f.caption.isNotEmpty() || f.title.isNotEmpty()
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val maxH = maxHeight
        Column(Modifier.fillMaxSize()) {
            if (f.images.isNotEmpty()) {
                Box(Modifier.weight(1f).fillMaxWidth()) {
                    key(f.images[image]) { ZoomableImage(figureFile(bookDir, f.images[image]), dark, onZoom) }
                    if (f.images.size > 1) {   // a figure in several images
                        Surface(Modifier.align(Alignment.BottomCenter).padding(8.dp), shape = CircleShape, tonalElevation = 4.dp, shadowElevation = 2.dp) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                IconButton({ image-- }, enabled = image > 0) { Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, "Previous image") }
                                Text("Image ${image + 1} of ${f.images.size}", style = MaterialTheme.typography.labelMedium)
                                IconButton({ image++ }, enabled = image < f.images.size - 1) { Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, "Next image") }
                            }
                        }
                    }
                }
            } else if (!hasCaption) Spacer(Modifier.weight(1f))
            if (showCaption && hasCaption) {
                HorizontalDivider(color = LocalContentColor.current.copy(alpha = .15f))
                val size = if (f.images.isEmpty()) Modifier.weight(1f) else Modifier.heightIn(max = maxH * 0.4f)
                Box(Modifier.fillMaxWidth().then(size).verticalScroll(rememberScrollState()), contentAlignment = Alignment.TopCenter) {
                    SelectionContainer {
                        Text(
                            captionText(f, dark), Modifier.widthIn(max = 760.dp).padding(horizontal = 18.dp, vertical = 14.dp),
                            style = MaterialTheme.typography.bodyMedium.copy(lineHeight = 21.sp),
                        )
                    }
                }
            }
        }
    }
}

/** Title (bold) and caption, with the reader's highlights in it. */
private fun captionText(f: ReaderFigure, dark: Boolean): AnnotatedString {
    val text = buildAnnotatedString {
        if (f.title.isNotEmpty()) {
            withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(f.title) }
            if (f.caption.isNotEmpty()) append(" ")
        }
        append(f.caption)
    }
    if (f.marks.isEmpty()) return text
    val b = AnnotatedString.Builder(text)
    for (m in f.marks) {
        if (m.text.length < 2) continue
        val c = (HighlightColors[m.color] ?: HighlightColors.getValue("yellow")).copy(alpha = if (dark) .35f else .45f)
        var at = text.text.indexOf(m.text)
        while (at >= 0) {
            b.addStyle(SpanStyle(background = c), at, at + m.text.length)
            at = text.text.indexOf(m.text, at + m.text.length)
        }
    }
    return b.toAnnotatedString()
}

/** The image fitted to the space; pinch or double-tap to zoom, drag to move about while zoomed. */
@Composable
private fun ZoomableImage(file: File?, dark: Boolean, onZoom: (Boolean) -> Unit) {
    var bmp by remember(file) { mutableStateOf<ImageBitmap?>(null) }
    var failed by remember(file) { mutableStateOf(false) }
    LaunchedEffect(file) { bmp = decodeImage(file, 4096); failed = bmp == null }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    BoxWithConstraints(Modifier.fillMaxSize().clipToBounds(), contentAlignment = Alignment.Center) {
        val b = bmp
        if (b == null) {
            if (failed) Icon(Icons.Outlined.BrokenImage, "The image can't be shown", Modifier.size(48.dp), tint = LocalContentColor.current.copy(alpha = .5f))
            else CircularProgressIndicator()
            return@BoxWithConstraints
        }
        val boxW = constraints.maxWidth.toFloat()
        val boxH = constraints.maxHeight.toFloat()
        val fit = min(boxW / b.width, boxH / b.height)
        val w = b.width * fit
        val h = b.height * fit
        fun clamp(o: Offset, s: Float): Offset {
            val mx = max(0f, (w * s - boxW) / 2)
            val my = max(0f, (h * s - boxH) / 2)
            return Offset(o.x.coerceIn(-mx, mx), o.y.coerceIn(-my, my))
        }
        /** Zoom to [to], the point [q] (from the middle) moving to [q] + [pan]. */
        fun zoom(to: Float, q: Offset, pan: Offset = Offset.Zero) {
            val s2 = to.coerceIn(1f, 8f)
            val u = (q - pan - offset) / scale
            if (s2 <= 1.01f) { scale = 1f; offset = Offset.Zero } else { offset = clamp(q - u * s2, s2); scale = s2 }
            onZoom(scale > 1.01f)
        }
        val d = LocalDensity.current
        Image(
            b, null,
            Modifier.size(with(d) { w.toDp() }, with(d) { h.toDp() })
                .graphicsLayer { scaleX = scale; scaleY = scale; translationX = offset.x; translationY = offset.y; alpha = if (dark) .92f else 1f }
                .background(Color.White),   // figures with a transparent background
            contentScale = ContentScale.Fit,
        )
        Box(
            Modifier.matchParentSize()
                .pointerInput(b) {
                    detectTapGestures(onDoubleTap = { p ->
                        val q = p - Offset(boxW / 2, boxH / 2)
                        if (scale > 1.01f) zoom(1f, q) else zoom(3f, q)
                    })
                }
                .pointerInput(b) {
                    // two fingers zoom; one finger moves the image while zoomed (else the pager swipes)
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false)
                        do {
                            val ev = awaitPointerEvent()
                            val fingers = ev.changes.count { it.pressed }
                            if (fingers > 1 || (fingers == 1 && scale > 1.01f)) {
                                val c = ev.calculateCentroid(useCurrent = true)
                                if (c != Offset.Unspecified) {
                                    zoom(scale * ev.calculateZoom(), c - Offset(boxW / 2, boxH / 2), ev.calculatePan())
                                    ev.changes.forEach { if (it.positionChanged()) it.consume() }
                                }
                            }
                        } while (ev.changes.any { it.pressed })
                    }
                },
        )
    }
}

/** All the paper's figures as a grid of thumbnails; tap one to open it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FigureGrid(figs: List<ReaderFigure>, bookDir: File, onOpen: (Int) -> Unit, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Figures", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onDismiss) { Text("Done") }
        }
        LazyVerticalGrid(
            GridCells.Adaptive(150.dp), Modifier.fillMaxWidth(),
            contentPadding = PaddingValues(16.dp), horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            itemsIndexed(figs) { i, f ->
                Column(Modifier.clip(RoundedCornerShape(8.dp)).clickable { onOpen(i) }.padding(2.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Thumbnail(f.images.firstOrNull()?.let { figureFile(bookDir, it) }, f.images.isEmpty())
                    Text(f.label.ifEmpty { "Figure" }, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val sub = f.title.ifEmpty { f.caption }
                    if (sub.isNotEmpty()) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

@Composable
private fun Thumbnail(file: File?, none: Boolean) {
    var b by remember(file) { mutableStateOf<ImageBitmap?>(null) }
    LaunchedEffect(file) { b = decodeImage(file, 480) }
    val dark = isSystemInDarkTheme()
    Box(
        Modifier.fillMaxWidth().height(130.dp).clip(RoundedCornerShape(8.dp)).background(Color.White)
            .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(8.dp)),
        contentAlignment = Alignment.Center,
    ) {
        b?.let { Image(it, null, Modifier.fillMaxSize().padding(4.dp).graphicsLayer { alpha = if (dark) .92f else 1f }, contentScale = ContentScale.Fit) }
            ?: if (none || file == null) Icon(Icons.Outlined.Image, null, tint = Color.Gray) else Unit
    }
}
