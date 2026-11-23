package app.lunta.foryou

import android.Manifest
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
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
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import app.lunta.AppModel
import app.lunta.Screen
import kotlinx.coroutines.launch
import uniffi.lunta_core.FindView
import java.text.DateFormat
import java.util.Calendar
import java.util.Date

/** New for You: the day's papers picked for your research (WatchInbox in the Apple apps). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ForYouScreen(model: AppModel) {
    val fy = model.forYou
    val ctx = LocalContext.current
    var settingsOpen by remember { mutableStateOf(false) }
    var earlierOpen by remember { mutableStateOf(false) }
    val askNotifications = notificationAsker(ctx)
    LaunchedEffect(Unit) {
        fy.reload()
        fy.markOpened()
        fy.summarizeIfNeeded()
        if (fy.feed?.hasSetup == true) askNotifications(false)
    }
    DisposableEffect(Unit) { onDispose { fy.markOpened() } }
    val feed = fy.feed

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("New for You") },
                navigationIcon = { IconButton({ model.pop() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } },
                actions = {
                    if (fy.running) {
                        IconButton({ fy.stop() }) { CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp) }
                    } else if (feed?.hasSetup == true) {
                        IconButton({ fy.runNow() }) { Icon(Icons.Filled.Refresh, "Look for new papers now") }
                    }
                    IconButton({ settingsOpen = true }) { Icon(Icons.Filled.Tune, "Your interests and what you follow") }
                },
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp)) {
            if (feed == null) {
                item { Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() } }
                return@LazyColumn
            }
            if (!feed.hasInterests) item { Intro { settingsOpen = true } } else item { Status(fy, feed.lastRun, feed.nextMorning, feed.interests) }
            val problem = feed.problem
            if (problem != null && !fy.running) item { Problem(problem, feed.fetchFailed, { fy.runNow() }) { settingsOpen = true } }
            if (feed.hasInterests && feed.latest.isNotEmpty() && (feed.digest.isNotEmpty() || fy.summarizing)) {
                item { Digest(model, feed.digest, feed.digestShare, fy.summarizing) }
            }
            if (feed.latest.isNotEmpty()) {
                items(feed.latest, key = { "l" + it.key }) { f -> FindCard(model, f) }
            } else if (feed.hasSetup && !fy.running && feed.problem == null) {
                item {
                    Text(
                        if (feed.earlier.isEmpty()) "Nothing yet. New papers are picked once a day; refresh to look now."
                        else "Nothing new today. New papers are picked once a day; refresh to look now.",
                        Modifier.padding(8.dp), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (feed.earlier.isNotEmpty()) {
                item {
                    Row(Modifier.fillMaxWidth().clickable { earlierOpen = !earlierOpen }.padding(horizontal = 8.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text("Earlier (${feed.earlier.size})", Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
                        Icon(if (earlierOpen) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore, null)
                    }
                }
                if (earlierOpen) items(feed.earlier, key = { "e" + it.key }) { f -> FindCard(model, f) }
            }
        }
    }
    if (settingsOpen) ForYouSettingsSheet(model, { settingsOpen = false }) { askNotifications(true) }
}

/** Asks for permission to post notifications (Android 13+): once on its own, again from Save. */
@Composable
private fun notificationAsker(ctx: Context): (Boolean) -> Unit {
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    return remember(launcher) {
        { fromSave: Boolean ->
            if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                val prefs = ctx.getSharedPreferences("lunta-foryou", Context.MODE_PRIVATE)
                val asked = prefs.getInt("notificationAsks", 0)
                // the system stops showing the prompt after two refusals anyway
                if (asked == 0 || (fromSave && asked < 2)) {
                    prefs.edit { putInt("notificationAsks", asked + 1) }
                    launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
        }
    }
}

/** Fetching failed (or partly): said, with Retry, instead of "Nothing new today". */
@Composable
private fun Problem(text: String, failed: Boolean, onRetry: () -> Unit, onSettings: () -> Unit) {
    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer)) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.WarningAmber, null, tint = MaterialTheme.colorScheme.onErrorContainer)
                Spacer(Modifier.width(8.dp))
                Text(if (failed) "Couldn’t reach OpenAlex" else "Some sources couldn’t be reached", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onErrorContainer)
            }
            Spacer(Modifier.height(6.dp))
            Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onErrorContainer)
            Row {
                TextButton(onRetry) { Icon(Icons.Filled.Refresh, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Retry") }
                if (text.contains("API key")) TextButton(onSettings) { Text("Add an API Key…") }
            }
        }
    }
}

