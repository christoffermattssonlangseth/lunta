package app.lunta.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.lunta.AppModel
import app.lunta.LibraryTidy.Companion.plural
import app.lunta.ai.OnDeviceAi

/**
 * Outline from highlights (a folder's or tag's menu, a paper list's share menu: the list or the
 * search shown): ask for an optional topic and, when the model is downloaded, whether the on-device
 * model groups the points; then the outline's Markdown to copy, share or save as a .md file.
 */
@Composable
fun OutlineDialog(model: AppModel) {
    val outline = model.outline
    val (what, ids) = outline.request ?: return
    val ctx = LocalContext.current
    LaunchedEffect(Unit) { OnDeviceAi.init(ctx) }
    val result = outline.result
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/markdown")) { uri ->
        val md = outline.result?.markdown ?: return@rememberLauncherForActivityResult
        if (uri != null) runCatching {
            ctx.contentResolver.openOutputStream(uri)?.use { it.write(md.toByteArray()) } ?: error("no stream")
        }.onSuccess { Toast.makeText(ctx, "Saved the outline", Toast.LENGTH_SHORT).show() }
            .onFailure { model.error = "Couldn't save the outline: ${it.message}" }
    }

    if (result == null) {
        var withModel by remember { mutableStateOf(OnDeviceAi.usable) }
        AlertDialog(
            onDismissRequest = { outline.close() },
            title = { Text("Outline from highlights") },
            text = {
                Column {
                    Text(
                        "The highlights and notes in $what (${plural(ids.size, "paper", "papers")}), each word for word with its citation, as Markdown.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(outline.topic, { outline.topic = it }, Modifier.fillMaxWidth(), singleLine = true, label = { Text("Topic (optional)") }, enabled = !outline.running)
                    Text(
                        "Only the highlights and notes that mention it (all when none does).",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (OnDeviceAi.usable) {
                        Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text("Group with on-device AI", Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
                            Switch(withModel, { withModel = it }, enabled = !outline.running)
                        }
                        Text(
                            if (withModel) "The model (${OnDeviceAi.model.name}) only groups the points into themes and names them; every point stays a highlight or your note. Up to ${app.lunta.LibraryTidy.MODEL_HIGHLIGHTS} highlights are given to it; it may take a minute."
                            else "Grouped by paper and section.",
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    if (outline.running) {
                        LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 12.dp))
                        Text(if (withModel && OnDeviceAi.usable) "The on-device model is grouping the highlights…" else "Making the outline…", style = MaterialTheme.typography.bodySmall)
                    }
                }
            },
            confirmButton = {
                TextButton({ outline.make(outline.topic.trim(), withModel && OnDeviceAi.usable) }, enabled = !outline.running && ids.isNotEmpty()) { Text("Make outline") }
            },
            dismissButton = { TextButton({ outline.close() }) { Text("Cancel") } },
        )
        return
    }

    Dialog({ outline.close() }, DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize().padding(12.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(16.dp)) {
                Text("Outline: ${result.title}", style = MaterialTheme.typography.titleLarge)
                Text(
                    "${plural(result.items.toInt(), "highlight or note", "highlights and notes")} from ${plural(result.papers.toInt(), "paper", "papers")}" +
                        if (result.byModel) " · themes by the on-device model (it can be wrong: check them)" else " · by paper and section",
                    Modifier.padding(vertical = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (result.note.isNotBlank()) Text(result.note, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary)
                Surface(Modifier.weight(1f).fillMaxWidth().padding(vertical = 8.dp), color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = .4f), shape = MaterialTheme.shapes.medium) {
                    SelectionContainer {
                        Text(
                            result.markdown, Modifier.verticalScroll(rememberScrollState()).padding(12.dp),
                            style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace,
                        )
                    }
                }
                Row {
                    TextButton({ copyOutline(ctx, result.markdown, result.html) }) { Text("Copy") }
                    TextButton({
                        val send = Intent(Intent.ACTION_SEND).setType("text/markdown").putExtra(Intent.EXTRA_TEXT, result.markdown).putExtra(Intent.EXTRA_SUBJECT, result.title)
                        ctx.startActivity(Intent.createChooser(send, "Share the outline"))
                    }) { Text("Share") }
                    TextButton({ save.launch(fileName(result.title)) }) { Text("Save .md") }
                    Spacer(Modifier.weight(1f))
                    TextButton({ outline.back() }) { Text("Back") }
                    TextButton({ outline.close() }) { Text("Done") }
                }
            }
        }
    }
}

/** The Markdown as text, with the HTML for apps that paste rich text. */
private fun copyOutline(ctx: Context, markdown: String, html: String) {
    (ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newHtmlText("Outline", markdown, html))
    Toast.makeText(ctx, "Copied the outline", Toast.LENGTH_SHORT).show()
}

private fun fileName(title: String): String {
    val base = title.replace(Regex("[\\\\/:*?\"<>|\\n]"), " ").trim().take(80).ifBlank { "Highlights" }
    return "$base.md"
}
