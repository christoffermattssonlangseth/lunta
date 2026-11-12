package app.lunta

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import app.lunta.ui.LuntaRoot
import app.lunta.ui.LuntaTheme
import androidx.core.content.IntentCompat

class MainActivity : ComponentActivity() {
    private val model get() = (application as LuntaApp).model

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        model.start()
        setContent { LuntaTheme { LuntaRoot(model) } }
        if (savedInstanceState == null) handle(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handle(intent)
    }

    override fun onResume() {
        super.onResume()
        if (model.opened) model.refreshNow()
    }

    private fun handle(i: Intent?) {
        i ?: return
        // debug builds: adb shell am start -n app.lunta/.MainActivity --es lunta.setup local [--es lunta.remoteDir DIR]
        if (BuildConfig.DEBUG && i.getStringExtra("lunta.setup") == "local" && model.mode == app.lunta.data.LibraryMode.None) {
            model.chooseLocal(i.getStringExtra("lunta.remoteDir") ?: "")
        }
        when (i.action) {
            Intent.ACTION_VIEW -> {
                val data = i.data ?: return
                if (data.scheme == BuildConfig.REDIRECT_SCHEME) {
                    model.finishGoogleSignIn(data.toString())
                } else if (data.scheme == "content" || data.scheme == "file") {
                    addPdf(data)
                } else {
                    model.handleSharedText(data.toString())
                }
            }
            Intent.ACTION_SEND -> {
                val type = i.type ?: ""
                if (type == "application/pdf") {
                    IntentCompat.getParcelableExtra(i, Intent.EXTRA_STREAM, Uri::class.java)?.let { addPdf(it) }
                } else {
                    val text = i.getStringExtra(Intent.EXTRA_TEXT) ?: i.getStringExtra(Intent.EXTRA_SUBJECT)
                    if (text != null) shared(text)
                }
            }
        }
    }

    // a share can arrive before the library is open (cold start): wait for it
    private fun shared(text: String) = whenOpen { model.handleSharedText(text) }

    private fun addPdf(uri: Uri) = whenOpen {
        val stream = contentResolver.openInputStream(uri)
        if (stream == null) model.error = "Couldn't read that PDF." else model.addPdf(stream, uri.lastPathSegment ?: "paper.pdf")
    }

    private fun whenOpen(block: () -> Unit) {
        if (model.mode == app.lunta.data.LibraryMode.None) {
            model.error = "Choose where your library lives first, then share again."
            return
        }
        model.scope.launchWhenOpen(model, block)
    }
}