@Composable
private fun Intro(onDescribe: () -> Unit) {
    Card(Modifier.fillMaxWidth().padding(vertical = 8.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer)) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.AutoAwesome, null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(8.dp))
                Text("Papers picked for your research", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "Describe what you work on in a few sentences. Every day Lunta searches OpenAlex, bioRxiv and medRxiv, ranks what's new against your interests and your library, and (with on-device AI) has the model read the best abstracts and say why each one fits you.",
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(12.dp))
            Button(onDescribe) { Icon(Icons.Filled.Edit, null); Spacer(Modifier.width(8.dp)); Text("Describe Your Research…") }
        }
    }
}

@Composable
private fun Status(fy: ForYouModel, lastRun: Long?, next: Long, interests: String) {
    val time = DateFormat.getTimeInstance(DateFormat.SHORT)
    val line = fy.stage ?: when {
        fy.running -> "Looking for new papers…"
        lastRun != null -> "Picked ${android.text.format.DateUtils.getRelativeTimeSpanString(lastRun, System.currentTimeMillis(), 60_000)} · next ${time.format(Date(next))}"
        else -> "Getting your first papers…"
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp)) {
        Text(line, style = MaterialTheme.typography.bodyMedium)
        Text(interests, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (fy.summarizing) Text("Summarizing the papers on this phone…", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary)
    }
}

/** "In short": the day's papers in a few sentences, each mention a link to the paper. */
@Composable
private fun Digest(model: AppModel, parts: List<uniffi.lunta_core.DigestPart>, share: String, summarizing: Boolean) {
    val ctx = LocalContext.current
    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("IN SHORT", Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
                if (share.isNotEmpty()) IconButton({ shareText(ctx, "New for You", share) }, Modifier.size(32.dp)) { Icon(Icons.Filled.Share, "Share the summary and its papers", Modifier.size(18.dp)) }
            }
            if (parts.isEmpty()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (summarizing) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(6.dp))
                    Text("Summarizing the papers…", style = MaterialTheme.typography.bodyMedium)
                }
            } else {
                val link = SpanStyle(color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Medium, textDecoration = TextDecoration.None)
                val text = buildAnnotatedString {
                    for (p in parts) {
                        val k = p.key
                        if (k == null) append(p.text) else {
                            pushStringAnnotation("key", k)
                            pushStyle(link); append(p.text); pop()
                            pop()
                        }
                    }
                }
                @Suppress("DEPRECATION")
                ClickableText(text, style = MaterialTheme.typography.bodyMedium.copy(color = MaterialTheme.colorScheme.onSurface)) { at ->
                    text.getStringAnnotations("key", at, at).firstOrNull()?.let { model.push(Screen.ForYouFind(it.item)) }
                }
            }
        }
    }
}

