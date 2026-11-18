// Lunta extension: finds a paper's metadata on article pages and shows a small panel to save it.
(() => {
  if (window.top !== window || document.getElementById("lunta-ext-root")) return;

  // entities a page escaped twice (bioRxiv: content="H&amp;amp;E" reads as "H&amp;E")
  const unescape = (t) => t.replace(/&(amp|lt|gt|quot|#39|apos);/g, (_, e) => ({ amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", apos: "'" })[e]);
  const meta = (...names) => {
    for (const n of names) {
      const el = document.querySelector(`meta[name="${n}" i], meta[property="${n}" i]`);
      if (el && el.content && el.content.trim()) return unescape(el.content.trim());
    }
    return "";
  };
  const metas = (n) => [...document.querySelectorAll(`meta[name="${n}" i]`)].map((m) => unescape(m.content.trim())).filter(Boolean);
  // requests the page itself would make (its PDF, figures, table pages), with the reader's access:
  // in Firefox a content script's own fetch isn't the page's, content.fetch is
  // eslint-disable-next-line no-undef
  const pageFetch = typeof content !== "undefined" && content && typeof content.fetch === "function" ? content.fetch.bind(content) : fetch;
  const DOI_RE = /10\.\d{4,9}\/[^\s"<>?#&]+/i;

  function findDoi() {
    for (const v of [meta("citation_doi"), meta("dc.identifier", "DC.Identifier"), meta("prism.doi"), meta("bepress_citation_doi")]) {
      const m = v && v.match(DOI_RE);
      if (m) return clean(m[0]);
    }
    const inUrl = decodeURIComponent(location.href).match(DOI_RE);
    if (inUrl) return clean(inUrl[0]);
    // a DOI printed in the article header (e.g. "DOI: 10.1519/…"); only the top of the page, not the references
    const head = (document.querySelector("article, main, #content") || document.body).innerText.slice(0, 4000);
    const m = head.match(/doi:?\s*(10\.\d{4,9}\/[^\s"<>]+)/i);
    return m ? clean(m[1]) : "";
  }

  function clean(d) {
    return d.replace(/[.,;)\]]+$/, "").replace(/\/(full|abstract|pdf|epdf)$/i, "").replace(/~.*$/, "");
  }

  // Where the PDF may be, best first: the page's citation_pdf_url, then the page's own Download PDF
  // button (Nature's citation_pdf_url can be a web page while its button has the PDF: "_reference.pdf"),
  // then other PDF links. Saving tries them in turn until one gives a PDF.
  function findPdfUrls() {
    const urls = [];
    const add = (u) => { try { u = new URL(u, location.href).href; if (!urls.includes(u)) urls.push(u); } catch (_) {} };
    // shown on the page (offsetParent is null for links in fixed sidebars too, so not that)
    const visible = (x) => x.getClientRects().length > 0;
    const supplement = (h) => /supplement|MOESM|\/esm\/|_ESM\.|suppl|mmc\d/i.test(h);
    const m = meta("citation_pdf_url");
    if (m) add(m);
    const link = document.querySelector('link[type="application/pdf"]');
    if (link && link.href) add(link.href);
    const all = [...document.querySelectorAll("a[href]")].filter((x) => !supplement(x.getAttribute("href")));
    const links = all.filter(visible);
    // the publisher's own download button (shown or not: some pages show it only in a menu)
    all.filter((x) => x.matches('[data-article-pdf], [data-test="download-pdf"], .c-pdf-download__link, [data-track-action="download pdf"]')
        || /^\s*(download\s+(article\s+)?pdf|view\s+pdf|pdf)\s*$/i.test(x.textContent || ""))
      .forEach((x) => add(x.href));
    links.filter((x) => /\.pdf($|[?#])|\/pdf(\/|$|\?)|\/epdf\//i.test(x.getAttribute("href"))).forEach((x) => add(x.href));
    return urls.slice(0, 4);
  }

  // publishers whose article pages Lunta can read (also when a page lacks citation_* tags)
  // (by platform: Elsevier; Atypon; Springer Nature; Silverchair; others)
  const KNOWN_HOSTS = new RegExp("(^|\\.)(" + [
    "sciencedirect\\.com", "cell\\.com", "thelancet\\.com",
    "nejm\\.org", "science\\.org", "pnas\\.org", "wiley\\.com", "tandfonline\\.com", "sagepub\\.com", "acs\\.org", "ahajournals\\.org",
    "asm\\.org", "annualreviews\\.org", "liebertpub\\.com", "physiology\\.org", "ascopubs\\.org", "atsjournals\\.org",
    "psychiatryonline\\.org", "royalsocietypublishing\\.org", "journals\\.uchicago\\.edu", "rsna\\.org", "ajronline\\.org",
    "healthaffairs\\.org", "neurology\\.org", "embopress\\.org",
    "nature\\.com", "springer\\.com", "biomedcentral\\.com", "springeropen\\.com",
    "jamanetwork\\.com", "oup\\.com", "ashpublications\\.org", "rupress\\.org", "aacrjournals\\.org", "diabetesjournals\\.org",
    "direct\\.mit\\.edu", "geoscienceworld\\.org", "karger\\.com", "portlandpress\\.com", "biologists\\.com", "iwaponline\\.com",
    "publications\\.aap\\.org", "journals\\.aai\\.org",
    "plos\\.org", "elifesciences\\.org", "frontiersin\\.org", "bmj\\.com", "biorxiv\\.org", "medrxiv\\.org", "ncbi\\.nlm\\.nih\\.gov", "mdpi\\.com",
  ].join("|") + ")$", "i");
  const ARTICLE_PATH = /\/(science\/article|article|articles|doi|content|fulltext|fullarticle|full)\b/i;
  const knownArticle = KNOWN_HOSTS.test(location.hostname) && ARTICLE_PATH.test(location.pathname);

  const doi = findDoi();
  const title = meta("citation_title", "dc.title", "og:title") || "";
  if (!doi && !meta("citation_title") && !knownArticle) return; // not an article page

  // PubMed and PMC: the article's ids (PubMed has no PDF of its own; Lunta finds a free one from these)
  // and the journal's page from PubMed's "Full text links" (for papers that aren't free)
  const onPubMed = /(^|\.)pubmed\.ncbi\.nlm\.nih\.gov$/i.test(location.hostname);
  const pmid = meta("citation_pmid") || (onPubMed && (location.pathname.match(/^\/(\d{1,9})\/?$/) || [])[1]) || "";
  const pmcid = (() => {
    const m = meta("citation_pmcid");
    if (m) return /^PMC/i.test(m) ? m.toUpperCase() : "PMC" + m;
    const here = location.href.match(/\/(?:pmc\/)?articles\/(PMC\d+)/i);
    if (here) return here[1].toUpperCase();
    if (!onPubMed) return "";
    const a = [...document.querySelectorAll("a[href]")].map((x) => x.href.match(/ncbi\.nlm\.nih\.gov\/(?:pmc\/)?articles\/(PMC\d+)/i)).find(Boolean);
    return a ? a[1].toUpperCase() : "";
  })();
  const journalUrl = (() => {
    if (!onPubMed) return "";
    const a = [...document.querySelectorAll(".full-text-links-list a[href], .full-text-links a[href]")]
      .find((x) => /^https?:/i.test(x.href) && !/ncbi\.nlm\.nih\.gov/i.test(x.href));
    return a ? a.href : (doi ? "https://doi.org/" + doi : "");
  })();

  const info = {
    doi,
    pmid,
    pmcid,
    title,
    authors: metas("citation_author"),
    journal: meta("citation_journal_title", "prism.publicationName"),
    date: (meta("citation_publication_date", "citation_date", "citation_online_date", "dc.date") || "").replace(/\//g, "-"),
    abstract: meta("citation_abstract", "dc.description", "description"),
    url: location.href,
  };
  const pdfUrls = findPdfUrls();
  const pdfUrl = pdfUrls[0] || "";

  // ---- panel (shadow DOM keeps the page's CSS out) ----
  const host = document.createElement("div");
  host.id = "lunta-ext-root";
  const root = host.attachShadow({ mode: "open" });
  root.innerHTML = `
    <style>
      :host { all: initial; }
      .wrap { position: fixed; right: 16px; top: 38%; z-index: 2147483647; font: 13px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
      .tab { width: 44px; height: 44px; border-radius: 12px; background: #fff; box-shadow: 0 4px 18px rgba(16,40,50,.22);
             display: grid; place-items: center; cursor: pointer; position: relative; border: 0; padding: 0; }
      .tab .logo, .tab .logo svg { display: block; width: 30px; height: 30px; }
      .dot { position: absolute; right: -3px; top: -3px; width: 12px; height: 12px; border-radius: 50%; border: 2px solid #fff; background: #b7c2c6; }
      .dot.in { background: #2f9e6b; }
      .card { position: absolute; right: 54px; top: -10px; width: 248px; background: #fff; color: #1d1d1f; border-radius: 14px;
              box-shadow: 0 10px 34px rgba(16,40,50,.25); padding: 14px; display: none; }
      .card.open { display: block; }
      .brand { display: flex; align-items: center; gap: 8px; font-weight: 650; color: #173e4c; margin-bottom: 8px; }
      .brand .logo, .brand .logo svg { display: block; width: 20px; height: 20px; }
      .t { font-weight: 600; margin-bottom: 4px; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
      .sub { color: #6e6e73; font-size: 12px; margin-bottom: 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .row { display: flex; align-items: center; justify-content: space-between; padding: 7px 0; border-top: 1px solid #eee; gap: 8px; }
      .ok { color: #2f9e6b; font-weight: 600; }
      .muted { color: #8e8e93; }
      button.btn { all: unset; cursor: pointer; padding: 5px 11px; border-radius: 8px; font-weight: 600; background: #2a6478; color: #fff; }
      button.btn.alt { background: #f2ece4; color: #8a4b22; }
      button.btn[disabled] { opacity: .5; cursor: default; }
      .err { color: #b3261e; font-size: 12px; margin-top: 6px; }
      .ftmsg { color: #3a3a3c; font-size: 12px; line-height: 1.35; margin-top: 2px; }
      .ftmsg:empty { display: none; }
      .ft { flex: 1; } button.btn.wide { display: block; width: 100%; box-sizing: border-box; text-align: center; }
      .ftmsg.ok { color: #2f7d58; } .ftmsg.warn { color: #8a4b22; }
      a.open { color: #2a6478; font-weight: 600; text-decoration: none; }
      @media (prefers-color-scheme: dark) {
        .card, .tab { background: #2a2a2c; color: #eee; } .dot { border-color: #2a2a2c; } .row { border-color: #3a3a3c; }
        .brand { color: #9ec9d8; } button.btn.alt { background: #3d3127; color: #f2b45a; } a.open { color: #9ec9d8; }
        .ftmsg { color: #c7c7cc; } .ftmsg.ok { color: #7fd3a8; } .ftmsg.warn { color: #f2b45a; }
      }
    </style>
    <div class="wrap">
      <button class="tab" title="Lunta"><span class="logo"></span><span class="dot"></span></button>
      <div class="card">
        <div class="brand"><span class="logo"></span>Lunta</div>
        <div class="t"></div>
        <div class="sub"></div>
        <div class="row"><span>Library</span><span class="lib muted">Checking…</span></div>
        <div class="row"><span>PDF</span><span class="pdf muted">–</span></div>
        <div class="row"><span class="ft"></span></div>
        <div class="ftmsg"></div>
        <div class="err"></div>
      </div>
    </div>`;
  // the logo drawn inline: Safari can refuse to load the extension's own image files into some pages
  // (a broken-image box on bioRxiv), and an inline SVG loads nothing
  const logo = `<svg viewBox="0 0 48 48" aria-hidden="true"><rect width="48" height="48" rx="9" fill="#1f4f60"/>
    <path d="M14 8h15l7 7v25H14z" fill="#f7f4ec"/><path d="M29 8v7h7z" fill="#d9d4c7"/>
    <path d="M18 10v28" stroke="#d9822b" stroke-width="1.2"/><circle cx="18" cy="23" r="1.6" fill="#d9822b"/>
    <path d="M21 17h11M21 20h11M21 28h11M21 31h9" stroke="#9fb3bb" stroke-width="1.4"/>
    <rect x="20.5" y="22.6" width="12" height="2.8" rx=".6" fill="#f0a64a"/></svg>`;
  root.querySelectorAll(".logo").forEach((l) => (l.innerHTML = logo));
  const $ = (s) => root.querySelector(s);
  $(".t").textContent = title || doi;
  $(".sub").textContent = [info.journal, doi && "doi:" + doi].filter(Boolean).join(" · ");
  $(".tab").addEventListener("click", () => $(".card").classList.toggle("open"));
  document.documentElement.appendChild(host);

  let state = { inLibrary: false };
  let ftBusy = false;

  // When the extension is updated or restarted (e.g. Lunta for Mac was updated), the Lunta box already on
  // an open page is cut off from it: messages fail or never get an answer, and its icon no longer
  // loads. Say so instead of "Checking…" for ever.
  const STALE = "Lunta was updated or restarted since this page was opened. Reload the page.";
  const send = (msg) =>
    new Promise((resolve) => {
      let done = false;
      const finish = (r) => { if (!done) { done = true; resolve(r); } };
      // status is quick; the rest (saving, the full text) can take minutes
      if (msg.type === "status") setTimeout(() => finish({ error: STALE, stale: true }), 15000);
      try {
        if (!chrome.runtime || !chrome.runtime.id) return finish({ error: STALE, stale: true });
        chrome.runtime.sendMessage(msg, (r) => finish(r || { error: chrome.runtime.lastError?.message || STALE }));
      } catch (e) {
        finish({ error: STALE, stale: true });
      }
    });

  function render() {
    $(".dot").classList.toggle("in", !!state.inLibrary);
    const lib = $(".lib");
    lib.innerHTML = "";
    if (state.inLibrary) {
      const a = document.createElement("a");
      a.className = "open";
      a.href = "lunta://open?id=" + encodeURIComponent(state.id);
      a.textContent = "✓ In library · Open";
      // Mac, Windows, Linux: Lunta opens the paper itself (a lunta:// link makes the browser ask
      // on every site); iPhone and iPad: the link
      a.addEventListener("click", async (ev) => {
        ev.preventDefault();
        const r = await send({ type: "open", id: state.id });
        if (r && r.link) location.href = a.href;
        else if (r && r.error) showError(r.error);
      });
      lib.className = "lib";
      lib.appendChild(a);
    } else if (state.pendingSave || state.pendingFullText) {
      // Safari on iPhone and iPad: sent, and the Lunta app adds it when it's opened
      lib.className = "lib";
      lib.appendChild(openLunta("✓ Sent · Open Lunta"));
    } else {
      const b = document.createElement("button");
      b.className = "btn";
      b.textContent = "Save to Lunta";
      b.onclick = save;
      lib.className = "lib";
      lib.appendChild(b);
    }
    const pdf = $(".pdf");
    pdf.innerHTML = "";
    if (state.hasPdf) {
      pdf.className = "pdf ok";
      pdf.textContent = "✓ Saved";
    } else if (state.pendingPdf) {
      pdf.className = "pdf ok";
      pdf.textContent = "✓ Sent";
    } else if (pdfUrl) {
      const b = document.createElement("button");
      b.className = "btn alt";
      b.textContent = "Save PDF";
      b.onclick = savePdf;
      pdf.className = "pdf";
      pdf.appendChild(b);
    } else if (state.pdfSearched) {
      // no free copy: the journal's page, where the PDF comes with the reader's own access
      pdf.className = "pdf muted";
      pdf.textContent = "No free PDF";
      if (journalUrl) {
        const a = document.createElement("a");
        a.className = "open"; a.href = journalUrl; a.target = "_blank"; a.rel = "noopener";
        a.textContent = "Journal ↗"; a.title = "Open the article at the journal; the PDF can be saved from there with your access";
        pdf.append(" · ", a);
      }
    } else {
      // no PDF link here (PubMed never has one): Lunta looks for a legal open-access copy
      const b = document.createElement("button");
      b.className = "btn alt";
      b.textContent = "Find PDF";
      b.title = "Look for a free, legal copy (PubMed Central, Europe PMC, the publisher's open-access version)";
      b.onclick = findPdf;
      pdf.className = "pdf";
      pdf.appendChild(b);
    }
    const ft = $(".ft");
    ft.innerHTML = "";
    const b = document.createElement("button");
    b.className = "btn alt wide";
    b.textContent = ftBusy ? "Working…" : "Use this page's full text";
    b.title = "Make the paper's reading view from this page as you see it (with your access), figures included";
    b.disabled = ftBusy;
    b.onclick = useFullText;
    ft.appendChild(b);
  }

  // Safari on iPhone and iPad: the work is done in the Lunta app, which this opens
  function openLunta(text) {
    const a = document.createElement("a");
    a.className = "open";
    a.href = "lunta://safari";
    a.textContent = text;
    return a;
  }

  function showError(e) {
    $(".err").textContent = e || "";
  }

  async function refresh() {
    const r = await send({ type: "status", doi, title });
    if (r.error) { $(".lib").textContent = r.stale ? "Reload the page" : "App not running"; showError(r.error); return; }
    state = r;
    showError("");
    render();
  }

  async function save() {
    const b = $(".lib button");
    if (b) { b.disabled = true; b.textContent = "Saving…"; }
    // without a PDF link on the page, Lunta looks for a free copy as it saves
    const r = await send({ type: "save", meta: pdfUrl ? info : { ...info, findPdf: true } });
    if (r.error) { showError(r.error); render(); return; }
    state = { ...r, pdfSearched: !pdfUrl && !r.hasPdf && !r.queued };
    render();
    if (!state.hasPdf && pdfUrl) savePdf(); // grab the PDF too while we have access
  }

  async function findPdf() {
    const b = $(".pdf button");
    if (b) { b.disabled = true; b.textContent = "Looking…"; }
    const r = state.inLibrary && state.id
      ? await send({ type: "findPdf", id: state.id, meta: info })
      : await send({ type: "save", meta: { ...info, findPdf: true } });
    if (r.error) { showError(r.error); render(); return; }
    state = { ...r, pdfSearched: !r.hasPdf && !r.queued };
    showError("");
    render();
  }

  function toBase64(buf) {
    let s = "";
    const bytes = new Uint8Array(buf);
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(s);
  }

  async function savePdf() {
    const b = $(".pdf button");
    if (b) { b.disabled = true; b.textContent = "Downloading…"; }
    if (!state.inLibrary && !state.pendingSave) {
      const s = await send({ type: "save", meta: info });
      if (s.error) { showError(s.error); render(); return; }
      state = s;
    }
    let msg = { type: "savePdf", id: state.id, pdfUrl, pdfUrls, name: (title || "paper").slice(0, 80) + ".pdf", meta: info };
    // fetch from the page first: same-origin requests carry the site's login cookies; each link in turn
    const tried = [];   // what each link gave, for the error message
    const short = (u) => { try { const x = new URL(u); return x.pathname.split("/").pop() || x.host; } catch (_) { return u; } };
    for (const u of pdfUrls) {
      try {
        const r = await pageFetch(u, { credentials: "include" });
        const buf = await r.arrayBuffer();
        const head = new Uint8Array(buf.slice(0, 4));
        if (r.ok && head[0] === 0x25 && head[1] === 0x50 && head[2] === 0x44 && head[3] === 0x46) {
          msg = { ...msg, base64: toBase64(buf), pdfUrl: undefined, pdfUrls: undefined };
          break;
        }
        tried.push(`${short(u)}: ${r.status} ${(r.headers.get("content-type") || "").split(";")[0] || "?"}${r.redirected ? " via " + short(r.url) : ""}`);
      } catch (e) { tried.push(`${short(u)}: ${e && e.message || "failed"}`); /* cross-origin: the extension tries it */ }
    }
    const r = await send(msg);
    if (r.error) showError(r.error + (tried.length ? " Tried: " + tried.join("; ") + "." : (pdfUrls.length ? "" : " No PDF link found on this page."))); else { state = r; showError(""); }
    render();
  }

  // ---- "Use this page's full text" ----

  function ftMessage(text, kind) {
    const el = $(".ftmsg");
    el.textContent = text || "";
    el.className = "ftmsg" + (kind ? " " + kind : "");
  }

  // words of paragraph text in a document (to tell a full-text page from an abstract page)
  function paragraphWords(doc) {
    let n = 0;
    doc.querySelectorAll("p").forEach((p) => {
      if (p.closest("#lunta-ext-root, nav, header, footer, aside")) return;
      n += (p.textContent || "").split(/\s+/).filter(Boolean).length;
    });
    return n;
  }

  // The page as the server delivered it (before MathJax and other scripts rewrote it): fetched
  // again with the browser's cookies, so the same access applies. Used when it carries the full
  // text the reader is looking at; otherwise the page as shown.
  async function pageHTML() {
    const live = "<!DOCTYPE html>\n" + document.documentElement.outerHTML.replace(/<div id="lunta-ext-root"><\/div>/, "");
    try {
      const r = await pageFetch(location.href, { credentials: "include" });
      const type = r.headers.get("content-type") || "";
      if (r.ok && /html/i.test(type)) {
        const text = await r.text();
        const fetched = new DOMParser().parseFromString(text, "text/html");
        const fw = paragraphWords(fetched), lw = paragraphWords(document);
        if (fw >= 300 && fw >= lw * 0.6) return { html: text, liveHtml: live, source: "as delivered" };
      }
    } catch (_) { /* fall back to the page as shown */ }
    return { html: live, liveHtml: null, source: "as shown" };
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

  // Figure (and equation) images on the page: every address the page uses for each, and the
  // one to download (the high-resolution link when there is one, e.g. ScienceDirect's "_lrg").
  function figureImages() {
    const holders = new Set();
    document.querySelectorAll('figure, [class*="figure"], [class*="Figure"], [id^="fig"], [id^="Fig"], [class*="formula"], [class*="equation"], .fig, .table-wrap')
      .forEach((el) => { if (!el.closest("#lunta-ext-root, nav, header, footer") && el.querySelector("img")) holders.add(el); });
    const out = [], seen = new Set();
    for (const h of holders) {
      // innermost holders only (a figure inside a figure-ish wrapper counts once)
      if ([...holders].some((o) => o !== h && h.contains(o))) continue;
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
        if (!fetchUrl || seen.has(fetchUrl)) return;
        seen.add(fetchUrl);
        out.push({ urls: list, fetch: fetchUrl });
      });
    }
    return out;
  }

  // Tables the page only links to (Springer Nature: "Full size table" → /articles/…/tables/1):
  // their own pages, fetched with the reader's access (same site), so the reading view has them.
  async function linkedTablePages() {
    const links = new Map();
    document.querySelectorAll('a[data-test="table-link"][href]').forEach((a) => {
      if (!a.closest("#lunta-ext-root")) links.set(a.href, a.getAttribute("href"));
    });
    const out = [];
    for (const [u, raw] of [...links].slice(0, 30)) {
      try {
        const r = await pageFetch(u, { credentials: "include" });
        if (!r.ok || !/html/i.test(r.headers.get("content-type") || "")) continue;
        const text = await r.text();
        if (!/<table[\s>]/i.test(text)) continue;
        out.push({ urls: [...new Set([u, raw])], base64: toBase64(new TextEncoder().encode(text).buffer) });
      } catch (_) { /* that table stays a link */ }
    }
    return out;
  }

  async function useFullText() {
    if (ftBusy) return;
    ftBusy = true;
    showError("");
    render();
    try {
      ftMessage("Reading the page…");
      const page = await pageHTML();
      const images = figureImages();
      let pages = [];
      if (document.querySelector('a[data-test="table-link"]')) {
        ftMessage("Fetching the tables…");
        pages = await linkedTablePages();
      }
      ftMessage(images.length ? `Fetching ${images.length} figure image${images.length === 1 ? "" : "s"}…` : "Sending the page to Lunta…");
      const r = await send({ type: "fullText", url: location.href, doi, title, html: page.html, liveHtml: page.liveHtml, images, pages });
      if (r.error) {
        ftMessage("");
        showError(r.error);
      } else {
        const kind = r.outcome === "kept" ? "warn" : "ok";
        const head = { added: "Added to Lunta.", attached: "Reading view made.", replaced: "Reading view updated.",
                       kept: "Reading view not changed.", unchanged: "Reading view already up to date." }[r.outcome] || "";
        ftMessage(`${head} ${r.message || ""}`.trim(), kind);
        if (r.queued) { $(".ftmsg").append(" ", openLunta("Open Lunta")); }
        if (r.inLibrary || r.queued) { state = r; }
      }
    } catch (e) {
      ftMessage("");
      showError(String(e && e.message || e));
    }
    ftBusy = false;
    render();
  }

  chrome.runtime.onMessage.addListener((m, _sender, sendResponse) => {
    if (m.type === "togglePanel") $(".card").classList.toggle("open");
    if (m.type === "fullTextProgress") ftMessage(m.text);
    if (m.type === "fetchImage") {
      // a figure the extension couldn't fetch itself: from the page (same-origin cookies)
      pageFetch(m.url, { credentials: "include" }).then(async (r) => {
        if (!r.ok) return sendResponse({ error: `HTTP ${r.status}` });
        const buf = await r.arrayBuffer();
        sendResponse({ base64: toBase64(buf), type: r.headers.get("content-type") || "" });
      }).catch((e) => sendResponse({ error: String(e) }));
      return true;
    }
  });

  refresh();
})();
