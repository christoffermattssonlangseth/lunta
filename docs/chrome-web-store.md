# Lunta in the Chrome Web Store, Firefox Add-ons and Edge Add-ons

Everything the Chrome Web Store Developer Dashboard asks for, ready to paste. The images are in
`appstore/chrome/`. The package is the release's `Lunta-<version>-Chrome-Extension.zip` (the
`extension/` folder zipped, with `manifest.json` at the top).

## Once: the developer account

1. Go to https://chrome.google.com/webstore/devconsole and sign in with the Google account that should own
   the listing.
2. Accept the developer agreement and pay the one-time **US$5** registration fee.
3. **Account** tab: publisher name *Lunta* (or your own name), and verify the contact email.

## Each new version

**Items › Lunta › Package › Upload new package** with the release's extension zip (its `manifest.json`
version must be higher than the last one uploaded), then **Submit for review**. Users get it automatically.

## Store listing

| Field | Value |
|---|---|
| Name | Lunta (from the manifest) |
| Summary (132) | Save papers and their PDFs to your Lunta library, with the access your browser already has, and cite them in Google Docs. (from the manifest) |
| Category | **Productivity › Education** (or Productivity › Tools) |
| Language | English |
| Store icon | `appstore/chrome/icon-128.png` |
| Screenshots (1280×800) | `screenshot-1-save.png`, `screenshot-2-in-library.png`, `screenshot-3-full-text.png` |
| Small promo tile (440×280) | `promo-small-440x280.png` |
| Marquee promo tile (1400×560, optional) | `promo-marquee-1400x560.png` |
| Homepage URL | https://github.com/christoffermattssonlangseth/lunta |
| Support URL | https://github.com/christoffermattssonlangseth/lunta/issues |

The screenshots show a made-up article on a made-up journal ("Journal of Example Biology"), not a real
publisher's page.

### Description

No lists of publishers, journals or styles: the Chrome Web Store rejected 1.0.3 for "keyword spam" (Yellow Argon) over the sentence naming NEJM, Cell Press, Science, PNAS… (18 November 2026).

```
Lunta saves scientific papers to your Lunta library, straight from the journal's page, with the access your browser already has.

On an article page, Lunta's panel shows the paper and whether it's already in your library. One click saves it, with its PDF: the extension downloads the PDF in your browser, so your institution's subscription or your sign-in applies.

USE THIS PAGE'S FULL TEXT
On a journal's article page, Lunta can turn the page you're reading into the paper's reading view: the text, figures, tables and linked references, laid out for reading on any screen. Your highlights are checked first: if any wouldn't be found in the new text, your current reading view is kept.

CITE IN GOOGLE DOCS
In any Google Docs document, the Lunta button opens a panel: search your library, pick papers and insert a citation where your cursor is, insert the bibliography, and update everything after edits. About 10,000 citation styles, from APA to your journal's. Numbers, ordering and the reference list stay right as you write; co-authors with their own Lunta can update the same document. Connect your Google account once: Lunta edits only the document you cite in, through Google's own Docs API.

ALSO
• Save a PDF open in Chrome's PDF viewer with the toolbar button.
• Works on article pages of journals, preprint servers and PubMed.

NEEDS THE LUNTA APP
The extension works together with Lunta for Mac, Windows or Linux, which must be open on the same computer. Get it free at https://github.com/christoffermattssonlangseth/lunta/releases/latest

PRIVATE
The extension talks only to the Lunta app on your own computer (127.0.0.1) and, when you cite in Google Docs, to Google Docs for the document you're citing in. It sends nothing to us or anyone else, has no account, no analytics and no ads.

Lunta is a reader and reference manager for scientific papers, for Mac, iPhone, iPad, Android, Windows and Linux.
```

## Privacy practices tab

**Single purpose**

```
Be the browser side of the user's Lunta reference manager: save the scientific paper on the current page (its details, PDF and full text) to the Lunta app running on the same computer, and cite papers from that library in the Google Docs document the user is writing.
```

**Permission justifications**

