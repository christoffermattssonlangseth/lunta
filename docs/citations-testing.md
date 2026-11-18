# Testing the citation add-ins in Word and Google Docs

The formatting, the API and the Google script run in automated tests (see "Citations in Word and Google
Docs" in [development.md](development.md)). What only the real Word and Google Docs can show is below: go
through it after changing `apple/citations/`, `extension/gdocs/`, `CitationAPI` or `CitationServer`. Use a scratch document and
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
- [ ] Switch Google account; a document of another account, and one shared view-only: the message says so.
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
- [ ] Connect Google account: the default browser opens Google's page; allow; the tab says Lunta is connected and
      the panel shows the search by itself. Deny: the panel says so. Quit and reopen Lunta: still connected
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
