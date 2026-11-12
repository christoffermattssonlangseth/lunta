package app.lunta.data

import org.json.JSONObject
import java.io.File

/**
 * Reading positions, in the files the Apple apps and the desktop app use (see
 * desktop/src-tauri/src/positions.rs): each device writes only its own
 * `<library>/positions/<device>.json`, `{paper: {offset, at (seconds since 2001), device}}`, and a
 * reader opens at the newest position from any device. The Syncer carries these files.
 */
class Positions(private val root: File, private val deviceId: String, private val label: String) {
    data class Latest(val offset: Int, val device: String, val mine: Boolean)

    private val swiftEpoch = 978_307_200.0
    private fun dir() = File(root, "positions")

    private fun read(f: File): JSONObject = runCatching { JSONObject(f.readText()) }.getOrDefault(JSONObject())

    @Synchronized
    fun save(paper: String, offset: Int) {
        val f = File(dir(), "$deviceId.json")
        val mine = read(f)
        mine.optJSONObject(paper)?.let { if (kotlin.math.abs(it.optInt("offset") - offset) < 40) return }
        mine.put(paper, JSONObject().put("offset", offset).put("at", System.currentTimeMillis() / 1000.0 - swiftEpoch).put("device", label))
        dir().mkdirs()
        val tmp = File(dir(), ".$deviceId.json.tmp")
        tmp.writeText(mine.toString())
        tmp.renameTo(f)
    }

    fun latest(paper: String): Latest? {
        var best: Triple<Int, Double, Pair<String, Boolean>>? = null
        for (f in dir().listFiles().orEmpty()) {
            if (f.name.startsWith(".") || f.extension != "json") continue
            val s = read(f).optJSONObject(paper) ?: continue
            val at = s.optDouble("at", 0.0)
            if (best == null || at > best.second) best = Triple(s.optInt("offset"), at, s.optString("device") to (f.name == "$deviceId.json"))
        }
        return best?.let { Latest(it.first, it.third.first, it.third.second) }
    }
}
