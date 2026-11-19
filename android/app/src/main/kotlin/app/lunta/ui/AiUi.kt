package app.lunta.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.lunta.ai.OnDeviceAi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.lunta_core.AiException
import uniffi.lunta_core.AiTextListener
import uniffi.lunta_core.AskSource
import uniffi.lunta_core.ChatTurn
import uniffi.lunta_core.Library
import java.util.concurrent.atomic.AtomicBoolean

private fun gb(b: Long) = "%.1f GB".format(b / 1e9)

/** The On-device AI part of the settings sheet: the model's download, and the Summary card's switch. */
@Composable
fun OnDeviceAiSettings() {
    val ctx = LocalContext.current
    LaunchedEffect(Unit) { OnDeviceAi.init(ctx); OnDeviceAi.refresh() }
    val ai = OnDeviceAi
    var confirm by remember { mutableStateOf(false) }
    Spacer(Modifier.height(24.dp))
    Text("On-device AI", style = MaterialTheme.typography.titleLarge)
    Text(
        "Summaries at the top of the reading view, and Ask: questions about a paper, answered from its text. " +
            "Runs on this phone with the open model ${ai.model.name} (Apache-2.0, by Qwen); nothing is sent anywhere.",
        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    ai.supportNote?.let { Text(it, Modifier.padding(top = 6.dp), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error) }
    Spacer(Modifier.height(8.dp))
    val size = ai.model.size.toLong()
    val blocked = ai.support == OnDeviceAi.Support.Unsupported || ai.support == OnDeviceAi.Support.TooLittleMemory
    when {
        ai.installed -> {
            Text("✓ ${ai.model.name} is downloaded (${gb(size)}).", style = MaterialTheme.typography.bodyLarge)
            if (ai.support == OnDeviceAi.Support.LowMemory && !ai.tryAnyway) {
                FilledTonalButton({ confirm = true }) { Text("Use it on this phone…") }
            }
            OutlinedButton({ ai.delete() }) { Text("Delete model") }
        }
        ai.downloading != null -> {
            val (done, total) = ai.downloading!!
            LinearProgressIndicator({ if (total > 0) done.toFloat() / total else 0f }, Modifier.fillMaxWidth().padding(vertical = 6.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("${gb(done)} of ${gb(total)}", Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                TextButton({ ai.cancelDownload() }) { Text("Cancel") }
            }
        }
        !blocked -> {
            val label = if (ai.partial > 0) "Resume download (${gb(ai.partial)} of ${gb(size)})" else "Download model (${gb(size)})"
            FilledTonalButton({ if (ai.support == OnDeviceAi.Support.LowMemory && !ai.tryAnyway) confirm = true else ai.download() }) { Text(label) }
            Text("Use Wi-Fi: it is a large download. It can be stopped and resumed, and is checked before use.",
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
    ai.error?.let { Text("The download stopped: $it", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
    Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("Show summaries in the reader", Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(ai.summaries, { ai.showSummaries(it) })
    }
    Text(
        "A few sentences on what the paper did and found, kept with the paper (summaries made on your Mac or computer show here too). Summaries can be wrong.",
        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    if (confirm) AlertDialog(
        onDismissRequest = { confirm = false },
        title = { Text("Try on-device AI?") },
        text = { Text("This phone has ${ai.memoryGb} GB of memory; the model needs at least 6 GB to run well. It may be slow, and Android may close other apps, or Lunta, while it works.") },
        confirmButton = { TextButton({ confirm = false; ai.tryAnyway = true; if (!ai.installed) ai.download() }) { Text("Try anyway") } },
        dismissButton = { TextButton({ confirm = false }) { Text("Cancel") } },
    )
}

private class AskMsg(val role: String, text: String, val sources: List<AskSource> = emptyList()) {
    var text by mutableStateOf(text)
}

/** Ask about the open paper: answers from its passages that match the question, with them below. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AskSheet(lib: Library, paperId: String, onDismiss: () -> Unit) {
    val ctx = LocalContext.current
    LaunchedEffect(Unit) { OnDeviceAi.init(ctx) }
    val scope = rememberCoroutineScope()
    val msgs = remember(paperId) { mutableStateListOf<AskMsg>() }
    var question by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    val stop = remember { arrayOf(AtomicBoolean(false)) }
    val list = rememberLazyListState()
    LaunchedEffect(msgs.size, msgs.lastOrNull()?.text) { if (msgs.isNotEmpty()) list.animateScrollToItem(msgs.size - 1) }
    DisposableEffect(Unit) { onDispose { stop[0].set(true) } }

    fun ask() {
        val q = question.trim()
        if (q.isEmpty() || busy) return
        question = ""
        if (!OnDeviceAi.usable) {
            msgs += AskMsg("user", q)
            msgs += AskMsg("notice", OnDeviceAi.supportNote?.takeIf { OnDeviceAi.support != OnDeviceAi.Support.LowMemory || !OnDeviceAi.installed }
                ?: "Ask needs the on-device model: download it in Settings › On-device AI (one time).")
            return
        }
        val history = msgs.filter { it.role != "notice" && it.text.isNotBlank() }.map { ChatTurn(it.role, it.text) }
        msgs += AskMsg("user", q)
        busy = true
        val flag = AtomicBoolean(false)
        stop[0] = flag
        scope.launch {
            val prompt = withContext(Dispatchers.IO) { runCatching { lib.askPaperPrompt(paperId, q, history, null) } }
            val p = prompt.getOrElse { msgs += AskMsg("notice", it.message ?: "$it"); busy = false; return@launch }
            val a = AskMsg("assistant", "", p.sources)
            msgs += a
            val engine = OnDeviceAi.engine()
            if (engine == null) { msgs.remove(a); msgs += AskMsg("notice", "The model couldn't be loaded."); busy = false; return@launch }
            var last = 0L
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    engine.answer(p, object : AiTextListener {
                        override fun onText(text: String): Boolean {
                            if (flag.get()) return false
                            val now = System.currentTimeMillis()
                            if (now - last > 150) { last = now; scope.launch { a.text = text } }
                            return true
                        }
                    })
                }
            }
            r.onSuccess { a.text = it }.onFailure { e ->
                if (a.text.isBlank()) msgs.remove(a)
                if (e !is AiException.Cancelled) msgs += AskMsg("notice", e.message ?: "$e")
            }
            busy = false
        }
    }

    ModalBottomSheet({ stop[0].set(true); onDismiss() }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().fillMaxHeight(.9f).padding(horizontal = 16.dp).imePadding()) {
            Text("Ask about this paper", style = MaterialTheme.typography.titleLarge)
            LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, verticalArrangement = Arrangement.spacedBy(10.dp), contentPadding = PaddingValues(vertical = 10.dp)) {
                if (msgs.isEmpty()) item {
                    Text("Ask about its methods, numbers or findings. Answers come from the passages that match your question, which are listed under each answer.",
                        style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                items(msgs) { m ->
                    when (m.role) {
                        "user" -> Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                            Text(m.text, Modifier.background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(14.dp)).padding(horizontal = 12.dp, vertical = 8.dp))
                        }
                        "notice" -> Text(m.text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                        else -> Column {
                            Text(if (m.text.isBlank()) "Thinking…" else m.text.replace("**", ""), style = MaterialTheme.typography.bodyLarge,
                                color = if (m.text.isBlank()) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface)
                            if (m.sources.isNotEmpty()) {
                                var open by remember { mutableStateOf(false) }
                                TextButton({ open = !open }, contentPadding = PaddingValues(0.dp)) {
                                    Text(if (open) "Hide the passages" else "The ${m.sources.size} passages it was given", style = MaterialTheme.typography.bodySmall)
                                }
                                if (open) for (s in m.sources) {
                                    Text("[${s.n}] ${s.text}", Modifier.padding(bottom = 6.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            }
                        }
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(question, { question = it }, Modifier.weight(1f), placeholder = { Text("Ask about this paper") }, enabled = !busy, maxLines = 4)
                Spacer(Modifier.width(8.dp))
                if (busy) TextButton({ stop[0].set(true) }) { Text("Stop") }
                else FilledTonalButton({ ask() }, enabled = question.isNotBlank()) { Text("Ask") }
            }
            Text(OnDeviceAi.NOTE, Modifier.padding(vertical = 8.dp), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, fontWeight = FontWeight.Normal)
        }
    }
}
