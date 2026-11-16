// Lunta extension: talks to the Lunta Mac app on 127.0.0.1:47480 (the app must be running).
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
const MAX_IMAGES = 80;
const MAX_IMAGE_BYTES = 15 * 1024 * 1024;
const MAX_TOTAL_IMAGE_BYTES = 80 * 1024 * 1024;

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
  const images = [];
  let skipped = 0, total = 0, failed = 0;
  const list = (msg.images || []).slice(0, MAX_IMAGES);
  skipped += Math.max(0, (msg.images || []).length - MAX_IMAGES);
  for (let i = 0; i < list.length; i++) {
    progress(`Fetching figure ${i + 1} of ${list.length}…`);
    const b = await fetchImage(list[i].fetch, tabId);
    if (!b) { failed++; continue; }
    if (b.length > MAX_IMAGE_BYTES || total + b.length > MAX_TOTAL_IMAGE_BYTES) { skipped++; continue; }
    total += b.length;
    images.push({ urls: list[i].urls, base64: bytesToBase64(b) });
  }
  const mb = (total / 1048576).toFixed(1);
  progress(`Lunta is making the reading view (${images.length} figure${images.length === 1 ? "" : "s"}, ${mb} MB)…`);
  const body = JSON.stringify({
    url: msg.url, html: msg.html, liveHtml: msg.liveHtml || undefined, doi: msg.doi || "", title: msg.title || "",
    images, imagesSkipped: skipped,
  });
  const r = await api("/fulltext", { method: "POST", headers: { "Content-Type": "application/json" }, body });
  const notes = [];
  if (failed) notes.push(`${failed} figure image${failed === 1 ? "" : "s"} couldn't be downloaded.`);
  if (skipped) notes.push(`${skipped} figure image${skipped === 1 ? " was" : "s were"} too large to send.`);
  if (notes.length) r.message = `${r.message || ""} ${notes.join(" ")}`.trim();
  return r;
}

async function handle(msg, sender) {
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
    case "fullText":
      return sendFullText(msg, sender && sender.tab ? sender.tab.id : null);
    default:
      throw new Error("unknown request");
  }
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  handle(msg, _sender).then(sendResponse, (e) => sendResponse({ error: String(e.message || e) }));
  return true; // async response
});

// Toolbar button: on a PDF open in Chrome's viewer, save that PDF; elsewhere, show the panel.
chrome.action.onClicked.addListener(async (tab) => {
  if (!tab.id || !tab.url) return;
  const looksPdf = /\.pdf($|[?#])/i.test(tab.url);
  if (looksPdf) {
    try {
      const bytes = await fetchPdf(tab.url);
      const name = decodeURIComponent(tab.url.split("/").pop().split(/[?#]/)[0] || "paper.pdf");
      const r = await api("/pdf?" + new URLSearchParams({ name }), { method: "POST", headers: { "Content-Type": "application/pdf" }, body: bytes });
      chrome.action.setBadgeBackgroundColor({ color: "#2a6478", tabId: tab.id });
      chrome.action.setBadgeText({ text: r.inLibrary ? "✓" : "!", tabId: tab.id });
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
