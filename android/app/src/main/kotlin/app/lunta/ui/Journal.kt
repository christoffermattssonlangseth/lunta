package app.lunta.ui

import android.annotation.SuppressLint
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.Base64
import android.view.ViewGroup
import android.webkit.*
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Article
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.outlined.OpenInBrowser
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import app.lunta.*
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import uniffi.lunta_core.*
import java.net.HttpURLConnection
import java.net.URL
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

// Limits for the figures sent with a page (the Chrome extension's, lower for a phone's memory):
// figures beyond them are left out and counted.
private const val MAX_IMAGES = 60
private const val MAX_IMAGE_BYTES = 10 * 1024 * 1024
private const val MAX_TOTAL_IMAGE_BYTES = 40 * 1024 * 1024
private const val MAX_TABLE_PAGES = 30

/** What the reader is told after "Use this page's full text". */
private data class PageResult(val title: String, val text: String, val read: Boolean)

/**
 * The paper's page on the journal's site (from its DOI), in a web view whose cookies persist, so a
 * sign-in or the institution's access sticks. "Use this page's full text" makes the paper's reading
 * view from the page as it is shown, with the figures it shows: the Chrome extension's button.
 */
@SuppressLint("SetJavaScriptEnabled")
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JournalScreen(model: AppModel, id: String) {
    val p = model.paper(id)
    val start = p?.let { model.journalUrl(it) }
    if (p == null || start == null) { LaunchedEffect(Unit) { model.pop() }; return }
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var web by remember { mutableStateOf<WebView?>(null) }
    var loading by remember { mutableIntStateOf(0) }
    var pageTitle by remember { mutableStateOf("") }
    var pageUrl by remember { mutableStateOf(start) }
    var canGoBack by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf<String?>(null) }
    // the core is making the reading view: it can't be stopped half-way
    var converting by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<PageResult?>(null) }
    var menu by remember { mutableStateOf(false) }
    val collector = remember { PageCollector() }
    var job by remember { mutableStateOf<Job?>(null) }

    fun close() { job?.cancel(); model.pop() }
    BackHandler {
        val w = web
        when {
            busy != null -> if (!converting) job?.cancel()
            w != null && w.canGoBack() -> w.goBack()
            else -> close()
        }
    }

    fun openReader() {
        result = null
        model.pop()
        // the reader beneath opens the new reading view again; from the info page, open it
        val under = model.screen
        if (!(under is Screen.Reader && under.id == id && !under.pdf)) model.push(Screen.Reader(id, false))
    }

    fun useFullText() {
        val w = web ?: return
        if (busy != null) return
        job = scope.launch {
            busy = "Reading the page…"
            try {
                result = collectAndConvert(model, ctx, w, collector, id, { converting = true }) { busy = it }
            } catch (e: CancellationException) {
                result = null
            } catch (e: Exception) {
                result = PageResult("Reading view not changed", e.message ?: e.toString(), false)
            } finally {
                busy = null
                converting = false
                job = null
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(pageTitle.ifBlank { p.meta.title }, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(Uri.parse(pageUrl).host ?: "", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                    }
                },
                navigationIcon = { IconButton({ close() }) { Icon(Icons.Filled.Close, "Close") } },
                actions = {
                    Box {
                        IconButton({ menu = true }) { Icon(Icons.Filled.MoreVert, "More") }
                        DropdownMenu(menu, { menu = false }) {
                            DropdownMenuItem({ Text("Reload") }, { menu = false; web?.reload() }, leadingIcon = { Icon(Icons.Outlined.Refresh, null) })
                            DropdownMenuItem({ Text("Back to the paper's page") }, { menu = false; web?.loadUrl(start) }, leadingIcon = { Icon(Icons.Outlined.Home, null) })
                            DropdownMenuItem({ Text("Open in browser") }, {
                                menu = false
                                runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(pageUrl))) }
                            }, leadingIcon = { Icon(Icons.Outlined.OpenInBrowser, null) })
                        }
                    }
                },
            )
        },
        bottomBar = {
            Surface(tonalElevation = 3.dp, shadowElevation = 3.dp) {
                Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp, vertical = 10.dp)) {
                    Text(
                        busy ?: "Sign in or use your institution's access if the page only shows the abstract.",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2,
                    )
                    Spacer(Modifier.height(8.dp))
                    if (busy != null) {
                        OutlinedButton({ job?.cancel() }, Modifier.fillMaxWidth(), enabled = !converting) { Text("Cancel") }
                    } else {
                        Button({ useFullText() }, Modifier.fillMaxWidth(), enabled = web != null) {
                            Icon(Icons.AutoMirrored.Outlined.Article, null); Spacer(Modifier.width(8.dp)); Text("Use this page's full text")
                        }
                    }
                }
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        Box(Modifier.padding(pad).fillMaxSize()) {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { c ->
                    WebView(c).apply {
                        layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                        settings.javaScriptEnabled = true
                        settings.domStorageEnabled = true
                        settings.loadWithOverviewMode = true
                        settings.useWideViewPort = true
                        settings.builtInZoomControls = true
                        settings.displayZoomControls = false
                        settings.allowFileAccess = false
                        settings.allowContentAccess = false
                        settings.setSupportMultipleWindows(false)
                        // sign-in pages (some institutions' single sign-on, Google) refuse embedded web views by their "wv" mark
                        settings.userAgentString = settings.userAgentString.replace("; wv)", ")")
                        CookieManager.getInstance().setAcceptCookie(true)
                        CookieManager.getInstance().setAcceptThirdPartyCookies(this, true)
                        addJavascriptInterface(collector, "LuntaPage")
                        webViewClient = object : WebViewClient() {
                            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                                val scheme = request.url.scheme?.lowercase()
                                if (scheme == "http" || scheme == "https") return false
                                if (scheme == "mailto" && request.hasGesture()) runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, request.url)) }
                                return true
                            }
                            override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
                                canGoBack = view.canGoBack()
                                url?.let { pageUrl = it }
                            }
                            override fun onPageFinished(view: WebView, url: String?) {
                                CookieManager.getInstance().flush()
                            }
                        }
                        webChromeClient = object : WebChromeClient() {
                            override fun onProgressChanged(view: WebView, newProgress: Int) { loading = newProgress }
                            override fun onReceivedTitle(view: WebView, title: String?) { pageTitle = title ?: "" }
                        }
                        // PDF and other downloads: the system's browser
                        setDownloadListener { url, _, _, _, _ -> runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url))) } }
                        web = this
                        loadUrl(start)
                    }
                },
                onRelease = {
                    CookieManager.getInstance().flush()
                    it.destroy()
                    if (web === it) web = null
                },
            )
            if (loading in 1..99) LinearProgressIndicator({ loading / 100f }, Modifier.fillMaxWidth().align(Alignment.TopCenter))
            if (busy != null) {
                // the page stays as it was read while Lunta works
                Box(
                    Modifier.fillMaxSize().background(MaterialTheme.colorScheme.scrim.copy(alpha = .25f))
                        .clickable(remember { MutableInteractionSource() }, null) {},
                    contentAlignment = Alignment.Center,
                ) {
                    Surface(shape = MaterialTheme.shapes.large, tonalElevation = 6.dp, shadowElevation = 4.dp) {
                        Row(Modifier.padding(horizontal = 20.dp, vertical = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                            Spacer(Modifier.width(14.dp))
                            Text(busy ?: "", style = MaterialTheme.typography.bodyMedium, modifier = Modifier.widthIn(max = 260.dp))
                        }
                    }
                }
            }
        }
    }

    result?.let { r ->
        AlertDialog(
            onDismissRequest = { result = null },
            title = { Text(r.title) },
            text = { Text(r.text) },
            confirmButton = {
                if (r.read) TextButton({ openReader() }) { Text("Read it") } else TextButton({ result = null }) { Text("OK") }
            },
            dismissButton = { if (r.read) TextButton({ result = null }) { Text("Stay here") } },
        )
    }
}

