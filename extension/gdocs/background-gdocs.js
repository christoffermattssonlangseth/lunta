// Google Docs citing, the background part: signing in to Google (the user's permission to edit
// their Google Docs documents, asked once), the Google Docs API, and Lunta's citation formatting
// through the app's extension endpoint (/cite/…, the same answers as the Apps Script sidebar
// gets from https://localhost:47481/api/…). The panel in the document (gdocs/panel.js) asks for
// everything here; the access token never reaches the page.
/* global LuntaCite, LuntaDocs, makeLuntaGDocs, LUNTA_GOOGLE_CLIENT_ID */
// In Safari on the Mac, Lunta for Mac does all of this instead (below, "Safari").

const GOOGLE_SCOPE = "https://www.googleapis.com/auth/documents";
const GOOGLE_AUTH = "https://accounts.google.com/o/oauth2/v2/auth";
const LUNTA_CITE = "http://127.0.0.1:47480/cite";

function identityAPI() {
  const b = typeof browser !== "undefined" && browser.identity;
  if (b && typeof b.launchWebAuthFlow === "function") return b;
  const c = typeof chrome !== "undefined" && chrome.identity;
  return c && typeof c.launchWebAuthFlow === "function" ? c : null;
}

function gdocsError(message, kind) {
  const e = new Error(message);
  e.kind = kind;
  return e;
}

// ---- the Google token (in session storage: gone when the browser quits) ----

let tokenInMemory = null;
const sessionStore = () => (typeof chrome !== "undefined" && chrome.storage && chrome.storage.session) || null;

async function savedToken() {
  if (tokenInMemory) return tokenInMemory;
  const s = sessionStore();
  if (!s) return null;
  try { return (await s.get("googleToken")).googleToken || null; } catch (_) { return null; }
}
async function saveToken(t) {
  tokenInMemory = t;
  const s = sessionStore();
  if (s) await (t ? s.set({ googleToken: t }) : s.remove("googleToken")).catch(() => {});
}

/**
 * An access token for the Docs API. Without `interactive`, only what needs no window (a saved
 * token, or Google answering at once because permission was given before); otherwise the sign-in
 * window. `selectAccount`: let the user pick another Google account.
 */
async function googleToken(interactive, selectAccount) {
  const saved = await savedToken();
  if (saved && saved.expires > Date.now() + 60000 && !selectAccount) return saved.token;
  if (!LUNTA_GOOGLE_CLIENT_ID) {
    throw gdocsError("Google Docs citing isn’t set up in this copy of the extension. Use the Apps Script sidebar meanwhile: Lunta › Citations in Word & Google Docs….", "setup");
  }
  const id = identityAPI();
  if (!id) {
    throw gdocsError("Safari on iPhone and iPad can’t sign in to Google for extensions. Cite with the Apps Script sidebar instead, or from Safari on a Mac with Lunta for Mac.", "unsupported");
  }
  const redirect = id.getRedirectURL();
  const q = new URLSearchParams({
    client_id: LUNTA_GOOGLE_CLIENT_ID, response_type: "token", redirect_uri: redirect, scope: GOOGLE_SCOPE, include_granted_scopes: "true",
  });
  if (!interactive) q.set("prompt", "none");
  else if (selectAccount) q.set("prompt", "select_account");
  let back;
  try {
    back = await id.launchWebAuthFlow({ url: `${GOOGLE_AUTH}?${q}`, interactive: !!interactive });
  } catch (e) {
    if (!interactive) throw gdocsError("Connect your Google account to cite in this document.", "auth");
    throw gdocsError(`Google sign-in didn’t finish (${e.message || e}). If Google said “redirect_uri_mismatch”, the OAuth client needs this redirect URL: ${redirect}`, "auth");
  }
  const h = new URLSearchParams(new URL(back).hash.slice(1));
  if (h.get("error")) {
    throw gdocsError(interactive ? `Google said: ${h.get("error")}.` : "Connect your Google account to cite in this document.", "auth");
  }
  if (!(h.get("scope") || "").split(" ").includes(GOOGLE_SCOPE)) {
    throw gdocsError("Lunta needs the permission to edit your Google Docs documents to write citations (it only touches the document you cite in). Connect again and tick it.", "auth");
  }
  const t = { token: h.get("access_token"), expires: Date.now() + (parseInt(h.get("expires_in") || "3600", 10) - 60) * 1000 };
  await saveToken(t);
  return t.token;
}

async function forgetGoogle() {
  const t = await savedToken();
  await saveToken(null);
  if (t) fetch("https://oauth2.googleapis.com/revoke?token=" + encodeURIComponent(t.token), { method: "POST" }).catch(() => {});
}

// ---- the Docs API ----

