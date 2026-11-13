package app.lunta.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.Article
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.unit.dp
import app.lunta.*
import kotlinx.coroutines.launch
import uniffi.lunta_core.CitationStyle
import uniffi.lunta_core.FileKind

/** A paper's info page: details, abstract, folders and tags, cite, files and the Read button. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun InfoScreen(model: AppModel, id: String) {
    val p = model.paper(id)
    if (p == null) { LaunchedEffect(Unit) { model.pop() }; return }
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var organizing by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    var citation by remember { mutableStateOf<Pair<String, String>?>(null) }
    var menu by remember { mutableStateOf(false) }
    val files by produceState(emptySet<String>(), p.id, p.hasEpub, p.hasPdf) { value = model.availableFiles(p.id) }
    val hasEpub = p.hasEpub || "book.epub" in files
    val hasPdf = p.hasPdf || "paper.pdf" in files
    val pickPdf = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { u -> ctx.contentResolver.openInputStream(u)?.let { model.attachPdf(p.id, it) } }
    }
    val folders = model.collections.filter { it.id in p.collections }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = { IconButton({ model.pop() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } },
                actions = {
                    IconButton({ model.toggleStar(p) }) {
                        Icon(if (p.starred) Icons.Filled.Star else Icons.Outlined.StarOutline, "Star", tint = if (p.starred) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    IconButton({ model.toggleMustRead(p) }) {
                        Icon(if (p.mustRead) Icons.Filled.Bookmark else Icons.Outlined.BookmarkBorder, "Must Read", tint = if (p.mustRead) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Box {
                        IconButton({ menu = true }) { Icon(Icons.Filled.MoreVert, "More") }
                        DropdownMenu(menu, { menu = false }) {
                            DropdownMenuItem({ Text(if (p.read) "Mark as unread" else "Mark as read") }, { menu = false; model.toggleRead(p) })
                            DropdownMenuItem({ Text("Folders & tags…") }, { menu = false; organizing = true })
                            DropdownMenuItem({ Text("Delete paper…") }, { menu = false; deleting = true })
                        }
                    }
                },
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).padding(bottom = 32.dp)) {
            Text(p.meta.title.ifBlank { "Untitled" }, style = MaterialTheme.typography.headlineSmall)
            if (p.meta.authors.isNotEmpty()) {
                Spacer(Modifier.height(8.dp))
                Text(p.meta.authors.joinToString(", "), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            val venue = listOf(p.meta.journal, p.meta.date.take(4)).filter { it.isNotBlank() }.joinToString(", ")
            if (venue.isNotEmpty()) Text(venue, Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodyMedium, fontStyle = FontStyle.Italic)
            if (p.meta.doi.isNotBlank()) {
                TextButton(
                    { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://doi.org/${p.meta.doi}"))) },
                    contentPadding = PaddingValues(0.dp),
                ) { Text("doi:${p.meta.doi}", style = MaterialTheme.typography.bodySmall) }
            }

            Spacer(Modifier.height(16.dp))
            // Read: the reading view when there is one, else the PDF
            Button(
                onClick = {
                    scope.launch {
                        if (hasEpub) { model.ensureFiles(p.id, FileKind.EPUB); model.markRead(p.id); model.push(Screen.Reader(p.id, false)) }
                        else if (hasPdf) { model.ensureFiles(p.id, FileKind.PDF); model.markRead(p.id); model.push(Screen.Reader(p.id, true)) }
                    }
                },
                enabled = hasEpub || hasPdf, modifier = Modifier.fillMaxWidth(),
            ) {
                Icon(Icons.Filled.AutoStories, null)
                Spacer(Modifier.width(8.dp))
                Text(if (hasEpub || hasPdf) "Read" else "No full text yet")
            }
            FlowRow(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (hasPdf && hasEpub) OutlinedButton({
                    scope.launch { model.ensureFiles(p.id, FileKind.PDF); model.markRead(p.id); model.push(Screen.Reader(p.id, true)) }
                }) { Icon(Icons.Outlined.PictureAsPdf, null); Spacer(Modifier.width(6.dp)); Text("Open PDF") }
                if (!hasEpub && (hasPdf || p.meta.doi.isNotBlank() || p.meta.pmcid.isNotBlank() || p.meta.arxiv.isNotBlank())) {
                    OutlinedButton({ model.makeReadingView(p.id) }) { Icon(Icons.AutoMirrored.Outlined.Article, null); Spacer(Modifier.width(6.dp)); Text("Make reading view") }
                }
                if (!hasPdf) {
                    OutlinedButton({ pickPdf.launch(arrayOf("application/pdf")) }) { Icon(Icons.Outlined.FileOpen, null); Spacer(Modifier.width(6.dp)); Text("Add PDF from Files") }
                    if (p.meta.doi.isNotBlank() || p.meta.arxiv.isNotBlank()) {
                        OutlinedButton({ model.findPdf(p.id) }) { Icon(Icons.Outlined.Search, null); Spacer(Modifier.width(6.dp)); Text("Find PDF") }
                    }
                } else {
                    TextButton({ pickPdf.launch(arrayOf("application/pdf")) }) { Text("Replace PDF…") }
                }
            }

            if (p.meta.abstractText.isNotBlank()) {
                Spacer(Modifier.height(20.dp))
                SectionTitle("Abstract")
                Text(p.meta.abstractText, style = MaterialTheme.typography.bodyMedium)
            }

            Spacer(Modifier.height(20.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                SectionTitle("Folders & Tags", Modifier.weight(1f))
                TextButton({ organizing = true }) { Text("Edit") }
            }
            if (folders.isEmpty() && p.tags.isEmpty()) Text("Not in any folder, no tags.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (f in folders) AssistChip({ model.push(Screen.Papers(Place.Folder(f.id))) }, { Text(f.name) }, leadingIcon = { Icon(Icons.Outlined.Folder, null, Modifier.size(18.dp)) })
                for (t in p.tags) AssistChip({ model.push(Screen.Papers(Place.Tag(t))) }, { Text(t) }, leadingIcon = { Icon(Icons.Outlined.Sell, null, Modifier.size(18.dp)) })
            }

            Spacer(Modifier.height(20.dp))
            SectionTitle("Cite")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for ((style, label) in listOf(CitationStyle.APA to "APA", CitationStyle.VANCOUVER to "Vancouver", CitationStyle.NATURE to "Nature")) {
                    OutlinedButton({
                        model.cite(p, style)?.let { text ->
                            copyText(ctx, label, text)
                            citation = label to text
                        }
                    }, contentPadding = PaddingValues(horizontal = 12.dp)) { Text(label) }
                }
            }
            citation?.let { (label, text) ->
                Card(Modifier.padding(top = 8.dp).fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
                    Column(Modifier.padding(12.dp)) {
                        Text("$label · copied", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
                        Text(text, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }

            Spacer(Modifier.height(24.dp))
            Text(
                listOfNotNull(
                    if (hasEpub) "Reading view" else null, if (hasPdf) "PDF" else null,
                    if (p.annotationCount > 0u) "${p.annotationCount} highlight${if (p.annotationCount == 1u) "" else "s"}" else null,
                ).joinToString(" · "),
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(16.dp))
            TextButton({ deleting = true }, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) {
                Icon(Icons.Outlined.Delete, null); Spacer(Modifier.width(6.dp)); Text("Delete Paper")
            }
        }
    }
    if (organizing) OrganizeSheet(model, p.id) { organizing = false }
    if (deleting) AlertDialog(
        onDismissRequest = { deleting = false },
        title = { Text("Delete this paper?") },
        text = { Text("“${p.meta.title}” and its files, highlights and notes are removed from your library, on every device that syncs it.") },
        confirmButton = { TextButton({ deleting = false; model.pop(); model.deletePaper(p.id) }, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Delete") } },
        dismissButton = { TextButton({ deleting = false }) { Text("Cancel") } },
    )
}

@Composable
private fun SectionTitle(text: String, modifier: Modifier = Modifier) =
    Text(text, modifier.padding(bottom = 6.dp), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)

fun copyText(ctx: Context, label: String, text: String) {
    (ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText(label, text))
    Toast.makeText(ctx, "Copied $label citation", Toast.LENGTH_SHORT).show()
}
