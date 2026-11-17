<p align="center">
  <img src="design/logo-1024.png" width="140" alt="Lunta logo">
</p>

<h1 align="center">Lunta</h1>

<p align="center"><strong>Reader and reference manager for scientific papers.</strong><br>
Mac · iPhone · iPad · Android · Windows · Linux</p>

<p align="center"><em>Lunta</em> is Swedish for a thick bundle of papers.</p>

---

Lunta turns every paper into a clean reading view that fits your screen, keeps your highlights and notes in
sync across your devices, and helps you find what to read next. Your library stays in your own iCloud or
Google Drive: no account, no server, nothing collected.

## Download

**[Latest release](https://github.com/christoffermattssonlangseth/lunta/releases/latest)** for Mac, Windows,
Linux and Android (the release page says which file you need). iPhone and iPad: TestFlight for now, App Store
soon.

**New to Lunta?** Start with **[Getting started](https://christoffermattssonlangseth.github.io/lunta/getting-started.html)**:
which setup fits your devices, and how to move your library from Zotero, Mendeley, ReadCube or EndNote
([step-by-step import guide](docs/importing.md)).

## What it does

- **Read:** each paper becomes a reflowable reading view, with tappable figures and citations, real
  equations, themes, and scroll or page turning. The PDF is one tap away.
- **Highlight and note:** four colours, a highlight mode for marking quickly, Apple Pencil on iPad. Highlights
  sync everywhere and are searchable.
- **Library:** add papers by DOI, PubMed ID, arXiv link or PDF, or from Chrome and Safari. Lunta finds legal
  open-access PDFs, and never swaps a published paper for its preprint. Folders, tags, Smart Folders,
  stars and a Must Read list, with full-text search.
- **New for You:** every morning, new papers picked for your research, with a short summary.
- **Listen and ask:** papers read aloud in a natural voice, and an assistant that answers questions about a
  paper or your library, both running on your device.
- **Cite:** 12 citation styles, BibTeX and RIS, and add-ins for Word and Google Docs.
- **Agents:** an [MCP server](docs/development.md#ai-agents-mcp) so Claude and other AI agents can search,
  read, cite and highlight in your library.

## Browser extension and add-ins

### Chrome extension (Mac, Windows, Linux)

Saves the paper you're looking at to Lunta, with its PDF, using the access your browser already has (your
institution's subscription, for example). On a journal's article page, **Use this page's full text** turns
the page you're reading into the paper's reading view. It works in Chrome, Edge, Brave and other
Chromium browsers.

1. Download **`Lunta-…-Chrome-Extension.zip`** from the
   [latest release](https://github.com/christoffermattssonlangseth/lunta/releases/latest) and unzip it.
   Keep the folder somewhere permanent (Chrome loads the extension from it).
2. Open **`chrome://extensions`** (Edge: `edge://extensions`) and turn on **Developer mode** (top right).
3. Click **Load unpacked** and choose the unzipped folder.
4. Pin it: click the puzzle piece in the toolbar, then the pin next to **Lunta**.

Lunta must be open on the same computer: the extension talks only to the app, never to a server. To update,
unzip the new version over the old folder and click ↻ on the extension in `chrome://extensions`.

**Android:** there are no extensions in Chrome for Android. Instead, open a paper's **Journal page** in Lunta,
sign in there if needed, and tap **Use this page's full text**. **iPhone and iPad:** share a page from Safari
to Lunta.

### Word and Google Docs (Mac)

Cite from your library while you write: citations in 12 styles (or any of ~10,000 journal styles in Word), a
reference list that updates itself, and renumbering after edits. On the Mac, choose **Lunta › Citations in
Word & Google Docs…** and follow the two steps there:

- **Word:** click **Trust Certificate** (your Mac asks for your password once), then **Install in Word** and
  restart Word. **Cite** appears on the Home tab.
- **Google Docs:** in your document, **Extensions › Apps Script**; paste the two files Lunta gives you
  (`Code.gs` and `Sidebar`), save, and reload the document. **Lunta › Cite…** appears in the menu.

Lunta for Mac must be open while you cite. More: [Lunta Citations](https://christoffermattssonlangseth.github.io/lunta/support.html).

### AI agents (MCP)

Claude and other AI agents can search, read, cite and highlight in your library through Lunta's MCP server:
see [docs/development.md](docs/development.md#ai-agents-mcp).

## Sync

- **Mac, iPhone and iPad** sync through iCloud Drive, with nothing to set up.
- **Android, Windows and Linux** sync through a *Lunta* folder in Google Drive.
- **Both:** a Mac can bridge the two (**Lunta › Google Drive for Android…**).

## Help and development

- Questions, bugs and ideas: [GitHub Issues](https://github.com/christoffermattssonlangseth/lunta/issues).
- Privacy: [Lunta collects nothing](https://christoffermattssonlangseth.github.io/lunta/app-privacy.html).
- Building from source, architecture and sync design: [docs/development.md](docs/development.md).

## License

MIT, see [LICENSE](LICENSE).
