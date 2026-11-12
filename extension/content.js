// Lunta extension: finds a paper's metadata on article pages and shows a small panel to save it.
(() => {
  if (window.top !== window || document.getElementById("lunta-ext-root")) return;

  const meta = (...names) => {
    for (const n of names) {
      const el = document.querySelector(`meta[name="${n}" i], meta[property="${n}" i]`);
      if (el && el.content && el.content.trim()) return el.content.trim();
    }
    return "";
  };
  const metas = (n) => [...document.querySelectorAll(`meta[name="${n}" i]`)].map((m) => m.content.trim()).filter(Boolean);
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

  function findPdfUrl() {
    const m = meta("citation_pdf_url");
    if (m) return new URL(m, location.href).href;
    const link = document.querySelector('link[type="application/pdf"]');
    if (link && link.href) return link.href;
    const a = [...document.querySelectorAll("a[href]")].find((x) =>
      /\.pdf($|[?#])|\/pdf(\/|$|\?)|\/epdf\//i.test(x.getAttribute("href")) && x.offsetParent !== null);
    return a ? a.href : "";
  }

  const doi = findDoi();
  const title = meta("citation_title", "dc.title", "og:title") || "";
  if (!doi && !meta("citation_title")) return; // not an article page

  const info = {
    doi,
    title,
    authors: metas("citation_author"),
    journal: meta("citation_journal_title", "prism.publicationName"),
    date: (meta("citation_publication_date", "citation_date", "citation_online_date", "dc.date") || "").replace(/\//g, "-"),
    abstract: meta("citation_abstract", "dc.description", "description"),
    url: location.href,
  };
  const pdfUrl = findPdfUrl();

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
      .tab img { width: 30px; height: 30px; border-radius: 7px; }
      .dot { position: absolute; right: -3px; top: -3px; width: 12px; height: 12px; border-radius: 50%; border: 2px solid #fff; background: #b7c2c6; }
      .dot.in { background: #2f9e6b; }
      .card { position: absolute; right: 54px; top: -10px; width: 248px; background: #fff; color: #1d1d1f; border-radius: 14px;
              box-shadow: 0 10px 34px rgba(16,40,50,.25); padding: 14px; display: none; }
      .card.open { display: block; }
      .brand { display: flex; align-items: center; gap: 8px; font-weight: 650; color: #173e4c; margin-bottom: 8px; }
      .brand img { width: 20px; height: 20px; border-radius: 5px; }
      .t { font-weight: 600; margin-bottom: 4px; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
      .sub { color: #6e6e73; font-size: 12px; margin-bottom: 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .row { display: flex; align-items: center; justify-content: space-between; padding: 7px 0; border-top: 1px solid #eee; gap: 8px; }
      .ok { color: #2f9e6b; font-weight: 600; }
      .muted { color: #8e8e93; }
      button.btn { all: unset; cursor: pointer; padding: 5px 11px; border-radius: 8px; font-weight: 600; background: #2a6478; color: #fff; }
      button.btn.alt { background: #f2ece4; color: #8a4b22; }
      button.btn[disabled] { opacity: .5; cursor: default; }
      .err { color: #b3261e; font-size: 12px; margin-top: 6px; }
      a.open { color: #2a6478; font-weight: 600; text-decoration: none; }
      @media (prefers-color-scheme: dark) {
        .card, .tab { background: #2a2a2c; color: #eee; } .dot { border-color: #2a2a2c; } .row { border-color: #3a3a3c; }
        .brand { color: #9ec9d8; } button.btn.alt { background: #3d3127; color: #f2b45a; } a.open { color: #9ec9d8; }
      }
    </style>
    <div class="wrap">
      <button class="tab" title="Lunta"><img alt=""><span class="dot"></span></button>
      <div class="card">
        <div class="brand"><img alt="">Lunta</div>
        <div class="t"></div>
        <div class="sub"></div>
        <div class="row"><span>Library</span><span class="lib muted">Checking…</span></div>
        <div class="row"><span>PDF</span><span class="pdf muted">–</span></div>
        <div class="err"></div>
      </div>
    </div>`;
  const icon = chrome.runtime.getURL("icons/icon-48.png");
  root.querySelectorAll("img").forEach((i) => (i.src = icon));
  const $ = (s) => root.querySelector(s);
  $(".t").textContent = title || doi;
  $(".sub").textContent = [info.journal, doi && "doi:" + doi].filter(Boolean).join(" · ");
  $(".tab").addEventListener("click", () => $(".card").classList.toggle("open"));
  document.documentElement.appendChild(host);

  let state = { inLibrary: false };

  const send = (msg) =>
    new Promise((resolve) => chrome.runtime.sendMessage(msg, (r) => resolve(r || { error: chrome.runtime.lastError?.message })));

  function render() {
    $(".dot").classList.toggle("in", !!state.inLibrary);
    const lib = $(".lib");
    lib.innerHTML = "";
    if (state.inLibrary) {
      const a = document.createElement("a");
      a.className = "open";
      a.href = "lunta://open?id=" + encodeURIComponent(state.id);
      a.textContent = "✓ In library · Open";
      lib.className = "lib";
      lib.appendChild(a);
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
    } else if (pdfUrl) {
      const b = document.createElement("button");
      b.className = "btn alt";
      b.textContent = "Save PDF";
      b.onclick = savePdf;
      pdf.className = "pdf";
      pdf.appendChild(b);
    } else {
      pdf.className = "pdf muted";
      pdf.textContent = "No PDF link on this page";
    }
  }

  function showError(e) {
    $(".err").textContent = e || "";
  }

  async function refresh() {
    const r = await send({ type: "status", doi, title });
    if (r.error) { $(".lib").textContent = "App not running"; showError(r.error); return; }
    state = r;
    showError("");
    render();
  }

  async function save() {
    const b = $(".lib button");
    if (b) { b.disabled = true; b.textContent = "Saving…"; }
    const r = await send({ type: "save", meta: info });
    if (r.error) { showError(r.error); render(); return; }
    state = r;
    render();
    if (!state.hasPdf && pdfUrl) savePdf(); // grab the PDF too while we have access
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
    if (!state.inLibrary) {
      const s = await send({ type: "save", meta: info });
      if (s.error) { showError(s.error); render(); return; }
      state = s;
    }
    let msg = { type: "savePdf", id: state.id, pdfUrl, name: (title || "paper").slice(0, 80) + ".pdf" };
    try {
      // fetch from the page first: same-origin requests carry the site's login cookies
      const r = await fetch(pdfUrl, { credentials: "include" });
      const buf = await r.arrayBuffer();
      const head = new Uint8Array(buf.slice(0, 4));
      if (r.ok && head[0] === 0x25 && head[1] === 0x50 && head[2] === 0x44 && head[3] === 0x46) {
        msg = { ...msg, base64: toBase64(buf), pdfUrl: undefined };
      }
    } catch (_) { /* cross-origin: let the extension fetch it */ }
    const r = await send(msg);
    if (r.error) showError(r.error); else { state = r; showError(""); }
    render();
  }

  chrome.runtime.onMessage.addListener((m) => {
    if (m.type === "togglePanel") $(".card").classList.toggle("open");
  });

  refresh();
})();
