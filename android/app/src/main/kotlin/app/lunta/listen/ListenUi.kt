package app.lunta.listen

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.VolumeUp
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.lunta.AppModel
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import uniffi.lunta_core.FileKind
import uniffi.lunta_core.NarrationSegment
import uniffi.lunta_core.Paper

private fun mb(n: Long) = "${(n + 500_000) / 1_000_000} MB"

/**
 * Listen in one reader: starts the narration from what the reader is looking at, lets the page
 * follow the voice, and makes a natural narration to keep with the paper. [js] runs a script in the
 * reading view and returns its string result.
 */
class ReaderListen(
    private val scope: CoroutineScope,
    private val model: AppModel,
    private val paper: Paper,
    private val js: suspend (String) -> String?,
) {
    private val id = paper.id
    var voiceDialog by mutableStateOf(false)

    /** The page is ready (or loaded again): if this paper is being read, the page follows the voice. */
    fun attach() {
        Narrator.attach(id,
            { el -> scope.launch { js("window.luntaHighlight && luntaHighlight(${JSONObject.quote(el)}) || ''") } },
            { on -> scope.launch { js("window.luntaListening && luntaListening($on) || ''") } })
    }

    fun detach() = Narrator.detach(id)

    private suspend fun pageItems(): List<NarrationSegment> {
        val json = js("window.luntaNarration ? luntaNarration(false) : '[]'") ?: "[]"
        val a = runCatching { JSONArray(json) }.getOrElse { JSONArray() }
        return (0 until a.length()).map { i -> a.getJSONObject(i).let { NarrationSegment(it.getString("id"), it.optString("kind", "text"), it.optString("text")) } }
    }

    /** ⋯ › Listen: reads aloud from the paragraph at the top of the screen. */
    fun start() {
        scope.launch {
            attach()
            if (Narrator.items.isNotEmpty() && Narrator.paperId == id) { if (!Narrator.playing) Narrator.resume(); return@launch }
            val items = pageItems()
            if (items.isEmpty()) { model.info = "Nothing to read aloud in this paper."; return@launch }
            val from = js("window.luntaFirstVisible ? luntaFirstVisible() : ''") ?: ""
            Narrator.start(id, paper.meta.title, items, from, recordedFiles())
        }
    }

    /** narration.m4a and narration.json, fetched from Drive if they are there and not here yet. */
    private suspend fun recordedFiles(): Pair<String, String>? {
        val lib = model.library ?: return null
        val remote = withContext(Dispatchers.IO) { runCatching { model.librarySync?.paperFiles(id)?.any { it.name == "narration.json" } }.getOrNull() } == true
        if (!paper.hasAudio && !remote) return null
        if (remote) runCatching { model.ensureFiles(id, FileKind.AUDIO, FileKind.AUDIO_INDEX) }
        val a = lib.filePath(id, FileKind.AUDIO) ?: return null
        val t = lib.filePath(id, FileKind.AUDIO_INDEX) ?: return null
        return a to t
    }

    /** ⋯ › Make Natural Narration: the whole paper in the natural voice, kept with it (syncs to the other devices). */
    fun makeNarration() {
        val lib = model.library ?: return
        if (Narrator.voiceStatus?.installed != true) { voiceDialog = true; return }
        scope.launch {
            val items = pageItems()
            if (items.isEmpty()) { model.info = "Nothing to read aloud in this paper."; return@launch }
            Narrator.makeNarration(lib, id, items) { err ->
                if (err == null) { model.info = "The natural narration of “${paper.meta.title}” is ready; it plays on your other devices too."; scope.launch { model.reload() } }
                else if (!err.contains("cancelled")) model.info = "Couldn't make the narration: $err"
            }
        }
    }
}

