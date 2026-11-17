package app.lunta.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.lunta.AppModel
import kotlinx.coroutines.*
import org.json.JSONArray
import uniffi.lunta_core.*

/** One entry of the paper's reference list, as the page reports it (luntaReferences(), References.swift). */
data class PaperRef(
    val index: Int, val label: String, val text: String, val doi: String, val arxiv: String,
    val pmid: String, val pmcid: String, val anchor: String, val cited: Int,
) {
    /** What "Add paper" is given for it; null when it has no identifier. */
    val input: String? get() = doi.ifEmpty { null } ?: arxiv.ifEmpty { null }?.let { "arXiv:$it" } ?: pmcid.ifEmpty { null } ?: pmid.ifEmpty { null }
}

fun parseRefs(json: String?): List<PaperRef> = runCatching {
    val a = JSONArray(json ?: return emptyList())
    (0 until a.length()).map { i ->
        val o = a.getJSONObject(i)
        PaperRef(o.optInt("index", i), o.optString("label"), o.optString("text"), o.optString("doi"), o.optString("arxiv"),
            o.optString("pmid"), o.optString("pmcid"), o.optString("anchor"), o.optInt("cited"))
    }
}.getOrDefault(emptyList())

private fun doiKey(d: String) = d.trim().lowercase().replace(Regex("^(https?://(dx\\.)?doi\\.org/|doi:)"), "")
private fun arxivKey(a: String) = a.trim().lowercase().removePrefix("arxiv:").replace(Regex("v\\d+$"), "")

/** The References sheet's state, kept while the reader is open (adding goes on with the sheet closed). */
class ReferencesState(private val scope: CoroutineScope, private val lib: Library, private val model: AppModel) {
    var refs by mutableStateOf<List<PaperRef>?>(null)
    var inLibrary by mutableStateOf<Map<Int, String>>(emptyMap())
    var adding by mutableStateOf<Set<Int>>(emptySet())
    var failed by mutableStateOf<Set<Int>>(emptySet())
    /** Add All Missing: done of total (null when not running). */
    var bulk by mutableStateOf<Pair<Int, Int>?>(null)
    private var bulkJob: Job? = null

    val missing: List<PaperRef> get() = refs.orEmpty().filter { it.input != null && it.index !in inLibrary }

    /** Which references are in the library, by DOI, arXiv, PubMed and PMC ids (no network). */
    suspend fun refresh() {
        val list = refs ?: return
        inLibrary = withContext(Dispatchers.IO) {
            val index = HashMap<String, String>()
            for (p in lib.papers(PaperFilter(null, null, false, false, false), SortKey.ADDED)) {
                val m = p.meta
                if (m.doi.isNotBlank()) {
                    val d = doiKey(m.doi)
                    index["doi:$d"] = p.id
                    if (d.startsWith("10.48550/arxiv.")) index["arxiv:${arxivKey(d.removePrefix("10.48550/arxiv."))}"] = p.id
                }
                if (m.arxiv.isNotBlank()) index["arxiv:${arxivKey(m.arxiv)}"] = p.id
                if (m.pmid.isNotBlank()) index["pmid:${m.pmid.trim()}"] = p.id
                if (m.pmcid.isNotBlank()) index["pmcid:${m.pmcid.trim().uppercase()}"] = p.id
            }
            list.mapNotNull { r ->
                val keys = listOfNotNull(
                    r.doi.ifEmpty { null }?.let { "doi:${doiKey(it)}" }, r.arxiv.ifEmpty { null }?.let { "arxiv:${arxivKey(it)}" },
                    r.pmid.ifEmpty { null }?.let { "pmid:$it" }, r.pmcid.ifEmpty { null }?.let { "pmcid:${it.uppercase()}" })
                keys.firstNotNullOfOrNull { index[it] }?.let { r.index to it }
            }.toMap()
        }
    }

    suspend fun add(r: PaperRef) {
        val input = r.input ?: return
        if (r.index in inLibrary || r.index in adding) return
        adding = adding + r.index; failed = failed - r.index
        val added = withContext(Dispatchers.IO) { runCatching { lib.addFromInput(input, null) }.getOrNull() }
        adding = adding - r.index
        if (added != null) inLibrary = inLibrary + (r.index to added.paper.id) else failed = failed + r.index
    }

    /** Add All Missing: three at a time, until done or cancelled. */
    fun addAllMissing() {
        val todo = missing
        if (todo.isEmpty() || bulkJob != null) return
        bulk = 0 to todo.size
        bulkJob = scope.launch {
            try {
                for (chunk in todo.chunked(3)) {
                    ensureActive()
                    coroutineScope { chunk.forEach { r -> launch { add(r) } } }
                    bulk = ((bulk?.first ?: 0) + chunk.size) to todo.size
                }
            } finally {
                bulk = null; bulkJob = null
                withContext(NonCancellable) { model.reload(); refresh() }
            }
        }
    }

    fun cancel() { bulkJob?.cancel() }

