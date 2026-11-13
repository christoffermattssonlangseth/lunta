package app.lunta.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import app.lunta.*
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.lunta_core.Paper
import uniffi.lunta_core.SortKey
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PapersScreen(model: AppModel, place: Place) {
    var query by rememberSaveable { mutableStateOf("") }
    var rows by remember { mutableStateOf<List<Row>>(emptyList()) }
    var openRow by remember { mutableStateOf<String?>(null) }
    var organizing by remember { mutableStateOf<String?>(null) }
    var sortMenu by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf(false) }

    LaunchedEffect(place, query, model.papers, model.collections, model.smartFolders) {
        if (query.isNotBlank()) delay(250)
        rows = model.rows(place, query)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(model.placeTitle(place), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = { IconButton({ model.pop() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } },
                actions = {
                    if (place != Place.ForYou) {
                        Box {
                            IconButton({ sortMenu = true }) { Icon(Icons.Filled.SwapVert, "Sort") }
                            DropdownMenu(sortMenu, { sortMenu = false }) {
                                for ((k, label) in listOf(SortKey.ADDED to "Recently added", SortKey.YEAR to "Year", SortKey.TITLE to "Title", SortKey.FIRST_AUTHOR to "First author")) {
                                    DropdownMenuItem(
                                        { Text(label) }, { model.changeSort(k); sortMenu = false },
                                        leadingIcon = { if (model.sort == k) Icon(Icons.Filled.Check, null) else Spacer(Modifier.size(24.dp)) },
                                    )
                                }
                            }
                        }
                        IconButton({ adding = true }) { Icon(Icons.Filled.Add, "Add paper") }
                    }
                },
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        if (place == Place.ForYou) {
            Box(Modifier.padding(pad).fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(Icons.Outlined.AutoAwesome, null, Modifier.size(48.dp), tint = MaterialTheme.colorScheme.tertiary)
                    Spacer(Modifier.height(12.dp))
                    Text("New for You", style = MaterialTheme.typography.titleLarge)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "A daily feed of new papers picked for your research is coming to Android. For now it lives in the iPhone and Mac apps.",
                        textAlign = TextAlign.Center, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            return@Scaffold
        }
        Column(Modifier.padding(pad).fillMaxSize()) {
            SearchField(query, { query = it })
            PullToRefreshBox(isRefreshing = model.syncing, onRefresh = { model.refreshNow() }, modifier = Modifier.weight(1f).fillMaxWidth()) {
                if (rows.isEmpty()) {
                    Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                        Text(
                            if (query.isNotBlank()) "No papers match “$query”." else if (place == Place.All) "No papers yet. Tap + to add one by DOI or link." else "Nothing here yet.",
                            color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center,
                        )
                    }
                } else {
                    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                        items(rows, key = { it.paper.id }) { r ->
                            SwipeActionRow(
                                open = openRow == r.paper.id,
                                onOpenChange = { o -> openRow = if (o) r.paper.id else if (openRow == r.paper.id) null else openRow },
                                actions = {
                                    SwipeAction(if (r.paper.starred) Icons.Filled.Star else Icons.Outlined.StarOutline, if (r.paper.starred) "Unstar" else "Star", MaterialTheme.colorScheme.tertiary) {
                                        openRow = null; model.toggleStar(r.paper)
                                    }
                                    SwipeAction(if (r.paper.mustRead) Icons.Filled.Bookmark else Icons.Outlined.BookmarkBorder, if (r.paper.mustRead) "Not Must Read" else "Must Read", MaterialTheme.colorScheme.primary) {
                                        openRow = null; model.toggleMustRead(r.paper)
                                    }
                                    SwipeAction(if (r.paper.read) Icons.Outlined.MarkEmailUnread else Icons.Outlined.MarkEmailRead, if (r.paper.read) "Unread" else "Read", MaterialTheme.colorScheme.primary) {
                                        openRow = null; model.toggleRead(r.paper)
                                    }
                                    SwipeAction(Icons.Outlined.Folder, "Folders", MaterialTheme.colorScheme.secondary) {
                                        openRow = null; organizing = r.paper.id
                                    }
                                },
                            ) {
                                PaperRow(r) { if (openRow != null) openRow = null else model.push(Screen.Info(r.paper.id)) }
                            }
                            HorizontalDivider(Modifier.padding(start = 32.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = .5f))
                        }
                    }
                }
            }
        }
    }
    organizing?.let { OrganizeSheet(model, it) { organizing = null } }
    if (adding) AddPaperDialog({ adding = false }) { model.addByInput(it) }
}

