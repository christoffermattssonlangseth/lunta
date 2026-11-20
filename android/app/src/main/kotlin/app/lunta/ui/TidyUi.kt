package app.lunta.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.lunta.AppModel
import app.lunta.LibraryTidy.Companion.plural
import app.lunta.ai.OnDeviceAi
import uniffi.lunta_core.DoiSuggestion
import uniffi.lunta_core.DuplicateGroup
import uniffi.lunta_core.GroupProposal
import uniffi.lunta_core.Paper
import uniffi.lunta_core.TagSuggestion

/**
 * Tidy library (+ › Tidy library…): a full-screen dialog like the reference check's report, with
 * the suggestions of LibraryTidy (app/lunta/Tidy.kt) in sections. Each suggestion is approved on its
 * own or all at once; tags, folders and DOIs can be undone from the snackbar; a merge is asked first.
 */
@Composable
fun TidyDialog(model: AppModel) {
    val tidy = model.tidy
    if (!tidy.open) return
    val ctx = LocalContext.current
    LaunchedEffect(Unit) { OnDeviceAi.init(ctx) }
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(tidy.undoable) {
        val ch = tidy.undoable ?: return@LaunchedEffect
        val r = snackbar.showSnackbar(ch.summary, "Undo", withDismissAction = true, duration = SnackbarDuration.Long)
        if (tidy.undoable === ch) tidy.undoable = null
        if (r == SnackbarResult.ActionPerformed) tidy.undo(ch)
    }
    LaunchedEffect(tidy.message) {
        val m = tidy.message ?: return@LaunchedEffect
        snackbar.showSnackbar(m, withDismissAction = true)
        if (tidy.message == m) tidy.message = null
    }
    var merging by remember { mutableStateOf<Pair<DuplicateGroup, String>?>(null) }
    var switching by remember { mutableStateOf<String?>(null) }

    Dialog({ tidy.open = false }, DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize().padding(12.dp), shape = MaterialTheme.shapes.large) {
            Box {
                Column(Modifier.padding(16.dp)) {
                    Text("Tidy library", style = MaterialTheme.typography.titleLarge)
                    Text(
                        "Suggestions for your ${plural(tidy.papers, "paper", "papers")}. Nothing changes until you approve it.",
                        Modifier.padding(vertical = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    tidy.working?.let { w ->
                        LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 6.dp))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(w, Modifier.weight(1f), style = MaterialTheme.typography.bodySmall, maxLines = 2)
                            if (tidy.workingOn == "pdf" || tidy.workingOn == "epub") TextButton({ tidy.stopWorking() }) { Text("Stop") }
                        }
                    }
                    if (tidy.loading && tidy.missing == null) {
                        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                    } else {
                        LazyColumn(Modifier.weight(1f).padding(vertical = 8.dp)) {
                            duplicatesSection(model) { g, keep -> merging = g to keep }
                            missingSection(model) { switching = it }
                            topicsSection(model)
                            tagsSection(model)
                        }
                    }
                    Row {
                        TextButton({ tidy.refresh() }, enabled = !tidy.loading) { Text("Refresh") }
                        Spacer(Modifier.weight(1f))
                        TextButton({ tidy.open = false }) { Text("Done") }
                    }
                }
                SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).padding(bottom = 56.dp))
            }
        }
    }

    merging?.let { (g, keep) ->
        val kept = model.paper(keep)
        AlertDialog(
            onDismissRequest = { merging = null },
            title = { Text("Merge ${g.paperIds.size} papers?") },
            text = {
                Text(
                    "“${kept?.meta?.title ?: "The selected paper"}” is kept. ${g.note}\n\nThe other ${if (g.paperIds.size == 2) "paper is" else "papers are"} then deleted. This can't be undone.",
                )
            },
            confirmButton = { TextButton({ merging = null; model.tidy.merge(g, keep) }) { Text("Merge") } },
            dismissButton = { TextButton({ merging = null }) { Text("Cancel") } },
        )
    }
    switching?.let { id ->
        val p = model.paper(id)
        val v = tidy.missing?.published?.firstOrNull { it.paperId == id }?.version
        AlertDialog(
            onDismissRequest = { switching = null },
            title = { Text("Switch to the published version?") },
            text = {
                Text(
                    "“${p?.meta?.title ?: "This preprint"}” takes the details of ${v?.journal?.ifBlank { null } ?: "the published version"}${v?.doi?.let { " ($it)" } ?: ""}. " +
                        "Your highlights, notes, tags, folders and citation key stay.",
                )
            },
            confirmButton = { TextButton({ switching = null; tidy.switchToPublished(id) }) { Text("Switch") } },
            dismissButton = { TextButton({ switching = null }) { Text("Cancel") } },
        )
    }
}