| Permission | Justification |
|---|---|
| `activeTab` | When the user clicks the toolbar button, the extension reads the current tab: it saves the PDF open in Chrome's PDF viewer, or opens Lunta's panel on the article page. |
| `scripting` | When the user clicks the toolbar button on a tab that was open before the extension was installed or updated, the extension adds its panel to that tab. |
| `identity` | To cite in Google Docs, the user connects their Google account once in Lunta's panel in a document: `identity.launchWebAuthFlow` shows Google's own sign-in and consent (scope: Google Docs documents) and returns an access token, kept only in the extension's background script. Nothing else uses it. |
| `storage` | Keeps that Google access token in session storage (gone when the browser quits) so the user isn't asked again on every click, and remembers the journal citation styles the user picked before. |
| Host permission `<all_urls>` | Scientific papers are published on thousands of sites (journals, publishers, preprint servers, university repositories). The content script looks for a paper's citation details (title, authors, DOI) in the page to show Lunta's panel, and, when the user asks, downloads the paper's PDF, figures and table pages with the user's own access. It does nothing on pages without a paper. The same access covers Google Docs documents (docs.google.com: the citing panel) and the Google Docs API (docs.googleapis.com: reading and writing the document the user cites in). |
| Host permission `http://127.0.0.1:47480/*` | The extension talks to the user's Lunta app on the same computer: to check whether the paper is already in the library, to hand it the paper, PDF or page the user chose to save, and to search the library and format citations for Google Docs. |

**Remote code:** No, I am not using remote code. (All scripts are in the package.)

**Data usage.** Tick:
- **Website content** (the article page's text and images, and PDFs, when the user saves a paper or uses
  the page's full text; the citation details of article pages, to show whether the paper is in the library;
  the Google Docs document the user cites in, read and written through the Google Docs API).

Leave everything else unticked (no personal information, health, financial, authentication, personal
communications, location, web history, or user activity is collected). Then certify all three:
- I do not sell or transfer user data to third parties, outside of the approved use cases.
- I do not use or transfer user data for purposes that are unrelated to my item's single purpose.
- I do not use or transfer user data to determine creditworthiness or for lending purposes.

All of it goes only to the Lunta app on the user's own computer; a Google Docs document goes only between the
browser and Google Docs, where it lives. Nothing is sent to the developer or anyone else. (The Google access
token isn't "collected": it stays in the browser's session storage.)

**Privacy policy URL:** https://christoffermattssonlangseth.github.io/lunta/app-privacy.html

## Distribution tab

- Visibility: **Public**.
- Regions: all.

Google's own review of the Docs permission (OAuth verification) is separate: see
[google-docs-extension.md](google-docs-extension.md).

## Review

Access to all sites triggers an in-depth review: usually a few days, sometimes up to a few weeks. If the
reviewer asks how to test it: install Lunta for Mac, Windows or Linux from the latest release and open it,
then open any article page, for example https://doi.org/10.1038/s41586-023-06139-9 (open access), and click
the Lunta tab at the right edge of the page. Citing: open a Google Docs document, click the Lunta button at the
bottom right, Connect Google account, search for the paper saved before and Insert Citation.

## After it's published

- Replace "Load unpacked" in the README and on the Getting started page with the store link.
- People who loaded the extension by hand should remove that copy (`chrome://extensions` › Remove) after
  installing from the store, or they'll have two.

## Microsoft Edge Add-ons (optional, free)

https://partner.microsoft.com/dashboard/microsoftedge: register (free), **Create new extension**, upload
the same zip, and reuse the texts and images above (Edge asks for the same privacy answers). Review takes
up to a week.

## Firefox Add-ons (free)

The same zip is a Firefox add-on (the manifest carries Firefox's id, `lunta@cmlangseth.margins`, and
declares that it collects no data). Firefox only installs add-ons that Mozilla has signed, so this is also
the only way Firefox users can keep it installed.

1. https://addons.mozilla.org/developers/: sign in with a Firefox account (free), accept the agreement.
2. **Submit a New Add-on** › **On this site** (listed in the store) › upload the release's extension zip.
   Mozilla's validator runs; it warns that `background.service_worker` isn't supported, which is expected
   (that line is for Chrome; Firefox uses `background.scripts`).
3. **Source code:** answer **No** (the code isn't minified or generated; reviewers read it as is).
4. Listing: reuse the name, summary and description above; category **Search Tools**, or **Other**;
   license **MIT**; the same screenshots; support site and homepage as above; the same privacy policy URL.
5. Notes to reviewer: the same as the Chrome review notes (install Lunta, open an article page, click the
   Lunta tab), plus: "Requests go only to the user's own Lunta app on 127.0.0.1:47480; page requests use
   `content.fetch` so the user's own access applies."

Review: automatic signing within minutes; a human review can follow later. Each new version: **Upload New
Version** with the new zip (higher version in the manifest).
