package app.lunta.ui

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.lunta.AppModel
import uniffi.lunta_core.NoticeLevel
import uniffi.lunta_core.PaperUpdates
import uniffi.lunta_core.RefCheckItem
import uniffi.lunta_core.RefVerdict
import uniffi.lunta_core.updatesHeadline
import uniffi.lunta_core.updatesLevel

/**
 * Check a Reference List (paste or pick a file) and its report; what a check of the library found
 * (retracted, corrected, preprints since published, with Switch / Link both / Keep the preprint).
 * In LuntaRoot, next to ImportDialogs; the library's + menu opens them (model.checks).
 */
@Composable
fun ChecksDialogs(model: AppModel) {
    val checks = model.checks
    val ctx = LocalContext.current
    val pickFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { checks.checkUri(it) }
    }
    LaunchedEffect(model.library) { checks.weekly() }

    if (checks.asking) {
        var text by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { checks.asking = false },
            title = { Text("Check a reference list") },
            text = {
                Column {
                    Text(
                        "Paste a reference list (numbered or author–year), BibTeX or RIS, or choose a file. Each reference is looked up in Crossref, DataCite, OpenAlex and PubMed and its details compared with the record. Nothing is added to your library.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(text, { text = it }, Modifier.fillMaxWidth().heightIn(min = 160.dp, max = 320.dp), label = { Text("References") })
                }
            },
            confirmButton = {
                TextButton({ checks.asking = false; checks.checkText(text, "the pasted list") }, enabled = text.isNotBlank()) { Text("Check") }
            },
            dismissButton = {
                Row {
                    TextButton({ checks.asking = false }) { Text("Cancel") }
                    TextButton({ checks.asking = false; pickFile.launch(arrayOf("*/*")) }) { Text("Choose file") }
                }
            },
        )
    }

    checks.report?.let { (title, r) ->
        Dialog({ checks.report = null }, DialogProperties(usePlatformDefaultWidth = false)) {
            Surface(Modifier.fillMaxSize().padding(12.dp), shape = MaterialTheme.shapes.large) {
                Column(Modifier.padding(16.dp)) {
                    Text("Checked $title", style = MaterialTheme.typography.titleLarge)
                    Text(r.summary, Modifier.padding(vertical = 6.dp), style = MaterialTheme.typography.bodyMedium)
                    Text(
                        "A reference that could not be found may still exist (books, reports, theses and very new papers are often missing): check it by hand.",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    val groups = listOf<Pair<String, (RefCheckItem) -> Boolean>>(
                        "Retracted or with notices" to { i -> level(i) != NoticeLevel.NONE },
                        "DOI points to a different work" to { i -> i.verdict == RefVerdict.WRONG_DOI },
                        "Could not be found in Crossref, OpenAlex or PubMed" to { i -> i.verdict == RefVerdict.NOT_FOUND },
                        "Minor differences" to { i -> i.verdict == RefVerdict.MINOR_DIFFERENCES },
                        "Couldn't be checked" to { i -> i.verdict == RefVerdict.FAILED },
                        "Verified" to { i -> i.verdict == RefVerdict.VERIFIED },
                        "Not looked up (not papers)" to { i -> i.verdict == RefVerdict.NOT_CHECKED },
                    )
                    LazyColumn(Modifier.weight(1f).padding(vertical = 8.dp)) {
                        for ((name, f) in groups) {
                            val items = r.items.filter(f)
                            if (items.isEmpty()) continue
                            item { Text("$name (${items.size})", Modifier.padding(top = 12.dp, bottom = 4.dp), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary) }
                            items(items, key = { "$name-${it.number}" }) { i -> RefItem(i) }
                        }
                    }
                    Row {
                        TextButton({
                            val send = Intent(Intent.ACTION_SEND).setType("text/markdown").putExtra(Intent.EXTRA_TEXT, r.markdown).putExtra(Intent.EXTRA_SUBJECT, "Reference check")
                            ctx.startActivity(Intent.createChooser(send, "Share the report"))
                        }) { Text("Share report") }
                        TextButton({
                            val send = Intent(Intent.ACTION_SEND).setType("text/csv").putExtra(Intent.EXTRA_TEXT, r.csv).putExtra(Intent.EXTRA_SUBJECT, "Reference check (CSV)")
                            ctx.startActivity(Intent.createChooser(send, "Share as CSV"))
                        }) { Text("CSV") }
                        Spacer(Modifier.weight(1f))
                        TextButton({ checks.report = null }) { Text("Done") }
                    }
                }
            }
        }
    }

    checks.summary?.let { s ->
        AlertDialog(
            onDismissRequest = { checks.summary = null },
            title = { Text("Retractions and published versions") },
            text = {
                LazyColumn {
                    item { Text(s.text, style = MaterialTheme.typography.bodyMedium) }
                    val flagged = (s.retracted + s.concern + s.corrected).distinct()
                    items(flagged, key = { "f-$it" }) { id ->
                        val p = model.paper(id)
                        val u = checks.updates[id]
                        if (p != null && u != null) Column(Modifier.padding(vertical = 6.dp)) {
                            Text(p.meta.title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                            Text(updatesHeadline(u), style = MaterialTheme.typography.bodySmall, color = Color(0xFFB91C1C))
                        }
                    }
                    items(s.published.filter { checks.updates[it]?.published != null }, key = { "p-$it" }) { id ->
                        val p = model.paper(id)
                        val v = checks.updates[id]?.published
                        if (p != null && v != null) Column(Modifier.padding(vertical = 6.dp)) {
                            Text(p.meta.title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                            Text("Published in ${v.journal.ifBlank { "a journal" }}${if (v.date.isBlank()) "" else " (${v.date.take(4)})"} · ${v.doi}", style = MaterialTheme.typography.bodySmall)
                            Row {
                                TextButton({ checks.choose(id, "switch") }) { Text("Switch") }
                                TextButton({ checks.choose(id, "link") }) { Text("Link both") }
                                TextButton({ checks.choose(id, "keep") }) { Text("Keep preprint") }
                            }
                        }
                    }
                }
            },
            confirmButton = { TextButton({ checks.summary = null }) { Text("Done") } },
        )
    }
}

private fun level(i: RefCheckItem): NoticeLevel =
    updatesLevel(PaperUpdates(i.notices, null, "", null, null, 0L))

@Composable
private fun RefItem(i: RefCheckItem) {
    Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(if (i.key.isBlank() || i.key == i.number.toString()) "[${i.number}]" else "[${i.number}] ${i.key}${if (i.inLibrary != null) " · in library" else ""}",
            style = MaterialTheme.typography.labelMedium, fontFamily = FontFamily.Monospace, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(i.reference, style = MaterialTheme.typography.bodyMedium)
        val notice = updatesHeadline(PaperUpdates(i.notices, null, "", null, null, 0L))
        if (notice.isNotBlank()) Text(notice.replace("This paper", "The work"), style = MaterialTheme.typography.bodySmall, color = Color(0xFFB91C1C), fontWeight = FontWeight.SemiBold)
        if (i.note.isNotBlank()) Text(i.note, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        for (d in i.differences) {
            Text("${d.field}: ${if (d.given.isBlank()) "not in the reference" else "“${d.given}”"} → “${d.found}” in ${i.source}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (i.link.isNotBlank()) Text(i.link, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
        HorizontalDivider(Modifier.padding(top = 6.dp))
    }
}
