// "Use this page's full text" in the app's journal page (JournalScreen.kt): collects the article
// page and its figure images as the reader sees them, with their access. A port of the Chrome
// extension's content script (extension/content.js: pageHTML, figureImages, linkedTablePages).
// Run with evaluateJavascript after "__TOKEN__" is replaced; answers through window.LuntaPage:
//   progress(token, text), done(token, json), fail(token, message)
// and, later, window.__luntaFetchImage(token, index, url) → image(token, index, base64 | "").
(() => {
  const TOKEN = "__TOKEN__";
  const native = window.LuntaPage;
  if (!native) return;

  const meta = (...names) => {
    for (const n of names) {
      const el = document.querySelector(`meta[name="${n}" i], meta[property="${n}" i]`);
      if (el && el.content && el.content.trim()) return el.content.trim();
    }
    return "";
  };
  const DOI_RE = /10\.\d{4,9}\/[^\s"<>?#&]+/i;
  const clean = (d) => d.replace(/[.,;)\]]+$/, "").replace(/\/(full|abstract|pdf|epdf)$/i, "").replace(/~.*$/, "");

  function findDoi() {
    for (const v of [meta("citation_doi"), meta("dc.identifier", "DC.Identifier"), meta("prism.doi"), meta("bepress_citation_doi")]) {
      const m = v && v.match(DOI_RE);
      if (m) return clean(m[0]);
    }
    let href = location.href;
    try { href = decodeURIComponent(href); } catch (_) { /* as is */ }
    const inUrl = href.match(DOI_RE);
    if (inUrl) return clean(inUrl[0]);
    const head = ((document.querySelector("article, main, #content") || document.body || {}).innerText || "").slice(0, 4000);
    const m = head.match(/doi:?\s*(10\.\d{4,9}\/[^\s"<>]+)/i);
    return m ? clean(m[1]) : "";
  }

  function toBase64(buf) {
    let s = "";
    const bytes = new Uint8Array(buf);
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(s);
  }

  // words of paragraph text in a document (to tell a full-text page from an abstract page)
  function paragraphWords(doc) {
    let n = 0;
    doc.querySelectorAll("p").forEach((p) => {
      if (p.closest("nav, header, footer, aside")) return;
      n += (p.textContent || "").split(/\s+/).filter(Boolean).length;
    });
    return n;
  }

  // The page as the server delivered it (before MathJax and other scripts rewrote it), fetched
  // again with the page's cookies so the same access applies; used when it carries the full text
  // the reader is looking at, otherwise the page as shown.
  async function pageHTML() {
    const live = "<!DOCTYPE html>\n" + document.documentElement.outerHTML;
    try {
      const r = await fetch(location.href, { credentials: "include" });
      const type = r.headers.get("content-type") || "";
      if (r.ok && /html/i.test(type)) {
        const text = await r.text();
        const fetched = new DOMParser().parseFromString(text, "text/html");
        const fw = paragraphWords(fetched), lw = paragraphWords(document);
        if (fw >= 300 && fw >= lw * 0.6) return { html: text, liveHtml: live };
      }
    } catch (_) { /* the page as shown */ }
    return { html: live, liveHtml: null };
  }

  const IMG_EXT = /\.(jpe?g|png|gif|webp|tiff?|svg)(\?|#|$)/i;
  const abs = (u) => { try { return u ? new URL(u, location.href).href : ""; } catch (_) { return ""; } };

  function largestFromSrcset(srcset) {
    if (!srcset) return "";
    let best = "", bestW = -1;
    for (const part of srcset.split(",")) {
      const [u, d] = part.trim().split(/\s+/);
      const w = d ? parseFloat(d) * (d.endsWith("x") ? 1000 : 1) : 0;
      if (u && w > bestW) { best = u; bestW = w; }
    }
    return abs(best);
  }

  // Figure (and equation) images: every address the page uses for each, and the one to download
  // (the high-resolution link when there is one, e.g. ScienceDirect's "_lrg").
  function figureImages() {
    const holders = new Set();
    document.querySelectorAll('figure, [class*="figure"], [class*="Figure"], [id^="fig"], [id^="Fig"], [class*="formula"], [class*="equation"], .fig, .table-wrap')
      .forEach((el) => { if (!el.closest("nav, header, footer") && el.querySelector("img")) holders.add(el); });
    const out = [], seen = new Set();
    for (const h of holders) {
      if ([...holders].some((o) => o !== h && h.contains(o))) continue; // innermost holders only
      const hiLinks = [...h.querySelectorAll("a[href]")].map((a) => a.href)
        .filter((u) => /_lrg\.|[-_]large\.|hi-?res|full-?size|\/original\//i.test(u) || IMG_EXT.test(u));
      const imgs = [...h.querySelectorAll("img")].filter((i) => !(i.naturalWidth && i.naturalWidth < 24));
      imgs.forEach((img, k) => {
        const urls = [img.currentSrc, img.src, abs(img.getAttribute("src")), abs(img.getAttribute("data-src")),
                      abs(img.getAttribute("data-lazy-src")), largestFromSrcset(img.getAttribute("srcset") || img.getAttribute("data-srcset"))];
        const pic = img.closest("picture");
        if (pic) pic.querySelectorAll("source").forEach((s) => urls.push(largestFromSrcset(s.getAttribute("srcset"))));
        const hi = k === 0 ? hiLinks.find((u) => /_lrg\.|large|hi-?res|full|original/i.test(u)) || hiLinks[0] : "";
        if (hi) urls.unshift(hi);
        const list = [...new Set(urls.filter((u) => u && !u.startsWith("data:")))];
        const fetchUrl = hi || largestFromSrcset(img.getAttribute("srcset")) || img.currentSrc || img.src || list[0];
        if (!fetchUrl || fetchUrl.startsWith("data:") || seen.has(fetchUrl)) return;
        seen.add(fetchUrl);
        out.push({ urls: list, fetch: fetchUrl });
      });
    }
    return out;
  }

  // Tables the page only links to (Springer Nature: "Full size table" → /articles/…/tables/1):
  // their own pages, fetched with the reader's access, so the reading view has them.
  async function linkedTablePages() {
    const links = new Map();
    document.querySelectorAll('a[data-test="table-link"][href]').forEach((a) => links.set(a.href, a.getAttribute("href")));
    const out = [];
    for (const [u, raw] of [...links].slice(0, 30)) {
      try {
        const r = await fetch(u, { credentials: "include" });
        if (!r.ok || !/html/i.test(r.headers.get("content-type") || "")) continue;
        const text = await r.text();
        if (!/<table[\s>]/i.test(text)) continue;
        out.push({ urls: [...new Set([u, raw])], html: text });
      } catch (_) { /* that table stays a link */ }
    }
    return out;
  }

  // a figure the app couldn't download itself: from the page (same-origin cookies)
  window.__luntaFetchImage = (token, index, url) => {
    if (token !== TOKEN) return;
    fetch(url, { credentials: "include" })
      .then(async (r) => (r.ok ? toBase64(await r.arrayBuffer()) : ""))
      .catch(() => "")
      .then((b) => native.image(token, index, b));
  };

  (async () => {
    try {
      native.progress(TOKEN, "Reading the page…");
      const page = await pageHTML();
      const images = figureImages();
      let pages = [];
      if (document.querySelector('a[data-test="table-link"]')) {
        native.progress(TOKEN, "Fetching the tables…");
        pages = await linkedTablePages();
      }
      native.done(TOKEN, JSON.stringify({
        url: location.href, doi: findDoi(), title: meta("citation_title", "dc.title", "og:title") || document.title || "",
        html: page.html, liveHtml: page.liveHtml, images, pages,
      }));
    } catch (e) {
      native.fail(TOKEN, String((e && e.message) || e));
    }
  })();
})();
