// Google Docs citing, the background part: signing in to Google (scope drive.file, asked once),
// giving Lunta each document in Google's picker (once per document), the Google Docs API, and Lunta's citation formatting
// through the app's extension endpoint (/cite/…, the same answers as the Apps Script sidebar
// gets from https://localhost:47481/api/…). The panel in the document (gdocs/panel.js) asks for
// everything here; the access token never reaches the page.
/* global LuntaCite, LuntaDocs, makeLuntaGDocs, LUNTA_GOOGLE_CLIENT_ID */
// In Safari on the Mac, Lunta for Mac does all of this instead (below, "Safari").

// drive.file: Lunta reaches only the documents the user gives it in Google's picker (non-sensitive;
// Google's verification refused the documents scope). See docs/google-docs-extension.md.
const GOOGLE_SCOPE = "https://www.googleapis.com/auth/drive.file";
// Google's picker for one document, on Lunta's site: an MV3 extension may not load Google's picker
// script, so this page does (site/picker.html). It is opened with identity.launchWebAuthFlow, gets
// the token, the document id and where to come back in its URL fragment (never sent to a server),
// and comes back to the extension's redirect URL with #picked=<id> or #error=<why>.
const PICKER_PAGE = "https://getlunta.app/picker.html";
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
    throw gdocsError("Lunta needs the permission to reach the Google Docs documents you give it to write citations (it only touches the document you cite in). Connect again and tick it.", "auth");
  }
  // another account: what the last one gave Lunta says nothing about this one
  if (selectAccount) await forgetGrants();
  const t = { token: h.get("access_token"), expires: Date.now() + (parseInt(h.get("expires_in") || "3600", 10) - 60) * 1000 };
  await saveToken(t);
  return t.token;
}

async function forgetGoogle() {
  const t = await savedToken();
  await saveToken(null);
  await forgetGrants();
  if (t) fetch("https://oauth2.googleapis.com/revoke?token=" + encodeURIComponent(t.token), { method: "POST" }).catch(() => {});
}

// ---- the documents given to Lunta in Google's picker (drive.file) ----
// Remembered in local storage (ids only) so a document given before that Google no longer opens
// for Lunta (another account; access taken back) is said so; the Docs API is always tried first.

const localStore = () => (typeof chrome !== "undefined" && chrome.storage && chrome.storage.local) || null;
let grantsInMemory = [];
async function grantedDocs() {
  const s = localStore();
  if (!s) return grantsInMemory;
  try { return (await s.get("grantedDocs")).grantedDocs || []; } catch (_) { return grantsInMemory; }
}
async function saveGrants(ids) {
  grantsInMemory = ids.slice(-500);
  const s = localStore();
  if (s) await s.set({ grantedDocs: grantsInMemory }).catch(() => {});
}
async function rememberGrant(docId) {
  const ids = (await grantedDocs()).filter((x) => x !== docId);
  ids.push(docId);
  await saveGrants(ids);
}
async function forgetGrant(docId) {
  const ids = await grantedDocs();
  if (ids.includes(docId)) await saveGrants(ids.filter((x) => x !== docId));
}
async function forgetGrants() { await saveGrants([]); }

const DOC_ID = /^[A-Za-z0-9_-]{20,}$/;

/** Google's picker on Lunta's site, set to this document; resolves once the user chose it. */
async function grantDocument(docId) {
  if (!DOC_ID.test(String(docId || ""))) throw gdocsError("This isn’t a Google Docs document.", "http");
  const id = identityAPI();
  if (!id) throw gdocsError("Safari on iPhone and iPad can’t sign in to Google for extensions.", "unsupported");
  const token = await googleToken(true);
  const back = id.getRedirectURL();
  const frag = new URLSearchParams({ doc: docId, token, back });
  let url;
  try {
    url = await id.launchWebAuthFlow({ url: `${PICKER_PAGE}#${frag}`, interactive: true });
  } catch (e) {
    throw gdocsError("Google’s window closed before you chose the document. Click Give Lunta access to this document again and click Select.", "access");
  }
  const h = new URLSearchParams(new URL(url).hash.slice(1));
  const err = h.get("error");
  if (err === "setup") throw gdocsError("Giving Lunta a document isn’t set up yet on getlunta.app (the Google Picker has no API key). Cite with the Apps Script sidebar meanwhile: Lunta › Citations in Word & Google Docs….", "setup");
  if (err === "cancelled") throw gdocsError("You closed Google’s window without choosing the document: Lunta still can’t open it. Click Give Lunta access to this document and click Select.", "access");
  if (err) throw gdocsError(`Google’s picker said: ${err}.`, "access");
  if (!h.getAll("picked").join(",").split(",").includes(docId)) {
    throw gdocsError("That wasn’t this document: choose this document in Google’s window (it is the one shown) and click Select.", "access");
  }
  await rememberGrant(docId);
  return { granted: true };
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
const APP_OPS = new Set(["status", "connect", "disconnect", "grant", "state", "update", "prepareInsert", "finishInsert", "details", "editCitation",
  "scanPlaceholders", "writePlaceholder", "scanZotero", "convertZotero"]);

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
      case "grant":
        return await grantDocument(msg.docId);
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
      default:
        try {
          const out = await gdocsFlow.op(msg);
          // opened: given to Lunta (here or in another browser with this account)
          if (msg.op === "state" && !(await grantedDocs()).includes(msg.docId)) await rememberGrant(msg.docId);
          return out;
        } catch (e) {
          if (e && e.kind === "access" && msg.docId && (await grantedDocs()).includes(msg.docId)) {
            // given before, but Google no longer opens it for this account
            await forgetGrant(msg.docId);
            e.message = "Lunta had this document before, but Google doesn’t open it for Lunta now (another Google account is connected, or the access was taken back). Give Lunta access to this document again, or use Switch Google account.";
          }
          throw e;
        }
    }
  } catch (e) {
    return { error: String((e && e.message) || e), kind: (e && e.kind) || "" };
  }
}