    fun addOne(r: PaperRef) { scope.launch { add(r); model.reload() } }
}

/** ⋮ › References: the paper's reference list, each In library (opens it) or Add. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReferencesSheet(state: ReferencesState, onDismiss: () -> Unit, onShow: (PaperRef) -> Unit, onOpen: (String) -> Unit, onSearch: (PaperRef) -> Unit) {
    var query by remember { mutableStateOf("") }
    var confirming by remember { mutableStateOf(false) }
    val refs = state.refs
    ModalBottomSheet(onDismiss) {
        LazyColumn(Modifier.padding(bottom = 24.dp)) {
            item { Text("References", Modifier.padding(horizontal = 24.dp, vertical = 8.dp), style = MaterialTheme.typography.titleLarge) }
            when {
                refs == null -> item { Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() } }
                refs.isEmpty() -> item {
                    Text("This reading view has no reference list Lunta can read.", Modifier.padding(horizontal = 24.dp, vertical = 8.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                else -> {
                    item {
                        Column(Modifier.padding(horizontal = 24.dp)) {
                            Text("${refs.size} ${if (refs.size == 1) "reference" else "references"} · ${state.inLibrary.size} in your library",
                                style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            val b = state.bulk
                            if (b != null) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                LinearProgressIndicator({ b.first.toFloat() / b.second.coerceAtLeast(1) }, Modifier.weight(1f))
                                Text("${b.first} of ${b.second}", style = MaterialTheme.typography.labelMedium)
                                TextButton({ state.cancel() }) { Text("Cancel") }
                            } else if (state.missing.isNotEmpty()) {
                                TextButton({ confirming = true }, contentPadding = PaddingValues(0.dp)) { Text("Add All Missing") }
                            }
                            OutlinedTextField(query, { query = it }, Modifier.fillMaxWidth().padding(vertical = 6.dp), singleLine = true,
                                placeholder = { Text("Filter") }, leadingIcon = { Icon(Icons.Outlined.Search, null) })
                        }
                    }
                    val q = query.trim()
                    val shown = if (q.isEmpty()) refs else refs.filter { it.text.contains(q, true) || it.label == q || it.doi.contains(q, true) }
                    items(shown, key = { it.index }) { r -> RefRow(state, r, onShow, onOpen, onSearch) }
                }
            }
        }
    }
    if (confirming) {
        val n = state.missing.size
        val skipped = refs.orEmpty().count { it.input == null }
        AlertDialog(
            onDismissRequest = { confirming = false },
            title = { Text(if (n == 1) "Add 1 reference to your library?" else "Add $n references to your library?") },
            text = { Text(if (skipped > 0) "$skipped without a DOI, arXiv or PubMed id are skipped." else "They are added a few at a time.") },
            confirmButton = { TextButton({ confirming = false; state.addAllMissing() }) { Text("Add $n") } },
            dismissButton = { TextButton({ confirming = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun RefRow(state: ReferencesState, r: PaperRef, onShow: (PaperRef) -> Unit, onOpen: (String) -> Unit, onSearch: (PaperRef) -> Unit) {
    val inLib = state.inLibrary[r.index]
    val dim = MaterialTheme.colorScheme.onSurfaceVariant
    Row(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 6.dp), verticalAlignment = Alignment.Top) {
        Text(r.label.ifEmpty { "•" }, Modifier.widthIn(min = 24.dp).padding(end = 10.dp, top = 2.dp), style = MaterialTheme.typography.labelMedium, color = dim, maxLines = 1)
        Column(Modifier.weight(1f)) {
            Text(r.text, Modifier.clickable { onShow(r) }, style = MaterialTheme.typography.bodyMedium, maxLines = 3, overflow = TextOverflow.Ellipsis,
                color = if (r.input != null || inLib != null) MaterialTheme.colorScheme.onSurface else dim)
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (r.cited > 0) Text("Cited ${r.cited}×", style = MaterialTheme.typography.labelSmall, color = dim)
                Spacer(Modifier.weight(1f))
                when {
                    inLib != null -> TextButton({ onOpen(inLib) }) {
                        Icon(Icons.Filled.CheckCircle, null, Modifier.size(16.dp), tint = Color(0xFF2E8B57)); Spacer(Modifier.width(4.dp)); Text("In library", color = Color(0xFF2E8B57))
                    }
                    r.index in state.adding -> Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp); Spacer(Modifier.width(6.dp)); Text("Adding…", style = MaterialTheme.typography.labelMedium, color = dim)
                    }
                    r.input != null -> Row(verticalAlignment = Alignment.CenterVertically) {
                        if (r.index in state.failed) Text("Couldn’t add", style = MaterialTheme.typography.labelSmall, color = Color(0xFFC96A00))
                        TextButton({ state.addOne(r) }) { Icon(Icons.Outlined.Add, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text("Add") }
                    }
                    else -> TextButton({ onSearch(r) }) { Text("Search", color = dim) }
                }
            }
        }
    }
}
