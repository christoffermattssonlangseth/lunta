// Google Docs citing, the background part: signing in to Google (the user's permission to edit
// their Google Docs documents, asked once), the Google Docs API, and Lunta's citation formatting
// through the app's extension endpoint (/cite/…, the same answers as the Apps Script sidebar
// gets from https://localhost:47481/api/…). The panel in the document (gdocs/panel.js) asks for
// everything here; the access token never reaches the page.
/* global LuntaCite, LuntaDocs, makeLuntaGDocs, LUNTA_GOOGLE_CLIENT_ID */

const GOOGLE_SCOPE = "https://www.googleapis.com/auth/documents";
const GOOGLE_AUTH = "https://accounts.google.com/o/oauth2/v2/auth";
const DOCS_API = "https://docs.googleapis.com/v1/documents/";
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
    throw gdocsError("This browser can’t sign in to Google for extensions (Safari). Cite with the Apps Script sidebar instead: Lunta › Citations in Word & Google Docs….", "unsupported");
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

async function docsCall(method, path, body, again) {
  const token = await googleToken(false);
  let r;
  try {
    r = await fetch(DOCS_API + path, {
      method, headers: { Authorization: "Bearer " + token, ...(body ? { "Content-Type": "application/json" } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch (_) {
    throw gdocsError("Google Docs couldn’t be reached. Check the internet connection and try again.", "network");
  }
  if (r.status === 401 && !again) {
    await saveToken(null);
    return docsCall(method, path, body, true);
  }
  const j = await r.json().catch(() => ({}));
  if (r.ok) return j;
  const msg = (j.error && j.error.message) || `Google Docs answered ${r.status}.`;
  if (r.status === 400 && /revision/i.test(msg)) throw gdocsError(msg, "revision");
  if (r.status === 401) throw gdocsError("Connect your Google account to cite in this document.", "auth");
  if (r.status === 403 || r.status === 404) {
    throw gdocsError("The Google account connected to Lunta can’t edit this document (another account owns it, or you can only view or comment). Use Switch account in Lunta’s panel, or ask for edit access.", "access");
  }
  throw gdocsError(`Google Docs: ${msg}`, "http");
}

const gdocsAPI = {
  get: (docId) => docsCall("GET", encodeURIComponent(docId) + "?includeTabsContent=true"),
  batchUpdate: (docId, requests, revisionId) =>
    docsCall("POST", encodeURIComponent(docId) + ":batchUpdate", { requests, writeControl: { requiredRevisionId: revisionId } }),
};

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
  },
});

const DOC_ID = /^[A-Za-z0-9_-]{20,}$/;

/** The panel's requests ({type: "gdocs", op, …}); answers {error, kind} instead of failing. */
async function handleGDocs(msg) {
  try {
    if (msg.docId !== undefined && !DOC_ID.test(String(msg.docId))) throw gdocsError("This isn’t a Google Docs document.", "http");
    switch (msg.op) {
      case "status": {
        const out = { configured: !!LUNTA_GOOGLE_CLIENT_ID, supported: !!identityAPI(), connected: false };
        if (out.configured && out.supported) out.connected = await googleToken(false).then(() => true, () => false);
        out.lunta = await luntaCite("/status", null, 8000).then((s) => ({ papers: s.papers }), (e) => ({ error: e.message }));
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
      case "state":
        return await gdocsFlow.state(msg.docId);
      case "update":
        return await gdocsFlow.update(msg.docId, { style: msg.style, styleTitle: msg.styleTitle });
      case "prepareInsert": {
        // nothing pasted unless the document can be written afterwards
        const st = await gdocsFlow.state(msg.docId);
        if (!msg.ids && st.hasBibliography) return { exists: true };
        const token = LuntaDocs.newToken();
        const ids = msg.ids || null;
        return { token, html: LuntaDocs.placeholderHTML(token, ids, "(…)"), url: LuntaDocs.placeholderUrl(token, ids), text: "(…)" };
      }
      case "finishInsert":
        return await gdocsFlow.finishInsert(msg.docId, msg.token, { style: msg.style, styleTitle: msg.styleTitle });
      default:
        throw gdocsError("unknown request", "http");
    }
  } catch (e) {
    return { error: String((e && e.message) || e), kind: (e && e.kind) || "" };
  }
}
