<p align="center">
  <img src="design/logo-1024.png" width="140" alt="Lunta logo">
</p>

<h1 align="center">Lunta</h1>

<p align="center">A reference manager for Mac and iPhone (and, new, Windows and Linux): collect papers, read them comfortably on the phone,
highlight and annotate, search everything, and export citations.</p>

<p align="center"><em>Lunta</em> is Swedish for a thick bundle of papers. (Formerly called Margins.)</p>

---

The library syncs between your Mac and iPhone through iCloud Drive. The desktop app for Windows and
Linux keeps the same library in any synced folder (Google Drive for desktop, Dropbox, OneDrive) or a
local one; see [Desktop app](#desktop-app-windows-and-linux). Devices that cannot keep a synced folder
(Android, or anyone who prefers it) can sync the same library through the Google Drive API; see
[Google Drive sync](#google-drive-sync).

## Features

- **Library:** add papers by DOI, link, PMCID or PDF (drag and drop on the Mac; share sheet on iPhone,
  from Safari, Mail or Files); metadata looked up from Crossref, Europe PMC, arXiv and OpenAlex, or read
  from the PDF's first page on-device; edit any field. Import from ReadCube Papers, BibTeX and RIS.
  Give a paper its PDF yourself: drop it on the paper (in the list or its details) or use **Add PDF…**
  (Mac) / **Add it from Files** (iPhone); **Replace PDF…** swaps it.
- **Organise:** folders with subfolders (drag papers and folders in the Mac sidebar), tags, stars,
  read/unread. **Smart folders** fill themselves from rules (words, tag, folder, year range, recently
  added, unread/starred, PDF or not, highlighted). Folders & Tags is one tap away on iPhone (swipe, long-press, or the reader's ⋯ menu). The iPhone
  library works like Mail: a list of places (all, starred, unread, New for You, folders, smart folders,
  tags), each opening its papers; swipe back from the left edge to return to the list. Tapping a paper
  opens its info page (abstract, folders, cite) with a **Read** button; delete a paper there, by
  swiping left or from its long-press menu. A paper found through New for You
  or Discover has **Read Paper** and **Show in Library** once added.
- **Read:** a reflowable reading view made from the paper (open-access XML from PMC/Europe PMC, arXiv's
  own HTML with real equations for arXiv papers, or the PDF itself) (large type on iPhone, zoomable figures, wide
  tables scroll in place) next to the original PDF. **Scroll or turn pages**: page mode lays the paper
  out as pages (two side by side on a wide Mac window); swipe or tap the edges on iPhone, arrow keys,
  space or a swipe on the Mac. Pinch to change the text size on iPhone. **Highlights and notes** in four colours, in the reading view
  and in the PDF (select text, pick a colour); a highlight made in one shows in the other; the
  **reading position syncs**, so a paper opens where you stopped on the other device.
- **Listen:** natural-voice narration (Kokoro) on the Mac and on the iPhone, following along in the text.
- **Clean up:** *Clean Up Library…* finds papers with missing or broken details (fixed from the DOI, or
  read from the PDF on the device), duplicates (merged, keeping tags, folders, highlights and the PDF), and
  reading views whose words ran together (remade).
- **Discover:** search 250 million papers on OpenAlex, add with one click, open what you added.
- **New for You:** a daily feed of new papers picked for your research. Describe your work in a few
  sentences (or let Lunta draft it from your library). Each day it casts a wide net (OpenAlex
  searches written from your paragraph, papers citing recent ones in your library, authors you follow,
  and the last days of bioRxiv and medRxiv), ranks the candidates on the device with sentence
  embeddings against your interests, your library and your past Add / 👍 More Like This / Not for Me
  choices, and has the on-device model read the best abstracts (with the papers you thumbed up as
  examples), keep only clear matches and say why each one fits.
  Authors you follow and papers citing ones you watch always show up. Each paper gets a one-sentence
  summary, and the top of the feed has a short **In short** digest of all of it (written on the Mac), with the papers it mentions linked. One notification a day.
- **Cite:** 12 styles (APA 7, Harvard, Chicago, Cell, MLA 9, Vancouver, IEEE, AMA, Nature, Science, PNAS, ACS), BibTeX, RIS, Markdown; share PDFs, EPUBs and citations. **Word and
  Google Docs add-ins** insert citations from the library and build the reference list.
- **Ask (✨):** an assistant that runs on the device: Apple Intelligence, or the open Qwen3 4B model where
  Apple Intelligence isn't available (e.g. iPhones in the EU). Ask about one paper (answers cite the
  passages, tap to jump there; it knows your highlights) or the whole library: find, compare and
  summarise papers, summarise a folder, find what's missing in it plus new papers that fill the gaps,
  and propose tags and folders for you to confirm. Conversations are kept for three days.
- **Windows and Linux (first version):** library (all / starred / unread, folders, smart folders, tags,
  full-text search, sort), details with folders, tags, star and read, citations (APA, Vancouver, Nature,
  BibTeX, RIS) to the clipboard; add papers by DOI, PMCID, arXiv id or link (reading view made by the
  Rust converter); give a paper its PDF (Add PDF… or drop it on the paper); the same reading view as the
  Apple apps (scroll or pages, text size, highlights in four colours with notes, synced reading position)
  and the PDF (PDF.js).
- **Agents:** an MCP server so Claude and other agents can search, read, cite and organise your library.

## Architecture

```
core/                  Rust: library storage + sync layout, metadata lookup (Crossref, Europe PMC,
                       arXiv, OpenAlex), open-access PDF finder, annotations, full-text search
                       (SQLite FTS5), BibTeX/RIS import and export, citations, EPUB reading
core/src/convert/      Rust port of the paper → EPUB converter (DOI, PMCID, PubMed, arXiv, JATS XML,
                       PDF via PDFium) for future Linux/Windows/Android apps; features "convert" and
                       "pdf", not in the Apple build. CLI: cargo run --bin paper2epub -- <input> [out-dir]
core/src/drive/        Google Drive sync for devices without a synced folder (Android, Linux, ...):
                       RemoteStore trait, Drive REST client + OAuth (PKCE), Syncer; feature "drive",
                       not in the Apple build. CLI: cargo run --bin lunta-sync -- connect|sync|status
android/               Android app (Kotlin, Jetpack Compose, Material 3) on the core through UniFFI Kotlin bindings;
                       reader scripts reused from desktop/src/reader/generated/
desktop/               Windows / Linux app (Tauri 2): Rust backend on the core (src-tauri/), plain
                       TypeScript + Vite frontend (src/), reader scripts extracted from the Swift (src/reader/)
apple/LuntaCore/     Swift package wrapping the Rust core (generated by UniFFI)
apple/Paper2EpubKit/   Swift converter: paper (link, DOI, PDF, JATS XML) → reflowable EPUB, on-device
apple/Mac/             macOS app (SwiftUI): library, reader, browser-extension and citation endpoints
apple/iOS/             iPhone app
apple/ShareExtension/  iPhone share extension (PDFs, links, DOIs → the app via an App Group inbox)
apple/Shared/          code shared by the Mac and iPhone apps (reader, Ask, highlights, folders, …)
apple/citations/       Word add-in, Google Docs Apps Script and the formatting code they share
tts/                   Kokoro narration helper for the Mac (mlx-audio)
extension/             Chrome extension: save papers and PDFs from article pages
converter/             the original Python paper2epub tool (pandoc-based) and its Claude skill
design/                logo (SVG + 1024 px PNG) and its dark / tinted variants; Mac icon = Icon Composer bundle apple/Lunta.icon (macOS 26 light/dark/tinted), iPhone icon has dark + tinted appearances
scripts/build-core.sh  builds the Rust core for iOS, the simulator and macOS → XCFramework
scripts/build-core-android.sh  builds the core (convert, pdf, drive) for Android + Kotlin bindings + PDFium
scripts/extract-reader-assets.mjs  copies the reader CSS/JS out of the Swift into desktop/src/reader/generated/
scripts/make-desktop-icon.swift    the desktop icon (Mac artwork on the macOS icon grid) for `tauri icon`
```

### Library layout (sync-safe)

```
<library>/papers/<id>/paper.pdf     original PDF (if any)
<library>/papers/<id>/book.epub     reflowable version made by the converter
<library>/papers/<id>/fulltext.txt  plain text for search
<library>/ops/<device-id>.jsonl     append-only change log, one per device
<library>/positions/<device>.json   last reading position per paper (changes often, so not in the log;
                                    {paper: {offset, at (seconds since 2001), device}}, as Swift writes it)
<library>/smart-folders.json        smart folder rules
<library>/watches.json              watched authors, topics and citations
<library>/foryou.json               New for You: your interests, the feed, Add / More Like This / Not for Me feedback
```

Blobs are written once. Every change (metadata, read/starred, tags, collections, highlights and
notes) is an entry in the writing device's own log, so devices never write the same file and iCloud
never sees a conflict. Each device replays all logs (newest change wins) and keeps its own search
index in its cache folder.

### Reading: one paper, two views

Each paper keeps its **original PDF** and gains a **reflowable reading view** (the converted EPUB,
rendered in-app with WebKit). Both apps offer both views:

- iPhone: reading view by default (text reflows to the screen), PDF one tap away.
- Mac: PDF by default, reading view one tap away.

The reading view scrolls or turns pages (⋯ › Layout on iPhone, the book button or ⌥⌘P on the Mac).
Page mode cuts the page into screen-sized CSS columns (`apple/Shared/PageTurn.swift`); on iPhone the
web view's scroll view pages natively, so a page follows your finger. Contents, narration, highlights
and the synced reading position all turn to the right page.

There is no conversion back to PDF: the original PDF is kept, so nothing is lost. Highlights and
notes store the quoted text with a little context (`quote`, `prefix`, `suffix`) and a character offset,
so a highlight made on the phone finds the same words on the Mac whatever the screen width. Select text,
then **Highlight** / **Add Note** (right-click on the Mac, or ⇧⌘H; the edit menu on iPhone). Tap a
highlight to change its colour or note; *Highlights & Notes* lists them and copies them as Markdown.

## AI agents (MCP)

Lunta ships an [MCP](https://modelcontextprotocol.io) server, `lunta-mcp`, inside the Mac app, so AI agents can work with
your library: Claude Code, Claude Desktop, Cursor or any MCP client. Connect one with **Lunta › Connect AI Agents…**, or by hand:

```sh
claude mcp add --scope user lunta -- /Applications/Lunta.app/Contents/MacOS/lunta-mcp
```

Tools: `search_papers`, `list_papers`, `get_paper`, `read_paper` (full text, or the passages matching a query), `cite`
(APA, Vancouver, Nature, BibTeX, RIS), `library_overview`, `find_new_papers` (OpenAlex), `add_paper`, `find_pdf`,
`tag_papers`, `add_to_collection`, `set_status`, `add_note`, `update_paper`.

The agent writes through its own change log (`ops/agent-….jsonl`), exactly like another device, so it can work while the
apps are open and its changes sync to the Mac and iPhone. The same binary is a command-line tool:

```sh
lunta search csf lactate
lunta read <id> "how many patients"
lunta cite <id> <id> --style vancouver
lunta path <id> pdf
```

Inside the apps, **Ask** (✨) runs a small agent on the device with the same kind of tools: Apple's
on-device model, or Qwen3 4B (4-bit, via MLX; a one-time 2.3 GB download) when Apple Intelligence is off
or unavailable. Pick the model in the panel's ⋯ menu.

## Citations in Word and Google Docs

**Lunta › Citations in Word & Google Docs…** sets them up. Both talk to the Mac app over
`https://localhost:47481` (a certificate made for this Mac only; trusting it needs your password once).

- **Word:** *Install in Word*, restart Word, then **Cite** on the Home tab. Search, pick papers, *Insert
  Citation*; *Insert Bibliography* where the references go; *Update All* after edits or a style change.
  Citations are content controls tagged with the paper ids, so they survive editing and renumber.
- **Google Docs:** Extensions › Apps Script; paste `Code.gs` and an HTML file `Sidebar` (both copied from
  the Lunta window; the sidebar carries a pairing key), reload, then **Lunta › Cite…**.

Styles: APA 7, Harvard, Chicago (author-date), Cell, MLA 9, Vancouver, IEEE, AMA, Nature, Science, PNAS and ACS, and in Word any of the ~10,000 journal styles of the [Citation Style Language](https://citationstyles.org) catalogue (*Find journal style…*: fetched once from zotero.org/styles, formatted in the core with [hayagriva](https://github.com/typst/hayagriva)). Lunta must be open while you cite.

## Build

```sh
brew install rustup xcodegen && rustup default stable
rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin
./scripts/build-core.sh                # Rust core → XCFramework + Swift bindings
cd apple && xcodegen generate && open Lunta.xcodeproj   # set your own DEVELOPMENT_TEAM in project.yml
```

Tests:

```sh
cd core && cargo test                  # core tests (cargo test -- --ignored for online lookups)
cd core && cargo run --release --bin paper2epub -- PMC7029158 out/   # Rust converter
cd core && cargo run --release --bin paper2epub -- --offline paper.pdf out/   # PDF, no lookups
PDFIUM_PATH=/path/to/libpdfium.dylib cargo test pdf -- --ignored   # PDF tests that need PDFium
cd apple/LuntaCore && swift test     # Swift ↔ Rust round trip
```

### PDFium (PDF conversion in the Rust converter)

The Rust converter reads PDFs with [PDFium](https://pdfium.googlesource.com/pdfium/) through
`pdfium-render` (feature `pdf`, on by default for `cargo build/test/run`, off in
`scripts/build-core.sh`: the Apple apps convert PDFs with PDFKit and never need PDFium). PDFium is
loaded when the first PDF is converted, from the first of:

1. `PDFIUM_PATH`: the library file, or the folder holding it;
2. next to the executable (or in `../lib`, `../Frameworks`, `../Resources`, `lib` from it);
3. the system library search path.

If none has it, converting a PDF fails with a message saying so; everything else still works.
Nothing is downloaded at build time and no binary is in the repository. Prebuilt libraries:
[bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases) (tested with
`chromium/8152`; `pdfium-render` is built for the 8125+ API). For local work, e.g. on a Mac:

```sh
mkdir -p ~/pdfium && curl -L https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-mac-arm64.tgz | tar xz -C ~/pdfium
PDFIUM_PATH=~/pdfium/lib cargo run --release --bin paper2epub -- --offline paper.pdf out/
```

How apps should ship it (include PDFium's `LICENSE`, BSD-3-Clause, and its `licenses/`):

- **Windows:** `pdfium-win-x64.tgz` (or `-arm64`); put `bin/pdfium.dll` next to the app's `.exe`.
- **Linux:** `pdfium-linux-x64.tgz` (`-arm64`; `-musl-x64` for Alpine); put `lib/libpdfium.so`
  next to the executable, or in the AppImage/Flatpak `lib/` (also found via `LD_LIBRARY_PATH`), or
  point `PDFIUM_PATH` at a distribution-installed copy.
- **Android:** `pdfium-android-arm64.tgz` (plus `-arm`, `-x64` for other ABIs); put `libpdfium.so`
  in `jniLibs/<abi>/`. The app's native-library folder is on the linker path, so it is found as a
  system library.
- **macOS (outside the Apple apps):** `pdfium-mac-arm64.tgz` / `-x64`; `libpdfium.dylib` in
  `Contents/Frameworks` of a bundle, or next to a command-line binary.

`P2E_DEBUG=1` prints the analysed paragraphs (same format as Paper2EpubKit's); `P2E_DEBUG_LINES=<page>`
and `P2E_DEBUG_CHARS=<page>` dump PDFium's lines and characters for one page.

### Browser extension

Chrome → `chrome://extensions` → Developer mode → Load unpacked → `extension/`. The Lunta Mac app
must be running; the extension talks to it on `127.0.0.1:47480` (extension requests only).

## Google Drive sync

`core/src/drive/` (cargo feature `drive`, on by default for desktop and the CLI, off in the Apple build)
syncs a library through Google Drive's API, for devices that have no synced folder: the Android app,
Linux, or anyone who prefers it. It uses **the same layout** as everything else (see
[Library layout](#library-layout-sync-safe)), in a Drive folder called `Lunta`, so a Mac whose library
folder lives inside "Google Drive for desktop" and API devices share one library.

```
 working folder (the Library opens it)  <-- Syncer -->  RemoteStore: GoogleDrive | LocalDirRemote
```

**How it syncs** (details in `core/src/drive/syncer.rs`):

- Each device writes only its own `ops/<device>.jsonl` and `positions/<device>.json`. The syncer uploads
  its own log (the whole file, when it grew), downloads the others', and **never uploads or modifies
  another device's log**. If the remote copy of the device's own log holds lines the local one lost
  (reinstall with the same device id), they are merged back, never dropped.
- `papers/<id>/*` is synced both ways, keyed by paper id: new or changed files go up or down, and a file
  deleted on one side is deleted on the other (unless the other side changed it since: an edit beats a
  delete). A paper file is written once or replaced whole, so there is nothing to merge.
- Shared root files (`smart-folders.json`, `watches.json`, `foryou.json`, any `*.json`): last writer wins by
  modification time. The overwritten local copy is kept under `.lunta-sync/conflicts/`.
- Paper files are transferred before positions and logs, so another device never sees an op for a paper whose
  files are not there yet. State is saved every few transfers: an interrupted sync resumes where it stopped
  (files already identical by md5 cost nothing).
- **Incremental**: a small state file remembers each file's remote id, md5, size and modified time and the
  local size and mtime. The first sync lists the Drive tree; later syncs use the Drive **changes** API (one
  request per 1000 changes, includes deletions) instead of listing, because a folder's `modifiedTime` is not
  reliable for "something inside changed". Local files are hashed only when size or mtime moved. Files up to
  5 MB are uploaded in one multipart request, larger ones with a resumable session in 8 MiB chunks. Deletes
  go to the Drive trash.
- Safety nets: it refuses to apply more than half of the tracked files (or more than 10) as deletions in one
  run, and refuses an empty remote after files were synced before.
- **Lazy files** (`SyncOptions::lazy`): PDFs, EPUBs and narration are left on Drive during a sync;
  `Syncer::fetch_paper(id, kinds)` / `fetch_file(path)` download them on demand, e.g. when the Android app
  opens a paper (`paper_files(id)` lists what exists remotely).

**API** (plain owned types, ready for UniFFI; not exported to Swift yet):

```rust
let auth = Authenticator::new(OAuthClient::from_env()?, Arc::new(FileTokenStore::new(tokens_path)));
auth.connect_loopback(&|url| open_browser(url), Duration::from_secs(300))?;   // desktop / CLI
// Android: OAuthClient::authorization_url(redirect, &Pkce::generate()?, state), open a Custom Tab,
// then Authenticator::complete(code, verifier, redirect). Tokens go through the TokenStore trait, so
// each platform keeps them in its own keychain / keystore.
let drive = GoogleDrive::new(auth, DriveRoot::default());          // folder "Lunta" in My Drive, or DriveRoot::Id(..)
let syncer = Syncer::new(library_dir, device_id, Box::new(drive), SyncOptions::new(state_path))?;
let report = syncer.sync_now()?;     // SyncReport { uploaded, downloaded, deleted_*, conflicts, errors, changed, .. }
syncer.status();                     // offline: pending uploads/downloads, remote-only files, last sync
library.refresh()?;                  // when report.changed
```

`LocalDirRemote` (a folder as the remote) implements the same trait and is what the tests use.

**CLI** for trying it by hand (`--state-dir` defaults to `<library>/.lunta-sync`, which holds the tokens,
the sync state and a generated device id):

```sh
export LUNTA_GOOGLE_CLIENT_ID=...apps.googleusercontent.com LUNTA_GOOGLE_CLIENT_SECRET=...
cd core
cargo run --bin lunta-sync -- connect --library ~/lunta-test     # opens the browser
cargo run --bin lunta-sync -- sync    --library ~/lunta-test [--lazy]
cargo run --bin lunta-sync -- status  --library ~/lunta-test
cargo run --bin lunta-sync -- fetch <paper-id> --library ~/lunta-test
# no Google account? any folder works as the remote:  --remote-dir /some/folder
```

### Setting up Google Cloud (once, by you)

Lunta ships **no** Google credentials; you create an OAuth client in your own project.

1. [console.cloud.google.com](https://console.cloud.google.com) → new project → *APIs & Services* → *Library* →
   enable the **Google Drive API**.
2. *OAuth consent screen* (Google Auth Platform → Branding/Audience): user type *External*, app name
   "Lunta", your e-mail. Leave it in **Testing** and add your Google account(s) under *Test users*.
   Add the scope `.../auth/drive` under *Data Access*.
3. *Credentials* → *Create credentials* → *OAuth client ID*:
   - type **Desktop app** (desktop, CLI): gives a client id and a client secret. Put them in
     `LUNTA_GOOGLE_CLIENT_ID` / `LUNTA_GOOGLE_CLIENT_SECRET`. (A desktop client's "secret" is not confidential;
     Google still requires it at the token endpoint.)
   - type **Android** (the Android app): needs the package name and the SHA-1 of the signing key; it has no
     secret. Google redirects to `com.googleusercontent.apps.<id>:/oauth2redirect`.
4. `lunta-sync connect` and sign in. The "Google hasn't verified this app" screen is expected in Testing:
   *Advanced* → *Go to Lunta (unsafe)*.

**Scope.** The default is `https://www.googleapis.com/auth/drive` (all files), because sharing with a Mac
that uses Google Drive for desktop means seeing files this app did not create; the narrower `drive.file`
only sees files the app itself created (so it works for a library used *only* through the API, `--scope file`,
but not for one made by the Mac, and not even across two different OAuth clients). `drive` is a
**restricted scope**: fine for personal use and test users, but publishing the app to the public needs
Google's OAuth verification and an annual security assessment. Note that in *Testing* status Google expires
refresh tokens after 7 days (you sign in again); *In production* (unverified, up to 100 users) tokens last.
A narrower route (`drive.file` plus Google's file picker, where the user grants access to chosen files) was
considered, but as far as the docs say it grants only the picked items, not whatever Drive for desktop adds to
the folder later, and the picker is a web widget; so `drive` it is. If you know better, `Scope` is one enum.

Tests: `cargo test` covers the syncer with a folder as the remote (a Mac-like device writing straight
into it plus API devices: adds, edits, highlights, deletes, paper files, shared-file conflicts, interrupted
and resumed syncs, lazy downloads, safety nets) and the Drive client against an in-memory mock of the Drive
API (listing, changes, multipart and resumable uploads, token refresh, retries). Against real Drive:

```sh
LUNTA_GOOGLE_CLIENT_ID=... LUNTA_GOOGLE_CLIENT_SECRET=... \
LUNTA_TEST_TOKEN_FILE=<state-dir>/google-tokens.json LUNTA_TEST_DRIVE_FOLDER=<id of an empty scratch folder> \
  cargo test drive::tests::real_drive -- --ignored
```

The Android app uses it through UniFFI (`core/src/drive/ffi.rs`: `DriveAuth` for sign-in, `LibrarySync`
for the Syncer, also with a plain folder as the remote for testing). Not done yet: wiring into the desktop
app (Settings → Google Drive) and "Move library to Google Drive" in the Apple apps.

## Desktop app (Windows and Linux)

`desktop/` is a [Tauri 2](https://tauri.app) app: a Rust backend that calls the core directly (no
UniFFI) and a web frontend in plain TypeScript with Vite (no UI framework: the app is a sidebar, a list
and a details pane, and the reader is plain DOM anyway, so a framework would mostly add weight). It also
runs on macOS, for testing.

- **Library folder:** chosen on first run (a folder in Google Drive for desktop, Dropbox or OneDrive to
  share the library with other computers; any local folder otherwise) and remembered with a per-install
  device id (`windows-1a2b3c4d`, `linux-…`) in the app's config folder. Like the apps, it writes only
  its own `ops/<device>.jsonl` and `positions/<device>.json`, and picks up other devices' changes
  every ten seconds. The search index lives in the app's cache folder. (To share with the Mac and
  iPhone, the library must be in a folder all of them sync; the Apple apps use iCloud Drive.)
- **Reading view:** the paper's EPUB is unpacked by the core and served to an iframe through the app's
  `lunta://` scheme, with the Apple apps' own reader CSS and scripts injected (extracted from the Swift
  by `scripts/extract-reader-assets.mjs`, run before every `npm run dev`/`build`; CI fails if they are
  stale). `desktop/src/reader/bridge.js` stands in for WKWebView's `window.webkit.messageHandlers`
  (posting to the app window) and for `evaluateJavaScript`. Highlights and notes go through the core's
  op log, positions into `positions/`, so both sync with the Mac and iPhone.
- **Organising:** select several papers in the list (⌘/Ctrl-click, Shift-click, ⌘/Ctrl-A). Drag papers
  (the whole selection if you drag one of them) onto a folder in the sidebar to add them to it
  (membership, not a move), or onto a tag to tag them; drag a folder onto another folder to nest it,
  or onto the Folders heading for the top level. The target lights up while you drag. (Done with
  mouse events rather than HTML5 drag and drop, which Tauri's file-drop handling breaks on Windows.)
- **Edit Details…** (button in the details, or the paper's context menu) edits title, authors,
  journal, date, volume/issue/pages, publisher, identifiers, abstract and keywords through the core,
  and can fill them in from the DOI. **Smart folders** are created with ＋ next to Smart Folders and
  edited or deleted from their context menu; same rules and `smart-folders.json` as the Mac app.
- **Security:** a content security policy is set in `tauri.conf.json` (own scripts only, the reader
  iframe and images from `lunta://`, PDF.js worker from blob:/self, IPC). Debug runs of the packaged
  frontend: `npm run build`, then `cargo build --features tauri/custom-protocol` in `src-tauri`
  (plain `cargo build` loads the Vite dev server, which has no CSP).
- **Adding papers** uses the core's metadata lookup and the Rust converter (`core/src/convert`). It
  can't analyse PDFs yet, so a paper whose only full text is a PDF gets no reading view (the converter's
  reason is shown); its PDF still opens.

Build and run (needs Rust and Node 20+; on Linux also `libwebkit2gtk-4.1-dev libgtk-3-dev
libayatana-appindicator3-dev librsvg2-dev patchelf`; on Windows the WebView2 runtime, already on
Windows 10/11):

```sh
cd desktop && npm install
npx tauri dev                           # run
npx tauri build --bundles msi,nsis      # Windows installers → src-tauri/target/release/bundle/
npx tauri build --bundles appimage,deb  # Linux packages
npx tauri build --bundles app           # macOS, for testing
```

`.github/workflows/desktop.yml` builds the Windows and Linux installers on GitHub Actions.

For scripted UI checks, debug builds run any `*.js` dropped into `$LUNTA_DEV_EVAL_DIR` in the window
(which then stays hidden unless `LUNTA_DEV_SHOW=1`) and, on macOS, can save a snapshot of it
(`dev_snapshot`). None of this is in release builds.

Not in the desktop app yet (the Mac app has them): highlighting in the PDF (PDF highlights made on the
Mac or iPhone are listed, not drawn), reading views from PDFs, adding a paper from a PDF alone,
import (Papers, BibTeX, RIS), Discover, New for You,
Clean Up, Listen, Ask, the MCP server, the browser extension and the Word / Google Docs add-ins.

## Android app

`android/` is a native app in Kotlin with Jetpack Compose and Material 3 (teal and copper, light and
dark, an adaptive icon made from `design/logo-1024.png`) on the Rust core through UniFFI's Kotlin
bindings. It follows the iPhone app's Mail-style layout.

- **Setup:** on first run, choose where the library lives: **Google Drive** (sign in with a Custom Tab
  and PKCE through the core's `DriveAuth`; the library is a "Lunta" folder in your Drive, in sync with a Mac
  or PC) or **a local library** on the phone (for trying it out). Drive sync is lazy: it runs on start,
  on pull-to-refresh and about every 30 minutes (WorkManager), and fetches a paper's EPUB or PDF
  when you open it.
- **Library:** places (All, Starred, Unread with counts, a New for You placeholder, folders as a tree,
  smart folders written by the Apple apps, tags) each opening a paper list with full-text search and
  sort; swipe from the right edge for star, read/unread and Folders & Tags. The paper page has the
  abstract, folders and tags, APA / Vancouver / Nature citations (copied), **Read**, Add PDF from Files
  (the system picker), Find PDF, Make reading view and delete (with confirmation).
- **Adding:** by DOI or link (core lookup and the Rust converter, PDFs through PDFium), a PDF from
  Files, or the share sheet of other apps (links, DOIs and PDFs).
- **Reader:** a WebView with the Apple apps' reader CSS and scripts (extracted from the Swift by
  `scripts/extract-reader-assets.mjs`, copied into the APK by Gradle); `android/app/src/main/assets/reader/bridge.js`
  stands in for `window.webkit.messageHandlers` (`addJavascriptInterface`). Text size, scroll or page
  turning (swipe or tap the edges), highlights in four colours with notes, a highlights list, contents,
  and the reading position, synced through `positions/` like the other apps. The PDF opens in a
  PdfRenderer view (no PDF highlights yet; ones made elsewhere are listed).

Build and run (JDK 21, Android SDK with platform 36, build-tools 36.0.0 and NDK 28, Rust with the
`aarch64-linux-android` target, `cargo install cargo-ndk`):

```sh
scripts/build-core-android.sh              # core → android/app/src/main/jniLibs, Kotlin bindings, PDFium (downloaded)
cd android && ./gradlew :app:installDebug  # with a device or emulator attached
echo 'lunta.googleClientId=<id>.apps.googleusercontent.com' >> android/local.properties   # for Google sign-in
```

`ABIS="arm64-v8a x86_64"` also builds for x86_64 emulators. The OAuth client is an "Android" client in
Google Cloud (package `app.lunta`, the SHA-1 of the signing key; see [Google Drive sync](#google-drive-sync));
without an id the app builds and explains what is missing. Debug builds can start straight on a local
library, optionally synced with a plain folder as the "remote":
`adb shell am start -n app.lunta/.MainActivity --es lunta.setup local --es lunta.remoteDir <dir>`
(`cargo run --example seed_library -- <dir> <doi>...` makes a library of public papers for it).
`.github/workflows/android.yml` builds the debug APK on GitHub Actions.

Not in the Android app yet: highlighting in the PDF, New for You, Discover, Ask, Listen, import,
Clean Up, creating or editing smart folders, editing metadata, Keystore-backed token storage (tokens
are in the app's private, non-backed-up folder), a release build and signing.

## Roadmap

1. ✅ Rust core + Swift bindings
2. ✅ Mac app: library, search, import (ReadCube Papers / BibTeX / RIS / PDF folders), Find PDF,
   continuous reading view + PDF view, citations
3. ✅ Chrome extension
4. ✅ iPhone app, share extension, iCloud Drive sync
5. ✅ Highlights and notes, synced reading position
6. ✅ Ask (on-device assistant) and MCP server for agents
7. ✅ Citation add-ins: Word and Google Docs
8. ✅ Duplicate and metadata cleanup, smart folders
9. ✅ New-paper alerts
10. ✅ arXiv HTML reading views (arXiv HTML, ar5iv, then PDF)
11. ✅ Page-turning reading view
12. ✅ Converter in Rust (`core/src/convert`) for apps beyond Apple: JATS (PMC, Europe PMC, eLife),
    arXiv HTML, EPUB packaging and cover identical to Paper2EpubKit's output; PDF layout analysis on
    PDFium with the same structure and nearly the same text (an open XML version via the PDF's DOI first)
13. ◐ Desktop app for Windows and Linux (`desktop/`, Tauri): library, reader with highlights and synced
    positions, PDF view, add by identifier; PDF highlighting, import and the rest next
14. ◐ Google Drive sync through the API for devices without a synced folder (`core/src/drive`): core, CLI and
    tests done, exported to the Android app; desktop settings screen and the Apple apps' "Move library to
    Google Drive" next
15. ◐ Android app (`android/`, Compose): library, reader with highlights and synced positions, PDF view,
    add by identifier / PDF / share, Google Drive sync; needs a Google OAuth client id to sign in;
    PDF highlighting, New for You and the rest next

## License

MIT, see [LICENSE](LICENSE).
