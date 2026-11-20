# Citing in Google Docs with the browser extension: what you set up

The extension (1.1.0 and later) cites in Google Docs without the Apps Script: a **Lunta** button on every
document opens a panel to search the library, insert citations and the bibliography, update them and pick
the style. It reads and writes the document through the **Google Docs API**, with the user's permission,
asked once per browser. How it works and why it was done this way: [development.md](development.md#citing-in-google-docs-from-the-browser-extension).

For that the extension needs an **OAuth client of your own** in Google Cloud. Lunta ships no Google
credentials; until `extension/gdocs/config.js` has a client ID, the panel says Google Docs citing isn't set up
and points to the Apps Script sidebar (which keeps working either way).

Everything below happens in your browser; nothing here creates resources for you.

## 1. The Google Cloud project (once)

You can reuse the project made for Google Drive sync (development.md, "Setting up Google Cloud"), or make a
new one. A separate project keeps the Drive scope's (restricted) verification apart from this one, which is
simpler: **a new project, "Lunta Docs"**, is recommended.

1. https://console.cloud.google.com → project picker → **New project** → name `Lunta Docs` → Create.
2. **APIs & Services › Library** → search **Google Docs API** → **Enable**. (No other API is needed.)

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

**Data Access › Add or remove scopes** → filter `documents` → tick
`https://www.googleapis.com/auth/documents` ("See, edit, create and delete all your Google Docs documents")
→ Update → Save. It is listed under **Your sensitive scopes**. Nothing else (the extension asks for no e-mail,
profile or Drive access).

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

## 4. Put the client ID in the extension

`extension/gdocs/config.js`:

```js
var LUNTA_GOOGLE_CLIENT_ID = "1234567890-abcdefg.apps.googleusercontent.com";
```

Commit it; it goes into the release's extension zip (Chrome, Edge, Firefox) and into the Safari extension
(where it is unused: in Safari, Lunta for Mac signs in with its own client, see "Safari" below). Reload the unpacked extension, open a Google Docs document, click the Lunta button
(bottom right) › **Connect Google account**. While the app is in Testing, Google shows "Google hasn't verified
this app": *Continue* (only test users can).

Try it against [citations-testing.md](citations-testing.md) ("Google Docs with the browser extension").

## 5. Publishing to everyone: Google's verification

`documents` is a **sensitive** scope (not restricted): public use needs Google's OAuth app verification, but
**no** security assessment (that's only for restricted scopes like full Drive). Until it's verified, at most 100
people can sign in, and each sees the "unverified app" warning; so verify before announcing it.

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
4. **Scope justification** (Data Access › the scope › justification), paste:

   ```
   Lunta is a reference manager. Its browser extension inserts citations and a bibliography into the Google Docs
   document the user is writing, and renumbers and reformats them (Update All, citation style changes). This needs
   to read the document's content (to find the existing citations, which are links in the text, footnotes and
   tabs, and the bibliography paragraphs) and to edit it (insert and replace citation text, rewrite the
   bibliography, keep the cited papers' details in named ranges). The extension only acts on the document open in
   the tab where the user clicks a Lunta button, never in the background. drive.file is not enough: the user opens
   documents in Google Docs, not through our app, so drive.file would not grant access to them; documents.readonly
   cannot write the citations.
   ```
5. **A demo video** (unlisted YouTube link): record the screen in English, showing (a) the extension's Lunta
   button in a Google Doc › Connect Google account, the whole consent screen with the **client ID visible in the
   browser's address bar** (zoom in on it), (b) inserting a citation and a bibliography, changing the style and
   Update All, so the scope's use is visible. 2–3 minutes is plenty.
6. **Audience › Publish app** (to "In production"), then **Verification Center › Prepare for verification** and
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
  Google OAuth (scope documents) and the Google Docs API on the document the user cites in."

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

   and build Lunta for Mac. Nothing else changes: same consent screen, same scope (`documents` only), same
   verification (the consent screen is the project's; a new client doesn't need verifying again). If the app is
   still in **Testing**, only test users can connect.
5. Try it: open a Google Docs document in Safari (with Lunta's Safari extension on) › the Lunta button ›
   **Connect Google account**. Your default browser opens Google's page; allow it; the page says Lunta is
   connected and the panel follows. Then [citations-testing.md](citations-testing.md), "Google Docs with the
   browser extension in Safari".

The refresh token stays in the Mac's Keychain (item "Lunta: Google Docs"); access tokens only in the app's
memory. Disconnect (or a permission taken back in the Google account) removes it.

## The narrower scope (`drive.file`), for later

`drive.file` is non-sensitive (no verification beyond the brand), but it only covers files the app created or
the user picked *for the app*, through Google's file picker or Drive's "Open with". A document opened in Google
Docs isn't one. It would work like this: the first time in each document, the panel opens a small page of ours
(hosted, since the picker is a remote Google script an MV3 extension may not load) with the Google Picker set to
that document; the user clicks **Select**, and from then on the extension may edit it. One more dialog per
document, a hosted page to keep working, and co-authors each picking the file once. If verification of
`documents` proves a problem, that's the fallback; the code change is the scope, the picker page and handling
"not granted yet" (403) by offering it.