// ---- sections ----

private fun LazyListScope.header(text: String, sub: String? = null) {
    item(key = "h-$text") {
        Column(Modifier.padding(top = 16.dp, bottom = 4.dp)) {
            Text(text, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
            sub?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}

private fun LazyListScope.duplicatesSection(model: AppModel, onMerge: (DuplicateGroup, String) -> Unit) {
    val tidy = model.tidy
    header("Possible duplicates (${tidy.duplicates.size})")
    if (tidy.duplicates.isEmpty()) item(key = "dup-none") { Quiet("No duplicates found.") }
    items(tidy.duplicates, key = { "dup-${it.key}" }) { g ->
        var keep by remember(g.key) { mutableStateOf(g.keep) }
        Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
            Text(g.reasonText, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.secondary)
            for (id in g.paperIds) {
                val p = model.paper(id) ?: continue
                Row(Modifier.fillMaxWidth().clickable { keep = id }.padding(vertical = 2.dp), verticalAlignment = Alignment.Top) {
                    RadioButton(keep == id, { keep = id })
                    Column(Modifier.weight(1f).padding(top = 10.dp)) {
                        PaperLines(p)
                        Text(contents(p), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline)
                    }
                }
            }
            if (g.note.isNotBlank()) Text(g.note, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Row {
                FilledTonalButton({ onMerge(g, keep) }, enabled = model.tidy.working == null) { Text("Merge, keep selected") }
                Spacer(Modifier.width(8.dp))
                TextButton({ tidy.dismiss(g.key) }) { Text("Not duplicates") }
            }
            HorizontalDivider(Modifier.padding(top = 6.dp))
        }
    }
}

private fun contents(p: Paper): String = listOfNotNull(
    if (p.hasPdf) "PDF" else "no PDF",
    if (p.hasEpub) "reading view" else "no reading view",
    p.annotationCount.toInt().takeIf { it > 0 }?.let { plural(it, "highlight", "highlights") },
).joinToString(" · ")

private fun LazyListScope.missingSection(model: AppModel, onSwitch: (String) -> Unit) {
    val tidy = model.tidy
    val m = tidy.missing ?: return
    header("Missing details")
    if (m.noDoi.isEmpty() && m.noPdf.isEmpty() && m.noReadingView.isEmpty() && m.published.isEmpty()) {
        item(key = "miss-none") { Quiet("Every paper has its DOI, PDF and reading view.") }
    }
    if (m.noDoi.isNotEmpty() || tidy.dois?.suggestions?.isNotEmpty() == true) item(key = "miss-doi") {
        MissingGroup(model, "No DOI", m.noDoi, "doi", "Find DOIs", "Doesn't need one") { tidy.findDois() }
    }
    tidy.dois?.let { d ->
        item(key = "doi-head") {
            Column(Modifier.padding(start = 8.dp)) {
                Text(
                    if (d.suggestions.isEmpty()) "No DOIs found." else "Found ${plural(d.suggestions.size, "DOI", "DOIs")}: check each before you add it.",
                    style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium,
                )
                if (d.notFound.isNotEmpty()) Quiet("Nothing was plainly found for ${plural(d.notFound.size, "paper", "papers")}.")
                for (e in d.errors) Text(e, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
        }
        if (d.suggestions.isNotEmpty()) item(key = "doi-list") { DoiList(model, d.suggestions) }
    }
    if (m.noPdf.isNotEmpty()) item(key = "miss-pdf") {
        MissingGroup(model, "No PDF", m.noPdf, "pdf", "Find PDFs", "Doesn't need one") { tidy.findPdfs(m.noPdf) }
    }
    if (m.noReadingView.isNotEmpty()) item(key = "miss-epub") {
        MissingGroup(model, "No reading view", m.noReadingView, "epub", "Make reading views", "Don't suggest") { tidy.makeReadingViews(m.noReadingView) }
    }
    if (m.published.isNotEmpty()) {
        item(key = "pub-head") {
            Text("Published preprints (${m.published.size})", Modifier.padding(top = 10.dp), style = MaterialTheme.typography.titleSmall)
            Quiet("Preprints whose published version the checks found (not in your library).")
        }
        items(m.published, key = { "pub-${it.paperId}" }) { s ->
            val p = model.paper(s.paperId) ?: return@items
            val v = s.version
            Column(Modifier.fillMaxWidth().padding(start = 8.dp, top = 6.dp)) {
                Text(p.meta.title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium, maxLines = 3, overflow = TextOverflow.Ellipsis)
                Text(
                    "Published in ${v.journal.ifBlank { "a journal" }}${if (v.date.isBlank()) "" else " (${v.date.take(4)})"} · ${v.doi}",
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                TextButton({ onSwitch(s.paperId) }, enabled = tidy.working == null, contentPadding = PaddingValues(0.dp)) { Text("Switch to published version") }
            }
        }
    }
}

/** "No PDF (12)" with its one-click action, and the papers (each can be left out of the suggestions). */
@Composable
private fun MissingGroup(model: AppModel, title: String, ids: List<String>, kind: String, action: String, dismissLabel: String, onAction: () -> Unit) {
    val tidy = model.tidy
    var expanded by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().padding(top = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.weight(1f).clickable { expanded = !expanded }, verticalAlignment = Alignment.CenterVertically) {
                Text("$title (${ids.size})", style = MaterialTheme.typography.titleSmall)
                Icon(if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore, if (expanded) "Hide papers" else "Show papers")
            }
            if (ids.isNotEmpty()) FilledTonalButton(onAction, enabled = tidy.working == null) { Text(action) }
        }
        if (expanded) for (id in ids) {
            val p = model.paper(id) ?: continue
            Row(Modifier.fillMaxWidth().padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f).padding(vertical = 4.dp)) { PaperLines(p) }
                IconButton({ tidy.dismiss("$kind:$id") }) { Icon(Icons.Filled.Close, dismissLabel) }
            }
        }
    }
}

@Composable
private fun DoiList(model: AppModel, list: List<DoiSuggestion>) {
    val tidy = model.tidy
    // a DOI another paper has is a duplicate to merge, not ticked
    val checked = remember(list) { mutableStateMapOf<String, Boolean>().apply { list.forEach { put(it.paperId, it.duplicateOf == null) } } }
    Column(Modifier.fillMaxWidth().padding(start = 8.dp)) {
        for (s in list) {
            val p = model.paper(s.paperId) ?: continue
            Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.Top) {
                Checkbox(checked[s.paperId] == true, { checked[s.paperId] = it })
                Column(Modifier.weight(1f).padding(top = 10.dp)) {
                    Text(p.meta.title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium, maxLines = 3, overflow = TextOverflow.Ellipsis)
                    val f = s.found
                    Text("→ ${f.title}", style = MaterialTheme.typography.bodySmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
                    Text(
                        listOf(f.journal, f.date.take(4)).filter { it.isNotBlank() }.joinToString(" · "),
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text("${s.doi} · ${s.source}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
                    s.duplicateOf?.let { other ->
                        Text(
                            "Another paper in your library has this DOI${model.paper(other)?.let { " (“${it.meta.title}”)" } ?: ""}: merge them instead (Possible duplicates).",
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error,
                        )
                    }
                }
            }
        }
        val chosen = list.filter { checked[it.paperId] == true }
        Row {
            FilledTonalButton({ tidy.applyDois(chosen) }, enabled = chosen.isNotEmpty() && tidy.working == null) { Text("Apply selected (${chosen.size})") }
            Spacer(Modifier.width(8.dp))
            TextButton({ tidy.applyDois(list.filter { it.duplicateOf == null }) }, enabled = tidy.working == null && list.any { it.duplicateOf == null }) { Text("Apply all") }
        }
    }
}

private fun LazyListScope.topicsSection(model: AppModel) {
    val tidy = model.tidy
    val ai = OnDeviceAi.usable
    header(
        "Suggested tags or folders",
        if (tidy.topicsByModel) "Suggested by the on-device model (${OnDeviceAi.model.name}) from your latest titles. It can be wrong: check them."
        else "Found from words your papers share (titles, keywords, abstracts).",
    )
    item(key = "topics-actions") {
        Column {
            tidy.topicsNote?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary) }
            Row {
                if (ai && !tidy.topicsByModel) OutlinedButton({ tidy.suggestTopicsWithModel() }, enabled = tidy.working == null) { Text("Suggest with on-device AI") }
                if (tidy.topicsByModel) TextButton({ tidy.heuristicTopics() }, enabled = tidy.working == null) { Text("Show the ones found from shared words") }
            }
        }
    }
    if (tidy.topics.isEmpty()) item(key = "topics-none") { Quiet("No topics to suggest.") }
    else item(key = "topics-list") { TopicList(model) }
}

@Composable
private fun TopicList(model: AppModel) {
    val tidy = model.tidy
    val list = tidy.topics
    // names edited and ticks, by the topic's papers (the name may change)
    val names = remember(list) { mutableStateMapOf<Int, String>().apply { list.forEachIndexed { i, g -> put(i, g.name) } } }
    val checked = remember(list) { mutableStateMapOf<Int, Boolean>().apply { list.indices.forEach { put(it, false) } } }
    var renaming by remember { mutableStateOf<Int?>(null) }
    Column(Modifier.fillMaxWidth()) {
        list.forEachIndexed { i, g ->
            var expanded by remember(g) { mutableStateOf(false) }
            Column(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked[i] == true, { checked[i] = it })
                    Column(Modifier.weight(1f).clickable { expanded = !expanded }) {
                        Text("${names[i] ?: g.name} — ${plural(g.paperIds.size, "paper", "papers")}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                        if (g.why.isNotBlank()) Text(g.why, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }
                    IconButton({ renaming = i }) { Icon(Icons.Filled.Edit, "Rename") }
                    IconButton({ expanded = !expanded }) { Icon(if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore, "Papers") }
                    IconButton({ tidy.dismissTopic(g) }) { Icon(Icons.Filled.Close, "Don't suggest") }
                }
                if (expanded) for (id in g.paperIds) {
                    val p = model.paper(id) ?: continue
                    Text("• ${p.meta.title}", Modifier.padding(start = 48.dp, bottom = 2.dp), style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        val chosen = list.indices.filter { checked[it] == true }.map { i -> list[i].copy(name = names[i]?.trim()?.ifBlank { null } ?: list[i].name) }
        Row(Modifier.padding(top = 4.dp)) {
            FilledTonalButton({ tidy.createGroups(chosen, false) }, enabled = chosen.isNotEmpty() && tidy.working == null) { Text("Create as tags") }
            Spacer(Modifier.width(8.dp))
            OutlinedButton({ tidy.createGroups(chosen, true) }, enabled = chosen.isNotEmpty() && tidy.working == null) { Text("Create as folders") }
        }
    }
    renaming?.let { i -> NameDialog("Rename", names[i] ?: list[i].name, "Rename", { renaming = null }) { names[i] = it; checked[i] = true } }
}

private fun LazyListScope.tagsSection(model: AppModel) {
    val tidy = model.tidy
    header("Tags for untagged papers (${tidy.tagSuggestions.size})", "Tags you already use, for papers that have none.")
    if (tidy.tagSuggestions.isEmpty()) item(key = "tags-none") { Quiet("No tags to suggest.") }
    else item(key = "tags-list") { TagList(model, tidy.tagSuggestions) }
    item(key = "end") { Spacer(Modifier.height(72.dp)) }
}

@Composable
private fun TagList(model: AppModel, list: List<TagSuggestion>) {
    val tidy = model.tidy
    val checked = remember(list) { mutableStateMapOf<Int, Boolean>() }
    Column(Modifier.fillMaxWidth()) {
        list.forEachIndexed { i, s ->
            val p = model.paper(s.paperId) ?: return@forEachIndexed
            Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.Top) {
                Checkbox(checked[i] == true, { checked[i] = it })
                Column(Modifier.weight(1f).padding(top = 10.dp)) {
                    Text("${p.meta.title} → ${s.tag}", style = MaterialTheme.typography.bodyMedium, maxLines = 3, overflow = TextOverflow.Ellipsis)
                    if (s.because.isNotBlank()) Text(s.because, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                IconButton({ tidy.dismiss("tag:${s.paperId}:${s.tag}") }) { Icon(Icons.Filled.Close, "Don't suggest") }
            }
        }
        val chosen = list.filterIndexed { i, _ -> checked[i] == true }
        Row(Modifier.padding(top = 4.dp)) {
            FilledTonalButton({ tidy.applyTags(chosen) }, enabled = chosen.isNotEmpty() && tidy.working == null) { Text("Apply selected (${chosen.size})") }
            Spacer(Modifier.width(8.dp))
            TextButton({ tidy.applyTags(list) }, enabled = tidy.working == null) { Text("Apply all") }
        }
    }
}

// ---- pieces ----

@Composable
private fun PaperLines(p: Paper) {
    Text(p.meta.title.ifBlank { "Untitled" }, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium, maxLines = 3, overflow = TextOverflow.Ellipsis)
    val sub = listOf(authorsLine(p), p.meta.date.take(4), p.meta.journal).filter { it.isNotBlank() }.joinToString(" · ")
    if (sub.isNotEmpty()) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

@Composable
private fun Quiet(text: String) =
    Text(text, Modifier.padding(vertical = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
