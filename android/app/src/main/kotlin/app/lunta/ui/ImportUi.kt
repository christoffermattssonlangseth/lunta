package app.lunta.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.lunta.AppModel

/**
 * Import Library: the choice (the export's folder, or a .bib / .ris / ZIP file), the pickers
 * (Storage Access Framework) and "Put the imported papers in a folder?". In LuntaRoot, so the
 * library's + menu and Settings both open it (model.imports.ask()).
 */
@Composable
fun ImportDialogs(model: AppModel) {
    val imports = model.imports
    val pickFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { imports.pickedFile(it) }
    }
    val pickFolder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        uri?.let { imports.pickedFolder(it) }
    }

    if (imports.choosing) {
        AlertDialog(
            onDismissRequest = { imports.choosing = false },
            title = { Text("Import a library") },
            text = {
                Column {
                    Text("From Zotero, Mendeley, ReadCube Papers, EndNote or JabRef: export the library (or one collection) as BibTeX or RIS.")
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "In Zotero: File › Export Library…, format BibTeX, tick “Export Files”, and put the folder it makes in Google Drive or on the phone (Downloads). Then choose that folder, so the PDFs come too. A ZIP of the folder works as well; the .bib or .ris file alone brings the papers without their PDFs.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            },
            confirmButton = {
                TextButton({ imports.choosing = false; pickFolder.launch(null) }) { Text("Choose folder") }
            },
            dismissButton = {
                Row {
                    TextButton({ imports.choosing = false }) { Text("Cancel") }
                    TextButton({ imports.choosing = false; pickFile.launch(arrayOf("*/*")) }) { Text("Choose file") }
                }
            },
        )
    }

    imports.pending?.let { source ->
        var name by remember(source) { mutableStateOf(source.suggestedFolder) }
        AlertDialog(
            onDismissRequest = { imports.cancel() },
            title = { Text("Put the imported papers in a folder?") },
            text = {
                Column {
                    Text(
                        "Exports don’t include your folders. To move a collection from Zotero or Mendeley, export it on its own and import it into a folder of the same name.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Spacer(Modifier.height(12.dp))
                    OutlinedTextField(name, { name = it }, singleLine = true, label = { Text("Folder") })
                }
            },
            confirmButton = {
                TextButton({ imports.start(name) }, enabled = name.isNotBlank()) { Text("Into folder") }
            },
            dismissButton = {
                Row {
                    TextButton({ imports.cancel() }) { Text("Cancel") }
                    TextButton({ imports.start("") }) { Text("No folder") }
                }
            },
        )
    }
}
