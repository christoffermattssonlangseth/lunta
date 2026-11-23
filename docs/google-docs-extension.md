# Citing in Google Docs with the browser extension: what you set up

The extension (1.1.0 and later) cites in Google Docs without the Apps Script: a **Lunta** button on every
document opens a panel to search the library, insert citations and the bibliography, update them and pick
the style. It reads and writes the document through the **Google Docs API**, with the user's permission,
asked once per browser, and only for the documents the user gives Lunta in **Google's picker** (scope
`drive.file`, once per document; see "Giving Lunta a document" below). How it works and why it was done this way: [development.md](development.md#citing-in-google-docs-from-the-browser-extension).

For that the extension needs an **OAuth client of your own** in Google Cloud. Lunta ships no Google
credentials; until `extension/gdocs/config.js` has a client ID, the panel says Google Docs citing isn't set up
and points to the Apps Script sidebar (which keeps working either way).

Everything below happens in your browser; nothing here creates resources for you.

## 1. The Google Cloud project (once)

You can reuse the project made for Google Drive sync (development.md, "Setting up Google Cloud"), or make a
new one. A separate project keeps the Drive scope's (restricted) verification apart from this one, which is
simpler: **a new project, "Lunta Docs"**, is recommended.

1. https://console.cloud.google.com → project picker → **New project** → name `Lunta Docs` → Create.
2. **APIs & Services › Library** → search **Google Docs API** → **Enable**. Then search **Google Picker API** →
   **Enable** (the picker that gives Lunta a document, in the extension and in Lunta for Mac). No other API.

## 2. The consent screen (Google Auth Platform)

**Google Auth Platform › Branding**

| Field | Value |
|---|---|
| App name | `Lunta` |
| User support email | your address |
| App logo | `appstore/chrome/icon-128.png` (optional; a logo means brand verification, a few days) |
| Application home page | `https://getlunta.app/` |
| Application privacy policy link | `https://getlunta.app/app-privacy.html` |
| Application terms of service link | leave empty |
| Authorized domains | `getlunta.app` (see "Verification" below) |
| Developer contact | your address |

**Audience**: User type **External**. Publishing status: leave it in **Testing** while you try it, and add
your Google account(s) under **Test users** (at most 100).

**Data Access › Add or remove scopes** → filter `drive.file` → tick
`https://www.googleapis.com/auth/drive.file` ("See, edit, create and delete only the specific Google Drive files
you use with this app") → Update → Save. It is listed under **Your non-sensitive scopes**. **Remove**
`https://www.googleapis.com/auth/documents` if it is still there (Google's verification refused it, and the
code no longer asks for it). Nothing else (the extension asks for no e-mail or profile).

## 3. The OAuth client

**Google Auth Platform › Clients › Create client**

- Application type: **Web application** (one client for every browser; the extension signs in with
  `identity.launchWebAuthFlow`, which Chrome, Edge and Firefox all have).
- Name: `Lunta extension`.
- **Authorized JavaScript origins**: none.
- **Authorized redirect URIs**: one per place the extension is installed from. Each is
  `identity.getRedirectURL()` of that copy:

| Where | Redirect URI | Where the id comes from |
|---|---|---|
| Chrome Web Store | `https://<chrome-id>.chromiumapp.org/` | the item's id in the Developer Dashboard (32 letters a–p) |
| Edge Add-ons | `https://<edge-id>.chromiumapp.org/` | Edge gives the extension its own id: Partner Center › the extension › *Extension identity* (or `edge://extensions` after installing from the store) |
| Firefox Add-ons | `https://<hash>.extensions.allizom.org/` | install it, then `about:debugging#/runtime/this-firefox` › Lunta › **Inspect** › console: `browser.identity.getRedirectURL()` |
| Your unpacked copy (development) | `https://<unpacked-id>.chromiumapp.org/` | `chrome://extensions` (the id of "Load unpacked") |

Don't know an id yet (not published)? Add the others later: the client can be edited any time, and if a
redirect URI is missing the panel's error says "redirect_uri_mismatch" **and shows the exact URL to add**.

→ **Create**. Copy the **Client ID** (`…apps.googleusercontent.com`). The client secret isn't used (the
extension gets short-lived access tokens directly; there are no refresh tokens to keep) and the client ID isn't
secret.

## 3b. The API key for Google's picker (the extension)

The extension gives Lunta a document with the Google Picker on a page of Lunta's site,
`https://getlunta.app/picker.html` (`site/picker.html`; an MV3 extension may not load Google's picker script
itself). The Picker on a web page needs a browser API key:

1. **APIs & Services › Credentials › Create credentials › API key**. Rename it `Lunta picker`.
2. **Application restrictions**: **Websites** → add `https://getlunta.app/*`.
3. **API restrictions**: **Restrict key** → tick only **Google Picker API** → Save.
4. Put it in `site/picker.html` (`var API_KEY = "…";`, marked TODO) and publish the site (push to main). Such a
   key is meant to be in a web page (it only works from getlunta.app, and only for the picker); it isn't a secret.

The page also has the project's **number** as the picker's app id (`APP_ID = "535276504844"`, the Lunta Docs
project, from the Cloud console's project dashboard): what the user picks is given to that project's clients. If
the project changes, change it there too. Until the key is in, the panel's "Give Lunta access to this document"
says it isn't set up yet.

Lunta for Mac (Safari) needs no key: it uses Google's picker for desktop apps during the sign-in (below).

## 4. Put the client ID in the extension

`extension/gdocs/config.js`:

```js
var LUNTA_GOOGLE_CLIENT_ID = "1234567890-abcdefg.apps.googleusercontent.com";
```

Commit it; it goes into the release's extension zip (Chrome, Edge, Firefox) and into the Safari extension
(where it is unused: in Safari, Lunta for Mac signs in with its own client, see "Safari" below). Reload the unpacked extension, open a Google Docs document, click the Lunta button
(bottom right) › **Connect Google account**, then **Give Lunta access to this document** › **Select** in Google's
window. While the app is in Testing, Google shows "Google hasn't verified this app": *Continue* (only test users can).

Try it against [citations-testing.md](citations-testing.md) ("Google Docs with the browser extension").

## 5. Publishing to everyone: Google's verification

`drive.file` is a **non-sensitive** scope: public use needs only Google's brand verification (name, logo, home
page, privacy policy), no scope justification or demo video. (The project first asked for `documents`, a sensitive
scope; Google's verification refused it and asked for `drive.file` with the Picker, Nov 2026.)

What Google checks, and what to prepare:

1. **Your domain.** The home page and privacy policy must be on a domain you've verified in
   [Google Search Console](https://search.google.com/search-console) with the same Google account that owns the
   Cloud project. Lunta's site is on its own domain, `getlunta.app` (Cloudflare DNS → GitHub Pages), verified as a
   *Domain* property with a DNS TXT record. Google refused the shared github.io address ("homepage not
   registered to you") even with that address verified as a URL prefix, which is why the site moved (20 Nov 2026).
2. **The home page** must say what Lunta is and link the privacy policy. `site/index.html` does; add one
   sentence about citing in Google Docs.
3. **The privacy policy** must say what the app does with Google user data, with Google's Limited Use
   wording. `site/app-privacy.html` has a section "Citing in Google Docs with the browser extension" for this:
   publish the site (push to main) before submitting, and keep it in step with what the extension does.
4. If Google still asks how the scope is used (e.g. in the open verification thread): Lunta's extension asks
   for `drive.file` only; the user gives it each document in the Google Picker (opened set to that document with
   `DocsView.setFileIds`), and the extension then reads and edits only that document with the Docs API, when the
   user clicks a Lunta button.
5. **Audience › Publish app** (to "In production"), then **Verification Center › Prepare for verification** and
   submit. Google answers by e-mail (from the Trust & Safety team), typically within days, sometimes a few weeks;
   answer their questions in the same thread.

Brand verification (logo, name) is part of it; keep the app name "Lunta" as in the stores.

## 6. Store listings

- **Chrome Web Store / Edge**: the new permissions (`identity`, `storage`) and texts are in
  [chrome-web-store.md](chrome-web-store.md). Upload 1.1.0 after the client ID is in `config.js` (or before, if
  you want to publish the rest first: without a client the panel only points to the Apps Script).
- **Firefox**: same zip. The manifest keeps `data_collection_permissions: none`: document content goes only
  between the browser and Google Docs (where the document lives) and the Lunta app on the user's own computer, not
  to the developer or third parties. Notes to reviewer: "Google Docs citing uses identity.launchWebAuthFlow with
  Google OAuth (scope drive.file) and the Google Docs API on the document the user cites in, which the user gives
  the extension in the Google Picker on https://getlunta.app/picker.html (opened with launchWebAuthFlow)."

## Safari: the OAuth client of Lunta for Mac

Safari has no `identity` API for extensions, and it doesn't let the panel paste into Docs itself. So in Safari
on the Mac, **Lunta for Mac does the Google part**: it signs in to Google itself and makes the Docs API calls for
the extension (how: [development.md](development.md#citing-in-google-docs-from-the-browser-extension), "Safari").
For that the app needs **a second OAuth client, of type Desktop app**, in the same project. Until it has one,
the panel in Safari says Google Docs citing isn't set up yet there and points to the Apps Script sidebar.

Do this once the project's verification (step 5) is through, or before it while trying it out as a test user:

1. https://console.cloud.google.com → the **Lunta Docs** project → **Google Auth Platform › Clients** →
   **Create client**.
2. Application type: **Desktop app**. Name: `Lunta for Mac`. → **Create**.
   There are no redirect URIs to enter: a Desktop client takes any `http://127.0.0.1:<port>` redirect, which is
   what the app uses (a free port each time, answered by the app itself; RFC 8252, with PKCE).
3. Copy the **Client ID** and the **Client secret** Google shows. Google calls the second one a secret, but for a
   Desktop client it isn't one (it ships inside the app, and Google's documentation for installed apps says so);
   Google still asks for it when the app fetches tokens, so it goes in too.
4. Put both in `apple/Mac/GoogleDocsAuth.swift`:

   ```swift
   enum GoogleDocsClient {
       static let id = "1234567890-abcdefg.apps.googleusercontent.com"
       static let secret = "GOCSPX-…"
   }
   ```

   and build Lunta for Mac. Nothing else changes: same consent screen, same scope (`drive.file` only), same
   verification (the consent screen is the project's; a new client doesn't need verifying again). If the app is
   still in **Testing**, only test users can connect.
5. Try it: open a Google Docs document in Safari (with Lunta's Safari extension on) › the Lunta button ›
   **Connect Google account**. Your default browser opens Google's page with Google's file picker showing that
   document; allow it and choose the document; the page says Lunta is connected and the panel follows. Then [citations-testing.md](citations-testing.md), "Google Docs with the
   browser extension in Safari".

The refresh token stays in the Mac's Keychain (item "Lunta: Google Docs"); access tokens only in the app's
memory. Disconnect (or a permission taken back in the Google account) removes it.

## Giving Lunta a document (`drive.file`)

With `drive.file`, Google lets Lunta's clients open only files the user picked *for Lunta* in Google's file picker
(or that Lunta created). A document opened in Google Docs isn't one, so the first time in each document:

1. The panel tries the document. Google answers 404 (or 403); the panel shows **Give Lunta access to this
   document** (never a silent failure).
2. **Chrome, Edge, Firefox**: the background opens `https://getlunta.app/picker.html` with
   `identity.launchWebAuthFlow`, with `#doc=<id>&token=<access token>&back=<the extension's redirect URL>` in the
   URL **fragment** (never the query: a fragment isn't sent to any server; the page removes it from the address at
   once and logs nothing). The page loads Google's Picker (`DocsView().setFileIds(<id>)`, list mode, `setAppId`,
   `setOAuthToken`, `setDeveloperKey`); the user clicks the document (the only one shown) and **Select**. The page
   goes to `back#picked=<id>` (or `#error=cancelled|setup`), which ends `launchWebAuthFlow`; it goes back only to
   a `*.chromiumapp.org` / `*.extensions.allizom.org` address, so no other site can use it.
   Why `launchWebAuthFlow`: it opens a window the extension controls and returns when that window reaches the
   redirect URL, in Chrome, Edge and Firefox alike, with no `window.opener`, message origin checks or content script
   on our site.
3. **Safari (Lunta for Mac)**: the app's sign-in shows Google's picker for desktop apps during consent
   ("OnePick": `trigger_onepick=true&file_ids=<id>`; the same as `core/src/drive/auth.rs` `picker_url`), already
   from **Connect Google account** in a document, and again from **Give Lunta access to this document**. Google's
   redirect to the app's loopback address carries `picked_file_ids`. No API key and no page of ours; OnePick is
   documented for desktop and mobile clients only, which is why the extension (a *Web application* client with
   the implicit flow) uses the hosted page instead.
4. Then the panel loads as before. The ids of documents given (or opened) are remembered (the extension:
   `chrome.storage.local` `grantedDocs`; the Mac: UserDefaults `GoogleDocsGrantedDocuments`), only so that a
   document given before that Google no longer opens (another account; access taken back) is said so. The Docs
   API is always tried first, so a document given in another browser works at once. Disconnecting or switching
   account forgets them.

A document Lunta may read but the account may only view or comment: writing fails with 403, said so ("can't edit
this document"), no picker. Co-authors each give the document once, in their own browser.

## The Apps Script sidebar

`Code.gs` has `@OnlyCurrentDoc`, so Apps Script asks only for `documents.currentonly` (the document the script is
bound to), not every document. It runs under Apps Script's own project, not Lunta Docs, and isn't part of this
verification.
