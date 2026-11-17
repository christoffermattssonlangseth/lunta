// Lunta in Google Docs: a button at the right edge of a document opens a small panel to search the
// Lunta library, insert citations and the bibliography, update them all, and pick the style. The
// document is read and written by the background script through the Google Docs API
// (gdocs/background-gdocs.js); this page script only shows the panel and, to insert at the
// cursor (which the API doesn't know), pastes a placeholder through Docs' own paste handling.
/* global LuntaCite */
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
      .tab img { width: 26px; height: 26px; border-radius: 6px; }
      .card { position: absolute; right: 0; bottom: 50px; width: 330px; max-height: min(620px, calc(100vh - 170px)); overflow: auto; background: #fff;
              border-radius: 14px; box-shadow: 0 10px 34px rgba(16,40,50,.28); padding: 12px; display: none; box-sizing: border-box; }
      .card.open { display: block; }
      header { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
      header img { width: 20px; height: 20px; border-radius: 5px; }
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
        <header><img alt=""><strong>Lunta</strong><select class="style" title="Citation style" disabled></select></header>
        <div class="setup" hidden><p class="note setupText"></p></div>
        <div class="connect" hidden>
          <p class="note">Lunta writes citations into this document through Google Docs, with your permission (asked once). Only the document you cite in is read or changed.</p>
          <div class="buttons"><button class="btn primary connectBtn">Connect Google account</button></div>
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
          <div class="buttons"><button class="btn edit" disabled>Edit Citation…</button></div>
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
      <button class="tab" title="Lunta: cite from your library"><img alt="Lunta"></button>
    </div>`;
  const $ = (s) => root.querySelector(s);
  const icon = chrome.runtime.getURL("icons/icon-48.png");
  root.querySelectorAll("img").forEach((i) => (i.src = icon));
  document.documentElement.appendChild(host);

  const send = (msg) => new Promise((resolve) => {
    try {
      chrome.runtime.sendMessage({ type: "gdocs", ...msg }, (r) => resolve(r || { error: (chrome.runtime.lastError && chrome.runtime.lastError.message) || "The extension didn’t answer. Reload the page." }));
    } catch (e) {
      resolve({ error: "The Lunta extension was updated or restarted: reload this page." });
    }
  });

  let current = null;   // the citation being edited: {index, ids, text}
  let picked = [], style = "apa", styleTitle = "", styleSaved = false, needsStyle = false, ready = false, busy = false, lastSearch = 0, opened = false;

  function status(m, err) { const s = $(".status"); s.textContent = m || ""; s.className = "status" + (err ? " err" : ""); }
  function setButtons() {
    const can = ready && !busy;
    $(".insert").disabled = !can || !picked.length || needsStyle;
    $(".bib").disabled = $(".refresh").disabled = !can || needsStyle;
    $(".style").disabled = !can;
    $(".edit").disabled = !can || needsStyle;
    $(".cadd").disabled = !can || !current || !picked.length;
  }
  function show(which) {
    ["setup", "connect", "main"].forEach((k) => { $("." + k).hidden = k !== which; });
  }

  function fillStyles() {
    const sel = $(".style");
    LuntaCite.fillStyles(sel, style, styleTitle);
    if (needsStyle) {
      const o = document.createElement("option");
      o.value = ""; o.textContent = "Choose the style…";
      sel.insertBefore(o, sel.firstChild);
      sel.value = "";
    }
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
    if (same.length === 1) { await openCitation(same[0], lists[same[0]], texts[same[0]]); return true; }
    if (same.length > 1) { chooseCitation(lists, texts, same, "These papers are cited in " + same.length + " places; which one?"); return true; }
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
    if (i >= 0) { openCitation(i, lists[i], texts[i]); return; }
    chooseCitation(lists, texts, lists.map((_, k) => k),
      sel ? "The selected text isn’t one citation; pick it here." : "Tip: click a citation in the text while this panel is open, and it opens here by itself.");
  }

  /** A list of citations (indexes `which`) to pick the one to edit. */
  function chooseCitation(lists, texts, which, hint) {
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
      row.onclick = () => openCitation(k, ids, texts[k]);
      list.appendChild(row);
    });
  }

  async function openCitation(index, ids, text) {
    current = { index, ids: ids.slice(), text };
    $(".cbox").hidden = false;
    $(".ctitle").textContent = "This citation " + (text ? "· " + text : "");
    $(".cadd-row").hidden = false;
    $(".chint").textContent = "Search above and pick papers to add them; ✕ removes one.";
    setButtons();
    const list = $(".clist");
    list.innerHTML = "";
    const rows = {};
    ids.forEach((id) => {
      const row = document.createElement("div");
      row.className = "crow";
      row.innerHTML = '<div class="ct"><div class="t">…</div><div class="m"></div></div><button class="cx" title="Remove from this citation">✕</button>';
      row.querySelector(".cx").onclick = () => changeCitation(current.ids.filter((x) => x !== id));
      rows[id] = row;
      list.appendChild(row);
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
      await openCitation(was.index, ids, "");
    });
  }

  function closeCitation() { current = null; $(".cbox").hidden = true; $(".clist").innerHTML = ""; setButtons(); }

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

  $(".style").onchange = () => {
    const sel = $(".style");
    if (sel.value === "__find") { sel.value = needsStyle ? "" : style; $(".journal").hidden = false; $(".jq").focus(); return; }
    if (!sel.value) return;
    const s = sel.value;
    exclusive(() => update(s, ""));
  };
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
  $(".insert").onclick = () => exclusive(() => insert(picked.map((p) => p.id)));
  $(".bib").onclick = () => exclusive(() => insert(null));
  $(".refresh").onclick = () => exclusive(() => update());
  $(".edit").onclick = () => { if (ready && !busy) editCitation().catch((e) => status(String(e.message || e), true)); };
  $(".cclose").onclick = () => closeCitation();
  $(".cadd").onclick = () => {
    if (!current || !picked.length) return;
    const ids = LuntaCite.unique(current.ids.concat(picked.map((p) => p.id)));
    picked = []; renderPicked();
    changeCitation(ids);
  };

  async function connect(selectAccount) {
    status("Waiting for Google…");
    const r = await send({ op: "connect", selectAccount });
    if (r.error) { status(r.error, true); return; }
    status("");
    load();
  }
  $(".connectBtn").onclick = () => connect(false);
  $(".account").onclick = () => connect(true);

  // ---- opening ----

  async function load() {
    ready = false; setButtons();
    status("Loading…");
    const s = await send({ op: "status" });
    if (s.error) { status(s.error, true); return; }
    if (!s.configured || !s.supported) {
      show("setup");
      $(".setupText").textContent = !s.supported
        ? "This browser can’t sign in to Google for extensions (Safari). Cite with the Apps Script sidebar: Lunta › Citations in Word & Google Docs…, or use Lunta’s extension in Chrome, Edge or Firefox."
        : "Google Docs citing isn’t set up in this copy of the extension. Cite with the Apps Script sidebar meanwhile: Lunta › Citations in Word & Google Docs….";
      status("");
      return;
    }
    if (!s.connected) { show("connect"); status(""); return; }
    show("main");
    const st = await send({ op: "state", docId: docId() });
    if (st.error) {
      if (st.kind === "auth") show("connect");
      status(st.error, true);
      return;
    }
    styleSaved = !!st.style;
    needsStyle = !st.style && st.citations.length > 0;
    style = LuntaCite.isStyle(st.style) ? st.style : "apa";
    styleTitle = st.styleTitle || "";
    fillStyles();
    ready = true; setButtons();
    if (s.lunta.error) status(s.lunta.error + (st.citations.length ? " Update All still works from the copy of the cited papers saved in this document." : ""), true);
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
