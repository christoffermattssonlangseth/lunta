package app.lunta.update

import org.json.JSONObject

/**
 * A release on GitHub, from https://api.github.com/repos/<repo>/releases/latest: its version (the
 * tag without the v), what's new, and the APK attached to it. Plain functions, unit-tested
 * (ReleasesTest); the downloading and installing is in Updater.
 */
data class Release(
    val tag: String,
    val version: String,
    /** What's new, as Markdown (the release notes above "Which file do I need?") */
    val notes: String,
    val page: String,
    val apkUrl: String,
    val apkName: String,
    val apkSize: Long,
)

object Releases {
    const val REPO = "christoffermattssonlangseth/lunta"
    const val LATEST = "https://api.github.com/repos/$REPO/releases/latest"

    /**
     * The release in GitHub's JSON, or null when it isn't one to offer: a draft or prerelease, or
     * one without its APK yet (the release exists a while before the builds are attached).
     */
    fun parse(json: String): Release? {
        val o = JSONObject(json)
        if (o.optBoolean("draft") || o.optBoolean("prerelease")) return null
        val tag = o.optString("tag_name").trim()
        if (tag.isEmpty()) return null
        val assets = o.optJSONArray("assets") ?: return null
        var apk: JSONObject? = null
        for (i in 0 until assets.length()) {
            val a = assets.optJSONObject(i) ?: continue
            val name = a.optString("name")
            if (name.endsWith("-Android.apk", ignoreCase = true) && a.optString("state", "uploaded") == "uploaded") { apk = a; break }
            if (apk == null && name.endsWith(".apk", ignoreCase = true)) apk = a
        }
        apk ?: return null
        val url = apk.optString("browser_download_url")
        if (!url.startsWith("https://")) return null
        return Release(
            tag = tag,
            version = tag.removePrefix("v").removePrefix("V"),
            notes = whatsNew(o.optString("body")),
            page = o.optString("html_url").ifEmpty { "https://github.com/$REPO/releases/tag/$tag" },
            apkUrl = url,
            apkName = apk.optString("name"),
            apkSize = apk.optLong("size", -1),
        )
    }

    /** The release notes without the downloads table and the line about what Lunta is. */
    fun whatsNew(body: String): String {
        val lines = body.replace("\r", "").lines()
        val end = lines.indexOfFirst { it.startsWith("## Which file do I need") }.let { if (it < 0) lines.size else it }
        return lines.subList(0, end)
            .filterNot { it.startsWith("Lunta is a reference manager for scientific papers") }
            .joinToString("\n").trim()
    }

    /** Whether [latest] (a tag or version) is newer than [current] (this app's versionName). */
    fun isNewer(latest: String, current: String): Boolean = compare(latest, current) > 0

    /**
     * Compares versions like 1.0.10 and v1.0.9 number by number (missing numbers are 0); a
     * pre-release (1.1.0-beta.1) is older than its release (1.1.0). Anything after + is ignored.
     */
    fun compare(a: String, b: String): Int {
        fun split(v: String): Pair<List<Long>, String?> {
            val s = v.trim().removePrefix("v").removePrefix("V").substringBefore('+')
            val core = s.substringBefore('-')
            val pre = if ('-' in s) s.substringAfter('-') else null
            return core.split('.').map { part -> part.takeWhile { it.isDigit() }.toLongOrNull() ?: 0L } to pre
        }
        val (x, xp) = split(a)
        val (y, yp) = split(b)
        for (i in 0 until maxOf(x.size, y.size)) {
            val c = x.getOrElse(i) { 0L }.compareTo(y.getOrElse(i) { 0L })
            if (c != 0) return c
        }
        return when {
            xp == null && yp == null -> 0
            xp == null -> 1
            yp == null -> -1
            else -> xp.compareTo(yp)
        }
    }

    /** Markdown to plain text for the dialog: no **, `, or # marks; bullets as •. */
    fun plain(markdown: String): String = markdown.lines().joinToString("\n") { line ->
        line.replace(Regex("^#{1,6}\\s+"), "")
            .replace(Regex("^\\s*[-*]\\s+"), "• ")
            .replace("**", "")
            .replace("`", "")
            .replace(Regex("\\[([^\\]]+)]\\([^)]+\\)"), "$1")
    }.trim()
}
