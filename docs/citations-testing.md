# Testing the citation add-ins in Word and Google Docs

The formatting, the API and the Google script run in automated tests (see "Citations in Word and Google
Docs" in [development.md](development.md)). What only the real Word and Google Docs can show is below: go
through it after changing `apple/citations/`, `extension/gdocs/`, `CitationAPI`, `CitationServer` or the Windows app's `citeserver.rs` / `citesetup.rs`. Use a scratch document and
a few papers you don't mind citing; a long document is quickest made by pasting the same paragraphs many
times.

For Word, check both the panel served by Lunta (sideloaded, `manifest.xml`) and the hosted one (the store
listing, `manifest-store.xml`, served from GitHub Pages after a push to main); bump `VERSION` in
`word/app.js` and the `?v=` in `word/index.html` together, or Word keeps the old files. For Google Docs,
copy both `Code.gs` and `Sidebar.html` again from the Lunta window.

## Word (Mac; repeat the starred items in Word on the web)

Setup and connection
- [ ] * Lunta open, certificate trusted: the panel says "Connected to Lunta · N papers".
- [ ] * Quit Lunta, open the hosted panel: "Lunta isn't running…" (or "Can't reach Lunta…" in a document
      without citations); no endless "Updating…". Start Lunta, Update All works without reloading the panel.
- [ ] Untrust the certificate in Keychain Access (on a test account): the message says to click Trust
      Certificate; after trusting, it connects.
- [ ] Search with 1000+ papers in the library: results while typing, no stale results after fast typing.

Citing and numbering
- [ ] * Insert three citations in APA; a fourth citing two papers; Insert Bibliography at the end. Every
      citation reads right, the list is sorted, Update All twice changes nothing (Undo history shows one
      Update, then nothing new).
- [ ] Two papers by the same author in the same year: "(Smith, 2020a)", "(Smith, 2020b)", and 2020a/2020b in
      the list; cited together: "(Smith, 2020a, 2020b)".
- [ ] Switch to Vancouver: numbers by first citation, a repeated paper keeps its number, "[1–3]" for three in
      a row, the list numbered 1., 2., …; switch to Nature: superscript; back to APA: not superscript.
- [ ] * Click inside a citation, pick another paper, Insert Citation: it joins that citation.
- [ ] Insert Bibliography with the cursor in the bibliography or with one already present: no second one
      (the existing one is selected and updated).
- [ ] Put the bibliography before the citations: numbering is unchanged (by citations only).
- [ ] With the cursor in the bibliography, Insert Citation: refused with a message.
- [ ] Find journal style… "Nature Neuroscience": downloads once, every citation and the list change; offline
      (Wi-Fi off) with a style used before: still works; a new style offline: a message, nothing changed.

Editing the document
- [ ] * Delete a citation (select its text, Delete), Update All: renumbered, its paper leaves the list
      (if cited nowhere else). Undo, Update All: back.
- [ ] Cut a paragraph with citations and paste it earlier: Update All renumbers in the new order.
- [ ] Copy a citation and paste it elsewhere (also into another document with the add-in): it is a citation
      there too.
- [ ] Type inside a citation's text, Update All: the citation's text is restored (it is Lunta's).
- [ ] Track Changes on: Update All after an edit shows only the citations that really changed; a second
      Update All adds no revisions. A deleted (tracked) citation still counts until accepted.
- [ ] Citations in footnotes and in a table: counted and updated.
- [ ] Titles with `<`, `&`, quotes, `*`, Greek, Chinese, and a title with `$x^2$`: the list shows them as text,
      italics only where the style wants them, no stray asterisks.

Other computers, other people
- [ ] Save, close, open again: the style is the same, Update All changes nothing.
- [ ] Delete a cited paper from the library, Update All: still in the list (from the document's copy), the
      status says so.
- [ ] Merge two duplicates in Lunta (Clean Up), one of them cited: Update All keeps one reference.
- [ ] A document made with the sideloaded add-in, opened with the store add-in: same style (not silently
      APA). A document with citations from an older add-in version: asks for the style before updating.
- [ ] Open the document on a computer without Lunta (or with Lunta closed), using the store add-in:
      citations and list unchanged; delete a citation, Update All: renumbered from the document's copy.
- [ ] A co-author with Lunta and a different library: Update All keeps every reference (formatted from the
      document's copy or found in their library by DOI).
- [ ] A citation of a paper that is in neither (an old document from another library, made before this
      version): Update All selects it, says what to do, and changes nothing.

## Word on Windows (a real PC; Lunta for Windows)

Not yet done on real hardware: go through it on a Windows 10/11 PC with Microsoft 365 Word (and, if at hand, Word
2019/2021 and a managed KI PC), with an ordinary (non-administrator) account. Install Lunta from the NSIS setup
(per user), open a library with a few papers.

Setup
- [ ] Settings ▾ › Citations in Word & Google Docs…: "Lunta is ready for the add-in on https://localhost:47481";
      step 1 not done, step 2 not done.
- [ ] Before trusting: `https://localhost:47481/word/index.html` in Edge shows a certificate warning (the server
      runs); `%APPDATA%\com.cmlangseth.lunta.desktop\citations\tls` has ca.pem, ca.key, server.pem, server.key,
      and ca.key is not readable PEM (DPAPI).
- [ ] Trust Certificate…: Windows' "Security Warning … Lunta Local CA (this computer only, localhost) … Do you
      want to install this certificate?" appears in front of Lunta (note if it hides behind the window). No: the
      window says you answered No, step 1 stays open. Again, Yes: step 1 ticked, no administrator prompt.
- [ ] certmgr.msc › Trusted Root Certification Authorities › Certificates: "Lunta Local CA…"; its details show
      Name Constraints (Permitted: DNS localhost, IP 127.0.0.1) as critical.
- [ ] Edge and Chrome: `https://localhost:47481/word/index.html` opens without a warning (padlock);
      `https://127.0.0.1:47481/api/status` gives `{"ok":true,…}`.
- [ ] Install in Word: step 2 ticked; regedit shows `HKCU\Software\Microsoft\Office\16.0\WEF\Developer`,
      value `6a1f3b2c-6d0e-4c8e-9b8e-4d2f3a7c9e15` = the manifest's path under `%APPDATA%\…\citations`.
- [ ] Quit Word (also from the tray / Task Manager), open it: Cite on the Home tab (or Home › Add-ins ›
      Developer Add-ins › Lunta Citations). The panel says "Connected to Lunta · N papers"; within a few
      seconds the Lunta window says "Word is connected".
- [ ] A managed PC (KI): note whether the certificate or the developer add-in is blocked by policy, and what the
      window and Word say.

Citing (the Mac list above, the starred items at least)
- [ ] * Insert Citation, Insert Bibliography, change style (APA → Vancouver → a journal style via Find journal
      style…), Update All twice changes nothing.
- [ ] * Resolve Citations with "(Lee et al., 2020)" placeholders; Confirm adds an outside paper: it appears in
      Lunta's list.
- [ ] Quit Lunta: the panel says Lunta isn't running; open Lunta, Update All works without reloading.
- [ ] Leave Lunta open overnight (or set the clock forward a day): still connects (the daily renewal check).

Word on the web (Edge or Chrome on the same PC; and on Linux with Chrome and Firefox)
- [ ] Save Manifest…, then Home › Add-ins › More Settings › Upload My Add-in: the panel connects as in Word.
- [ ] The store add-in (manifest-store.xml, panel from getlunta.app): connects (CORS and Private Network Access
      from getlunta.app to localhost).
- [ ] Linux with certutil: Trust Certificate adds it to Chrome and each Firefox profile (restart the browser);
      without certutil: Save Certificate… and the import steps in the window work.

Removing
- [ ] Remove…: Windows asks to delete the root certificate (Yes); the registry value is gone; after restarting
      Word the add-in is gone; the window shows step 1 and 2 open again; Trust Certificate works again (a new
      authority, a new Windows prompt).

## Google Docs (Chrome and Safari)

- [ ] First open after pasting both files: the sidebar loads, style menu enabled only once the
      document's style is read; "Connected to Lunta".
- [ ] Old `Code.gs` with the new `Sidebar.html`: the sidebar says to copy both files again.
- [ ] Pairing: a Sidebar.html with a wrong key (edit it): the sidebar asks for the pairing key; the key from
      the Lunta window works and is remembered after reloading.
- [ ] Chrome asks to allow local network access: after allowing, it connects; if blocked, the message says
      where to allow it.
- [ ] Insert, numbering, a/b, styles, bibliography: as for Word. Update All twice: the version history
      shows no second change.
- [ ] The bibliography as the last paragraph of the document: Update All works (no "Can't remove the last
      paragraph" error).
- [ ] Bibliography made a numbered list, font changed to 10 pt with a hanging indent: Update All keeps the
      look.
- [ ] Selection instead of a cursor, Insert Citation: inserted after the selection, the selection kept.
- [ ] Cursor inside a citation, Insert Citation: the papers join it.
- [ ] Citations in footnotes, in a table, and in a second tab: numbered in document order (tab by tab).
- [ ] Two people: one types while the other runs Update All on a long document: no citation gets another's
      text (the sidebar starts over; after three tries it says to click again).
- [ ] 200 citations, 150 papers: Update All finishes (note the time); a second Update All is quicker.
- [ ] Lunta closed: Update All works from the document's copy; Insert Citation says to open Lunta.
- [ ] File › Make a copy: the copy has the script and its citations update.
- [ ] Download as .docx: citations are plain text with links, the list is there.
- [ ] `Code.gs` 3 in a document last updated by `Code.gs` 2: the style is kept (read from the document
      properties), and after one Update All it is in the named ranges too (the extension shows it).

## Google Docs with the browser extension (Chrome, Edge, Firefox)

Load `extension/` unpacked with a client ID in `gdocs/config.js` (see
[google-docs-extension.md](google-docs-extension.md)); your account must be a test user. Lunta for Mac, and
once the desktop app on Windows or Linux.

- [ ] The Lunta button on a document (bottom right, clear of Docs' own buttons, also with the side panel
      and in a narrow window); the toolbar button opens the same panel there. Light and dark mode.
- [ ] Connect Google account: Google's consent screen once; afterwards opening the panel doesn't ask again,
      also after an hour (the token is renewed without a window) and after restarting the browser (one
      silent renewal, no window). Untick the permission on the consent screen: the panel says why it needs it.
- [ ] Give Lunta access to this document (`drive.file`; the API key in `site/picker.html` and the site
      published): a new document shows the step with its button; Google's picker window opens showing only that
      document; Select: the window closes and the panel loads. Close the window or Cancel: the panel says Lunta
      still can't open it. Reload the page: no picker again (also after restarting the browser). Another
      document: the picker again. Without the API key: the panel says it isn't set up yet.
- [ ] Switch Google account; a document of another account, and one shared view-only: the message says so
      (view-only: "can't edit", no picker). Take Lunta's access away at myaccount.google.com/permissions and
      reconnect: a document given before says Lunta had it before and offers the picker.
- [ ] Search (also a pasted DOI), pick three papers, Insert Citation: one citation at the cursor, numbered,
      the text around it untouched; the citation looks like plain text.
- [ ] Cursor inside a citation: the papers join it. Right after one: a new citation. Text selected: the
      citation goes after it, the text stays. Cursor in a footnote, in a table cell, in a second tab: works,
      numbered in document order.
- [ ] Insert Bibliography with the cursor at the end of a "References" heading: plain paragraphs after it (not
      in heading style, not a list); again: no second one, the first is updated.
- [ ] Style menu: Vancouver → Nature (superscript) → APA → a journal style found with *Find journal style…*;
      reload the page: the document's style is shown.
- [ ] Update All twice: version history shows no second change.
- [ ] Type in the document while Update All runs on a long document (200 citations): no citation gets
      another's text.
- [ ] Undo (⌘Z) right after Insert Citation removes it in one or two steps; the document isn't left with a
      "(…)" link (if it is, Update All makes it a citation).
- [ ] Close the tab right after Insert Citation (before "Inserting…" ends), reopen, Update All: the citation is
      made, numbered.
- [ ] Lunta closed: the panel says so; Update All works from the document's copy; Insert Citation says to open
      Lunta.
- [ ] The same document with the Apps Script sidebar (`Code.gs` 3): it sees the extension's citations, style and
      papers; the extension sees the sidebar's. A co-author on another computer with another library, through
      the extension: Update All keeps every reference.
- [ ] A document with citations from `Code.gs` 2 (style only in document properties): the panel asks for the
      style first and changes nothing until it is picked.
- [ ] Without a client ID (`config.js` empty): the panel says to use the Apps Script.
- [ ] Download as .docx: citations are plain text with links, the list is there.

## Google Docs with the browser extension in Safari (Mac)

Lunta for Mac with the Desktop client in `GoogleDocsAuth.swift` (see [google-docs-extension.md](google-docs-extension.md),
"Safari"), its Safari extension turned on (Safari › Settings › Extensions) and allowed on docs.google.com.

- [ ] Without the Desktop client (`GoogleDocsClient.id` empty): the panel says it isn't set up yet in Safari and
      points to the Apps Script. Lunta closed: the panel says to open Lunta for Mac.
- [ ] Connect Google account: the default browser opens Google's page with its file picker showing this document;
      allow and choose it; the tab says Lunta is connected and the panel shows the search by itself. Not choosing
      it: the panel says so and offers Give Lunta access to this document (the picker again). Another document:
      Give Lunta access to this document, the same page. Deny: the panel says so. Quit and reopen Lunta: still connected
      (Keychain), no window. Switch Google account: Google's account picker.
- [ ] Insert Citation: the panel says "Press ⌘V in the document where the citation goes"; ⌘V (the cursor where it
      was before clicking the button): "(…)" appears and becomes the citation within a few seconds. Cancel while
      waiting: nothing changes; pasting later and Update All makes it a citation.
- [ ] ⌘⇧V (Paste without formatting) instead: the panel says it was pasted as plain text; ⌘Z, then ⌘V: it
      becomes the citation.
- [ ] Insert Bibliography the same way; again: no second one, the first is updated.
- [ ] Click a citation with the panel open: it opens in the panel; ✕ and Add Selected Papers work. Style menu and
      Update All as in Chrome.
- [ ] The same document in Chrome with the extension: the same citations, style and papers.
- [ ] Safari on iPhone or iPad: the panel points to the Apps Script.

## Resolve Citations (Word, Google Docs, LaTeX)

A document with: "(Name et al. 2024)" for a paper in the library (only one by that first author that year),
"(Name 2019; Other 2018)" where Other isn't in the library, "[Name 2024 hint]" with two such papers, "(see Fig. 2)",
"(n = 2025)", and one Lunta citation already in it.

- [ ] Word and Google Docs (each browser, Safari through Lunta for Mac): Resolve Citations… lists the three
      placeholders, not the ordinary brackets nor the existing citation. Nothing in the document changes and nothing
      is added to the library until a Confirm.
- [ ] Confirm All Exact Matches confirms only the first; it becomes a citation in the current style and the
      bibliography updates. Track Changes (Word) shows only that change.
- [ ] For "(Name 2019; Other 2018)": confirm Name, Skip Other: one citation followed by "(Other 2018)" as plain text.
      Choose Again, then confirm a Crossref candidate for Other: the paper appears in Lunta (with its PDF if open)
      and in the citation.
- [ ] Search… finds another paper by title words; Confirm cites it.
- [ ] Type before a placeholder after the scan, then confirm it: it is still replaced in the right place; delete a
      placeholder, then confirm it: the panel says it isn't there any more.
- [ ] LaTeX: File › Resolve Citations in a .tex File… on a file with `\cite{?name2024}`, `\citep[p.~2]{a,name2024?}`,
      a commented `% \cite{?x2020}` and "(Name et al. 2024)": three listed; Write puts the keys in, keeps `a` and the
      options, leaves the comment, writes `.tex.bak`, and the folder's kept .bib has the papers. The project compiles.

## A paper's options: page, prefix, suffix, author (Word, Google Docs)

In Word (Mac, Windows, on the web), the extension's panel (Chrome and Safari) and the Apps Script sidebar
(Edit Citation at Cursor).

- [ ] Click a citation of two papers, **Page…** next to the first: the form opens with the cursor in the number.
      Type 12, Tab to the prefix, "see", Enter: "(see Smith, 2020, p. 12; Lee, 2021)" (APA). Escape closes without
      changing anything. Tab reaches every field and both buttons.
- [ ] Kind Figure, 3–5: "figs. 3–5". Without author: "(2020, p. 12)". Author in the text: "Smith (2020, p. 12)".
      A suffix ", emphasis added" keeps its comma; "for a review" gets a space.
- [ ] The same in Vancouver ("[see 1, p. 12]"), Nature (superscript "1(p12)"; author in the text is not raised:
      "Smith [1]"), Chicago ("(Smith 2020, 12)"), MLA ("(Smith 12)"), and a journal style (Find journal style…, e.g.
      "Cell"): the journal's own locator form.
- [ ] Update All twice: nothing changes the second time; the options are still there after saving, closing and
      opening the document, and on another computer with Lunta.
- [ ] Word: open the document with an older add-in (the hosted panel cached before version 13, or Lunta for Mac
      before this release): the citation keeps its papers (no "isn't in your library"), Update All writes it without
      its page; the current panel's Update All puts the page back.
- [ ] Google Docs: the same document in the extension and the Apps Script sidebar shows the same options. An older
      `Code.gs` (3) rewriting the citation drops its options (expected), not its papers.
- [ ] Remove a paper (✕) from a citation with options: the other paper keeps its own.
- [ ] Type text right before and after a citation with options (Google Docs), Update All: the options stay on that
      citation, not on the typed text.

## Converting citations from Zotero, EndNote and Mendeley

Use copies of real manuscripts (or make test documents with each program: a few citations, one with a page, a
prefix and "Omit Author" / "Exclude Author", one with several papers, one long EndNote citation with 10+ papers,
and the bibliography). Keep the originals; compare the converted copy side by side.

Word (Mac, then Windows, then Word on the web)
- [ ] Zotero (Store citations as: Fields): Convert Citations from Zotero, EndNote or Mendeley…: the review says
      "N citations, M papers: X in your library, Y to add", from Zotero, the papers to add listed. Nothing in the
      document has changed (Track Changes on: no revisions yet), nothing was added to Lunta.
- [ ] Untick one paper, Convert: the ticked papers appear in Lunta, every citation whose papers are all there is a
      Lunta citation (click it: the panel shows its papers and Page… shows Zotero's page and prefix), the text around
      each keeps its font, size, italics and spacing (also inside a sentence, before a full stop, in a table).
      The citation with the unticked paper is unchanged and listed; Zotero's bibliography stays (listed).
      Convert again, tick it: now all converted, Zotero's bibliography becomes Lunta's (its place and paragraph
      style kept), the style is the document's.
- [ ] After converting, Zotero's own Refresh finds no Zotero citations left (Zotero tab › Refresh).
- [ ] Zotero with "Store citations as bookmarks": the panel says to switch Zotero to fields; nothing changes.
- [ ] EndNote 20/21 (Mac and Windows): citations with Cite While You Write formatting, one with a page
      ("Pages" in Edit & Manage Citations), prefix/suffix, Exclude Author, Display as Author (Year); a citation
      of many papers (stored as EN.CITE.DATA); EndNote's bibliography. All converted with their options; the
      bibliography becomes Lunta's. Titles with italics, non-English characters (å, ü, Chinese) and "&" come through.
- [ ] Mendeley Cite (the add-in) and Mendeley Desktop (legacy fields): both converted; a document with both too.
- [ ] A document mixing Lunta citations with Zotero ones: the Lunta ones are untouched, numbering covers all after.
- [ ] Undo: Word's Undo after converting undoes one citation at a time (Word has no undo group for add-ins):
      the review's advice to keep a copy is there.
- [ ] Word 2019 or older (no WordApi 1.5): Mendeley Cite citations convert; field citations are listed as left,
      with the reason.
- [ ] A long thesis (200+ citations, 150 papers): the review comes within ~20 s; Convert finishes (note the time).

Google Docs (extension in Chrome, then Safari through Lunta for Mac; then the Apps Script sidebar)
- [ ] A document cited with Zotero's Google Docs plugin: Convert Citations from Zotero…: the review as above.
      Convert: citations relinked (click one: its papers and Zotero's page), Zotero's bibliography becomes
      Lunta's, Update All formats them; the version history shows one change for the conversion.
- [ ] Zotero's plugin afterwards: it no longer lists the converted citations (Zotero › Refresh).
- [ ] A document with Paperpile citations: the panel says they can't be converted; nothing changes.
- [ ] Someone types in the document during Convert (long document): it starts over or says to try again; no
      citation gets the wrong papers.