/**
 * Collects the page (its HTML as delivered and as shown, figure images, linked table pages) with
 * page.js, downloads the figures with the page's cookies, and hands it all to the core.
 */
private suspend fun collectAndConvert(
    model: AppModel, ctx: android.content.Context, w: WebView, collector: PageCollector, id: String,
    converting: () -> Unit, say: (String) -> Unit,
): PageResult {
    val token = collector.begin(say)
    try {
        val script = withContext(Dispatchers.IO) { ctx.assets.open("journal/page.js").bufferedReader().use { it.readText() } }
        w.evaluateJavascript(script.replace("__TOKEN__", token), null)
        val json = withTimeoutOrNull(120_000) { collector.page() } ?: throw Exception("The page didn't answer. Wait until it has loaded, then try again.")
        val page = withContext(Dispatchers.Default) { JSONObject(json) }
        val pageUrl = page.optString("url")
        val ua = w.settings.userAgentString

        // figures: downloaded here with the page's cookies, else fetched by the page itself
        val list = page.optJSONArray("images") ?: JSONArray()
        val images = mutableListOf<PageImage>()
        var skipped = (list.length() - MAX_IMAGES).coerceAtLeast(0)
        var failed = 0
        var total = 0L
        val n = minOf(list.length(), MAX_IMAGES)
        for (i in 0 until n) {
            say("Fetching figure ${i + 1} of $n…")
            val o = list.getJSONObject(i)
            val url = o.optString("fetch")
            var got = withContext(Dispatchers.IO) { download(url, ua, pageUrl) }
            if (got is Got.Failed && w.url == pageUrl) got = fromPage(w, collector, token, i, url)
            when (got) {
                is Got.Failed -> failed++
                is Got.TooLarge -> skipped++
                is Got.Bytes -> {
                    if (total + got.data.size > MAX_TOTAL_IMAGE_BYTES) { skipped++; continue }
                    total += got.data.size
                    images += PageImage(strings(o.optJSONArray("urls")).ifEmpty { listOf(url) }, got.data)
                }
            }
        }
        // table pages a Springer Nature article only links to, as the page fetched them
        val pages = page.optJSONArray("pages") ?: JSONArray()
        for (i in 0 until minOf(pages.length(), MAX_TABLE_PAGES)) {
            val o = pages.getJSONObject(i)
            images += PageImage(strings(o.optJSONArray("urls")), o.optString("html").toByteArray(Charsets.UTF_8))
        }

        say("Making the reading view…")
        converting()
        val sub = PageSubmission(
            paperId = id,
            html = page.optString("html"),
            liveHtml = page.optString("liveHtml").takeIf { !page.isNull("liveHtml") && it.isNotEmpty() },
            url = pageUrl,
            doi = page.optString("doi"),
            title = page.optString("title"),
            images = images,
            imagesSkipped = skipped.toUInt(),
        )
        val r = model.usePageFullText(sub, say)
        return describe(r, failed)
    } finally {
        collector.end()
    }
}

