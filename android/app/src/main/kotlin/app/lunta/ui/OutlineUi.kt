package app.lunta.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.ClickableText
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.lunta.AppModel
import app.lunta.LibraryTidy.Companion.plural
import app.lunta.ai.OnDeviceAi
import uniffi.lunta_core.HighlightSummary
import uniffi.lunta_core.SummaryLength
import uniffi.lunta_core.SummarySource
import uniffi.lunta_core.SummaryStyle

/**
 * Outline from highlights (a folder's or tag's menu, a paper list's share menu: the list or the
 * search shown): ask for an optional topic and, when the model is downloaded, whether the on-device
 * model groups the points; then the outline's Markdown to copy, share or save as a .md file.
 * Or a Summary: the on-device model condenses the highlights, every sentence citing those it is
 * based on (checked by the core); a citation shows its quote, with Open in Paper.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OutlineDialog(model: AppModel) {
    val outline = model.outline
    val (what, ids) = outline.request ?: return
    val ctx = LocalContext.current
    LaunchedEffect(Unit) { OnDeviceAi.init(ctx) }
    val result = outline.result
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/markdown")) { uri ->
        val md = outline.summary?.markdown ?: outline.result?.markdown ?: return@rememberLauncherForActivityResult
        if (uri != null) runCatching {
            ctx.contentResolver.openOutputStream(uri)?.use { it.write(md.toByteArray()) } ?: error("no stream")
        }.onSuccess { Toast.makeText(ctx, "Saved", Toast.LENGTH_SHORT).show() }
            .onFailure { model.error = "Couldn't save the outline: ${it.message}" }
    }

    val summary = outline.summary
    if (summary != null) {
        SummaryResult(model, summary, onSave = { save.launch(fileName(summary.title)) })
        return
    }

    if (result == null) {
        var withModel by remember { mutableStateOf(OnDeviceAi.usable) }
        AlertDialog(
            onDismissRequest = { outline.close() },
            title = { Text(if (outline.asSummary) "Summarize highlights" else "Outline from highlights") },
            text = {
                Column {
                    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(bottom = 8.dp)) {
                        SegmentedButton(!outline.asSummary, { outline.asSummary = false }, SegmentedButtonDefaults.itemShape(0, 2), enabled = !outline.running) { Text("Outline") }
                        SegmentedButton(outline.asSummary, { outline.asSummary = true }, SegmentedButtonDefaults.itemShape(1, 2), enabled = !outline.running) { Text("Summary") }
                    }
                    Text(
                        if (outline.asSummary) "The highlights and notes in $what (${plural(ids.size, "paper", "papers")}), condensed by the on-device model into a few sentences, each citing the highlights it is based on."
                        else "The highlights and notes in $what (${plural(ids.size, "paper", "papers")}), each word for word with its citation, as Markdown.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(outline.topic, { outline.topic = it }, Modifier.fillMaxWidth(), singleLine = true, label = { Text("Topic (optional)") }, enabled = !outline.running)
                    Text(
                        "Only the highlights and notes that mention it (all when none does).",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (outline.asSummary) {
                        if (OnDeviceAi.usable) {
                            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(top = 8.dp)) {
                                SegmentedButton(!outline.longer, { outline.longer = false }, SegmentedButtonDefaults.itemShape(0, 2), enabled = !outline.running) { Text("Short") }
                                SegmentedButton(outline.longer, { outline.longer = true }, SegmentedButtonDefaults.itemShape(1, 2), enabled = !outline.running) { Text("Longer") }
                            }
                            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(top = 6.dp)) {
                                SegmentedButton(!outline.bullets, { outline.bullets = false }, SegmentedButtonDefaults.itemShape(0, 2), enabled = !outline.running) { Text("Prose") }
                                SegmentedButton(outline.bullets, { outline.bullets = true }, SegmentedButtonDefaults.itemShape(1, 2), enabled = !outline.running) { Text("Bullet points") }
                            }
                            Text(
                                "Sentences that cite nothing, or say more than the highlights they cite, are left out. Many highlights are summarized in parts, then combined: it may take a few minutes.",
                                Modifier.padding(top = 6.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        } else {
                            Text(
                                "Summaries need the on-device model: download it in Settings › On-device AI (one time). The outline works without it.",
                                Modifier.padding(top = 8.dp), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.tertiary,
                            )
                        }
                    } else if (OnDeviceAi.usable) {
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
                        Text(
                            when {
                                outline.asSummary -> "The on-device model is summarizing the highlights…"
                                withModel && OnDeviceAi.usable -> "The on-device model is grouping the highlights…"
                                else -> "Making the outline…"
                            },
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
            },
            confirmButton = {
                if (outline.asSummary) {
                    TextButton(
                        { outline.summarize(outline.topic.trim(), if (outline.longer) SummaryLength.LONGER else SummaryLength.SHORT, if (outline.bullets) SummaryStyle.BULLETS else SummaryStyle.PROSE) },
                        enabled = !outline.running && ids.isNotEmpty() && OnDeviceAi.usable,
                    ) { Text("Summarize") }
                } else {
                    TextButton({ outline.make(outline.topic.trim(), withModel && OnDeviceAi.usable) }, enabled = !outline.running && ids.isNotEmpty()) { Text("Make outline") }
                }
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
                outline.summaryFailed?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary) }
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
private fun copyOutline(ctx: Context, markdown: String, html: String, what: String = "outline") {
    (ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newHtmlText(what, markdown, html))
    Toast.makeText(ctx, "Copied the $what", Toast.LENGTH_SHORT).show()
}

/** The summary: its sentences with tappable citations, the quote of the one tapped (Open in Paper), the quotes cited. */
@Composable
private fun SummaryResult(model: AppModel, s: HighlightSummary, onSave: () -> Unit) {
    val outline = model.outline
    val ctx = LocalContext.current
    var selected by remember(s) { mutableStateOf<SummarySource?>(null) }
    Dialog({ outline.close() }, DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize().padding(12.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(16.dp)) {
                Text(s.title, style = MaterialTheme.typography.titleLarge)
                Text(
                    "${plural(s.items.toInt(), "highlight or note", "highlights and notes")} from ${plural(s.papers.toInt(), "paper", "papers")} · ${OnDeviceAi.model.name}",
                    Modifier.padding(vertical = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (s.note.isNotBlank()) Text(s.note, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary)
                if (outline.running) LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 6.dp))
                Column(Modifier.weight(1f).fillMaxWidth().padding(vertical = 8.dp).verticalScroll(rememberScrollState())) {
                    val link = SpanStyle(color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold)
                    val text = buildAnnotatedString {
                        s.sentences.forEachIndexed { i, x ->
                            if (s.bullets) { if (i > 0) append("\n"); append("•  ") } else if (i > 0) append(" ")
                            val end = x.text.lastOrNull()?.takeIf { it in ".!?" }?.toString() ?: "."
                            append((if (x.text.lastOrNull()?.let { it in ".!?" } == true) x.text.dropLast(1) else x.text) + " [")
                            x.citations.forEachIndexed { k, n ->
                                if (k > 0) append(", ")
                                pushStringAnnotation("cite", n.toString())
                                pushStyle(link); append(n.toString()); pop()
                                pop()
                            }
                            append("]$end")
                        }
                    }
                    @Suppress("DEPRECATION")
                    ClickableText(text, style = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface)) { at ->
                        text.getStringAnnotations("cite", at, at).firstOrNull()?.let { a -> selected = s.sources.firstOrNull { it.number.toString() == a.item } }
                    }
                    Text(
                        "Written by the on-device model from your highlights. Check it against the quotes below.",
                        Modifier.padding(top = 8.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    selected?.let { x ->
                        Surface(Modifier.fillMaxWidth().padding(top = 8.dp), color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = .5f), shape = MaterialTheme.shapes.medium) {
                            Column(Modifier.padding(12.dp)) {
                                Text("[${x.number}] ${where(x)}", style = MaterialTheme.typography.labelLarge)
                                Text(quoteOf(x), Modifier.padding(vertical = 4.dp), style = MaterialTheme.typography.bodyMedium)
                                Row {
                                    TextButton({ outline.openAt(x) }) { Text("Open in Paper") }
                                    Spacer(Modifier.weight(1f))
                                    TextButton({ selected = null }) { Text("Close") }
                                }
                            }
                        }
                    }
                    HorizontalDivider(Modifier.padding(vertical = 10.dp))
                    Text("Highlights cited", style = MaterialTheme.typography.titleSmall)
                    for (x in s.sources) {
                        Row(Modifier.fillMaxWidth().padding(top = 8.dp)) {
                            Text("[${x.number}]", Modifier.width(36.dp), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold,
                                color = if (selected?.number == x.number) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.primary)
                            Column(Modifier.weight(1f)) {
                                SelectionContainer { Text(quoteOf(x), style = MaterialTheme.typography.bodyMedium) }
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(where(x), Modifier.weight(1f), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                    TextButton({ outline.openAt(x) }) { Text("Open in Paper") }
                                }
                            }
                        }
                    }
                }
                Row(Modifier.horizontalScroll(rememberScrollState())) {
                    TextButton({ copyOutline(ctx, s.markdown, s.html, "summary") }) { Text("Copy") }
                    TextButton({
                        val send = Intent(Intent.ACTION_SEND).setType("text/markdown").putExtra(Intent.EXTRA_TEXT, s.markdown).putExtra(Intent.EXTRA_SUBJECT, s.title)
                        ctx.startActivity(Intent.createChooser(send, "Share the summary"))
                    }) { Text("Share") }
                    TextButton(onSave) { Text("Save .md") }
                    TextButton(
                        { outline.summarize(outline.topic.trim(), if (outline.longer) SummaryLength.LONGER else SummaryLength.SHORT, if (outline.bullets) SummaryStyle.BULLETS else SummaryStyle.PROSE) },
                        enabled = !outline.running,
                    ) { Text("Regenerate") }
                    TextButton({ outline.back() }) { Text("Back") }
                    TextButton({ outline.close() }) { Text("Done") }
                }
            }
        }
    }
}

private fun quoteOf(x: SummarySource): String =
    if (x.quote.isEmpty()) "Note: ${x.note}" else "“${x.quote}”" + if (x.note.isEmpty()) "" else " — Note: ${x.note}"

private fun where(x: SummarySource): String = if (x.section.isEmpty()) x.label else "${x.label} · ${x.section}"

private fun fileName(title: String): String {
    val base = title.replace(Regex("[\\\\/:*?\"<>|\\n]"), " ").trim().take(80).ifBlank { "Highlights" }
    return "$base.md"
}