/** One paper: what it is, why it was picked, its summary (or the abstract's start), and actions. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun FindCard(model: AppModel, f: FindView) {
    val fy = model.forYou
    val ctx = LocalContext.current
    var addMenu by remember { mutableStateOf(false) }
    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp).clickable { model.push(Screen.ForYouFind(f.key)) }) {
        Column(Modifier.padding(14.dp)) {
            Text(f.title, style = MaterialTheme.typography.titleMedium, maxLines = 3, overflow = TextOverflow.Ellipsis)
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (f.openAccess) { Icon(Icons.Outlined.LockOpen, "Open access", Modifier.size(14.dp), tint = MaterialTheme.colorScheme.tertiary); Spacer(Modifier.width(4.dp)) }
                Text(f.byline, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            f.published?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            if (f.preview.isNotBlank()) {
                Spacer(Modifier.height(6.dp))
                Text(
                    f.preview, maxLines = if (f.summary != null) 6 else 4, overflow = TextOverflow.Ellipsis,
                    style = if (f.summary != null) MaterialTheme.typography.bodyMedium else MaterialTheme.typography.bodySmall,
                    color = if (f.summary != null) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(6.dp))
            Row {
                Icon(Icons.Outlined.AutoAwesome, null, Modifier.size(16.dp).padding(top = 2.dp), tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(6.dp))
                Text(f.reason, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary, maxLines = 3, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(4.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                val id = f.inLibrary
                if (id != null) {
                    TextButton({ openInLibrary(model, id) }) { Icon(Icons.Filled.Check, null, Modifier.size(18.dp)); Spacer(Modifier.width(4.dp)); Text("In your library") }
                } else {
                    Box {
                        val busy = f.key in fy.adding
                        Surface(
                            shape = MaterialTheme.shapes.extraLarge, color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.combinedClickable(enabled = !busy, onClick = { fy.add(f, false) }, onLongClick = { addMenu = true }),
                        ) {
                            Row(Modifier.padding(horizontal = 14.dp, vertical = 7.dp), verticalAlignment = Alignment.CenterVertically) {
                                if (busy) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
                                else Icon(Icons.Filled.Add, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onPrimary)
                                Spacer(Modifier.width(4.dp))
                                Text(if (busy) "Adding…" else "Add", color = MaterialTheme.colorScheme.onPrimary, style = MaterialTheme.typography.labelLarge)
                            }
                        }
                        DropdownMenu(addMenu, { addMenu = false }) {
                            DropdownMenuItem({ Text("Add as Must Read") }, { addMenu = false; fy.add(f, true) }, leadingIcon = { Icon(Icons.Outlined.BookmarkBorder, null) })
                        }
                    }
                }
                Spacer(Modifier.weight(1f))
                IconButton({ shareText(ctx, f.title, f.title + (if (f.link.isNotEmpty()) "\n" + f.link else "")) }) { Icon(Icons.Outlined.Share, "Share", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
                IconButton({ fy.boost(f.key) }) {
                    Icon(if (f.boosted) Icons.Filled.ThumbUp else Icons.Outlined.ThumbUp, if (f.boosted) "Undo More Like This" else "More Like This",
                        tint = if (f.boosted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
                IconButton({ fy.dismiss(f.key) }) { Icon(Icons.Outlined.ThumbDown, "Not for Me", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
        }
    }
}

private fun openInLibrary(model: AppModel, id: String) {
    model.scope.launch { model.reload(); model.push(Screen.Info(id)) }
}

private fun shareText(ctx: Context, subject: String, text: String) {
    val send = Intent(Intent.ACTION_SEND).apply { type = "text/plain"; putExtra(Intent.EXTRA_SUBJECT, subject); putExtra(Intent.EXTRA_TEXT, text) }
    ctx.startActivity(Intent.createChooser(send, null))
}

/** A paper from the feed: its details and abstract, why it was picked, and what to do with it. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun ForYouFindScreen(model: AppModel, key: String) {
    val fy = model.forYou
    val ctx = LocalContext.current
    val feed = fy.feed
    val f = feed?.let { (it.latest + it.earlier).firstOrNull { x -> x.key == key } }
    if (f == null) { LaunchedEffect(feed) { if (feed != null) model.pop() }; return }
    Scaffold(
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = { IconButton({ model.pop() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } },
                actions = {
                    IconButton({ shareText(ctx, f.title, f.title + (if (f.link.isNotEmpty()) "\n" + f.link else "")) }) { Icon(Icons.Filled.Share, "Share") }
                },
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).padding(bottom = 32.dp)) {
            Text(f.title, style = MaterialTheme.typography.headlineSmall)
            if (f.authors.isNotEmpty()) {
                Spacer(Modifier.height(6.dp))
                Text(f.authors.take(12).joinToString(", ") + if (f.authors.size > 12) " et al." else "", style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(4.dp))
            Text(listOfNotNull(f.journal.ifBlank { null }, f.published ?: f.date.ifBlank { null }, if (f.openAccess) "Open access" else null).joinToString(" · "),
                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (f.link.isNotEmpty()) {
                Text(if (f.doi.isNotEmpty()) "DOI ${f.doi}" else f.link, Modifier.padding(top = 4.dp).clickable {
                    runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(f.link))) }
                }, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
            }
            Spacer(Modifier.height(14.dp))
            Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer)) {
                Row(Modifier.padding(12.dp)) {
                    Icon(Icons.Outlined.AutoAwesome, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(8.dp))
                    Column {
                        Text(f.reason, style = MaterialTheme.typography.bodyMedium)
                        f.summary?.let { Spacer(Modifier.height(6.dp)); Text(it, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium) }
                    }
                }
            }
            Spacer(Modifier.height(14.dp))
            val id = f.inLibrary
            if (id != null) {
                Button({ openInLibrary(model, id) }) { Icon(Icons.Filled.LibraryBooks, null); Spacer(Modifier.width(8.dp)); Text("Show in Library") }
            } else {
                val busy = f.key in fy.adding
                Row {
                    Button({ fy.add(f, false) }, enabled = !busy) { Icon(Icons.Filled.Add, null); Spacer(Modifier.width(6.dp)); Text(if (busy) "Adding…" else "Add to Library") }
                    Spacer(Modifier.width(8.dp))
                    OutlinedButton({ fy.add(f, true) }, enabled = !busy) { Icon(Icons.Outlined.BookmarkBorder, null); Spacer(Modifier.width(6.dp)); Text("Must Read") }
                }
            }
            Row(Modifier.padding(top = 6.dp)) {
                TextButton({ fy.boost(f.key) }) {
                    Icon(if (f.boosted) Icons.Filled.ThumbUp else Icons.Outlined.ThumbUp, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp))
                    Text(if (f.boosted) "Undo More Like This" else "More Like This")
                }
                TextButton({ fy.dismiss(f.key); model.pop() }) { Icon(Icons.Outlined.ThumbDown, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Not for Me") }
            }
            if (f.abstractText.isNotBlank()) {
                Spacer(Modifier.height(12.dp))
                Text("Abstract", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.height(4.dp))
                Text(f.abstractText, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

// ---- settings ----

private val PREPRINTS = listOf("any" to "Journals and preprints", "few" to "Mostly journals", "none" to "Journals only")

/** Your interests, how many papers a day and when, your ORCID iD, and what you follow (ForYouSettings). */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun ForYouSettingsSheet(model: AppModel, onDismiss: () -> Unit, onSaved: () -> Unit) {
    val fy = model.forYou
    val s = fy.settings ?: run { LaunchedEffect(Unit) { fy.reload() }; return }
    val scope = rememberCoroutineScope()
    var text by remember { mutableStateOf(s.interests) }
    var perDay by remember { mutableIntStateOf(s.perDay.toInt().coerceIn(3, 20)) }
    var morning by remember { mutableIntStateOf(s.morning.toInt()) }
    var preprints by remember { mutableStateOf(s.preprints) }
    var drafting by remember { mutableStateOf(false) }
    var orcidText by remember { mutableStateOf("") }
    var connecting by remember { mutableStateOf(false) }
    var orcidProblem by remember { mutableStateOf<String?>(null) }
    var connected by remember { mutableStateOf(false) }
    var pickingTime by remember { mutableStateOf(false) }
    var following by remember { mutableStateOf(false) }
    var preprintMenu by remember { mutableStateOf(false) }
    var openalexKey by remember { mutableStateOf(s.openalexKey) }
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    ModalBottomSheet(onDismiss, sheetState = sheet) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 24.dp).padding(bottom = 32.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("New for You", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
                TextButton(onDismiss) { Text("Cancel") }
                Button({
                    // a key after a failed run: look again with it
                    val newKey = openalexKey.trim() != s.openalexKey
                    fy.save(text, perDay, morning, preprints, connected || (newKey && openalexKey.isNotBlank() && fy.feed?.fetchFailed == true),
                            openalexKey = if (newKey) openalexKey else null)
                    onSaved()
                    onDismiss()
                }) { Text("Save") }
            }
            Spacer(Modifier.height(12.dp))
            Text("Your research", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(text, { text = it }, Modifier.fillMaxWidth().heightIn(min = 140.dp), placeholder = { Text("A few sentences on your topics, methods and systems") })
            if (fy.canDraft) {
                TextButton({
                    drafting = true
                    scope.launch {
                        val d = runCatching { fy.draftInterests() }.getOrNull()
                        if (!d.isNullOrBlank()) text = d
                        drafting = false
                    }
                }, enabled = !drafting) {
                    if (drafting) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp) else Icon(Icons.Filled.AutoFixHigh, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(if (drafting) (if (s.orcid == null) "Reading your library…" else "Reading your papers…")
                         else (if (s.orcid == null) "Draft from My Library" else "Draft from My Papers and Library"))
                }
            }
            Hint("Lunta turns these sentences into searches and uses them to rank and screen new papers.")

            Spacer(Modifier.height(16.dp))
            Text("Your ORCID iD", style = MaterialTheme.typography.titleMedium)
            val me = s.orcid
            if (me != null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Connected: ${me.name}, ${me.works} paper${if (me.works == 1u) "" else "s"}", Modifier.weight(1f))
                    TextButton({ fy.disconnectOrcid() }) { Text("Disconnect", color = MaterialTheme.colorScheme.error) }
                }
                Hint("Papers citing yours always show up. Your papers rank like your library and are never offered as new.")
            } else {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(orcidText, { orcidText = it }, Modifier.weight(1f), singleLine = true, placeholder = { Text("0000-0000-0000-0000") })
                    TextButton({
                        connecting = true; orcidProblem = null
                        scope.launch {
                            orcidProblem = fy.connectOrcid(orcidText)
                            connecting = false
                            if (orcidProblem == null) { orcidText = ""; connected = true }
                        }
                    }, enabled = !connecting && orcidText.isNotBlank()) { Text(if (connecting) "Connecting…" else "Connect") }
                }
                orcidProblem?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                Hint("Optional. Lunta finds your papers on OpenAlex: papers citing them always show up, and your own never do.")
            }
            if (me != null && me.coauthors.isNotEmpty()) {
                Spacer(Modifier.height(12.dp))
                Text("People you publish with", style = MaterialTheme.typography.titleMedium)
                for (c in me.coauthors) {
                    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(c.name)
                            Text("${c.shared} paper${if (c.shared == 1u) "" else "s"} together", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        if (c.following) Text("Following", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        else TextButton({ fy.follow(c.author) }) { Text("Follow") }
                    }
                }
                Hint("Follow someone to get their new papers.")
            }

            Spacer(Modifier.height(16.dp))
            Text("Every day", style = MaterialTheme.typography.titleMedium)
            Row(Modifier.fillMaxWidth().clickable { pickingTime = true }.padding(vertical = 10.dp)) {
                Text("Every morning at", Modifier.weight(1f))
                Text(clock(morning), color = MaterialTheme.colorScheme.primary)
            }
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("Up to $perDay papers a day", Modifier.weight(1f))
                IconButton({ perDay = (perDay - 1).coerceAtLeast(3) }, enabled = perDay > 3) { Icon(Icons.Filled.Remove, "Fewer") }
                IconButton({ perDay = (perDay + 1).coerceAtMost(20) }, enabled = perDay < 20) { Icon(Icons.Filled.Add, "More") }
            }
            Box {
                Row(Modifier.fillMaxWidth().clickable { preprintMenu = true }.padding(vertical = 10.dp)) {
                    Text("Papers from", Modifier.weight(1f))
                    Text(PREPRINTS.firstOrNull { it.first == preprints }?.second ?: PREPRINTS[0].second, color = MaterialTheme.colorScheme.primary)
                }
                DropdownMenu(preprintMenu, { preprintMenu = false }) {
                    for ((k, label) in PREPRINTS) DropdownMenuItem({ Text(label) }, { preprints = k; preprintMenu = false })
                }
            }
            Hint("The day’s papers are picked at this time on a computer (or here, or as soon as Lunta runs after it), and this phone gets a notification. “Mostly journals” allows about one preprint in four.")

            Spacer(Modifier.height(16.dp))
            Text("Also follow", style = MaterialTheme.typography.titleMedium)
            for (w in s.watches) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Icon(when (w.kind) { "author" -> Icons.Outlined.Person; "citing" -> Icons.Outlined.FormatQuote; "journal" -> Icons.Outlined.Newspaper; else -> Icons.Outlined.Search }, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(w.describe, Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    IconButton({ fy.removeWatch(w.id) }) { Icon(Icons.Outlined.Delete, "Remove") }
                }
            }
            TextButton({ following = true }) { Icon(Icons.Filled.Add, null); Spacer(Modifier.width(6.dp)); Text("Follow a Journal, Author, Paper or Topic…") }
            Hint("New papers by authors you follow, and papers citing ones you watch, always show up. Papers in journals you follow come first and pass at a lower bar.")

            Spacer(Modifier.height(16.dp))
            Text("OpenAlex", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(openalexKey, { openalexKey = it }, Modifier.fillMaxWidth(), singleLine = true, placeholder = { Text("API key (optional)") })
            val ctx = LocalContext.current
            TextButton({ ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://openalex.org/settings/api"))) }) { Text("Get a Free Key") }
            Hint("New for You searches OpenAlex. Without a key, OpenAlex gives everyone on your network one small daily allowance, which a university or mobile network can use up by morning. A free key gives Lunta its own.")
        }
    }

    if (pickingTime) {
        val state = rememberTimePickerState(morning / 60, morning % 60, android.text.format.DateFormat.is24HourFormat(LocalContext.current))
        AlertDialog(
            onDismissRequest = { pickingTime = false },
            title = { Text("Every morning at") },
            text = { TimePicker(state) },
            confirmButton = { TextButton({ morning = state.hour * 60 + state.minute; pickingTime = false }) { Text("OK") } },
            dismissButton = { TextButton({ pickingTime = false }) { Text("Cancel") } },
        )
    }
    if (following) FollowDialog(fy) { following = false }
}

