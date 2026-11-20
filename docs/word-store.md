# Lunta Citations in Microsoft's add-in store (AppSource)

Once listed, anyone installs it from Word: **Home › Add-ins**, search **Lunta Citations**, **Add**. It works
in Word on Mac, on Windows and on the web, with Lunta for Mac or Lunta for Windows (or Linux, for Word on the web)
open on the same computer, with no files to copy. (Lunta for Windows can also put the add-in into Word itself:
Install in Word, see development.md.)

- Manifest to upload: `apple/citations/word/manifest-store.xml` (validated with
  `npx office-addin-manifest validate`). The panel it loads is the hosted copy on GitHub Pages
  (`https://getlunta.app/word/index.html`), published on every push to main.
- Store logo (300 × 300): `appstore/microsoft/logo-300.png`.
- Screenshots (1366 × 768 or 1280 × 720, PNG): take them in real Word with the panel open (see below).

## Once: the account

1. https://partner.microsoft.com/dashboard › sign in with a Microsoft account (or create one, free).
2. Enroll in **Microsoft 365 and Copilot** (the program for Office add-ins; formerly "Office Store").
   Account type **Individual**. Registration is free; Microsoft may verify your identity (a few days).

## The submission

**Marketplace offers › + New offer › Office add-in.** Name: `Lunta Citations`.

**Packages:** upload `manifest-store.xml`.

**Properties:**
- Category: **Productivity** (secondary: **Education**, if offered).
- Industries: **Education**, **Healthcare + Life Sciences** (optional).
- Privacy policy URL: `https://getlunta.app/app-privacy.html`
- Support URL: `https://getlunta.app/support.html`
- End-user license agreement: the standard contract (or the MIT license URL:
  `https://github.com/christoffermattssonlangseth/lunta/blob/main/LICENSE`).
- "Does your add-in require additional purchases or software?" **Yes**: it needs the free Lunta app.

**Listing (English):**

Name: `Lunta Citations`

Summary (100):
```
Cite papers from your Lunta library in Word, with the reference list kept up to date.
```

Description:
```
Lunta Citations brings your Lunta library into Word. Search your papers, insert a citation where the cursor is, and Lunta builds the reference list.

• 12 built-in styles (APA 7, Harvard, Chicago, Vancouver, IEEE, AMA, MLA, Nature, Science, Cell, PNAS, ACS) and about 10,000 journal styles from the Citation Style Language.
• Several papers in one citation: pick them together, or click inside a citation and add more.
• Update All renumbers citations, rebuilds the bibliography and switches styles in one step, and is safe to run as often as you like.
• Documents keep the details of the papers they cite, so co-authors without your library, and you with Lunta closed, can still update them.
• Private: the add-in talks only to the Lunta app on your own computer. No account, no server, nothing collected.

REQUIRES THE LUNTA APP
Lunta Citations needs Lunta for Mac or Windows (free) running on the same computer: https://github.com/christoffermattssonlangseth/lunta/releases/latest. In Lunta, choose Citations in Word & Google Docs… (on the Mac in the Lunta menu, on Windows under Settings) and click Trust Certificate once.

Lunta is a free, open-source reader and reference manager for scientific papers, for Mac, iPhone, iPad, Android, Windows and Linux.
```

Search keywords: `citations, references, bibliography, reference manager, APA, papers`

Screenshots: 2–4, from real Word (Mac), 1366 × 768: the panel with search results; a document with
numbered citations and the bibliography; the style menu. (Add the Lunta icon logo: `logo-300.png`.)

**Notes for certification (testing instructions):**
```
Lunta Citations is a citation tool for the Lunta reference manager. It needs the Lunta desktop app, which is free and has no account: Lunta for Mac or Windows, https://github.com/christoffermattssonlangseth/lunta/releases/latest (Mac: download Lunta-<version>-Mac-AppleSilicon.zip, unzip, open; Windows: Lunta-<version>-Windows-Setup.exe).

The add-in talks only to the Lunta app on the same computer, at https://localhost:47481. To test on a Mac:
1. Install and open Lunta. Add a paper: click + and paste the DOI 10.1038/s41586-023-06139-9 (and a second, 10.1038/s41592-025-02814-z).
2. In Lunta: Lunta › Citations in Word & Google Docs… › Trust Certificate (macOS asks for the password once; this trusts Lunta's certificate for localhost only).
3. In Word: Home › Cite. The panel says "Connected to Lunta". Search "Geneformer", pick the paper, Insert Citation; click Insert Bibliography; change the style to Vancouver and click Update All.

On Windows: install and open Lunta, add the papers as in step 1, then Settings ▾ › Citations in Word & Google Docs… › Trust Certificate… (Windows asks to install "Lunta Local CA": Yes; it is valid for localhost only), then step 3.

Without the Lunta app (for example Word on the web without Lunta running on that computer), the panel shows a clear message that the Lunta app is needed and how to get it; it does nothing else.

No data is collected or sent to any server.
```

**Availability:** all markets. **Submit.** Certification usually takes 3–5 working days; Microsoft emails
the result, with a report if anything needs changing.

## After it's listed

- The listing link goes in the README ("Word and Google Docs") and on `site/support.html`.
- Updates to the panel ship with every push to main (it's hosted); only manifest changes (name, icons,
  permissions, buttons) need a new submission with a higher `<Version>`.
