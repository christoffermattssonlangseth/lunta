// Lunta extension: talks to the Lunta app (for Mac, Windows or Linux) on 127.0.0.1:47480 (the app must be running).
// In Safari on iPhone and iPad there is no such server: requests go to the extension's native
// handler instead, which leaves them for the Lunta app (see "Safari on iPhone and iPad" below).
const API = "http://127.0.0.1:47480";

async function api(path, options = {}) {
  let r;
  try {
    r = await fetch(API + path, { ...options, headers: { ...(options.headers || {}), "X-Margins-Client": "extension" } });
  } catch (e) {
    throw new Error("Lunta isn't running. Open the Lunta app and try again.");
  }
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || `Lunta answered ${r.status}`);
  return j;
}

function base64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function fetchPdf(url) {
  // the browser's own cookies are sent, so institutional / logged-in access applies
  const r = await fetch(url, { credentials: "include" });
  if (!r.ok) throw new Error(`The PDF link answered ${r.status}`);
  const buf = new Uint8Array(await r.arrayBuffer());
  if (!(buf[0] === 0x25 && buf[1] === 0x50 && buf[2] === 0x44 && buf[3] === 0x46)) {
    throw new Error("The PDF link returned a web page, not a PDF (you may need to log in or open the PDF first).");
  }
  return buf;
}

// Limits for "Use this page's full text": figures beyond these are left out (and counted).
// iPhone and iPad: smaller, for the memory Safari and the app have there.
const LIMITS = { images: 80, imageBytes: 15 * 1024 * 1024, totalBytes: 80 * 1024 * 1024 };
const LIMITS_IOS = { images: 40, imageBytes: 6 * 1024 * 1024, totalBytes: 30 * 1024 * 1024 };

// ---- Safari on iPhone and iPad ----
// No Lunta server on the device: background.js sends the requests as native messages to the
// extension (SafariWebExtensionHandler), which writes them in pieces into the app group; the Lunta
// app does the work when it is next opened. Used only when the native side says it is on iOS
// (Safari on the Mac, Chrome and Firefox have no native messaging here and use 127.0.0.1).
const NATIVE_PIECE = 512 * 1024; // characters of base64 or text per message
let nativeMode = null; // Promise<boolean>

function useNative() {
  if (!nativeMode) {
    nativeMode = (async () => {
      const rt = typeof browser !== "undefined" && browser.runtime;
      if (!rt || typeof rt.sendNativeMessage !== "function") return false;
      try {
        const r = await rt.sendNativeMessage("com.cmlangseth.margins.ios", { op: "ping" });
        return !!(r && r.platform === "ios");
      } catch (_) {
        nativeMode = null; // ask again next time
        return false;
      }
    })();
  }
  return nativeMode;
}

async function native(msg) {
  let r;
  try {
    r = await browser.runtime.sendNativeMessage("com.cmlangseth.margins.ios", msg);
  } catch (e) {
    throw new Error("Lunta's Safari extension didn't answer. Open the Lunta app once, then try again.");
  }
  if (!r || r.error) throw new Error((r && r.error) || "Lunta's Safari extension didn't answer.");
  return r;
}

// A request for the app: `files` are added piece by piece ({name, base64} or {name, text}),
// then the request is handed over in one step (the app never sees half of it).
async function nativeBegin() {
  return (await native({ op: "begin" })).token;
}
async function nativeAppend(token, name, data, isText) {
  for (let i = 0; i < data.length || i === 0; ) {
    let end = Math.min(data.length, i + NATIVE_PIECE);
    // don't split a character written as two UTF-16 units
    if (isText && end < data.length && /[\ud800-\udbff]/.test(data[end - 1])) end--;
    await native({ op: "append", token, file: name, [isText ? "text" : "base64"]: data.slice(i, end) });
    if (end >= data.length) break;
    i = end;
  }
}
async function nativeSubmit(request, files = []) {
  const token = await nativeBegin();
  try {
    for (const f of files) await nativeAppend(token, f.name, f.text != null ? f.text : f.base64, f.text != null);
    return await native({ op: "commit", token, request });
  } catch (e) {
    native({ op: "abort", token }).catch(() => {});
    throw e;
  }
}