/** A Docs API request with the token ({status, json}); a token Google no longer takes: a new one, once. */
async function docsSend(method, url, body, again) {
  const token = await googleToken(false);
  const r = await fetch(url, {
    method, headers: { Authorization: "Bearer " + token, ...(body ? { "Content-Type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (r.status === 401 && !again) {
    await saveToken(null);
    return docsSend(method, url, body, true);
  }
  return { status: r.status, json: await r.json().catch(() => ({})) };
}

const gdocsAPI = makeLuntaGDocs.docsAPI((method, url, body) => docsSend(method, url, body));

// ---- Lunta ----

function luntaCite(path, body, timeout) {
  // this script's fetch (LuntaCite's own is the one of where it was loaded)
  const f = (url, init) => fetch(url, init);
  return LuntaCite.request(LUNTA_CITE + path, { body, timeout, fetch: f, headers: { "X-Margins-Client": "extension" } }).catch((e) => {
    if (e.kind === "offline") e.message = "Can’t reach Lunta. Open the Lunta app (for Mac, Windows or Linux) and try again.";
    else if (e.kind === "http" && e.status === 404) e.message = "This version of Lunta can’t format citations for the browser extension. Update Lunta and try again.";
    throw e;
  });
}

const gdocsFlow = makeLuntaGDocs({
  cite: LuntaCite, model: LuntaDocs, docs: gdocsAPI,
  lunta: {
    bibliography: (b) => luntaCite("/bibliography", b),
    render: (b) => luntaCite("/render", b, 90000),
    placeholders: (b) => luntaCite("/placeholders", b),
  },
});

// ---- Safari: Lunta for Mac does the Google part ----
// Safari has no identity API for extensions: Lunta for Mac signs in to Google (an OAuth client for
// installed apps, the token in the Mac's Keychain) and answers the panel's requests itself, with
// this same flow.js (apple/Mac/GoogleDocsScript.swift). The extension only passes them on.

const LUNTA_APP_GDOCS = "http://127.0.0.1:47480/gdocs";
const APP_OPS = new Set(["status", "connect", "disconnect", "state", "update", "prepareInsert", "finishInsert", "details", "editCitation",
  "scanPlaceholders", "writePlaceholder", "scanZotero", "convertZotero", "claimSites", "plainText"]);

/** Safari on the Mac (no sign-in for extensions; Lunta for Mac nearby). Not on iPhone and iPad. */
async function docsThroughApp() {
  if (identityAPI()) return false;
  /* global useNative */
  if (typeof useNative === "function" && (await useNative())) return false;
  return true;
}

async function askApp(msg) {
  let r;
  try {
    r = await fetch(LUNTA_APP_GDOCS, {
      method: "POST", headers: { "Content-Type": "application/json", "X-Margins-Client": "extension" }, body: JSON.stringify(msg),
    });
  } catch (_) {
    throw gdocsError("Can’t reach Lunta. Open Lunta for Mac and try again: in Safari, the Lunta app is what signs in to Google and writes the citations.", "offline");
  }
  const j = await r.json().catch(() => null);
  if (r.status === 404 && !(j && j.kind)) {
    throw gdocsError("This version of Lunta for Mac can’t cite in Google Docs for Safari. Update Lunta and try again.", "http");
  }
  if (!j) throw gdocsError(`Lunta answered ${r.status}.`, "http");
  if (!r.ok && !j.error) throw gdocsError(`Lunta answered ${r.status}.`, "http");
  return j;   // {error, kind} included: as the panel expects them
}

/** The panel's requests ({type: "gdocs", op, …}); answers {error, kind} instead of failing. */
async function handleGDocs(msg) {
  try {
    if (APP_OPS.has(msg.op) && (await docsThroughApp())) return await askApp(msg);
    switch (msg.op) {
      case "status": {
        const iOS = !identityAPI();   // here without the identity API: Safari on iPhone and iPad
        const out = { configured: !!LUNTA_GOOGLE_CLIENT_ID, supported: !iOS, connected: false, ...(iOS ? { iOS: true } : {}) };
        if (out.configured && out.supported) out.connected = await googleToken(false).then(() => true, () => false);
        out.lunta = await luntaCite("/status", null, 8000).then((s) => ({ papers: s.papers, api: s.api }), (e) => ({ error: e.message }));
        return out;
      }
      case "connect":
        await googleToken(true, !!msg.selectAccount);
        return { connected: true };
      case "disconnect":
        await forgetGoogle();
        return { connected: false };
      case "search":
        return { hits: await luntaCite("/search?q=" + encodeURIComponent(msg.q || "")) };
      case "styles":
        return { hits: await luntaCite("/styles?q=" + encodeURIComponent(msg.q || ""), null, 60000) };
      // Resolve Citations: Lunta's candidates for a placeholder, and adding one the writer confirmed
      case "suggest":
        return await luntaCite("/suggest", msg.query ? { query: String(msg.query) } : { cite: msg.cite || {} }, 60000);
      case "addPaper":
        return await luntaCite("/add", { meta: msg.meta || {} }, 60000);
      // converting Zotero's citations: the library's papers for their works
      case "match":
        return await luntaCite("/match", { items: Array.isArray(msg.items) ? msg.items : [] }, 60000);
      // Check Citations: one citation's sentence against its papers (the model takes a while), and
      // the paper opened at a passage in the app
      case "claims":
        return await luntaCite("/claims", msg.body || {}, 180000);
      case "openPaper":
        return await luntaCite("/open", { id: String(msg.id || ""), find: String(msg.find || "") }, 8000);
      default:
        return await gdocsFlow.op(msg);
    }
  } catch (e) {
    return { error: String((e && e.message) || e), kind: (e && e.kind) || "" };
  }
}