@Composable
private fun SearchField(query: String, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = query, onValueChange = onChange,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        placeholder = { Text("Search titles, authors and full text") },
        leadingIcon = { Icon(Icons.Filled.Search, null) },
        trailingIcon = { if (query.isNotEmpty()) IconButton({ onChange("") }) { Icon(Icons.Filled.Close, "Clear") } },
        singleLine = true, shape = CircleShape,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(),
    )
}

@Composable
private fun PaperRow(r: Row, onClick: () -> Unit) {
    val p = r.paper
    Row(
        Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background).clickable(onClick = onClick).padding(vertical = 10.dp, horizontal = 12.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Box(Modifier.width(16.dp).padding(top = 7.dp)) {
            if (!p.read) Box(Modifier.size(9.dp).clip(CircleShape).background(MaterialTheme.colorScheme.tertiary))
        }
        Column(Modifier.weight(1f)) {
            Text(
                p.meta.title.ifBlank { "Untitled" }, style = MaterialTheme.typography.bodyLarge,
                fontWeight = if (p.read) FontWeight.Normal else FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis,
            )
            val sub = listOf(authorsLine(p), p.meta.journal, p.meta.year()).filter { it.isNotBlank() }.joinToString(" · ")
            if (sub.isNotEmpty()) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            r.snippet?.let { Text(highlighted(it), Modifier.padding(top = 3.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis) }
        }
        Row(Modifier.padding(start = 8.dp, top = 2.dp), horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            if (p.mustRead) Icon(Icons.Filled.Bookmark, "Must Read", Modifier.size(16.dp), tint = MaterialTheme.colorScheme.primary)
            if (p.starred) Icon(Icons.Filled.Star, "Starred", Modifier.size(16.dp), tint = MaterialTheme.colorScheme.tertiary)
            if (p.annotationCount > 0u) Icon(Icons.Filled.Highlight, "Highlights", Modifier.size(16.dp), tint = HighlightColors.getValue("yellow"))
            if (p.hasPdf) Icon(Icons.Outlined.PictureAsPdf, "PDF", Modifier.size(16.dp), tint = MaterialTheme.colorScheme.outline)
        }
    }
}

private fun Paper.year() = meta.date.take(4)
private fun uniffi.lunta_core.PaperMeta.year() = date.take(4)

/** The core marks the matching words in a snippet with [ and ]. */
@Composable
private fun highlighted(s: String): AnnotatedString {
    val bg = MaterialTheme.colorScheme.tertiaryContainer
    return buildAnnotatedString {
        var inHit = false
        for (ch in s) when {
            ch == '[' && !inHit -> inHit = true
            ch == ']' && inHit -> inHit = false
            inHit -> withStyle(SpanStyle(background = bg, fontWeight = FontWeight.SemiBold)) { append(ch) }
            else -> append(ch)
        }
    }
}

// ---- swipe from the trailing edge only ----

@Composable
private fun RowScope.SwipeAction(icon: ImageVector, label: String, color: Color, onClick: () -> Unit) {
    Column(
        Modifier.width(ACTION_WIDTH).fillMaxHeight().background(color).clickable(onClick = onClick),
        verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, label, tint = Color.White)
        Text(label, style = MaterialTheme.typography.labelSmall, color = Color.White)
    }
}

private val ACTION_WIDTH = 76.dp

/** A row that reveals [actions] when dragged to the left (never to the right, unlike a dismiss). */
@Composable
private fun SwipeActionRow(open: Boolean, onOpenChange: (Boolean) -> Unit, actions: @Composable RowScope.() -> Unit, content: @Composable () -> Unit) {
    val maxPx = with(LocalDensity.current) { (ACTION_WIDTH * 3).toPx() }
    val offset = remember { Animatable(0f) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(open) { offset.animateTo(if (open) -maxPx else 0f) }
    Box(Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
        Row(Modifier.matchParentSize(), horizontalArrangement = Arrangement.End) { actions() }
        Box(
            Modifier
                .offset { IntOffset(offset.value.roundToInt(), 0) }
                .draggable(
                    orientation = Orientation.Horizontal,
                    state = rememberDraggableState { d -> scope.launch { offset.snapTo((offset.value + d).coerceIn(-maxPx, 0f)) } },
                    onDragStopped = { v ->
                        val shouldOpen = v < -900f || (v < 900f && offset.value < -maxPx / 2.5f)
                        if (shouldOpen != open) onOpenChange(shouldOpen)
                        offset.animateTo(if (shouldOpen) -maxPx else 0f)
                    },
                ),
        ) { content() }
    }
}