function bytesToBase64(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

function looksImage(b, type) {
  if (/^image\//i.test(type || "")) return true;
  // JPEG, PNG, GIF, WebP (RIFF), TIFF, SVG
  return (b[0] === 0xff && b[1] === 0xd8) || (b[0] === 0x89 && b[1] === 0x50) || (b[0] === 0x47 && b[1] === 0x49) ||
    (b[0] === 0x52 && b[1] === 0x49 && b[8] === 0x57) || (b[0] === 0x49 && b[1] === 0x49) || (b[0] === 0x4d && b[1] === 0x4d) ||
    (b[0] === 0x3c && (b[1] === 0x73 || b[1] === 0x3f));
}

// One figure image with the reader's access: the extension first (any host, the browser's
// cookies), else the page itself (same-origin requests the extension can't make).
async function fetchImage(url, tabId) {
  try {
    const r = await fetch(url, { credentials: "include" });
    if (r.ok) {
      const b = new Uint8Array(await r.arrayBuffer());
      if (looksImage(b, r.headers.get("content-type"))) return b;
    }
  } catch (_) { /* try from the page */ }
  if (tabId == null) return null;
  try {
    const r = await chrome.tabs.sendMessage(tabId, { type: "fetchImage", url });
    if (r && r.base64) {
      const b = base64ToBytes(r.base64);
      if (looksImage(b, r.type)) return b;
    }
  } catch (_) { /* give up on this one */ }
  return null;
}

async function sendFullText(msg, tabId) {
  const progress = (text) => { if (tabId != null) chrome.tabs.sendMessage(tabId, { type: "fullTextProgress", text }).catch(() => {}); };
  const viaApp = await useNative();
  const lim = viaApp ? LIMITS_IOS : LIMITS;
  // iPhone and iPad: each figure goes to the app as soon as it's fetched (not all held here)
  const token = viaApp ? await nativeBegin() : null;
  try {
    const images = [];
    let skipped = 0, total = 0, failed = 0;
    const list = (msg.images || []).slice(0, lim.images);
    skipped += Math.max(0, (msg.images || []).length - lim.images);
    const add = async (urls, base64) => {
      if (!token) return images.push({ urls, base64 });
      const file = `img-${images.length}`;
      await nativeAppend(token, file, base64, false);
      images.push({ urls, file });
    };
    for (let i = 0; i < list.length; i++) {
      progress(`Fetching figure ${i + 1} of ${list.length}…`);
      const b = await fetchImage(list[i].fetch, tabId);
      if (!b) { failed++; continue; }
      if (b.length > lim.imageBytes || total + b.length > lim.totalBytes) { skipped++; continue; }
      total += b.length;
      await add(list[i].urls, bytesToBase64(b));
    }
    // pages the content script fetched itself (tables an article page only links to), sent as they are
    for (const p of (msg.pages || []).slice(0, 30)) {
      if (p && p.base64 && Array.isArray(p.urls)) await add(p.urls, p.base64);
    }
    const mb = (total / 1048576).toFixed(1);
    const figures = `${images.length} figure${images.length === 1 ? "" : "s"}, ${mb} MB`;
    let r;
    if (token) {
      progress(`Sending the page to Lunta (${figures})…`);
      await nativeAppend(token, "page.html", msg.html, true);
      if (msg.liveHtml) await nativeAppend(token, "live.html", msg.liveHtml, true);
      r = await native({ op: "commit", token, request: {
        kind: "fulltext", url: msg.url, doi: msg.doi || "", title: msg.title || "", html: "page.html",
        liveHtml: msg.liveHtml ? "live.html" : undefined, images, imagesSkipped: skipped,
      } });
    } else {
      progress(`Lunta is making the reading view (${figures})…`);
      const body = JSON.stringify({
        url: msg.url, html: msg.html, liveHtml: msg.liveHtml || undefined, doi: msg.doi || "", title: msg.title || "",
        images, imagesSkipped: skipped,
      });
      r = await api("/fulltext", { method: "POST", headers: { "Content-Type": "application/json" }, body });
    }
    const notes = [];
    if (failed) notes.push(`${failed} figure image${failed === 1 ? "" : "s"} couldn't be downloaded.`);
    if (skipped) notes.push(`${skipped} figure image${skipped === 1 ? " was" : "s were"} too large to send.`);
    if (notes.length) r.message = `${r.message || ""} ${notes.join(" ")}`.trim();
    return r;
  } catch (e) {
    if (token) native({ op: "abort", token }).catch(() => {});
    throw e;
  }
}

async function handle(msg, sender) {
  if (msg.type !== "fullText" && (await useNative())) return handleNative(msg);
  switch (msg.type) {
    case "status": {
      const q = new URLSearchParams({ doi: msg.doi || "", title: msg.title || "" });
      return api("/status?" + q);
    }
    case "save":
      return api("/save", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(msg.meta) });
    case "savePdf": {
      // bytes fetched by the page (same-origin cookies) arrive as base64; otherwise fetch here
      const bytes = msg.base64 ? base64ToBytes(msg.base64) : await fetchPdf(msg.pdfUrl);
      const q = new URLSearchParams({ id: msg.id || "", name: msg.name || "paper.pdf" });
      return api("/pdf?" + q, { method: "POST", headers: { "Content-Type": "application/pdf" }, body: bytes });
    }
    case "findPdf":
      return api("/findpdf?" + new URLSearchParams({ id: msg.id || "" }), { method: "POST" });
    case "fullText":
      return sendFullText(msg, sender && sender.tab ? sender.tab.id : null);
    default:
      throw new Error("unknown request");
  }
}

// iPhone and iPad: the same requests, left for the Lunta app (answers say {queued: true, message})
async function handleNative(msg) {
  switch (msg.type) {
    case "status":
      return native({ op: "status", doi: msg.doi || "", title: msg.title || "" });
    case "save":
      return nativeSubmit({ kind: "save", meta: msg.meta || {} });
    case "savePdf": {
      const base64 = msg.base64 || bytesToBase64(await fetchPdf(msg.pdfUrl));
      return nativeSubmit({ kind: "pdf", pdf: "paper.pdf", paperId: msg.id || "", name: msg.name || "paper.pdf", meta: msg.meta || {} },
        [{ name: "paper.pdf", base64 }]);
    }
    case "findPdf":
      return nativeSubmit({ kind: "findpdf", paperId: msg.id || "", meta: msg.meta || {} });
    default:
      throw new Error("unknown request");
  }
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  handle(msg, _sender).then(sendResponse, (e) => sendResponse({ error: String(e.message || e) }));
  return true; // async response
});

