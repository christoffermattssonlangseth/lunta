package app.lunta.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.lunta.*
import app.lunta.data.LibraryMode

@Composable
fun LuntaRoot(model: AppModel) {
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Box(Modifier.fillMaxSize()) {
            when {
                model.mode == LibraryMode.None -> SetupScreen(model)
                !model.opened -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                else -> AppNav(model)
            }
            if (model.opened && model.mode != LibraryMode.None) ImportDialogs(model)
            model.status?.let { s ->
                Surface(
                    Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(12.dp),
                    shape = MaterialTheme.shapes.large, tonalElevation = 6.dp, shadowElevation = 4.dp,
                ) {
                    Row(Modifier.padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(12.dp))
                        Text(s, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                    }
                }
            }
            model.error?.let { e ->
                AlertDialog(
                    onDismissRequest = { model.error = null },
                    title = { Text("Lunta") },
                    text = { Text(e) },
                    confirmButton = { TextButton({ model.error = null }) { Text("OK") } },
                )
            }
            UpdateDialogs(model.updater)
            // Listen: the paper being read aloud, wherever its reader isn't on screen
            app.lunta.listen.NowPlayingBar(Modifier.align(Alignment.BottomCenter)) { id ->
                if (model.screen != Screen.Reader(id, false)) {
                    if (model.screen is Screen.Reader) model.pop()
                    model.push(Screen.Reader(id, false))
                }
            }
            model.info?.let { e ->
                AlertDialog(
                    onDismissRequest = { model.info = null },
                    text = { Text(e) },
                    confirmButton = { TextButton({ model.info = null }) { Text("OK") } },
                )
            }
        }
    }
}

@Composable
private fun AppNav(model: AppModel) {
    BackHandler(enabled = model.stack.size > 1) { model.pop() }
    AnimatedContent(
        targetState = model.screen,
        transitionSpec = {
            val deeper = depth(targetState) >= depth(initialState)
            if (deeper) slideInHorizontally(tween(220)) { it / 4 } togetherWith slideOutHorizontally(tween(220)) { -it / 6 }
            else slideInHorizontally(tween(220)) { -it / 6 } togetherWith slideOutHorizontally(tween(220)) { it / 4 }
        },
        label = "screen",
    ) { s ->
        when (s) {
            Screen.Home -> HomeScreen(model)
            is Screen.Papers -> if (s.place == Place.ForYou) app.lunta.foryou.ForYouScreen(model) else PapersScreen(model, s.place)
            is Screen.ForYouFind -> app.lunta.foryou.ForYouFindScreen(model, s.key)
            is Screen.Info -> InfoScreen(model, s.id)
            is Screen.Reader -> ReaderScreen(model, s.id, s.pdf)
            is Screen.Journal -> JournalScreen(model, s.id)
        }
    }
}

private fun depth(s: Screen) = when (s) { Screen.Home -> 0; is Screen.Papers -> 1; is Screen.Info -> 2; is Screen.Reader -> 3; is Screen.Journal -> 4; is Screen.ForYouFind -> 2 }
