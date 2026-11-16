# Lunta on the App Store (iPhone and iPad)

Everything App Store Connect asks for, ready to paste. Limits are Apple's.

## App record

| Field | Value |
|---|---|
| Name (30) | Lunta |
| Subtitle (30) | Paper reader & reference manager |
| Bundle ID | com.cmlangseth.margins.ios |
| SKU | lunta-ios |
| Primary category | Productivity |
| Secondary category | Education |
| Price | Free |
| Age rating | 4+ (answer "None" to every content question; no web browser, no user-generated content shared with others) |
| Privacy policy URL | https://christoffermattssonlangseth.github.io/lunta/app-privacy.html |
| Support URL | https://christoffermattssonlangseth.github.io/lunta/getting-started.html |
| Copyright | 2026 Christoffer Mattsson Langseth |

## Promotional text (170)

Read papers the way they deserve: clean reading views with figures you can tap, highlights that sync everywhere, and a daily pick of new papers for you.

## Description (4000)

Lunta is a reader and reference manager for scientific papers.

READ, DON'T SQUINT
Every paper becomes a clean, reflowable reading view, made from the paper's open XML version, the journal's web page, arXiv, or the PDF itself. Text that fits your screen, real equations, references linked from the text. Figures appear as cards: tap one, or a "Figure 2" in the text, to see it large, then swipe through the paper's figures. Scroll or turn pages, choose the text size and a theme: Light, Paper, Quiet or true Black.

HIGHLIGHT AND NOTE
Highlight in four colours, add notes, in the reading view or the PDF. Highlight mode marks every selection at once. On iPad, highlight with Apple Pencil and draw on PDFs. All highlights are searchable across your library.

YOUR LIBRARY
Add papers by DOI, PubMed ID, arXiv link or PDF, or share them from Safari. Lunta finds legal open-access PDFs for you, and never swaps a published paper for its preprint. Folders, tags, Smart Folders, stars and a Must Read queue. Full-text search across everything. Import from ReadCube Papers, BibTeX and RIS.

NEW FOR YOU
Each morning Lunta picks new papers that match your interests, the authors and topics you follow, and your journals, with a short summary and a notification.

LISTEN
Papers read aloud in a natural voice, with the sentence being read highlighted. Great for the commute.

ASK
Ask questions about a paper or your whole library. The assistant runs on your device (Apple Intelligence, or a free open model), so your papers stay private.

EVERYWHERE
Your library lives in your own iCloud and syncs between iPhone, iPad and Mac. Lunta is also on Android, Windows and Linux, and cites into Word and Google Docs.

PRIVATE BY DESIGN
No account, no ads, no tracking. Lunta collects nothing.

## Keywords (100)

papers,research,reference,citation,pdf,reader,science,journal,pubmed,arxiv,highlight,library,academic

## What's new (first release)

First release on the App Store: reading views with tappable figures, highlight mode, themes, Apple Pencil on iPad, New for You every morning.

## App Privacy questionnaire

"Do you or your third-party partners collect data from this app?" → **No, we do not collect data from this app.**
(The library is stored in the user's own iCloud; lookups go to public scholarly services without personal data; the assistant runs on device. See the privacy policy.)

## Notes for App Review

Lunta is a reader and reference manager for scientific papers. No account or login is needed.

To try it: tap + and paste the DOI 10.1038/s41586-023-06139-9 (an open-access Nature paper). Lunta fetches its details and full text and makes a reading view; tap the paper to read it. Tap "Fig. 1" in the text, or a figure card, to open the figure view.

- Background audio (UIBackgroundModes: audio): Lunta reads papers aloud ("Listen" in the reader's ⋯ menu), and the narration continues when the screen locks, like a podcast.
- Background fetch: a daily background refresh reads the "New for You" list the user's own devices prepared (stored in their iCloud) and shows one morning notification (notifications can be turned off in iOS Settings).
- On-device model: if Apple Intelligence isn't available, the "Ask" assistant offers an optional one-time download of an open model (Qwen3 4B, about 2.3 GB) that then runs entirely on the device. Nothing is sent to a server.
- Papers and PDFs come from public, legal open-access sources (PubMed Central, Europe PMC, arXiv, publishers' open-access copies).
- iCloud: the library is kept in the user's iCloud Drive so it syncs with their other devices; without iCloud it is kept on the device.

## Screenshots needed

- iPhone 6.9" (1320 × 2868): library, reading view, figure view, highlights, New for You.
- iPad 13" (2064 × 2752): split view with library + reader, Pencil markup on a PDF, figure grid.
