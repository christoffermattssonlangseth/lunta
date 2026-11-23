package app.lunta.foryou

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.ClickableText
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.lunta.AppModel
import app.lunta.Screen
import kotlinx.coroutines.launch
import uniffi.lunta_core.HighlightPaper
import uniffi.lunta_core.HighlightsView

/**
 * "From the journals you follow" in New for You: the latest research papers in journals you choose,
 * whatever their topic (the core's JournalHighlights), with a few sentences on the notable ones by the
 * on-device model, each sentence citing the papers it is about.
 */
fun LazyListScope.journalHighlights(model: AppModel, showAll: Boolean, onShowAll: () -> Unit, onSettings: () -> Unit) {
    val fy = model.forYou
    val v = fy.highlights ?: return
    item { HighlightsHeader(fy, v) }
    if (!v.on) {
        item {
            Card(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                Column(Modifier.padding(16.dp)) {
                    Text("Every Monday, the week’s research papers in journals you choose, whatever their topic, with a few sentences on the notable ones.",
                        style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(8.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Button({ fy.highlightsDefaults() }, enabled = !fy.highlightsRunning) { Icon(Icons.Outlined.Newspaper, null); Spacer(Modifier.width(6.dp)); Text("Nature, Science and Cell") }
                        TextButton(onSettings) { Text("Choose…") }
                    }
                }
            }
        }
        return
    }
    v.problem?.let { p ->
        item {
            Card(Modifier.fillMaxWidth().padding(vertical = 6.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer)) {
                Column(Modifier.padding(16.dp)) {
                    Text("Couldn’t get the journals’ papers", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onErrorContainer)
                    Text(p, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onErrorContainer)
                    TextButton({ fy.runHighlights() }) { Icon(Icons.Filled.Refresh, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Retry") }
                }
            }
        }
    }
    if (v.digest.isNotEmpty() || fy.highlightsWriting) item { HighlightsDigest(model, v) }
    if (v.papers.isEmpty() && v.problem == null) {
        item {
            Text(if (fy.highlightsRunning || v.lastRun == null) "Looking for the latest papers…" else "No research papers in these journals since the last look.",
                Modifier.padding(8.dp), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
    items(if (showAll) v.papers else v.papers.take(10), key = { "h" + it.key }) { p -> HighlightCard(model, p) }
    if (v.papers.size > 10) {
        item { TextButton(onShowAll) { Text(if (showAll) "Fewer" else "All ${v.papers.size} papers") } }
    }
}

@Composable
private fun HighlightsHeader(fy: ForYouModel, v: HighlightsView) {
    Row(Modifier.fillMaxWidth().padding(start = 8.dp, top = 18.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text("From the journals you follow", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
            if (v.subtitle.isNotEmpty()) Text(v.subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (fy.highlightsRunning) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
        else if (v.on) IconButton({ fy.runHighlights() }) { Icon(Icons.Filled.Refresh, "Look for the latest papers now") }
    }
}

@Composable
private fun HighlightsDigest(model: AppModel, v: HighlightsView) {
    val ctx = LocalContext.current
    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer)) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(if (v.cadence == "daily") "SINCE YESTERDAY" else "THIS WEEK", Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
                if (v.digestShare.isNotEmpty()) IconButton({
                    val send = Intent(Intent.ACTION_SEND).apply { type = "text/plain"; putExtra(Intent.EXTRA_SUBJECT, "From the journals you follow"); putExtra(Intent.EXTRA_TEXT, v.digestShare) }
                    ctx.startActivity(Intent.createChooser(send, null))
                }) { Icon(Icons.Outlined.Share, "Share the summary") }
            }
            if (v.digest.isEmpty()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp); Spacer(Modifier.width(8.dp))
                    Text("Summarizing the papers…", style = MaterialTheme.typography.bodyMedium)
                }
            } else {
                val link = SpanStyle(color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Medium)
                val text = buildAnnotatedString {
                    for (d in v.digest) {
                        append(d.text.trimEnd('.', ' ')); append(" (")
                        d.cites.forEachIndexed { i, c ->
                            if (i > 0) append(", ")
                            pushStringAnnotation("key", c.key); pushStyle(link); append(c.number.toString()); pop(); pop()
                        }
                        append("). ")
                    }
                }
                @Suppress("DEPRECATION")
                ClickableText(text, style = MaterialTheme.typography.bodyMedium.copy(color = MaterialTheme.colorScheme.onSurface)) { at ->
                    text.getStringAnnotations("key", at, at).firstOrNull()?.let { model.push(Screen.ForYouFind("hl:" + it.item)) }
                }
            }
        }
    }
}

/** One paper: its number (what the summary cites), title, journal, a line from the abstract, Add / Not for Me. */
@Composable
private fun HighlightCard(model: AppModel, p: HighlightPaper) {
    val fy = model.forYou
    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp).clickable { model.push(Screen.ForYouFind("hl:" + p.key)) }) {
        Column(Modifier.padding(14.dp)) {
            Text("${p.number}. ${p.title}", style = MaterialTheme.typography.titleMedium, maxLines = 3, overflow = TextOverflow.Ellipsis)
            Text(p.byline, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (p.line.isNotBlank()) {
                Spacer(Modifier.height(6.dp))
                Text(p.line, style = MaterialTheme.typography.bodyMedium, maxLines = 4, overflow = TextOverflow.Ellipsis)
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                val id = p.inLibrary
                if (id != null) {
                    TextButton({ model.scope.launch { model.reload(); model.push(Screen.Info(id)) } }) { Icon(Icons.Filled.Check, null, Modifier.size(18.dp)); Spacer(Modifier.width(4.dp)); Text("In your library") }
                } else {
                    val busy = "hl:" + p.key in fy.adding
                    FilledTonalButton({ fy.addHighlight(p, false) }, enabled = !busy) { Icon(Icons.Filled.Add, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text(if (busy) "Adding…" else "Add") }
                }
                Spacer(Modifier.weight(1f))
                IconButton({ fy.dismissHighlight(p.key) }) { Icon(Icons.Outlined.ThumbDown, "Not for Me", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
        }
    }
}

/** A paper from the highlights: its details and abstract, and what to do with it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HighlightFindScreen(model: AppModel, key: String) {
    val fy = model.forYou
    val ctx = LocalContext.current
    val v = fy.highlights
    val p = v?.papers?.firstOrNull { it.key == key }
    if (p == null) { LaunchedEffect(v) { if (v != null) model.pop() }; return }
    Scaffold(
        topBar = { TopAppBar(title = {}, navigationIcon = { IconButton({ model.pop() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } }) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).padding(bottom = 32.dp)) {
            Text(p.title, style = MaterialTheme.typography.headlineSmall)
            if (p.authors.isNotEmpty()) {
                Spacer(Modifier.height(6.dp))
                Text(p.authors.take(12).joinToString(", ") + if (p.authors.size > 12) " et al." else "", style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(4.dp))
            Text(listOfNotNull(p.journal.ifBlank { null }, p.published ?: p.date.ifBlank { null }).joinToString(" · "),
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (p.link.isNotEmpty()) {
                Text(if (p.doi.isNotEmpty()) "DOI ${p.doi}" else p.link, Modifier.padding(top = 4.dp).clickable {
                    runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(p.link))) }
                }, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
            }
            Spacer(Modifier.height(14.dp))
            val id = p.inLibrary
            if (id != null) {
                Button({ model.scope.launch { model.reload(); model.push(Screen.Info(id)) } }) { Icon(Icons.Filled.LibraryBooks, null); Spacer(Modifier.width(8.dp)); Text("Show in Library") }
            } else {
                val busy = "hl:" + p.key in fy.adding
                Row {
                    Button({ fy.addHighlight(p, false) }, enabled = !busy) { Icon(Icons.Filled.Add, null); Spacer(Modifier.width(6.dp)); Text(if (busy) "Adding…" else "Add to Library") }
                    Spacer(Modifier.width(8.dp))
                    OutlinedButton({ fy.addHighlight(p, true) }, enabled = !busy) { Icon(Icons.Outlined.BookmarkBorder, null); Spacer(Modifier.width(6.dp)); Text("Must Read") }
                }
            }
            TextButton({ fy.dismissHighlight(p.key); model.pop() }) { Icon(Icons.Outlined.ThumbDown, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Not for Me") }
            if (p.abstractText.isNotBlank()) {
                Spacer(Modifier.height(12.dp))
                Text("Abstract", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.height(4.dp))
                Text(p.abstractText, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

/** The settings: weekly or daily, and which journals (with a toggle for each journal you follow). */
@Composable
fun JournalHighlightsSettings(fy: ForYouModel) {
    val scope = rememberCoroutineScope()
    var other by remember { mutableStateOf("") }
    var looking by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    val daily = fy.highlights?.cadence == "daily"
    Spacer(Modifier.height(16.dp))
    Text("From the journals you follow", style = MaterialTheme.typography.titleMedium)
    Row(verticalAlignment = Alignment.CenterVertically) {
        FilterChip(!daily, { fy.setHighlightCadence("weekly") }, { Text("Every Monday") })
        Spacer(Modifier.width(8.dp))
        FilterChip(daily, { fy.setHighlightCadence("daily") }, { Text("Every morning") })
    }
    for (c in fy.journalChoices) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(c.name)
                if (c.followed) Text("You follow it", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Switch(c.on, { on -> scope.launch { problem = fy.setHighlightJournal(c.name, on) } })
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        OutlinedTextField(other, { other = it }, Modifier.weight(1f), singleLine = true, placeholder = { Text("Another journal") })
        TextButton({
            looking = true; problem = null
            scope.launch { problem = fy.setHighlightJournal(other.trim(), true); looking = false; if (problem == null) other = "" }
        }, enabled = !looking && other.isNotBlank()) { Text(if (looking) "Looking…" else "Add") }
    }
    problem?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
    Text("The latest research papers in these journals, whatever their topic (no news, editorials or corrections), with a few sentences on the notable ones when the on-device model is downloaded. Each sentence cites the papers it is about, and says only what their titles and abstracts say.",
        Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
