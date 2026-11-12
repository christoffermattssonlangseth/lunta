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
local one; see [Desktop app](#desktop-app-windows-and-linux).

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
- **Cite:** APA, Vancouver, Nature, BibTeX, RIS, Markdown; share PDFs, EPUBs and citations. **Word and
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
core/src/convert/      Rust port of the paper → EPUB converter (DOI, PMCID, PubMed, arXiv, JATS XML;
                       no PDF analysis yet) for future Linux/Windows/Android apps; feature "convert",
                       not in the Apple build. CLI: cargo run --bin paper2epub -- <input> [out-dir]
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
design/                logo (SVG + 1024 px PNG)
scripts/build-core.sh  builds the Rust core for iOS, the simulator and macOS → XCFramework
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

Styles: APA 7, Vancouver, Nature. Lunta must be open while you cite.

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
cd apple/LuntaCore && swift test     # Swift ↔ Rust round trip
```

### Browser extension

Chrome → `chrome://extensions` → Developer mode → Load unpacked → `extension/`. The Lunta Mac app
must be running; the extension talks to it on `127.0.0.1:47480` (extension requests only).

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
12. ◐ Converter in Rust (`core/src/convert`) for apps beyond Apple: JATS (PMC, Europe PMC, eLife),
    arXiv HTML, EPUB packaging and cover done, output identical to Paper2EpubKit's; PDF analysis next
13. ◐ Desktop app for Windows and Linux (`desktop/`, Tauri): library, reader with highlights and synced
    positions, PDF view, add by identifier; PDF highlighting, import and the rest next

## License

MIT, see [LICENSE](LICENSE).