private fun clock(minutes: Int): String {
    val c = Calendar.getInstance().apply { set(Calendar.HOUR_OF_DAY, minutes / 60); set(Calendar.MINUTE, minutes % 60) }
    return DateFormat.getTimeInstance(DateFormat.SHORT).format(c.time)
}

@Composable
private fun Hint(text: String) = Text(text, Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)

/** Follow a journal, author, paper (citing it) or topic (NewWatchView). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun FollowDialog(fy: ForYouModel, onDismiss: () -> Unit) {
    var kind by remember { mutableStateOf("journal") }
    var value by remember { mutableStateOf("") }
    var looking by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val kinds = listOf("journal" to "Journal", "author" to "Author", "citing" to "Citing…", "topic" to "Topic")
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Follow") },
        text = {
            Column {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for ((k, label) in kinds) FilterChip(kind == k, { kind = k; problem = null }, { Text(label) })
                }
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(value, { value = it }, Modifier.fillMaxWidth(), singleLine = true, placeholder = {
                    Text(when (kind) {
                        "journal" -> "Journal, e.g. Nature Neuroscience"
                        "topic" -> "Words, e.g. velocity-based training"
                        "author" -> "Author's name or ORCID iD"
                        else -> "DOI of the paper"
                    })
                })
                problem?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            TextButton({
                looking = true; problem = null
                scope.launch {
                    problem = fy.addWatch(kind, value)
                    looking = false
                    if (problem == null) onDismiss()
                }
            }, enabled = value.isNotBlank() && !looking) { Text(if (looking) "Looking…" else "Follow") }
        },
        dismissButton = { TextButton(onDismiss) { Text("Cancel") } },
    )
}