/** The outcome in plain words (the core's own message names the backup folder, not needed here). */
private fun describe(r: PageFullText, failed: Int): PageResult {
    fun n(k: Int, one: String) = if (k == 1) "1 $one" else "$k ${one}s"
    val what = "From this page: ${r.words} words, ${n(r.images.toInt(), "figure")}."
    val extra = buildList {
        if (failed > 0) add("${n(failed, "figure image")} couldn't be downloaded.")
        addAll(r.notes)
    }.joinToString(" ").let { if (it.isEmpty()) "" else " $it" }
    return when (r.kind) {
        PageOutcomeKind.ADDED -> PageResult("Added to your library", "The paper was added with this page's full text.\n\n$what$extra", true)
        PageOutcomeKind.ATTACHED -> PageResult("Reading view made", "The paper's reading view is now this page's full text.\n\n$what$extra", true)
        PageOutcomeKind.REPLACED -> {
            val h = r.highlights.toInt()
            val kept = if (h > 0) " Your ${n(h, "highlight")} ${if (h == 1) "is" else "are"} all found in the new text." else ""
            PageResult("Reading view replaced", "Reading view replaced. The old one is backed up.$kept\n\n$what$extra", true)
        }
        PageOutcomeKind.KEPT -> {
            val k = r.unmatched.size
            val quotes = r.unmatched.take(3).joinToString("\n") { "• “${it.take(80)}${if (it.length > 80) "…" else ""}”" }
            PageResult(
                "Kept your reading view",
                "${n(k, "highlight")} wouldn't be found in the page's text, so your reading view stays as it is:\n\n$quotes" +
                    (if (k > 3) "\n…" else "") + "\n\nThe page's version is saved in the app's backups.",
                false,
            )
        }
        PageOutcomeKind.UNCHANGED -> PageResult("Already up to date", "The reading view already matches this page.", true)
    }
}

private fun strings(a: JSONArray?): List<String> =
    if (a == null) emptyList() else (0 until a.length()).mapNotNull { a.optString(it).takeIf { s -> s.isNotBlank() } }

private sealed interface Got {
    class Bytes(val data: ByteArray) : Got
    data object TooLarge : Got
    data object Failed : Got
}

