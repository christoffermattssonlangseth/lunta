package app.lunta.ui

import android.annotation.SuppressLint
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.graphics.Rect
import android.view.ActionMode
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.webkit.*
import java.io.ByteArrayInputStream
import java.io.File

const val READER_HOST = "lunta.local"

/**
 * The reading view's web view. It serves the unpacked EPUB (and the Apple apps' reader scripts, with
 * [bridge.js]) from https://lunta.local/, injects the scripts into the continuous document, and
 * receives what those scripts post to window.webkit.messageHandlers through [LuntaBridge].
 */
@SuppressLint("SetJavaScriptEnabled", "ViewConstructor")
class LuntaWebView(context: Context, private val bookDir: File, bridge: LuntaBridge) : WebView(context) {
    private val readerScripts = listOf("style.js", "tracking.js", "narration.js", "annotation.js", "position.js", "paged.js")

    init {
        settings.javaScriptEnabled = true
        settings.allowFileAccess = false
        settings.allowContentAccess = false
        settings.domStorageEnabled = false
        settings.setSupportZoom(false)
        settings.builtInZoomControls = false
        settings.mediaPlaybackRequiresUserGesture = true
        addJavascriptInterface(bridge, "LuntaNative")
        webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                if (request.url.host != READER_HOST) return notFoundIfLocal(request)
                return serve(request.url.path ?: "")
            }
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean = request.url.host != READER_HOST
        }
    }

    private fun notFoundIfLocal(r: WebResourceRequest): WebResourceResponse? = null

    // text selection is offered through the app's own highlight bar: the system's floating menu is emptied
    override fun startActionMode(callback: ActionMode.Callback?): ActionMode? = super.startActionMode(quiet(callback))
    override fun startActionMode(callback: ActionMode.Callback?, type: Int): ActionMode? = super.startActionMode(quiet(callback), type)

    private fun quiet(cb: ActionMode.Callback?): ActionMode.Callback? = when (cb) {
        null -> null
        is ActionMode.Callback2 -> object : ActionMode.Callback2() {
            override fun onCreateActionMode(m: ActionMode, menu: Menu) = cb.onCreateActionMode(m, menu).also { menu.clear() }
            override fun onPrepareActionMode(m: ActionMode, menu: Menu): Boolean { cb.onPrepareActionMode(m, menu); menu.clear(); return true }
            override fun onActionItemClicked(m: ActionMode, item: MenuItem) = false
            override fun onDestroyActionMode(m: ActionMode) = cb.onDestroyActionMode(m)
            override fun onGetContentRect(m: ActionMode, v: View, outRect: Rect) = cb.onGetContentRect(m, v, outRect)
        }
        else -> object : ActionMode.Callback {
            override fun onCreateActionMode(m: ActionMode, menu: Menu) = cb.onCreateActionMode(m, menu).also { menu.clear() }
            override fun onPrepareActionMode(m: ActionMode, menu: Menu): Boolean { cb.onPrepareActionMode(m, menu); menu.clear(); return true }
            override fun onActionItemClicked(m: ActionMode, item: MenuItem) = false
            override fun onDestroyActionMode(m: ActionMode) = cb.onDestroyActionMode(m)
        }
    }

    private fun serve(path: String): WebResourceResponse {
        val parts = path.trimStart('/').split('/')
        if (parts.firstOrNull() == "reader" && parts.size == 2 && parts[1].matches(Regex("[A-Za-z0-9._-]+"))) {
            return runCatching { response(mime(parts[1]), context.assets.open("reader/${parts[1]}").readBytes()) }.getOrElse { notFound() }
        }
        if (parts.firstOrNull() == "epub" && parts.size >= 2) {
            val rel = parts.drop(1).joinToString("/")
            val f = File(bookDir, rel).canonicalFile
            if (!f.path.startsWith(bookDir.canonicalPath + File.separator) || !f.isFile) return notFound()
            var data = f.readBytes()
            if (f.name == "all.xhtml") {
                val text = String(data, Charsets.UTF_8)
                val i = text.lastIndexOf("</body>")
                if (i >= 0) {
                    val tags = (listOf("bridge.js") + readerScripts).joinToString("\n") { "<script src=\"/reader/$it\"></script>" }
                    data = (text.substring(0, i) + tags + "\n" + text.substring(i)).toByteArray(Charsets.UTF_8)
                }
            }
            return response(mime(f.name), data)
        }
        return notFound()
    }

    private fun response(mime: String, data: ByteArray) =
        WebResourceResponse(mime, "utf-8", 200, "OK", mapOf("Cache-Control" to "no-store"), ByteArrayInputStream(data))

    private fun notFound() = WebResourceResponse("text/plain", "utf-8", 404, "Not Found", emptyMap(), ByteArrayInputStream(ByteArray(0)))

    private fun mime(name: String): String = when (name.substringAfterLast('.').lowercase()) {
        "xhtml", "xht" -> "application/xhtml+xml"
        "html", "htm" -> "text/html"
        "css" -> "text/css"
        "js" -> "text/javascript"
        "svg" -> "image/svg+xml"
        "png" -> "image/png"
        "jpg", "jpeg" -> "image/jpeg"
        "gif" -> "image/gif"
        "webp" -> "image/webp"
        "woff" -> "font/woff"
        "woff2" -> "font/woff2"
        "otf" -> "font/otf"
        "ttf" -> "font/ttf"
        "mml" -> "application/mathml+xml"
        else -> "application/octet-stream"
    }
}

/** window.webkit.messageHandlers.<name>.postMessage(body) from the page, delivered on the main thread. */
class LuntaBridge(private val onMessage: (name: String, bodyJson: String) -> Unit) {
    private val main = Handler(Looper.getMainLooper())

    @JavascriptInterface
    fun post(name: String, body: String) {
        main.post { onMessage(name, body) }
    }
}