/** The player at the bottom of the reader while listening (NarrationBar in Narrator.swift). */
@Composable
fun NarrationBar(onClose: () -> Unit, onDownload: () -> Unit) {
    val n = Narrator
    Surface(tonalElevation = 6.dp, shadowElevation = 8.dp) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp).padding(bottom = 8.dp)) {
            LinearProgressIndicator({ n.progress }, Modifier.fillMaxWidth().padding(top = 0.dp).height(3.dp))
            Spacer(Modifier.height(8.dp))
            Text(n.section.ifEmpty { "Listening" }, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (n.mode != Narrator.Mode.System) {
                    Surface(shape = RoundedCornerShape(8.dp), color = MaterialTheme.colorScheme.primaryContainer) {
                        Text("Natural voice", Modifier.padding(horizontal = 6.dp, vertical = 1.dp), style = MaterialTheme.typography.labelSmall)
                    }
                    Spacer(Modifier.width(6.dp))
                }
                if (n.waiting && n.playing) { CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 1.5.dp); Spacer(Modifier.width(6.dp)) }
                Text(n.title, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                SpeedMenu()
                Spacer(Modifier.weight(1f))
                Controls(big = true)
                Spacer(Modifier.weight(1f))
                IconButton(onClose) { Icon(Icons.Filled.Close, "Stop listening") }
            }
            val d = n.download
            when {
                d != null -> Text("Downloading the natural voice… ${mb(d.first)} of ${mb(d.second)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                n.note == "offer" || n.note == "novoice" -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(if (n.note == "offer") "Using the phone's voice." else "This phone has no voice to read with.", Modifier.weight(1f), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    TextButton(onDownload) { Text("Natural voice…") }
                }
                n.note != null -> Text(n.note!!, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
private fun Controls(big: Boolean) {
    val n = Narrator
    Row(verticalAlignment = Alignment.CenterVertically) {
        IconButton({ n.previous() }) { Icon(Icons.Filled.SkipPrevious, "Previous paragraph") }
        IconButton({ n.toggle() }, Modifier.size(if (big) 56.dp else 44.dp)) {
            Icon(if (n.playing) Icons.Filled.PauseCircle else Icons.Filled.PlayCircle, if (n.playing) "Pause" else "Play",
                Modifier.size(if (big) 48.dp else 34.dp), tint = MaterialTheme.colorScheme.primary)
        }
        IconButton({ n.next() }) { Icon(Icons.Filled.SkipNext, "Next paragraph") }
        if (big) IconButton({ n.nextSection() }) { Icon(Icons.Filled.LastPage, "Next section") }
    }
}

@Composable
private fun SpeedMenu() {
    val n = Narrator
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton({ open = true }) { Text("${n.speed.toString().removeSuffix(".0")}×", fontWeight = FontWeight.SemiBold) }
        DropdownMenu(open, { open = false }) {
            for (s in n.speeds) DropdownMenuItem({ Text("${s.toString().removeSuffix(".0")}×") }, { open = false; n.changeSpeed(s) },
                trailingIcon = { if (s == n.speed) Icon(Icons.Filled.Check, null) })
            if (n.mode == Narrator.Mode.Natural) {
                HorizontalDivider()
                for (v in n.voices) DropdownMenuItem({ Text(v.name) }, { open = false; n.changeVoice(v.id) },
                    trailingIcon = { if (v.id == n.voice) Icon(Icons.Filled.Check, null) })
            }
        }
    }
}

/** Now Playing: the paper being read aloud, wherever its reader isn't on screen (NowPlaying.swift). */
@Composable
fun NowPlayingBar(modifier: Modifier, onOpen: (String) -> Unit) {
    val n = Narrator
    val show = n.items.isNotEmpty() && n.readerPaper != n.paperId
    AnimatedVisibility(show, modifier, enter = slideInVertically { it } + fadeIn(), exit = slideOutVertically { it } + fadeOut()) {
        Surface(Modifier.navigationBarsPadding().padding(12.dp), shape = CircleShape, tonalElevation = 6.dp, shadowElevation = 6.dp) {
            Row(Modifier.padding(start = 16.dp, end = 4.dp).heightIn(min = 52.dp), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f).clickable { onOpen(n.paperId) }, verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.AutoMirrored.Filled.VolumeUp, null, tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(10.dp))
                    Column {
                        Text(n.title.ifEmpty { "Listening" }, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(n.section.ifEmpty { if (n.mode == Narrator.Mode.System) "Phone voice" else "Natural voice" },
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
                Controls(big = false)
                IconButton({ n.stop() }) { Icon(Icons.Filled.Close, "Stop") }
            }
        }
    }
}

/** The natural voice: what it is, its download (with progress), the voice, and deleting it. */
@Composable
fun NaturalVoiceSettings() {
    val n = Narrator
    LaunchedEffect(Unit) { n.refreshStatus() }
    val st = n.voiceStatus
    val d = n.download
    Text("Listen", style = MaterialTheme.typography.titleLarge)
    Spacer(Modifier.height(4.dp))
    when {
        d != null -> {
            Text("Downloading the natural voice… ${mb(d.first)} of ${mb(d.second)}", style = MaterialTheme.typography.bodyMedium)
            LinearProgressIndicator({ if (d.second > 0) d.first.toFloat() / d.second else 0f }, Modifier.fillMaxWidth().padding(vertical = 8.dp))
            OutlinedButton({ n.cancelDownload() }) { Text("Cancel") }
        }
        st?.installed == true -> {
            Text("The natural voice is on this phone (${mb(st.diskBytes.toLong())}). It runs here; nothing you listen to leaves the phone.",
                style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            var open by remember { mutableStateOf(false) }
            Box(Modifier.padding(top = 8.dp)) {
                OutlinedButton({ open = true }) { Text("Voice: " + (n.voices.firstOrNull { it.id == n.voice }?.name ?: n.voice)) }
                DropdownMenu(open, { open = false }) {
                    for (v in n.voices) DropdownMenuItem({ Text(v.name) }, { open = false; n.changeVoice(v.id) },
                        trailingIcon = { if (v.id == n.voice) Icon(Icons.Filled.Check, null) })
                }
            }
            TextButton({ n.removeVoice() }) { Text("Delete the natural voice", color = MaterialTheme.colorScheme.error) }
        }
        else -> {
            Text("Papers are read aloud in the phone's voice. The natural voice (Kokoro, an open speech model that runs on the phone) " +
                "sounds much more like a person: a one-time download of ${mb(st?.downloadBytes?.toLong() ?: 0)} from the sherpa-onnx project on GitHub.",
                style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            FilledTonalButton({ n.downloadVoice() }, Modifier.padding(top = 8.dp)) { Text("Download the natural voice") }
        }
    }
}

/** The natural voice offered from the player or ⋯ menu. */
@Composable
fun NaturalVoiceDialog(onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Natural Voice") },
        text = { Column { NaturalVoiceSettings() } },
        confirmButton = { TextButton(onDismiss) { Text("Done") } },
    )
}

/** Make Natural Narration's progress, while it runs. */
@Composable
fun NarrationRenderNote(modifier: Modifier, paperId: String) {
    val r = Narrator.rendering ?: return
    if (r.first != paperId) return
    Surface(modifier.navigationBarsPadding().padding(16.dp), shape = RoundedCornerShape(16.dp), tonalElevation = 6.dp, shadowElevation = 3.dp) {
        Row(Modifier.padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator({ if (r.third > 0) r.second.toFloat() / r.third else 0f }, Modifier.size(18.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(10.dp))
            Text("Natural narration: ${r.second} of ${r.third} paragraphs", style = MaterialTheme.typography.bodySmall)
            TextButton({ Narrator.cancelNarration() }) { Text("Cancel") }
        }
    }
}