/** A figure with the page's cookies (the same access as the page), as an image or nothing. */
private fun download(url: String, userAgent: String, referer: String): Got {
    if (!url.startsWith("https://") && !url.startsWith("http://")) return Got.Failed
    return try {
        val c = URL(url).openConnection() as HttpURLConnection
        try {
            c.connectTimeout = 15_000
            c.readTimeout = 30_000
            c.instanceFollowRedirects = true
            c.setRequestProperty("User-Agent", userAgent)
            c.setRequestProperty("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
            if (referer.isNotEmpty()) c.setRequestProperty("Referer", referer)
            CookieManager.getInstance().getCookie(url)?.let { c.setRequestProperty("Cookie", it) }
            if (c.responseCode !in 200..299) return Got.Failed
            if (c.contentLengthLong > MAX_IMAGE_BYTES) return Got.TooLarge
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(64 * 1024)
            c.inputStream.use { input ->
                while (true) {
                    val k = input.read(buf)
                    if (k < 0) break
                    out.write(buf, 0, k)
                    if (out.size() > MAX_IMAGE_BYTES) return Got.TooLarge
                }
            }
            val data = out.toByteArray()
            if (looksImage(data, c.contentType)) Got.Bytes(data) else Got.Failed
        } finally {
            c.disconnect()
        }
    } catch (e: Exception) {
        Got.Failed
    }
}

/** A figure fetched by the page itself (requests only the page's own site would answer). */
private suspend fun fromPage(w: WebView, collector: PageCollector, token: String, index: Int, url: String): Got {
    val answer = collector.expectImage(index)
    w.evaluateJavascript("window.__luntaFetchImage && window.__luntaFetchImage(${JSONObject.quote(token)}, $index, ${JSONObject.quote(url)})", null)
    val b64 = withTimeoutOrNull(30_000) { answer.await() } ?: return Got.Failed
    if (b64.isEmpty() || b64.length > MAX_IMAGE_BYTES / 3 * 4 + 4) return if (b64.isEmpty()) Got.Failed else Got.TooLarge
    val data = withContext(Dispatchers.Default) { runCatching { Base64.decode(b64, Base64.DEFAULT) }.getOrNull() } ?: return Got.Failed
    return if (looksImage(data, null)) Got.Bytes(data) else Got.Failed
}

/** JPEG, PNG, GIF, WebP, TIFF or SVG (the extension's looksImage). */
private fun looksImage(b: ByteArray, type: String?): Boolean {
    if (type != null && type.trim().lowercase().startsWith("image/")) return true
    if (b.size < 12) return false
    fun at(i: Int) = b[i].toInt() and 0xff
    return (at(0) == 0xff && at(1) == 0xd8) || (at(0) == 0x89 && at(1) == 0x50) || (at(0) == 0x47 && at(1) == 0x49) ||
        (at(0) == 0x52 && at(1) == 0x49 && at(8) == 0x57) || (at(0) == 0x49 && at(1) == 0x49) || (at(0) == 0x4d && at(1) == 0x4d) ||
        (at(0) == 0x3c && (at(1) == 0x73 || at(1) == 0x3f))
}

/**
 * What page.js sends back (window.LuntaPage). Only answers carrying the current collection's token
 * count; between collections everything is ignored.
 */
class PageCollector {
    private val main = Handler(Looper.getMainLooper())
    @Volatile private var token = ""
    @Volatile private var say: (String) -> Unit = {}
    @Volatile private var answer: CompletableDeferred<String>? = null
    private val images = ConcurrentHashMap<Int, CompletableDeferred<String>>()

    fun begin(onProgress: (String) -> Unit): String {
        images.clear()
        say = onProgress
        answer = CompletableDeferred()
        token = UUID.randomUUID().toString()
        return token
    }

    fun end() {
        token = ""
        say = {}
        answer = null
        images.clear()
    }

    suspend fun page(): String = (answer ?: throw IllegalStateException("not collecting")).await()

    fun expectImage(index: Int): CompletableDeferred<String> = CompletableDeferred<String>().also { images[index] = it }

    @JavascriptInterface
    fun progress(t: String, text: String) {
        if (t == token && t.isNotEmpty()) main.post { if (t == token) say(text) }
    }

    @JavascriptInterface
    fun done(t: String, json: String) {
        if (t == token && t.isNotEmpty()) answer?.complete(json)
    }

    @JavascriptInterface
    fun fail(t: String, message: String) {
        if (t == token && t.isNotEmpty()) answer?.completeExceptionally(Exception("The page couldn't be read: $message"))
    }

    @JavascriptInterface
    fun image(t: String, index: Int, base64: String) {
        if (t == token && t.isNotEmpty()) images.remove(index)?.complete(base64)
    }
}
