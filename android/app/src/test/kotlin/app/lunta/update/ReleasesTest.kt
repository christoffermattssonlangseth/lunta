package app.lunta.update

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReleasesTest {
    private val body = "## What's new in 1.0.6\r\n\r\n- **Updates:** Lunta updates itself.\r\n\r\n" +
        "Lunta is a reference manager for scientific papers: your library…\r\n\r\n" +
        "## Which file do I need?\r\n\r\n| Your computer | Download |\r\n"

    /** The parts of GitHub's /releases/latest answer that matter, as GitHub sends them. */
    private fun json(tag: String = "v1.0.6", apk: Boolean = true, prerelease: Boolean = false) = """
        {
          "html_url": "https://github.com/christoffermattssonlangseth/lunta/releases/tag/$tag",
          "tag_name": "$tag", "name": "Lunta $tag", "draft": false, "prerelease": $prerelease,
          "body": ${org.json.JSONObject.quote(body)},
          "assets": [
            {"name": "Lunta-1.0.6-Chrome-Extension.zip", "state": "uploaded", "size": 120000,
             "browser_download_url": "https://github.com/christoffermattssonlangseth/lunta/releases/download/$tag/Lunta-1.0.6-Chrome-Extension.zip"},
            ${if (apk) """{"name": "Lunta-1.0.6-Android.apk", "state": "uploaded", "size": 98765432,
             "browser_download_url": "https://github.com/christoffermattssonlangseth/lunta/releases/download/$tag/Lunta-1.0.6-Android.apk"},""" else ""}
            {"name": "latest.json", "state": "uploaded", "size": 2000,
             "browser_download_url": "https://github.com/christoffermattssonlangseth/lunta/releases/download/$tag/latest.json"}
          ]
        }
    """.trimIndent()

    @Test fun parsesTheLatestRelease() {
        val r = Releases.parse(json())!!
        assertEquals("v1.0.6", r.tag)
        assertEquals("1.0.6", r.version)
        assertEquals("Lunta-1.0.6-Android.apk", r.apkName)
        assertEquals("https://github.com/christoffermattssonlangseth/lunta/releases/download/v1.0.6/Lunta-1.0.6-Android.apk", r.apkUrl)
        assertEquals(98765432L, r.apkSize)
        assertEquals("https://github.com/christoffermattssonlangseth/lunta/releases/tag/v1.0.6", r.page)
        assertEquals("## What's new in 1.0.6\n\n- **Updates:** Lunta updates itself.", r.notes)
        assertEquals("What's new in 1.0.6\n\n• Updates: Lunta updates itself.", Releases.plain(r.notes))
    }

    @Test fun noApkYetOrPrereleaseIsNoUpdate() {
        assertNull(Releases.parse(json(apk = false)))
        assertNull(Releases.parse(json(prerelease = true)))
    }

    @Test fun comparesVersionsNumberByNumber() {
        assertTrue(Releases.isNewer("v1.0.10", "1.0.9"))
        assertTrue(Releases.isNewer("1.1", "1.0.9"))
        assertTrue(Releases.isNewer("2.0.0", "1.99.99"))
        assertFalse(Releases.isNewer("v1.0.5", "1.0.5"))
        assertFalse(Releases.isNewer("1.0.5", "1.0.6"))
        assertFalse(Releases.isNewer("1.0", "1.0.0"))
        assertTrue(Releases.isNewer("1.1.0", "1.1.0-beta.2"))
        assertFalse(Releases.isNewer("1.1.0-beta.2", "1.1.0"))
        assertEquals(0, Releases.compare("1.0.6+build.7", "v1.0.6"))
    }
}