// Toolbar button: on a PDF open in the browser's viewer, save that PDF; elsewhere, show the panel.
const FIREFOX = typeof navigator !== "undefined" && /Firefox\//.test(navigator.userAgent);

chrome.action.onClicked.addListener(async (tab) => {
  // Firefox lets people turn off the extension's access to sites: ask again on the click (the
  // request has to come straight from the click, before anything is awaited)
  if (FIREFOX && chrome.permissions) chrome.permissions.request({ origins: ["<all_urls>"] }).catch(() => {});
  if (!tab.id || !tab.url) return;
  const looksPdf = /\.pdf($|[?#])/i.test(tab.url);
  if (looksPdf) {
    try {
      const bytes = await fetchPdf(tab.url);
      const name = decodeURIComponent(tab.url.split("/").pop().split(/[?#]/)[0] || "paper.pdf");
      const r = (await useNative())
        ? await nativeSubmit({ kind: "pdf", pdf: "paper.pdf", name }, [{ name: "paper.pdf", base64: bytesToBase64(bytes) }])
        : await api("/pdf?" + new URLSearchParams({ name }), { method: "POST", headers: { "Content-Type": "application/pdf" }, body: bytes });
      chrome.action.setBadgeBackgroundColor({ color: "#2a6478", tabId: tab.id });
      chrome.action.setBadgeText({ text: r.inLibrary || r.queued ? "✓" : "!", tabId: tab.id });
      if (r.queued) chrome.action.setTitle({ title: "Lunta: " + r.message, tabId: tab.id });
    } catch (e) {
      chrome.action.setBadgeBackgroundColor({ color: "#c2703d", tabId: tab.id });
      chrome.action.setBadgeText({ text: "!", tabId: tab.id });
      chrome.action.setTitle({ title: "Lunta: " + (e.message || e), tabId: tab.id });
    }
    return;
  }
  // the panel lives in the page (content.js); on a tab opened before the extension was loaded it
  // isn't there yet: load it now. On a page with no paper it never appears: say so on the button.
  if (await toggle(tab.id)) return;
  try {
    await chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["content.js"] });
  } catch (e) {
    return flag(tab.id, "Lunta can't run on this page (Chrome's own pages and the Web Store are off limits).");
  }
  if (!(await toggle(tab.id))) flag(tab.id, "No paper found on this page: Lunta looks for a DOI or the citation details journals put in the page. Open the article's own page (not a search result) and try again.");
});

async function toggle(tabId) {
  try {
    await chrome.tabs.sendMessage(tabId, { type: "togglePanel" });
    return true;
  } catch (_) {
    return false; // no panel in this tab
  }
}

function flag(tabId, why) {
  chrome.action.setBadgeBackgroundColor({ color: "#8a8a8e", tabId });
  chrome.action.setBadgeText({ text: "?", tabId });
  chrome.action.setTitle({ title: "Lunta: " + why, tabId });
  setTimeout(() => {
    chrome.action.setBadgeText({ text: "", tabId }).catch(() => {});
    chrome.action.setTitle({ title: "Save to Lunta", tabId }).catch(() => {});
  }, 6000);
}
