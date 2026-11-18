package app.lunta.update

import android.app.Application
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import app.lunta.BuildConfig
import app.lunta.data.Settings
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * Updates for the APK from GitHub Releases (there is no store): asks GitHub's API for the latest
 * release at most once a day when the app starts (if "Check for updates automatically" is on) or
 * when asked in Settings; offers the new version with its notes; downloads the APK into the cache,
 * checks it is Lunta signed with the same key as this one, and hands it to the system's package
 * installer (which asks once for permission to install apps from Lunta). Only GitHub is contacted.
 *
 * Off in builds made with lunta.selfUpdate=false (a Play Store build), which also leave out the
 * install permission and the FileProvider (src/selfupdate/AndroidManifest.xml).
 */
class Updater(private val app: Application, private val settings: Settings, private val scope: CoroutineScope) {
    val enabled: Boolean get() = BuildConfig.SELF_UPDATE
    val current: String get() = BuildConfig.VERSION_NAME

    /** A newer release to offer (the dialog); null when there is none or it was put off. */
    var available by mutableStateOf<Release?>(null)
    var checking by mutableStateOf(false)
        private set
    /** Downloading: the fraction done, or -1 when the size isn't known; null otherwise. */
    var progress by mutableStateOf<Float?>(null)
        private set
    /** What a check from Settings found ("newest version", errors). */
    var message by mutableStateOf<String?>(null)
    /** The APK waits for "Install unknown apps" to be allowed for Lunta. */
    var needsPermission by mutableStateOf(false)

    var auto by mutableStateOf(settings.autoUpdate)
        private set

    private var downloaded: File? = null

    fun changeAuto(on: Boolean) { settings.autoUpdate = on; auto = on }

    /** On start: a quiet check, if it is on and the last was a day ago or more. */
    fun checkIfDue() {
        if (!enabled || !settings.autoUpdate || checking) return
        val now = System.currentTimeMillis()
        if (now - settings.lastUpdateCheckMs in 0 until DAY) return
        check(manual = false)
    }

    fun check(manual: Boolean) {
        if (!enabled || checking) return
        checking = true
        scope.launch {
            try {
                val r = withContext(Dispatchers.IO) { fetchLatest() }
                settings.lastUpdateCheckMs = System.currentTimeMillis()
                if (r != null && Releases.isNewer(r.version, current)) available = r
                else if (manual) message = "Lunta $current is the newest version."
            } catch (e: Exception) {
                if (manual) message = "Couldn't check for updates: ${e.message ?: e.javaClass.simpleName}"
            } finally {
                checking = false
            }
        }
    }

    /** Download, check and install [r]. */
    fun download(r: Release) {
        if (progress != null) return
        available = null
        progress = -1f
        scope.launch {
            try {
                val apk = withContext(Dispatchers.IO) { fetchApk(r) }
                withContext(Dispatchers.IO) { checkApk(apk) }
                downloaded = apk
                install()
            } catch (e: Exception) {
                message = "Couldn't download Lunta ${r.version}: ${e.message ?: e.javaClass.simpleName}"
            } finally {
                progress = null
            }
        }
    }

    /** Back in the app (after allowing installs from Lunta): install what was downloaded. */
    fun resume() {
        if (needsPermission && app.packageManager.canRequestPackageInstalls()) {
            needsPermission = false
            install()
        }
    }

    /** Settings › Apps › Special app access › Install unknown apps, for Lunta. */
    fun openInstallPermission() {
        app.startActivity(
            Intent(android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${app.packageName}"))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    }

    private fun install() {
        val apk = downloaded ?: return
        if (!app.packageManager.canRequestPackageInstalls()) { needsPermission = true; return }
        val uri = FileProvider.getUriForFile(app, "${app.packageName}.updates", apk)
        app.startActivity(
            Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    }

    private fun connect(url: String): HttpURLConnection =
        (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 15_000
            readTimeout = 30_000
            instanceFollowRedirects = true   // the APK is served from GitHub's download host
            setRequestProperty("User-Agent", "Lunta-Android/$current")
        }

    private fun fetchLatest(): Release? {
        val c = connect(Releases.LATEST)
        c.setRequestProperty("Accept", "application/vnd.github+json")
        try {
            return when (c.responseCode) {
                200 -> Releases.parse(c.inputStream.bufferedReader().use { it.readText() })
                404 -> null   // no release yet
                else -> throw Exception("GitHub answered ${c.responseCode}")
            }
        } finally {
            c.disconnect()
        }
    }

    private fun fetchApk(r: Release): File {
        val dir = File(app.cacheDir, "updates").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }   // earlier downloads
        val out = File(dir, r.apkName.ifBlank { "Lunta-${r.version}-Android.apk" }.replace('/', '_'))
        val part = File(dir, out.name + ".part")
        val c = connect(r.apkUrl)
        try {
            if (c.responseCode != 200) throw Exception("GitHub answered ${c.responseCode}")
            val total = c.contentLengthLong.takeIf { it > 0 } ?: r.apkSize
            c.inputStream.use { input ->
                part.outputStream().use { output ->
                    val buf = ByteArray(64 * 1024)
                    var done = 0L
                    var shown = -1
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        output.write(buf, 0, n)
                        done += n
                        if (total > 0) {
                            val pct = (done * 100 / total).toInt()
                            if (pct != shown) { shown = pct; scope.launch { progress = pct / 100f } }
                        }
                    }
                }
            }
            if (r.apkSize > 0 && part.length() != r.apkSize) throw Exception("the download was incomplete")
        } finally {
            c.disconnect()
        }
        if (!part.renameTo(out)) throw Exception("couldn't save the download")
        return out
    }

    /** The APK must be Lunta, signed with the key this copy was signed with (else Android refuses it anyway). */
    @Suppress("DEPRECATION")
    private fun checkApk(apk: File) {
        val pm = app.packageManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val flags = PackageManager.GET_SIGNING_CERTIFICATES
            val archive = pm.getPackageArchiveInfo(apk.path, flags) ?: throw Exception("the file isn't an Android app")
            if (archive.packageName != app.packageName) throw Exception("the file isn't Lunta")
            val theirs = archive.signingInfo?.apkContentsSigners?.toSet().orEmpty()
            val mine = pm.getPackageInfo(app.packageName, flags).signingInfo?.apkContentsSigners?.toSet().orEmpty()
            if (theirs.isEmpty() || theirs != mine) {
                throw Exception("it is signed with another key than this copy of Lunta" + if (BuildConfig.DEBUG) " (this is a development build)" else "")
            }
        } else {
            val archive = pm.getPackageArchiveInfo(apk.path, 0) ?: throw Exception("the file isn't an Android app")
            if (archive.packageName != app.packageName) throw Exception("the file isn't Lunta")
        }
    }

    companion object {
        const val DAY = 24L * 60 * 60 * 1000
    }
}
