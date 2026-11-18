package app.lunta.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.unit.dp
import app.lunta.update.Releases
import app.lunta.update.Updater

/** "Lunta X is available", the download, the install permission and what a check found. */
@Composable
fun UpdateDialogs(u: Updater) {
    if (!u.enabled) return
    val uri = LocalUriHandler.current
    u.available?.let { r ->
        AlertDialog(
            onDismissRequest = { u.available = null },
            title = { Text("Lunta ${r.version} is available") },
            text = {
                Column(Modifier.heightIn(max = 380.dp).verticalScroll(rememberScrollState())) {
                    Text("You have ${u.current}.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (r.notes.isNotBlank()) {
                        Spacer(Modifier.height(10.dp))
                        Text(Releases.plain(r.notes), style = MaterialTheme.typography.bodyMedium)
                    }
                }
            },
            confirmButton = { TextButton({ u.download(r) }) { Text("Download") } },
            dismissButton = {
                Row {
                    TextButton({ uri.openUri(r.page) }) { Text("Release page") }
                    TextButton({ u.available = null }) { Text("Later") }
                }
            },
        )
    }
    u.progress?.let { p ->
        AlertDialog(
            onDismissRequest = {},
            title = { Text("Downloading Lunta") },
            text = {
                if (p < 0) LinearProgressIndicator(Modifier.fillMaxWidth())
                else LinearProgressIndicator(progress = { p }, modifier = Modifier.fillMaxWidth())
            },
            confirmButton = {},
        )
    }
    if (u.needsPermission) {
        AlertDialog(
            onDismissRequest = { u.needsPermission = false },
            title = { Text("Allow Lunta to install its updates") },
            text = { Text("Android asks once: turn on “Allow from this source” for Lunta, then come back and the update installs.") },
            confirmButton = { TextButton({ u.openInstallPermission() }) { Text("Open Settings") } },
            dismissButton = { TextButton({ u.needsPermission = false }) { Text("Not now") } },
        )
    }
    u.message?.let { m ->
        AlertDialog(
            onDismissRequest = { u.message = null },
            text = { Text(m) },
            confirmButton = { TextButton({ u.message = null }) { Text("OK") } },
        )
    }
}

/** The Updates part of the settings sheet. */
@Composable
fun UpdateSettings(u: Updater) {
    if (!u.enabled) return
    Spacer(Modifier.height(24.dp))
    Text("Updates", style = MaterialTheme.typography.titleLarge)
    Row(Modifier.fillMaxWidth().padding(top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("Check for updates automatically", Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(u.auto, { u.changeAuto(it) })
    }
    Text(
        "Once a day Lunta asks GitHub, where it is published, whether there is a newer version. Nothing about you or your library is sent.",
        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Spacer(Modifier.height(8.dp))
    FilledTonalButton({ u.check(manual = true) }, enabled = !u.checking && u.progress == null) {
        Text(if (u.checking) "Checking…" else "Check for updates")
    }
}
