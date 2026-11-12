package app.lunta.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import app.lunta.AppModel
import uniffi.lunta_core.Paper
import uniffi.lunta_core.PaperCollection

@Composable
fun AddPaperDialog(onDismiss: () -> Unit, onAdd: (String) -> Unit) {
    var text by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add Paper") },
        text = {
            Column {
                OutlinedTextField(
                    text, { text = it }, Modifier.fillMaxWidth(), singleLine = true,
                    label = { Text("DOI, link or PMCID") },
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                )
                Text("e.g. 10.1038/s41586-020-2649-2 or an article link", Modifier.padding(top = 6.dp), style = MaterialTheme.typography.bodySmall)
            }
        },
        confirmButton = { TextButton({ onAdd(text.trim()); onDismiss() }, enabled = text.isNotBlank()) { Text("Add") } },
        dismissButton = { TextButton(onDismiss) { Text("Cancel") } },
    )
}

@Composable
fun NameDialog(title: String, initial: String, confirm: String, onDismiss: () -> Unit, onDone: (String) -> Unit) {
    var text by remember { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { OutlinedTextField(text, { text = it }, Modifier.fillMaxWidth(), singleLine = true, label = { Text("Name") }) },
        confirmButton = { TextButton({ onDone(text.trim()); onDismiss() }, enabled = text.isNotBlank()) { Text(confirm) } },
        dismissButton = { TextButton(onDismiss) { Text("Cancel") } },
    )
}

/** Folders and tags of one paper (the iPhone app's Organize sheet). */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun OrganizeSheet(model: AppModel, paperId: String, onDismiss: () -> Unit) {
    val paper = model.paper(paperId) ?: return
    var newTag by remember { mutableStateOf("") }
    var newFolder by remember { mutableStateOf(false) }
    ModalBottomSheet(onDismiss) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 24.dp).padding(bottom = 32.dp)) {
            Text("Folders & Tags", style = MaterialTheme.typography.titleLarge)
            Text(paper.meta.title, style = MaterialTheme.typography.bodySmall, maxLines = 2, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                Text("Folders", Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
                TextButton({ newFolder = true }) { Text("New folder") }
            }
            if (model.collections.isEmpty()) Text("No folders yet.", style = MaterialTheme.typography.bodyMedium)
            for ((c, depth) in flatten(model.collections)) {
                val on = c.id in paper.collections
                Row(
                    Modifier.fillMaxWidth().padding(start = 16.dp * depth),
                    verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
                ) {
                    Checkbox(on, { model.setMember(paper, c.id, it) })
                    Text(c.name, style = MaterialTheme.typography.bodyLarge)
                }
            }
            Spacer(Modifier.height(16.dp))
            Text("Tags", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
            val all = (model.tags + paper.tags).distinct().sortedBy { it.lowercase() }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (t in all) FilterChip(t in paper.tags, { model.setTag(paper, t, t !in paper.tags) }, { Text(t) })
            }
            Row(Modifier.padding(top = 8.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                OutlinedTextField(
                    newTag, { newTag = it }, Modifier.weight(1f), singleLine = true, label = { Text("New tag") },
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None),
                )
                Spacer(Modifier.width(8.dp))
                FilledTonalButton({ model.setTag(paper, newTag.trim(), true); newTag = "" }, enabled = newTag.isNotBlank()) { Text("Add") }
            }
        }
    }
    if (newFolder) NameDialog("New Folder", "", "Create", { newFolder = false }) { name ->
        model.newFolder(name) { c -> model.setMember(paper, c.id, true) }
    }
}

private fun flatten(all: List<PaperCollection>): List<Pair<PaperCollection, Int>> {
    val byParent = all.groupBy { it.parent?.takeIf { p -> all.any { c -> c.id == p } } }
    val out = mutableListOf<Pair<PaperCollection, Int>>()
    fun walk(parent: String?, depth: Int) { for (c in byParent[parent].orEmpty()) { out += c to depth; walk(c.id, depth + 1) } }
    walk(null, 0)
    return out
}

fun authorsLine(p: Paper): String {
    val a = p.meta.authors
    return when {
        a.isEmpty() -> ""
        a.size == 1 -> a[0]
        a.size == 2 -> "${lastName(a[0])} & ${lastName(a[1])}"
        else -> "${lastName(a[0])} et al."
    }
}

private fun lastName(full: String): String = if (full.contains(",")) full.substringBefore(",").trim() else full.trim().substringAfterLast(' ')
