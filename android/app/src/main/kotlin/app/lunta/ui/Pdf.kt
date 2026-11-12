package app.lunta.ui

import android.graphics.Bitmap
import android.graphics.Color as AColor
import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.lunta.AppModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.lunta_core.FileKind
import uniffi.lunta_core.Paper
import java.io.File

/** The original PDF, page by page with Android's PdfRenderer (no PDF highlighting in this version). */
@Composable
fun PdfReader(model: AppModel, paper: Paper) {
    val path = remember(paper.id) { model.library?.filePath(paper.id, FileKind.PDF) }
    var zoom by remember { mutableFloatStateOf(1f) }
    var overflow by remember { mutableStateOf(false) }
    BackHandler { model.pop() }
    Scaffold(
        topBar = {
            Surface(color = MaterialTheme.colorScheme.surface, shadowElevation = 3.dp) {
                Row(Modifier.statusBarsPadding().height(56.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    IconButton({ model.pop() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
                    Text(paper.meta.title, Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    IconButton({ zoom = (zoom / 1.25f).coerceAtLeast(1f) }) { Icon(Icons.Filled.ZoomOut, "Zoom out") }
                    IconButton({ zoom = (zoom * 1.25f).coerceAtMost(4f) }) { Icon(Icons.Filled.ZoomIn, "Zoom in") }
                    if (paper.hasEpub) {
                        Box {
                            IconButton({ overflow = true }) { Icon(Icons.Filled.MoreVert, "More") }
                            DropdownMenu(overflow, { overflow = false }) {
                                DropdownMenuItem({ Text("Open the reading view") }, {
                                    overflow = false; model.pop(); model.push(app.lunta.Screen.Reader(paper.id, false))
                                })
                            }
                        }
                    }
                }
            }
        },
    ) { pad ->
        if (path == null) {
            Box(Modifier.padding(pad).fillMaxSize(), contentAlignment = Alignment.Center) { Text("The PDF isn't on this device yet.") }
        } else {
            PdfPages(File(path), zoom, Modifier.padding(pad).fillMaxSize())
        }
    }
}

@Composable
private fun PdfPages(file: File, zoom: Float, modifier: Modifier) {
    val renderer by produceState<PdfRenderer?>(null, file) {
        value = withContext(Dispatchers.IO) { runCatching { PdfRenderer(ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)) }.getOrNull() }
    }
    val r = renderer
    DisposableEffect(r) { onDispose { runCatching { r?.close() } } }
    if (r == null) { Box(modifier, contentAlignment = Alignment.Center) { CircularProgressIndicator() }; return }
    val lock = remember(r) { Mutex() }
    val pageCount = remember(r) { r.pageCount }
    val density = LocalDensity.current
    val screenW = LocalConfiguration.current.screenWidthDp.dp
    val pageW = screenW * zoom
    val px = with(density) { pageW.roundToPx() }
    Box(modifier.background(MaterialTheme.colorScheme.surfaceVariant).horizontalScroll(rememberScrollState())) {
        LazyColumn(Modifier.width(pageW), state = rememberLazyListState(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            items(pageCount) { i ->
                val bmp by produceState<Bitmap?>(null, i, px) {
                    value = withContext(Dispatchers.Default) {
                        lock.withLock {
                            runCatching {
                                r.openPage(i).use { page ->
                                    val h = (px.toFloat() * page.height / page.width).toInt().coerceAtLeast(1)
                                    Bitmap.createBitmap(px, h, Bitmap.Config.ARGB_8888).also {
                                        it.eraseColor(AColor.WHITE)
                                        page.render(it, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                                    }
                                }
                            }.getOrNull()
                        }
                    }
                }
                val b = bmp
                if (b != null) Image(b.asImageBitmap(), "Page ${i + 1}", Modifier.fillMaxWidth(), contentScale = ContentScale.FillWidth)
                else Box(Modifier.fillMaxWidth().height(pageW * 1.3f).background(androidx.compose.ui.graphics.Color.White))
            }
        }
    }
}
