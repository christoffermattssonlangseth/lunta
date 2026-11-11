// Margins extension: talks to the Margins Mac app on 127.0.0.1:47480 (the app must be running).
const API = "http://127.0.0.1:47480";

async function api(path, options = {}) {
  let r;
  try {
    r = await fetch(API + path, { ...options, headers: { ...(options.headers || {}), "X-Margins-Client": "extension" } });
  } catch (e) {
    throw new Error("Margins isn't running. Open the Margins app and try again.");
  }
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || `Margins answered ${r.status}`);
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

async function handle(msg) {
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
    default:
      throw new Error("unknown request");
  }
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  handle(msg).then(sendResponse, (e) => sendResponse({ error: String(e.message || e) }));
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
      chrome.action.setTitle({ title: "Margins: " + (e.message || e), tabId: tab.id });
    }
    return;
  }
  chrome.tabs.sendMessage(tab.id, { type: "togglePanel" }).catch(() => {});
});
