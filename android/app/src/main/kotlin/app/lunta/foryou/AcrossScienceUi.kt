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
import uniffi.lunta_core.AcrossPaper
import uniffi.lunta_core.AcrossView

/**
 * "Across science" in New for You: each morning the standout new research paper in each broad field
 * of science, outside your own topics (the core's AcrossScience: one OpenAlex request, picked by
 * impact), with one sentence a field by the on-device model, each citing the paper it is about.
 */
fun LazyListScope.acrossScience(model: AppModel) {
    val fy = model.forYou
    val v = fy.across ?: return
    item { AcrossHeader(fy, v) }
    if (!v.on) {
        item {
            Card(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                Column(Modifier.padding(16.dp)) {
                    Text("Every morning, the standout new paper in each field of science, outside your own: from cell biology and medicine to physics, mathematics and the social sciences, picked from the top journals.",
                        style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(8.dp))
                    Button({ fy.setAcrossOn(true) }, enabled = !fy.acrossRunning) { Icon(Icons.Outlined.Public, null); Spacer(Modifier.width(6.dp)); Text("Show Across Science") }
                }
            }
        }
        return
    }
    v.problem?.let { p ->
        item {
            Card(Modifier.fillMaxWidth().padding(vertical = 6.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer)) {
                Column(Modifier.padding(16.dp)) {
                    Text("Couldn’t get today’s papers across science", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onErrorContainer)
                    Text(p, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onErrorContainer)
                    TextButton({ fy.runAcross() }) { Icon(Icons.Filled.Refresh, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Retry") }
                }
            }
        }
    }
    if (v.digest.isNotEmpty() || fy.acrossWriting) item { AcrossDigest(model, v) }
    if (v.papers.isEmpty() && v.problem == null) {
        item {
            Text(if (fy.acrossRunning || v.lastRun == null) "Looking for today’s papers…" else "No new papers in these fields today.",
                Modifier.padding(8.dp), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
    items(v.papers, key = { "a" + it.key }) { p -> AcrossCard(model, p) }
}

@Composable
private fun AcrossHeader(fy: ForYouModel, v: AcrossView) {
    Row(Modifier.fillMaxWidth().padding(start = 8.dp, top = 18.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text("Across science", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
            if (v.subtitle.isNotEmpty()) Text(v.subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (fy.acrossRunning) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
        else if (v.on) IconButton({ fy.runAcross() }) { Icon(Icons.Filled.Refresh, "Look for today’s papers") }
    }
}

@Composable
private fun AcrossDigest(model: AppModel, v: AcrossView) {
    val ctx = LocalContext.current
    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer)) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("TODAY", Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
                if (v.digestShare.isNotEmpty()) IconButton({
                    val send = Intent(Intent.ACTION_SEND).apply { type = "text/plain"; putExtra(Intent.EXTRA_SUBJECT, "Across science"); putExtra(Intent.EXTRA_TEXT, v.digestShare) }
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
                val label = SpanStyle(fontWeight = FontWeight.SemiBold)
                for (d in v.digest) {
                    val text = buildAnnotatedString {
                        pushStyle(label); append(d.fieldLabel + ": "); pop()
                        append(d.text.trimEnd('.', ' ')); append(" (")
                        d.cites.forEachIndexed { i, c ->
                            if (i > 0) append(", ")
                            pushStringAnnotation("key", c.key); pushStyle(link); append(c.number.toString()); pop(); pop()
                        }
                        append(").")
                    }
                    @Suppress("DEPRECATION")
                    ClickableText(text, Modifier.padding(vertical = 2.dp), style = MaterialTheme.typography.bodyMedium.copy(color = MaterialTheme.colorScheme.onSurface)) { at ->
                        text.getStringAnnotations("key", at, at).firstOrNull()?.let { model.push(Screen.ForYouFind("as:" + it.item)) }
                    }
                }
            }
        }
    }
}

/** One paper: its field, number (what the summary cites), title, journal, a line from the abstract, Add / Not for Me. */
@Composable
private fun AcrossCard(model: AppModel, p: AcrossPaper) {
    val fy = model.forYou
    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp).clickable { model.push(Screen.ForYouFind("as:" + p.key)) }) {
        Column(Modifier.padding(14.dp)) {
            Text(p.fieldLabel.uppercase(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
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
                    val busy = "as:" + p.key in fy.adding
                    FilledTonalButton({ fy.addAcross(p, false) }, enabled = !busy) { Icon(Icons.Filled.Add, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text(if (busy) "Adding…" else "Add") }
                }
                Spacer(Modifier.weight(1f))
                IconButton({ fy.dismissAcross(p.key) }) { Icon(Icons.Outlined.ThumbDown, "Not for Me", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
        }
    }
}

/** A paper from Across science: its details and abstract, and what to do with it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AcrossFindScreen(model: AppModel, key: String) {
    val fy = model.forYou
    val ctx = LocalContext.current
    val v = fy.across
    val p = v?.papers?.firstOrNull { it.key == key }
    if (p == null) { LaunchedEffect(v) { if (v != null) model.pop() }; return }
    Scaffold(
        topBar = { TopAppBar(title = { Text(p.fieldLabel, style = MaterialTheme.typography.titleSmall) }, navigationIcon = { IconButton({ model.pop() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } }) },
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
                val busy = "as:" + p.key in fy.adding
                Row {
                    Button({ fy.addAcross(p, false) }, enabled = !busy) { Icon(Icons.Filled.Add, null); Spacer(Modifier.width(6.dp)); Text(if (busy) "Adding…" else "Add to Library") }
                    Spacer(Modifier.width(8.dp))
                    OutlinedButton({ fy.addAcross(p, true) }, enabled = !busy) { Icon(Icons.Outlined.BookmarkBorder, null); Spacer(Modifier.width(6.dp)); Text("Must Read") }
                }
            }
            TextButton({ fy.dismissAcross(p.key); model.pop() }) { Icon(Icons.Outlined.ThumbDown, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Not for Me") }
            if (p.abstractText.isNotBlank()) {
                Spacer(Modifier.height(12.dp))
                Text("Abstract", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.height(4.dp))
                Text(p.abstractText, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

/** The settings: on or off, 1 or 2 papers a field, and which fields. */
@Composable
fun AcrossScienceSettings(fy: ForYouModel) {
    val on = fy.across?.on == true
    val perField = fy.across?.perField?.toInt() ?: 1
    Spacer(Modifier.height(16.dp))
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text("Across science", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
        Switch(on, { fy.setAcrossOn(it) })
    }
    if (on) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Papers a field", Modifier.padding(end = 8.dp))
            FilterChip(perField == 1, { fy.setAcrossPerField(1) }, { Text("1") })
            Spacer(Modifier.width(8.dp))
            FilterChip(perField == 2, { fy.setAcrossPerField(2) }, { Text("2") })
        }
        for (f in fy.acrossFields) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(f.label, Modifier.weight(1f))
                Switch(f.on, { fy.setAcrossField(f.id, it) })
            }
        }
    }
    Text("Each morning the standout new research paper in each field, from the top journals (by journal and early citations), leaving out your own topics: they are in your feed. One OpenAlex request a day. Not for Me here doesn’t change your feed.",
        Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
