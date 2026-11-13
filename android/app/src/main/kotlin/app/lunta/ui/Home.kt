package app.lunta.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.lunta.*
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.ui.platform.LocalContext
import app.lunta.data.LibraryMode
import uniffi.lunta_core.PaperCollection
import java.text.DateFormat
import java.util.Date

/** The list of places, like Mail's mailboxes: all, starred, unread, New for You, folders, smart folders, tags. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(model: AppModel) {
    var adding by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }
    var settings by remember { mutableStateOf(false) }
    var newFolder by remember { mutableStateOf<String?>(null) }   // parent id, "" = top level
    var folderMenu by remember { mutableStateOf<PaperCollection?>(null) }
    var renaming by remember { mutableStateOf<PaperCollection?>(null) }
    var deleting by remember { mutableStateOf<PaperCollection?>(null) }
    val collapsed = remember { mutableStateListOf<String>() }
    val ctx = LocalContext.current
    val pickPdf = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { u -> ctx.contentResolver.openInputStream(u)?.let { model.addPdf(it, u.lastPathSegment ?: "paper.pdf") } }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Library") },
                actions = {
                    if (model.syncing) CircularProgressIndicator(Modifier.padding(horizontal = 12.dp).size(20.dp), strokeWidth = 2.dp)
                    Box {
                        IconButton({ menu = true }) { Icon(Icons.Filled.Add, "Add paper") }
                        DropdownMenu(menu, { menu = false }) {
                            DropdownMenuItem({ Text("DOI or link…") }, { menu = false; adding = true }, leadingIcon = { Icon(Icons.Filled.Link, null) })
                            DropdownMenuItem({ Text("PDF from Files…") }, { menu = false; pickPdf.launch(arrayOf("application/pdf")) }, leadingIcon = { Icon(Icons.Filled.PictureAsPdf, null) })
                        }
                    }
                    IconButton({ settings = true }) { Icon(Icons.Filled.Settings, "Settings") }
                },
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        PullToRefreshBox(
            isRefreshing = model.syncing,
            onRefresh = { model.refreshNow() },
            modifier = Modifier.padding(pad).fillMaxSize(),
        ) {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp)) {
                item { PlaceRow(Icons.Outlined.Inbox, "All Papers", model.count(Place.All)) { model.push(Screen.Papers(Place.All)) } }
                item { PlaceRow(Icons.Outlined.StarOutline, "Starred", model.count(Place.Starred)) { model.push(Screen.Papers(Place.Starred)) } }
                item { PlaceRow(Icons.Outlined.BookmarkBorder, "Must Read", model.count(Place.MustRead)) { model.push(Screen.Papers(Place.MustRead)) } }
                item { PlaceRow(Icons.Outlined.MarkEmailUnread, "Unread", model.count(Place.Unread)) { model.push(Screen.Papers(Place.Unread)) } }
                item { PlaceRow(Icons.Outlined.AutoAwesome, "New for You", null, hint = "Soon") { model.push(Screen.Papers(Place.ForYou)) } }

                item { SectionHeader("Folders") { IconButton({ newFolder = "" }) { Icon(Icons.Filled.CreateNewFolder, "New folder") } } }
                val tree = folderTree(model.collections, collapsed.toSet())
                if (tree.isEmpty()) item { Hint("No folders yet.") }
                items(tree.size) { i ->
                    val (c, depth, hasKids) = tree[i]
                    Box {
                        PlaceRow(
                            if (hasKids && c.id in collapsed) Icons.Outlined.Folder else Icons.Outlined.FolderOpen, c.name, model.count(Place.Folder(c.id)),
                            indent = depth,
                            onLongClick = { folderMenu = c },
                            trailing = if (hasKids) ({
                                IconButton({ if (c.id in collapsed) collapsed.remove(c.id) else collapsed.add(c.id) }, Modifier.size(32.dp)) {
                                    Icon(if (c.id in collapsed) Icons.Filled.ExpandMore else Icons.Filled.ExpandLess, "Expand")
                                }
                            }) else null,
                        ) { model.push(Screen.Papers(Place.Folder(c.id))) }
                        DropdownMenu(folderMenu?.id == c.id, { folderMenu = null }) {
                            DropdownMenuItem({ Text("New subfolder…") }, { folderMenu = null; newFolder = c.id })
                            DropdownMenuItem({ Text("Rename…") }, { folderMenu = null; renaming = c })
                            DropdownMenuItem({ Text("Delete…") }, { folderMenu = null; deleting = c })
                        }
                    }
                }

                if (model.smartFolders.isNotEmpty()) {
                    item { SectionHeader("Smart Folders") {} }
                    items(model.smartFolders.size) { i ->
                        val f = model.smartFolders[i]
                        PlaceRow(Icons.Outlined.Folder, f.name, null) { model.push(Screen.Papers(Place.Smart(f.id))) }
                    }
                }
                item { SectionHeader("Tags") {} }
                if (model.tags.isEmpty()) item { Hint("Tags you add to papers show up here.") }
                items(model.tags.size) { i ->
                    val t = model.tags[i]
                    PlaceRow(Icons.Outlined.Sell, t, model.count(Place.Tag(t))) { model.push(Screen.Papers(Place.Tag(t))) }
                }
                model.syncError?.let { e ->
                    item { Text("Sync problem: $e", Modifier.padding(16.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                }
            }
        }
    }

    if (adding) AddPaperDialog({ adding = false }) { model.addByInput(it) }
    newFolder?.let { parent ->
        NameDialog("New Folder", "", "Create", { newFolder = null }) { model.newFolder(it, parent.ifEmpty { null }) }
    }
    renaming?.let { c -> NameDialog("Rename Folder", c.name, "Rename", { renaming = null }) { model.renameFolder(c.id, it) } }
    deleting?.let { c ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text("Delete “${c.name}”?") },
            text = { Text("The papers in it stay in your library.") },
            confirmButton = { TextButton({ model.deleteFolder(c.id); deleting = null }) { Text("Delete") } },
            dismissButton = { TextButton({ deleting = null }) { Text("Cancel") } },
        )
    }
    if (settings) SettingsSheet(model) { settings = false }
}

private data class FolderNode(val c: PaperCollection, val depth: Int, val hasKids: Boolean)

private fun folderTree(all: List<PaperCollection>, collapsed: Set<String>): List<FolderNode> {
    val byParent = all.groupBy { it.parent?.takeIf { p -> all.any { c -> c.id == p } } }
    val out = mutableListOf<FolderNode>()
    fun walk(parent: String?, depth: Int) {
        for (c in byParent[parent].orEmpty()) {
            val kids = byParent[c.id].orEmpty().isNotEmpty()
            out += FolderNode(c, depth, kids)
            if (kids && c.id !in collapsed) walk(c.id, depth + 1)
        }
    }
    walk(null, 0)
    return out
}

@Composable
private fun SectionHeader(text: String, action: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
        action()
    }
}

@Composable
private fun Hint(text: String) = Text(text, Modifier.padding(horizontal = 16.dp, vertical = 8.dp), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PlaceRow(
    icon: ImageVector, title: String, count: Int?, indent: Int = 0, hint: String? = null,
    onLongClick: (() -> Unit)? = null, trailing: (@Composable () -> Unit)? = null, onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .padding(start = 16.dp + 20.dp * indent, end = 8.dp).heightIn(min = 52.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.width(16.dp))
        Text(title, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (count != null) Text(count.toString(), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (hint != null) Text(hint, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.tertiary)
        trailing?.invoke() ?: Spacer(Modifier.width(8.dp))
        if (trailing == null) Icon(Icons.Filled.ChevronRight, null, tint = MaterialTheme.colorScheme.outline)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsSheet(model: AppModel, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismiss) {
        Column(Modifier.padding(horizontal = 24.dp).padding(bottom = 32.dp)) {
            Text("Library", style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            val where = when {
                model.mode == LibraryMode.Drive -> "Google Drive, folder “${model.settings.driveFolder}”"
                model.librarySync != null -> "On this phone, synced with a test folder"
                else -> "On this phone"
            }
            Text(where, style = MaterialTheme.typography.bodyLarge)
            Text("${model.papers.size} papers", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (model.librarySync != null) {
                val last = if (model.lastSyncMs == 0L) "never" else DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(model.lastSyncMs))
                Text("Last sync: $last", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                model.syncError?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                Spacer(Modifier.height(12.dp))
                FilledTonalButton({ model.refreshNow() }, enabled = !model.syncing) { Text(if (model.syncing) "Syncing…" else "Sync now") }
                Text("Lunta also syncs in the background about every ${model.settings.syncMinutes} minutes.", Modifier.padding(top = 4.dp), style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(20.dp))
            OutlinedButton({ onDismiss(); model.leaveLibrary(model.mode == LibraryMode.Drive) }) {
                Text(if (model.mode == LibraryMode.Drive) "Sign out and choose another library" else "Choose another library")
            }
            Spacer(Modifier.height(20.dp))
            Text("Lunta for Android 0.1 · Rust core, PDFium for PDFs (see licenses in the app’s assets).", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
