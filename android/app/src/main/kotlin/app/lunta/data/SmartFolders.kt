package app.lunta.data

import org.json.JSONArray
import uniffi.lunta_core.Paper
import java.io.File

/**
 * Smart folders, read from `<library>/smart-folders.json` (written by the Apple apps; same rules as
 * desktop/src-tauri/src/smart.rs). Evaluated here on the list of all papers; the text rule needs the
 * ids of the papers the full-text search found.
 */
data class SmartFolder(
    val id: String, val name: String, val text: String, val tag: String, val folder: String,
    val unreadOnly: Boolean, val starredOnly: Boolean, val yearFrom: String, val yearTo: String,
    val addedDays: Long, val pdf: String, val highlightsOnly: Boolean,
) {
    fun matches(p: Paper, textHits: Set<String>?, nowMs: Long): Boolean {
        if (folder.isNotEmpty() && folder !in p.collections) return false
        if (tag.isNotEmpty() && tag !in p.tags) return false
        if (starredOnly && !p.starred) return false
        if (unreadOnly && p.read) return false
        if (textHits != null && p.id !in textHits) return false
        val year = p.meta.date.take(4).toIntOrNull()
        yearFrom.trim().toIntOrNull()?.let { if ((year ?: 0) < it) return false }
        yearTo.trim().toIntOrNull()?.let { if ((year ?: 9999) > it) return false }
        if (addedDays > 0 && p.added < nowMs - addedDays * 86_400_000L) return false
        when (pdf) { "has" -> if (!p.hasPdf) return false; "missing" -> if (p.hasPdf) return false }
        if (highlightsOnly && p.annotationCount == 0u) return false
        return true
    }

    companion object {
        fun load(root: File): List<SmartFolder> = runCatching {
            val a = JSONArray(File(root, "smart-folders.json").readText())
            (0 until a.length()).map { i ->
                val o = a.getJSONObject(i)
                SmartFolder(
                    o.optString("id"), o.optString("name"), o.optString("text"), o.optString("tag"), o.optString("folder"),
                    o.optBoolean("unreadOnly"), o.optBoolean("starredOnly"), o.optString("yearFrom"), o.optString("yearTo"),
                    o.optLong("addedDays"), o.optString("pdf", "any"), o.optBoolean("highlightsOnly"),
                )
            }
        }.getOrDefault(emptyList())
    }
}
