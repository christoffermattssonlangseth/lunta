// Lunta in Google Docs: a button at the right edge of a document opens a small panel to search the
// Lunta library, insert citations and the bibliography, update them all, and pick the style. The
// document is read and written by the background script through the Google Docs API
// (gdocs/background-gdocs.js; in Safari, by Lunta for Mac); this page script only shows the panel
// and, to insert at the cursor (which the API doesn't know), pastes a placeholder through Docs' own
// paste handling (Safari: puts it on the clipboard for the reader's ⌘V, gdocs/clipboard.js).
/* global LuntaCite, LuntaClipboard, LuntaResolve */
(() => {
  if (window.top !== window || document.getElementById("lunta-gdocs-root")) return;
  const docId = () => (location.pathname.match(/\/document\/(?:u\/\d+\/)?d\/([A-Za-z0-9_-]{20,})/) || [])[1] || "";
  if (!docId()) return;
  const FIREFOX = /Firefox\//.test(navigator.userAgent);

  // journal styles used before: in the extension's storage, not Google's localStorage
  const journals = { data: {} };
  LuntaCite.useStorage({ getItem: (k) => journals.data[k] || null, setItem: (k, v) => { journals.data[k] = v; chrome.storage.local.set({ [k]: v }).catch(() => {}); } });
  chrome.storage.local.get("luntaJournals").then((r) => { if (r && r.luntaJournals) journals.data.luntaJournals = r.luntaJournals; }).catch(() => {});

  const host = document.createElement("div");
  host.id = "lunta-gdocs-root";
  // typing in the panel is for the panel, not Docs' shortcuts
  ["keydown", "keyup", "keypress"].forEach((t) => host.addEventListener(t, (e) => e.stopPropagation()));
  const root = host.attachShadow({ mode: "open" });
  root.innerHTML = `
    <style>
      :host { all: initial; }
      .wrap { position: fixed; right: 72px; bottom: 96px; z-index: 2147483000; font: 13px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; color: #1d1d1f; }
      .tab { width: 40px; height: 40px; border-radius: 12px; background: #fff; box-shadow: 0 3px 14px rgba(16,40,50,.25); border: 0; padding: 0; cursor: pointer; display: grid; place-items: center; }
      .tab .logo, .tab .logo svg { display: block; width: 26px; height: 26px; }
      .card { position: absolute; right: 0; bottom: 50px; width: 330px; max-height: min(620px, calc(100vh - 170px)); overflow: auto; background: #fff;
              border-radius: 14px; box-shadow: 0 10px 34px rgba(16,40,50,.28); padding: 12px; display: none; box-sizing: border-box; }
      .card.open { display: block; }
      header { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
      header .logo, header .logo svg { display: block; width: 20px; height: 20px; }
      header strong { color: #173e4c; font-weight: 650; flex: 1; }
      select, input { font: inherit; box-sizing: border-box; border: 1px solid #c9ced2; border-radius: 7px; padding: 5px 7px; background: #fff; color: inherit; }
      select { max-width: 190px; }
      input[type=search] { width: 100%; margin: 4px 0 6px; }
      .results { max-height: 230px; overflow: auto; }
      .hit { padding: 6px 7px; border-radius: 7px; cursor: pointer; border: 1px solid transparent; }
      .hit:hover { background: #f2f5f6; }
      .hit.on { background: #e3f0f4; border-color: #9cc6d4; }
      .hit .t { font-weight: 600; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
      .hit .m { color: #6e6e73; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .picked { color: #2a6478; font-size: 12px; margin: 6px 0; }
      .picked:empty { display: none; }
      .buttons { display: flex; gap: 6px; margin-top: 6px; }
      button.btn { flex: 1; font: inherit; font-weight: 600; cursor: pointer; padding: 6px 8px; border-radius: 8px; border: 0; background: #f2ece4; color: #8a4b22; }
      button.btn.primary { background: #2a6478; color: #fff; }
      button.btn[disabled] { opacity: .45; cursor: default; }
      button.link { all: unset; cursor: pointer; color: #2a6478; font-size: 12px; text-decoration: underline; }
      .status { color: #3a3a3c; font-size: 12px; margin: 8px 0 0; white-space: pre-wrap; }
      .status.err { color: #b3261e; }
      .note { color: #6e6e73; font-size: 12px; margin: 6px 0; }
      .box { border-top: 1px solid #eee; margin-top: 8px; padding-top: 8px; }
      .row { display: flex; gap: 6px; align-items: center; }
      .row strong { flex: 1; color: #2a6478; font-size: 12px; text-transform: uppercase; letter-spacing: .4px; }
      .crow { display: flex; gap: 6px; align-items: flex-start; padding: 5px 0; border-top: 1px solid #eee; }
      .crow:first-child { border-top: 0; }
      .crow .ct { flex: 1; min-width: 0; }
      .crow .t { font-weight: 600; font-size: 12.5px; }
      .crow .m { color: #6e6e73; font-size: 11.5px; }
      .crow.pick { cursor: pointer; border-radius: 6px; padding: 5px 6px; }
      .crow.pick:hover { background: #f2f5f6; }
      button.cx { all: unset; cursor: pointer; width: 22px; height: 22px; line-height: 22px; text-align: center; border-radius: 50%; opacity: .5; font-size: 12px; flex: 0 0 auto; }
      button.cx:hover { opacity: 1; background: #eee; }
      .row input { flex: 1; margin: 0; }
      .pasteBox p { margin: 4px 0; }
      .rs-ph { border-top: 1px solid #eee; padding: 8px 0 4px; }
      .rs-ph:first-child { border-top: 0; }
      .rs-text { font-weight: 600; font-family: ui-monospace, Menlo, monospace; font-size: 12px; }
      .rs-label { font-size: 12px; color: #2a6478; margin: 6px 0 2px; }
      .rs-note, .rs-state, .rs-chosen { font-size: 12px; color: #6e6e73; margin: 4px 0; }
      .rs-state.err { color: #b3261e; }
      .rs-done .rs-state { color: #2a6478; }
      .rs-q { flex: 1; min-width: 0; }
      .rlist .hit .m.rs-src { font-size: 11px; }
      .crow .o { color: #2a6478; font-size: 11.5px; }
      .crow .o:empty { display: none; }
      button.co { all: unset; cursor: pointer; font-size: 11.5px; color: #2a6478; padding: 2px 4px; border-radius: 5px; flex: 0 0 auto; }
      button.co:hover, button.co:focus { background: #eef4f6; outline: none; }
      .opts { border: 1px solid #dde3e6; border-radius: 8px; padding: 6px; margin: 2px 0 8px; }
      .orow { display: flex; gap: 5px; margin: 4px 0; }
      .orow input, .orow select { flex: 1; min-width: 0; }
      .orow select.olabel { flex: 0 0 auto; }
      .cv-sum { font-weight: 600; margin: 4px 0; }
      .cv-h { font-size: 12px; color: #2a6478; margin: 8px 0 2px; }
      .cv-row { display: flex; gap: 6px; align-items: flex-start; font-size: 12px; padding: 3px 0; }
      .cv-note { font-size: 12px; color: #6e6e73; margin: 6px 0; }
      .cl-sum { font-weight: 600; margin: 4px 0; }
      .cl-note { font-size: 11.5px; opacity: .75; margin: 4px 0; }
      .cl-item { border-top: 1px solid #8883; padding: 8px 0 6px; }
      .cl-item.cl-right .cl-s, .cl-item.cl-dismissed .cl-s { opacity: .6; }
      .cl-cite { font-size: 11px; color: #2a6478; }
      .cl-s { font-size: 12.5px; margin: 2px 0 4px; }
      .cl-r { margin: 4px 0 4px 6px; }
      .cl-rh { display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; }
      .cl-badge { font-size: 11px; font-weight: 600; color: #fff; border-radius: 9px; padding: 1px 7px; white-space: nowrap; }
      .cl-ok { background: #2e7d4f; } .cl-part { background: #c77c12; } .cl-bad { background: #b3261e; }
      .cl-none { background: #7a7a80; } .cl-maybe { background: #2a6478; }
      .cl-paper { font-size: 11.5px; opacity: .8; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
      .cl-by, .cl-rnote { font-size: 11.5px; opacity: .75; margin: 3px 0; }
      .cl-err { font-size: 12px; color: #b3261e; }
      a.cl-q { display: block; font-size: 12px; color: #2a6478; margin: 3px 0; text-decoration: none; cursor: pointer; }
      a.cl-q:hover { text-decoration: underline; }
      .cl-ps summary { font-size: 11.5px; cursor: pointer; color: #2a6478; margin: 3px 0; }
      a.cl-p { display: block; font-size: 11.5px; color: inherit; text-decoration: none; border-left: 2px solid #9cc6d4; padding: 2px 0 2px 6px; margin: 4px 0; cursor: pointer; }
      a.cl-p:hover { background: #8881; }
      .cl-pm { display: block; font-size: 10.5px; opacity: .7; }
      .cl-btns { display: flex; gap: 6px; align-items: center; margin-top: 4px; }
      .cl-btns .btn { flex: 0 0 auto; font-size: 11.5px; padding: 3px 8px; }
      .cl-state { font-size: 11.5px; color: #2e7d4f; }
      .cl-dismissed .cl-state { color: inherit; opacity: .7; }
      [hidden] { display: none !important; }
      @media (prefers-color-scheme: dark) {
        .card, .tab { background: #2a2a2c; color: #eee; } header strong { color: #9ec9d8; }
        select, input { background: #1f1f21; border-color: #48484a; }
        .hit:hover { background: #343437; } .hit.on { background: #233b44; border-color: #3f7486; } .hit .m, .note { color: #a1a1a6; }
        .status { color: #c7c7cc; } .status.err { color: #ff8a80; } button.btn { background: #3d3127; color: #f2b45a; }
        .picked, button.link { color: #9ec9d8; } .box { border-color: #3a3a3c; }
      }
    </style>
    <div class="wrap">
      <div class="card">
        <header><span class="logo"></span><strong>Lunta</strong><select class="style" title="Citation style" disabled></select></header>
        <div class="setup" hidden><p class="note setupText"></p></div>
        <div class="connect" hidden>
          <p class="note connectText">Lunta writes citations into this document through Google Docs, with your permission (asked once). Lunta reaches only the documents you give it in Google’s window, one by one.</p>
          <div class="buttons"><button class="btn primary connectBtn">Connect Google account</button></div>
        </div>
        <div class="grant" hidden>
          <p class="note grantText">Google lets Lunta open only the documents you give it. Click below, then click Select in Google’s window: the document is chosen already. Once per document.</p>
          <div class="buttons"><button class="btn primary grantBtn">Give Lunta access to this document</button></div>
          <p class="note"><button class="link grantAccount">Switch Google account</button></p>
        </div>
        <div class="main" hidden>
          <div class="journal box" hidden>
            <div class="row"><input class="jq" type="search" placeholder="Journal name, e.g. Neuron, eLife" autocomplete="off"><button class="link jclose">Close</button></div>
            <div class="jresults results"></div>
          </div>
          <input class="q" type="search" placeholder="Search your library…" autocomplete="off">
          <div class="results hits"></div>
          <div class="picked"></div>
          <div class="buttons"><button class="btn primary insert" disabled>Insert Citation</button></div>
          <div class="buttons"><button class="btn bib" disabled>Insert Bibliography</button><button class="btn refresh" disabled>Update All</button></div>
          <div class="buttons"><button class="btn edit" disabled>Edit Citation…</button><button class="btn resolve" disabled title="Turn placeholders typed while writing, like (Langseth et al. 2025) or [Smith 2019 cortex], into citations: you confirm each one">Resolve Citations…</button></div>
          <div class="buttons"><button class="btn convert" disabled title="Make citations inserted with Zotero Lunta citations: you review the papers first, nothing changes until you click Convert">Convert Citations from Zotero…</button></div>
          <div class="buttons"><button class="btn plain" disabled title="For the version you submit: every Lunta citation and the bibliography become ordinary text, as they look now. Do it on a copy of the document.">Convert Citations to Plain Text…</button></div>
          <div class="plainbox box" hidden>
            <div class="row"><strong>Convert Citations to Plain Text</strong><button class="link plainclose">Close</button></div>
            <p class="note plaintext"></p>
            <div class="buttons"><button class="btn primary plaingo">Convert to Plain Text</button><button class="btn plaincancel">Cancel</button></div>
          </div>
          <div class="cvbox box" hidden>
            <div class="row"><strong>Convert Citations</strong><button class="link cvclose">Close</button></div>
            <div class="cvlist"></div>
          </div>
          <div class="buttons"><button class="btn claims" disabled title="Does each cited paper plausibly support its sentence? Lunta shows the best-matching passages (and, with an on-device model, a suggested verdict). Nothing in the document changes.">Check Citations…</button></div>
          <div class="clbox box" hidden>
            <div class="row"><strong>Check Citations</strong><button class="link clstop" hidden>Stop</button><button class="link clclose">Close</button></div>
            <div class="cllist"></div>
          </div>
          <div class="rbox box" hidden>
            <div class="row"><strong>Resolve Citations</strong><button class="link rclose">Close</button></div>
            <p class="note">Lunta proposes, you decide: each placeholder becomes a citation only when you confirm it. Papers not in your library are added when confirmed.</p>
            <div class="rlist"></div>
          </div>
          <div class="pasteBox box" hidden>
            <p class="pasteText"><strong>Press ⌘V in the document where the citation goes.</strong></p>
            <p class="note">Lunta put it on the clipboard (Safari doesn’t let extensions paste). It becomes the citation once it’s in the document. <button class="link pasteCancel">Cancel</button></p>
          </div>
          <div class="cbox box" hidden>
            <div class="row"><strong class="ctitle">This citation</strong><button class="link cclose">Close</button></div>
            <div class="clist"></div>
            <div class="buttons cadd-row"><button class="btn primary cadd" disabled>Add Selected Papers</button></div>
            <p class="note chint"></p>
          </div>
          <p class="note">Click in the document where the citation goes first. <button class="link account">Switch Google account</button></p>
        </div>
        <p class="status"></p>
      </div>
      <button class="tab" title="Lunta: cite from your library" aria-label="Lunta"><span class="logo"></span></button>
    </div>`;
  const $ = (s) => root.querySelector(s);
  // the logo drawn inline (as in content.js): Safari can refuse to load the extension's image files into pages
  const logo = `<svg viewBox="0 0 48 48" aria-hidden="true"><rect width="48" height="48" rx="9" fill="#1f4f60"/>
    <path d="M14 8h15l7 7v25H14z" fill="#f7f4ec"/><path d="M29 8v7h7z" fill="#d9d4c7"/>
    <path d="M18 10v28" stroke="#d9822b" stroke-width="1.2"/><circle cx="18" cy="23" r="1.6" fill="#d9822b"/>
    <path d="M21 17h11M21 20h11M21 28h11M21 31h9" stroke="#9fb3bb" stroke-width="1.4"/>
    <rect x="20.5" y="22.6" width="12" height="2.8" rx=".6" fill="#f0a64a"/></svg>`;
  root.querySelectorAll(".logo").forEach((l) => (l.innerHTML = logo));
  document.documentElement.appendChild(host);

  const send = (msg) => new Promise((resolve) => {
    try {
      chrome.runtime.sendMessage({ type: "gdocs", ...msg }, (r) => resolve(r || { error: (chrome.runtime.lastError && chrome.runtime.lastError.message) || "The extension didn’t answer. Reload the page." }));
    } catch (e) {
      resolve({ error: "The Lunta extension was updated or restarted: reload this page." });
    }
  });

  let current = null;   // the citation being edited: {index, ids, text, opts}
  let editingOpts = null;   // the paper whose options are open
  let picked = [], style = "apa", styleTitle = "", styleSaved = false, needsStyle = false, ready = false, busy = false, lastSearch = 0, opened = false;
  let viaApp = false;   // Safari on the Mac: Lunta for Mac signs in to Google and edits the document
  let luntaApi = 0;     // the citation API's version (3: Resolve Citations)
  let checking = false, stopChecking = false, claimItems = [];   // Check Citations
  let pasting = null;   // Safari, waiting for the reader's ⌘V: {cancelled}

  function status(m, err) { const s = $(".status"); s.textContent = m || ""; s.className = "status" + (err ? " err" : ""); }
  function setButtons() {
    const can = ready && !busy;
    $(".insert").disabled = !can || !picked.length || needsStyle;
    $(".bib").disabled = $(".refresh").disabled = !can || needsStyle;
    $(".style").disabled = !can;
    $(".edit").disabled = !can || needsStyle;
    $(".resolve").disabled = !can || needsStyle;
    $(".convert").disabled = !can || needsStyle;
    $(".plain").disabled = $(".plaingo").disabled = !can || needsStyle;
    $(".claims").disabled = !ready || checking;
    $(".cadd").disabled = !can || !current || !picked.length;
  }
  function show(which) {
    ["setup", "connect", "grant", "main"].forEach((k) => { $("." + k).hidden = k !== which; });
  }

  let styleAsked = null;   // the menu's value last acted on (see styleChosen)
  function fillStyles() {
    const sel = $(".style");
    LuntaCite.fillStyles(sel, style, styleTitle);
    if (needsStyle) {
      const o = document.createElement("option");
      o.value = ""; o.textContent = "Choose the style…";
      sel.insertBefore(o, sel.firstChild);
      sel.value = "";
    }
    styleAsked = sel.value;   // what the menu shows now isn't a new choice
  }

  /** Runs one change at a time. */
  async function exclusive(fn) {
    if (busy || !ready) return;
    busy = true; setButtons();
    try { await fn(); } catch (e) { status(String(e.message || e), true); }
    busy = false; setButtons();
  }

  function check(r) {
    if (r && r.error) {
      if (r.kind === "auth") { ready = false; show("connect"); }
      if (r.kind === "access") { ready = false; show("grant"); }
      const e = new Error(r.error); e.kind = r.kind; throw e;
    }
    return r;
  }

  function renderPicked() {
    $(".picked").textContent = picked.length ? "Citing: " + LuntaCite.pickedLabel(picked) : "";
    root.querySelectorAll(".hits .hit").forEach((el) => el.classList.toggle("on", picked.some((p) => p.id === el.dataset.id)));
    setButtons();
  }

  async function search(q) {
    const box = $(".hits");
    if (!q.trim()) { box.innerHTML = ""; return; }
    const mine = ++lastSearch;
    const r = await send({ op: "search", q });
    if (mine !== lastSearch) return;
    if (r.error) { box.innerHTML = ""; status(r.error, true); return; }
    box.innerHTML = "";
    if (!r.hits.length) box.innerHTML = '<p class="note">No papers match.</p>';
    r.hits.forEach((h) => {
      const el = document.createElement("div");
      el.className = "hit"; el.dataset.id = h.id;
      el.innerHTML = '<div class="t"></div><div class="m"></div>';
      el.querySelector(".t").textContent = h.title || "Untitled";
      el.querySelector(".m").textContent = LuntaCite.hitMeta(h);
      el.onclick = () => {
        const i = picked.findIndex((p) => p.id === h.id);
        if (i >= 0) picked.splice(i, 1); else picked.push(h);
        renderPicked();
      };
      box.appendChild(el);
    });
    renderPicked();
  }

  // ---- inserting at the cursor: Docs' own paste ----

  const delay = (ms) => new Promise((ok) => setTimeout(ok, ms));
  function editor() {
    const frame = document.querySelector(".docs-texteventtarget-iframe");
    const doc = frame && frame.contentDocument;
    const target = doc && (doc.querySelector("[contenteditable]") || doc.body);
    if (!target) throw new Error("Google Docs’ editor wasn’t found on this page (Google may have changed it). Meanwhile, cite with the Apps Script sidebar: Lunta › Citations in Word & Google Docs….");
    return { frame, doc, target };
  }
  /** The text selected in the document (Docs puts it in its input target on a copy). */
  function selectedText(ed) {
    ed.target.innerHTML = "";
    ed.target.dispatchEvent(new CustomEvent("copy"));
    return (ed.doc.body.textContent || "").replace(/[​-‍﻿]/g, "");
  }
  async function pasteAtCursor(html, url, text) {
    const ed = editor();
    try { ed.frame.contentWindow.focus(); ed.target.focus(); } catch (_) { /* still try */ }
    // a selection would be replaced by the paste: the citation goes after it instead
    if (selectedText(ed)) {
      ed.doc.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", keyCode: 39 }));
      await delay(60);
      if (selectedText(ed)) throw new Error("Click where the citation should go (without text selected) and try again.");
    }
    ed.target.innerHTML = "";
    let evt;
    if (FIREFOX) {
      evt = new ClipboardEvent("paste", { dataType: "text/html", data: html });
    } else {
      const dt = new DataTransfer();
      dt.setData("text/html", html);
      evt = new ClipboardEvent("paste", { clipboardData: dt });
    }
    // Docs reads what landed in its input target right after the paste event: put it there
    setTimeout(() => {
      const a = ed.doc.createElement("a");
      a.href = url;
      a.textContent = text;
      ed.target.replaceChildren(a);
    });
    ed.target.dispatchEvent(evt);
    await delay(50);
  }

  // ---- Edit Citation: the papers of one citation, to remove some or add more ----

  // The cursor in a citation: Google Docs shows the link under the cursor in a bubble, which is
  // part of the page (not the drawn canvas), so the citation's papers can be read from it.
  const CITE_PREFIX = "https://margins.local/cite?ids=";
  function citeLinkUnderCursor() {
    for (const a of document.querySelectorAll('a[href^="' + CITE_PREFIX + '"]')) {
      if (host.contains(a)) continue;
      const shown = a.checkVisibility ? a.checkVisibility({ visibilityProperty: true, opacityProperty: true })
                                      : a.getClientRects().length > 0;
      if (!shown) continue;
      const r = a.getBoundingClientRect();
      if (r.width === 0 || r.right < 0 || r.left > innerWidth) continue;
      return a.getAttribute("href");
    }
    return null;
  }

  let lastLink = null, autoBusy = false;
  setInterval(async () => {
    if (!ready || busy || autoBusy || !$(".card").classList.contains("open")) return;
    const href = citeLinkUnderCursor();
    if (href === lastLink) return;
    lastLink = href;
    const ids = href && LuntaCite.parseLink(href);
    if (!ids || !ids.length) return;
    if (current && current.ids.join(",") === ids.join(",")) return;
    autoBusy = true;
    try { await openByIds(ids); } catch (e) { status(String(e.message || e), true); }
    autoBusy = false;
  }, 400);

  /** Opens the citation with these papers; when several citations have the same ones, asks which. */
  async function openByIds(ids) {
    const st = check(await send({ op: "state", docId: docId() }));
    const texts = st.texts || [], lists = st.citations || [], key = ids.join(",");
    const same = lists.map((l, k) => (l.join(",") === key ? k : -1)).filter((k) => k >= 0);
    const codes = st.options || [];
    if (same.length === 1) { await openCitation(same[0], lists[same[0]], texts[same[0]], codes[same[0]]); return true; }
    if (same.length > 1) { chooseCitation(lists, texts, same, "These papers are cited in " + same.length + " places; which one?", codes); return true; }
    return false;
  }

  /** The citation to edit: the one under the cursor or selected in the text, else the reader picks one from a list. */
  async function editCitation() {
    const href = citeLinkUnderCursor(), ids = href && LuntaCite.parseLink(href);
    if (ids && ids.length && await openByIds(ids)) return;
    status("Reading the document’s citations…");
    const st = check(await send({ op: "state", docId: docId() }));
    const texts = st.texts || [], lists = st.citations || [];
    if (!lists.length) { status("This document has no citations yet."); return; }
    let sel = "";
    try { sel = selectedText(editor()).trim(); } catch (_) { /* no selection to read */ }
    const norm = (t) => String(t || "").replace(/\s+/g, " ").trim();
    let i = -1;
    if (sel.length >= 2) {
      const s = norm(sel);
      const hits = texts.map((t, k) => ({ t: norm(t), k })).filter((x) => x.t && (x.t === s || s.includes(x.t) || x.t.includes(s)));
      if (hits.length === 1) i = hits[0].k;
      else if (hits.length > 1) {
        const exact = hits.filter((x) => x.t === s);
        if (exact.length === 1) i = exact[0].k;
      }
    }
    status("");
    if (i >= 0) { openCitation(i, lists[i], texts[i], (st.options || [])[i]); return; }
    chooseCitation(lists, texts, lists.map((_, k) => k),
      sel ? "The selected text isn’t one citation; pick it here." : "Tip: click a citation in the text while this panel is open, and it opens here by itself.", st.options);
  }

  /** A list of citations (indexes `which`) to pick the one to edit. */
  function chooseCitation(lists, texts, which, hint, codes) {
    current = null; setButtons();
    $(".cbox").hidden = false;
    $(".ctitle").textContent = "Which citation?";
    $(".cadd-row").hidden = true;
    $(".chint").textContent = hint;
    const list = $(".clist");
    list.innerHTML = "";
    which.forEach((k) => {
      const ids = lists[k];
      const row = document.createElement("div");
      row.className = "crow pick";
      row.innerHTML = '<div class="ct"><div class="t"></div><div class="m"></div></div>';
      row.querySelector(".t").textContent = texts[k] || "(…)";
      row.querySelector(".m").textContent = "Citation " + (k + 1) + " of " + lists.length + " · " + ids.length + (ids.length === 1 ? " paper" : " papers");
      row.onclick = () => openCitation(k, ids, texts[k], (codes || [])[k]);
      list.appendChild(row);
    });
  }

  async function openCitation(index, ids, text, code) {
    const same = current && current.index === index && current.ids.join(",") === ids.join(",");
    current = { index, ids: ids.slice(), text: text || (same ? current.text : ""), opts: LuntaCite.decodeOptions(code) || {} };
    if (!same) editingOpts = null;
    $(".cbox").hidden = false;
    $(".ctitle").textContent = "This citation " + (current.text ? "· " + current.text : "");
    $(".cadd-row").hidden = false;
    $(".chint").textContent = "Search above and pick papers to add them; Page… adds a page, prefix or suffix, or leaves the author out; ✕ removes one.";
    setButtons();
    const list = $(".clist");
    list.innerHTML = "";
    const rows = {};
    ids.forEach((id) => {
      const row = document.createElement("div");
      row.className = "crow";
      row.innerHTML = '<div class="ct"><div class="t">…</div><div class="m"></div><div class="o"></div></div>' +
        '<button class="co" title="Page, prefix, suffix, author (for this paper in this citation)">Page…</button><button class="cx" title="Remove from this citation">✕</button>';
      row.querySelector(".o").textContent = LuntaCite.optionsLabel(current.opts[id]);
      row.querySelector(".cx").onclick = () => changeCitation(current.ids.filter((x) => x !== id));
      row.querySelector(".co").onclick = () => { editingOpts = editingOpts === id ? null : id; openCitation(current.index, current.ids, current.text, LuntaCite.encodeOptions(current.opts)); };
      rows[id] = row;
      list.appendChild(row);
      if (editingOpts === id) {
        const form = LuntaCite.optionsForm(document, current.opts[id], (o) => setPaperOptions(id, o),
          () => { editingOpts = null; openCitation(current.index, current.ids, current.text, LuntaCite.encodeOptions(current.opts)); });
        list.appendChild(form);
        setTimeout(form.focusFirst, 0);
      }
    });
    const d = await send({ op: "details", docId: docId(), ids });
    if (!current || current.index !== index || d.error) return;
    (d.entries || []).forEach((e) => {
      const row = rows[e.id]; if (!row) return;
      if (e.missing) { row.querySelector(".t").textContent = "Not in your library"; row.querySelector(".m").textContent = "Remove it, or add the paper to Lunta."; return; }
      row.querySelector(".t").textContent = e.title || "Untitled";
      row.querySelector(".m").textContent = LuntaCite.hitMeta(e);
    });
  }

  function changeCitation(ids) {
    if (!current) return;
    const was = current;
    return exclusive(async () => {
      status(ids.length ? "Changing the citation…" : "Removing the citation…");
      const r = check(await send({ op: "editCitation", docId: docId(), index: was.index, expect: was.ids, ids,
        ...(styleSaved ? {} : { style, styleTitle }) }));
      done(r);
      if (!ids.length || r.removed) { closeCitation(); return; }
      await openCitation(was.index, ids, "", LuntaCite.encodeOptions(LuntaCite.cleanOptions(was.opts, ids)));
    });
  }

  /** One paper's options in the citation being edited, then Update All. */
  function setPaperOptions(id, o) {
    if (!current) return;
    const was = current;
    const map = LuntaCite.cleanOptions(Object.assign({}, was.opts, { [id]: o }), was.ids);
    const code = LuntaCite.encodeOptions(map);
    return exclusive(async () => {
      status("Changing the citation…");
      const r = check(await send({ op: "editCitation", docId: docId(), index: was.index, expect: was.ids, ids: was.ids, options: code,
        ...(styleSaved ? {} : { style, styleTitle }) }));
      done(r);
      editingOpts = null;
      await openCitation(was.index, was.ids, "", code);
    });
  }

  function closeCitation() { current = null; editingOpts = null; $(".cbox").hidden = true; $(".clist").innerHTML = ""; setButtons(); }

  async function insert(ids) {
    const prep = check(await send({ op: "prepareInsert", docId: docId(), ids }));
    if (prep.exists) {
      status("This document already has a bibliography. Updating it…");
      return update();
    }
    await pasteAtCursor(prep.html, prep.url, prep.text);
    status(ids ? "Inserting the citation…" : "Inserting the bibliography…");
    const r = check(await send({ op: "finishInsert", docId: docId(), token: prep.token, ...(styleSaved ? {} : { style, styleTitle }) }));
    done(r);
    if (ids) { picked = []; renderPicked(); }
  }

  // Safari: the placeholder goes on the clipboard in the click itself (Safari writes the clipboard
  // only in answer to one), while the app checks the document; then the reader pastes it.
  function startInsert(ids) {
    if (busy || !ready) return;
    if (!viaApp) { exclusive(() => insert(ids)); return; }
    const prep = send({ op: "prepareInsert", docId: docId(), ids });
    const copied = LuntaClipboard.copy(navigator.clipboard, typeof ClipboardItem === "function" ? ClipboardItem : null, prep);
    copied.catch(() => {});
    exclusive(() => insertByPaste(ids, prep, copied));
  }

  async function insertByPaste(ids, prepared, copied) {
    const prep = check(await prepared);
    if (prep.exists) {
      status("This document already has a bibliography. Updating it…");
      return update();
    }
    try {
      await copied;
    } catch (e) {
      throw new Error("Safari didn’t let Lunta put the " + (ids ? "citation" : "bibliography") + " on the clipboard (" + (e.message || e) + "). Click the button again.");
    }
    // back to the document, so ⌘V goes where its cursor is
    try { const ed = editor(); ed.frame.contentWindow.focus(); ed.target.focus(); } catch (_) { /* the reader clicks there */ }
    const mine = { cancelled: false };
    pasting = mine;
    $(".pasteText").innerHTML = "<strong>Press ⌘V in the document where the " + (ids ? "citation" : "bibliography") + " goes.</strong>";
    $(".pasteBox").hidden = false;
    status("");
    let r;
    try {
      r = await LuntaClipboard.wait(send, {
        docId: docId(), token: prep.token, bare: prep.bare, extra: styleSaved ? {} : { style, styleTitle },
        cancelled: () => mine.cancelled, pause: () => delay(1500),
        onPlain: (m) => status(m, true),
      });
    } finally {
      if (pasting === mine) pasting = null;
      $(".pasteBox").hidden = true;
    }
    if (r.cancelled) { status("Not inserted. (If you paste it later, Update All makes it the citation.)"); return; }
    done(r);
    if (ids) { picked = []; renderPicked(); }
  }

  async function update(newStyle, newTitle) {
    status("Updating…");
    const r = check(await send({ op: "update", docId: docId(), ...(newStyle ? { style: newStyle, styleTitle: newTitle || "" } : styleSaved ? {} : { style, styleTitle }) }));
    done(r);
  }

  function done(r) {
    style = r.style || style; styleTitle = r.styleTitle || ""; styleSaved = true; needsStyle = false;
    fillStyles();
    status(r.summary || "");
  }

  // ---- the style menu ----

  // Google Docs catches the page's mouse and keyboard events, so a change in the menu doesn't always reach
  // its change event (the menu felt stuck): its value is also watched, as the boxes' are (watchBox), and a
  // new choice is applied once, whichever way it arrives.
  function styleChosen() {
    const sel = $(".style");
    if (sel.disabled || sel.value === styleAsked) return;
    styleAsked = sel.value;
    if (sel.value === "__find") { sel.value = needsStyle ? "" : style; styleAsked = sel.value; $(".journal").hidden = false; $(".jq").focus(); return; }
    if (!sel.value || (sel.value === style && styleSaved)) return;
    const s = sel.value;
    exclusive(() => update(s, ""));
  }
  $(".style").onchange = styleChosen;
  $(".style").addEventListener("input", styleChosen);
  setInterval(() => { const sel = $(".style"); if (sel.offsetParent && document.activeElement === sel) styleChosen(); }, 150);
  async function findJournal(q) {
    const box = $(".jresults");
    if (!q.trim()) { box.innerHTML = ""; return; }
    box.innerHTML = '<p class="note">Searching…</p>';
    const r = await send({ op: "styles", q });
    if (r.error) { box.innerHTML = ""; status(r.error, true); return; }
    LuntaCite.showJournals(box, r.hits, (s, title) => {
      $(".journal").hidden = true; $(".jq").value = ""; box.innerHTML = "";
      exclusive(() => update(s, title));
    });
  }
  $(".jclose").onclick = () => { $(".journal").hidden = true; $(".jq").value = ""; $(".jresults").innerHTML = ""; };

  // Typing in the boxes: Google Docs catches keyboard and input events for its editor before they
  // reach this panel, so the boxes are also watched directly while the panel is open (a search when
  // the text has changed and stayed put a moment), and Enter searches at once.
  function watchBox(sel, run) {
    const el = $(sel);
    let last = "", seen = "", since = 0;
    const go = () => { last = el.value; run(el.value); };
    el.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); go(); } });
    el.addEventListener("input", () => { seen = el.value; since = Date.now(); });
    setInterval(() => {
      if (!el.offsetParent) return;   // the panel (or this box) is closed
      const v = el.value;
      if (v === last) return;
      if (v !== seen) { seen = v; since = Date.now(); return; }
      if (Date.now() - since >= 200) go();
    }, 120);
  }
  watchBox(".q", search);
  watchBox(".jq", findJournal);
  $(".insert").onclick = () => startInsert(picked.map((p) => p.id));
  $(".bib").onclick = () => startInsert(null);
  $(".pasteCancel").onclick = () => { if (pasting) pasting.cancelled = true; };
  $(".refresh").onclick = () => exclusive(() => update());
  $(".edit").onclick = () => { if (ready && !busy) editCitation().catch((e) => status(String(e.message || e), true)); };
  $(".cclose").onclick = () => closeCitation();
  $(".cadd").onclick = () => {
    if (!current || !picked.length) return;
    const ids = LuntaCite.unique(current.ids.concat(picked.map((p) => p.id)));
    picked = []; renderPicked();
    changeCitation(ids);
  };

  // ---- Resolve Citations: placeholders typed while writing (gdocs/citations-resolve.js) ----

  let resolver = null;
  function openResolve() {
    if (!ready || busy) return;
    if (luntaApi < 3) { status("This version of Lunta can’t resolve citations. Update Lunta and try again.", true); return; }
    exclusive(async () => {
      status("Looking for placeholders…");
      const r = check(await send({ op: "scanPlaceholders", docId: docId() }));
      $(".rbox").hidden = false;
      resolver = resolver || LuntaResolve.create({
        box: $(".rlist"), doc: document, status,
        api: {
          suggest: async (cite) => check(await send({ op: "suggest", cite })),
          search: async (query) => check(await send({ op: "suggest", query })),
          add: async (cand) => check(await send({ op: "addPaper", meta: cand.meta || cand })),
          // one change to the document at a time, as the other buttons
          apply: async (placeholder, ids, leftover) => {
            while (busy) await delay(200);
            busy = true; setButtons();
            try {
              const w = check(await send({ op: "writePlaceholder", docId: docId(), placeholder, ids, leftover, ...(styleSaved ? {} : { style, styleTitle }) }));
              style = w.style || style; styleTitle = w.styleTitle || ""; styleSaved = true; fillStyles();
            } finally { busy = false; setButtons(); }
          },
        },
      });
      resolver.load(r.placeholders || []);
    });
  }
  $(".resolve").onclick = openResolve;

  // ---- Convert Citations from Zotero (its links and named ranges; common.js reads them) ----

  let conversion = null;
  function openConvert() {
    if (!ready || busy) return;
    if (luntaApi < 4) { status("This version of Lunta can’t convert citations. Update Lunta and try again.", true); return; }
    exclusive(async () => {
      status("Looking for Zotero citations…");
      const z = check(await send({ op: "scanZotero", docId: docId() }));
      const notes = [];
      if (z.unreadable) notes.push(z.unreadable + " Zotero citation" + (z.unreadable === 1 ? "" : "s") + " had no details in the document and will stay as they are.");
      if (z.paperpile) notes.push("Paperpile’s citations (" + z.paperpile + ") can’t be converted: Paperpile keeps their details on its own server.");
      if (!z.cites.length) { status(notes.length ? notes.join(" ") : "This document has no Zotero citations.", notes.length > 0); return; }
      const first = LuntaCite.convertPlan(z.cites);
      const m = check(await send({ op: "match", items: first.works.map((w) => w.meta) }));
      const plan = LuntaCite.convertPlan(z.cites, m.matches || []);
      conversion = { plan, bibs: z.bibs, unreadable: z.unreadable || 0 };
      if (z.bibs.length) notes.push(z.unreadable ? "Zotero’s bibliography stays, as some of its citations can’t be read." : "Zotero’s bibliography becomes Lunta’s once all its citations are converted.");
      $(".cvbox").hidden = false;
      LuntaCite.convertReview(document, $(".cvlist"), plan, notes, (keys) => runConversion(keys), closeConvert);
      status(LuntaCite.convertSummary(plan) + ". Review, then Convert.");
    });
  }
  function closeConvert() { conversion = null; $(".cvbox").hidden = true; $(".cvlist").innerHTML = ""; }
  function runConversion(keys) {
    const conv = conversion;
    if (!conv) return;
    exclusive(async () => {
      const added = {};
      let newPapers = 0;
      for (const k of keys) {
        const w = conv.plan.works[conv.plan.byKey[k]];
        status("Adding “" + (w.meta.title || "a paper") + "” to your library…");
        const p = check(await send({ op: "addPaper", meta: w.meta }));
        if (p && p.id) { added[k] = p.id; if (p.added) newPapers++; }
      }
      const res = LuntaCite.convertResolve(conv.plan, added);
      const left = res.left.map((l) => ({ text: conv.plan.citations[l.index].text, reason: l.reason }));
      const cites = res.convert.map((c) => ({ key: conv.plan.citations[c.index].key, ids: c.ids, options: LuntaCite.encodeOptions(c.options) }));
      // Zotero's bibliography stays while any of its citations does (left out, or unreadable)
      const keepBibs = left.length > 0 || conv.unreadable > 0;
      if (keepBibs && conv.bibs.length) left.push({ text: "", reason: "Zotero’s bibliography stays until its citations are converted" });
      status("Converting " + cites.length + " citations…");
      const r = check(await send({ op: "convertZotero", docId: docId(), cites, bibs: keepBibs ? [] : conv.bibs, ...(styleSaved ? {} : { style, styleTitle }) }));
      done(r);
      closeConvert();
      status(LuntaCite.convertDone(r.converted, left, newPapers) + " " + (r.summary || ""), left.length > 0);
    });
  }
  // ---- Check Citations: each citation's sentence against the cited papers (common.js, Lunta's /claims) ----

  function showClaims() {
    LuntaCite.claimsReview(document, $(".cllist"), claimItems, {
      open: (link) => {
        const l = LuntaCite.parseOpenLink(link);
        if (l) send({ op: "openPaper", id: l.id, find: l.find }).then((r) => { if (r && r.error) status(r.error, true); });
      },
      changed: () => status(LuntaCite.claimSummary(LuntaCite.claimCounts(claimItems, LuntaCite.claimDecisions())) + "."),
    });
  }
  async function openClaims() {
    if (!ready || checking) return;
    if (luntaApi < 5) { status("This version of Lunta can’t check citations. Update Lunta and try again.", true); return; }
    checking = true; stopChecking = false; setButtons();
    try {
      status("Reading the citations…");
      const r = check(await send({ op: "claimSites", docId: docId() }));
      const sites = r.sites || [];
      if (!sites.length) { status("This document has no Lunta citations to check."); return; }
      $(".clbox").hidden = false; $(".clstop").hidden = false;
      claimItems = [];
      $(".cllist").textContent = LuntaCite.CLAIM_PRIVACY;
      claimItems = await LuntaCite.runClaims(sites, async (body) => check(await send({ op: "claims", body })),
        (i, n) => status("Checking citation " + (i + 1) + " of " + n + "…"), () => stopChecking, Object.fromEntries(Object.entries(r.papers || {}).map(([k, m]) => [k, { m }])));
      showClaims();
      status(LuntaCite.claimSummary(LuntaCite.claimCounts(claimItems, LuntaCite.claimDecisions())) + (stopChecking ? " (stopped)." : "."));
    } catch (e) {
      status(String((e && e.message) || e), true);
    } finally {
      checking = false; $(".clstop").hidden = true; setButtons();
    }
  }
  $(".claims").onclick = openClaims;
  $(".clstop").onclick = () => { stopChecking = true; };
  $(".clclose").onclick = () => { stopChecking = true; $(".clbox").hidden = true; $(".cllist").innerHTML = ""; claimItems = []; status(""); };
  $(".convert").onclick = openConvert;
  // Convert Citations to Plain Text: warned first (it can't be undone), then Update All and the links removed
  const closePlain = () => { $(".plainbox").hidden = true; };
  $(".plain").onclick = () => { $(".plaintext").textContent = LuntaCite.PLAIN_WARNING; $(".plainbox").hidden = false; status(""); };
  $(".plainclose").onclick = closePlain;
  $(".plaincancel").onclick = closePlain;
  $(".plaingo").onclick = () => exclusive(async () => {
    closePlain();
    status("Converting to plain text…");
    const r = check(await send({ op: "plainText", docId: docId(), ...(styleSaved ? {} : { style, styleTitle }) }));
    styleSaved = false;
    status(r.summary || "");
  });
  $(".cvclose").onclick = closeConvert;
  $(".rclose").onclick = () => { $(".rbox").hidden = true; if (resolver) resolver.load([]); $(".rlist").innerHTML = ""; status(""); };

  async function connect(selectAccount) {
    status(viaApp ? "Lunta for Mac opened Google’s sign-in in your browser: sign in there, allow Lunta and choose this document (it is the one shown), then come back here…" : "Waiting for Google…");
    const r = await send({ op: "connect", selectAccount, docId: docId() });   // Safari: the sign-in gives Lunta this document too
    if (r.error) { status(r.error, true); return; }
    // Safari: the app signs in, in a browser tab of its own; ask it until it's done
    if (r.started) {
      const until = Date.now() + 5 * 60000;
      for (;;) {
        await delay(1500);
        const s = await send({ op: "status" });
        if (s.error) { status(s.error, true); return; }
        if (s.connected && !s.connecting) break;
        if (!s.connecting) { status(s.authError || "Google sign-in didn’t finish. Try again.", true); return; }
        if (Date.now() > until) { status("Google sign-in didn’t finish. Try again.", true); return; }
      }
    }
    status("");
    load();
  }
  $(".connectBtn").onclick = () => connect(false);
  $(".account").onclick = () => connect(true);
  $(".grantAccount").onclick = () => connect(true);

  // drive.file: Google's picker, set to this document (the extension: a window from getlunta.app;
  // Safari: Lunta for Mac opens Google's page in the browser), then everything as before
  async function grant() {
    $(".grantBtn").disabled = true;
    status(viaApp ? "Lunta for Mac opened Google’s page in your browser: choose this document there (it is the one shown) and click Select, then come back here…"
      : "In Google’s window, click this document (it is the one shown), then Select…");
    try {
      const r = await send({ op: "grant", docId: docId() });
      if (r.error) { status(r.error, true); return; }
      if (r.started) {
        const until = Date.now() + 5 * 60000;
        for (;;) {
          await delay(1500);
          const s = await send({ op: "status", docId: docId() });
          if (s.error) { status(s.error, true); return; }
          if (!s.connecting) {
            if (s.authError) { status(s.authError, true); return; }
            break;
          }
          if (Date.now() > until) { status("Google’s page didn’t finish. Try again.", true); return; }
        }
      }
      status("");
      load();
    } finally {
      $(".grantBtn").disabled = false;
    }
  }
  $(".grantBtn").onclick = grant;

  // ---- opening ----

  async function load() {
    ready = false; setButtons();
    status("Loading…");
    const s = await send({ op: "status" });
    if (s.error) { status(s.error, true); return; }
    viaApp = !!s.viaApp;
    luntaApi = (s.lunta && s.lunta.api) || 0;
    if (viaApp) {
      $(".connectText").textContent = "In Safari, Lunta for Mac writes the citations into this document through Google Docs. Connect it to your Google account (your browser opens Google’s page) and choose this document there: Lunta reaches only the documents you give it.";
    }
    if (!s.configured || !s.supported) {
      show("setup");
      $(".setupText").textContent = !s.supported
        ? "Safari on iPhone and iPad can’t cite in Google Docs from the extension. Cite with the Apps Script sidebar (Lunta › Citations in Word & Google Docs… on a Mac sets it up), or from Safari on a Mac with Lunta for Mac."
        : viaApp
          ? "Citing in Google Docs from Safari isn’t set up yet in this version of Lunta for Mac. Cite with the Apps Script sidebar meanwhile: Lunta › Citations in Word & Google Docs…, or use Lunta’s extension in Chrome, Edge or Firefox."
          : "Google Docs citing isn’t set up in this copy of the extension. Cite with the Apps Script sidebar meanwhile: Lunta › Citations in Word & Google Docs….";
      status("");
      return;
    }
    if (!s.connected) { show("connect"); status(""); return; }
    show("main");
    const st = await send({ op: "state", docId: docId() });
    if (st.error) {
      if (st.kind === "auth") show("connect");
      if (st.kind === "access") show("grant");
      status(st.error, true);
      return;
    }
    styleSaved = !!st.style;
    needsStyle = !st.style && st.citations.length > 0;
    style = LuntaCite.isStyle(st.style) ? st.style : "apa";
    styleTitle = st.styleTitle || "";
    fillStyles();
    ready = true; setButtons();
    if (s.lunta && s.lunta.error) status(s.lunta.error + (st.citations.length ? " Update All still works from the copy of the cited papers saved in this document." : ""), true);
    else if (needsStyle) status("Choose this document’s citation style above: Lunta didn’t find it saved in the document (written with an older copy of the Apps Script?).", true);
    else status(`Connected to Lunta · ${s.lunta.papers} papers` + (st.pending ? " · Update All finishes citations inserted before" : ""));
  }

  $(".tab").addEventListener("click", () => {
    const card = $(".card");
    card.classList.toggle("open");
    if (card.classList.contains("open")) {
      if (!opened || !ready) { opened = true; load(); }
      setTimeout(() => $(".q").focus(), 0);
    }
  });
  chrome.runtime.onMessage.addListener((m) => {
    if (m && m.type === "togglePanel") $(".tab").click();
  });
})();
