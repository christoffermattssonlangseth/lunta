# Bringing your library to Lunta

Lunta imports from any reference manager that can export **BibTeX (.bib)** or **RIS (.ris)**: Zotero,
Mendeley, ReadCube Papers, EndNote, Paperpile, JabRef, BibDesk and others. Titles, authors, DOIs, tags and
notes come along; duplicates (same DOI, or the same title) are merged rather than added twice.

Importing is done in **Lunta for Mac**: **File › Import from Zotero, Mendeley, ReadCube, EndNote…** (⇧⌘I).
The library then syncs to your iPhone and iPad through iCloud, and to Android, Windows and Linux through
Google Drive (see [Using Lunta on several devices](#using-lunta-on-several-devices)).

The import asks two things:

1. **The export file** (.bib or .ris).
2. **Optionally, a folder of PDFs.** Lunta matches each PDF to its paper by the DOI printed in it, or by
   finding the paper's title on the first pages. PDFs that match nothing are listed, not added.

After the import, Lunta makes a reading view for every paper it has a PDF or an open version of. For papers
without a PDF, **File › Find Missing PDFs** fetches legal open-access copies (PubMed Central, Europe PMC,
arXiv, publishers' open versions). A published paper is never given its preprint instead.

Menu names in the other apps vary a little between versions.

## Zotero

1. **File › Export Library…**, format **BibTeX**, and tick **Export Files**.
2. Zotero writes a folder with the .bib file and a `files` folder with the PDFs.
3. In Lunta, choose the .bib file. The PDFs are found from the paths in the export; if any are missed,
   choose the same export folder when Lunta asks for a folder of PDFs.

Zotero's notes and tags come along. Collections don't (BibTeX has no place for them); use tags in Zotero
first if you want to keep them.

## Mendeley

**Mendeley Reference Manager** (the current app):

1. Select all papers (⌘A), then **File › Export…** (or right-click › Export) and save as **BibTeX**.
2. In Lunta, choose the .bib file and skip the folder step.
3. The export doesn't include the PDFs. Use **File › Find Missing PDFs** for open-access papers. For the
   rest, if you have the PDFs in a folder, use **File › Import Folder of PDFs…** to attach them.

**Mendeley Desktop** (the older app): **File › Export…** as BibTeX. If your Mendeley setup stores PDF
paths in the export, Lunta copies the PDFs automatically. Otherwise choose Mendeley's PDF folder (set
under Tools › Options › File Organizer) when Lunta asks for a folder of PDFs.

## ReadCube Papers

1. Select all papers, then **Export › BibTeX** (or RIS).
2. Also choose **Export PDFs** to save the PDFs to a folder.
3. In Lunta, choose the .bib file, then that PDF folder.

## EndNote

1. **File › Export…**, output style **RefMan (RIS) Export**, save as .ris (or .txt and rename to .ris).
2. EndNote keeps PDFs in the library's `.Data/PDF` folder, next to the `.enl` file.
3. In Lunta, choose the .ris file, then the `.Data/PDF` folder.

## Paperpile

1. **Settings › Export**, BibTeX, whole library.
2. Paperpile stores PDFs in a **Paperpile** folder in your Google Drive. Download it (or use Google Drive
   for desktop) and choose it when Lunta asks for a folder of PDFs.

## JabRef, BibDesk and other BibTeX libraries

Choose your .bib file directly. File links in the entries (JabRef's `file`, BibDesk's `bdsk-file`) are
followed and the PDFs copied in.

## Just a folder of PDFs

**File › Import Folder of PDFs…** matches PDFs to papers already in Lunta. To add new papers from PDFs,
drop them on the paper list (or **File › Add PDF…**): Lunta reads each PDF's DOI or title and fills in the
details.

## Using Lunta on several devices

- **Mac, iPhone and iPad:** the library is kept in your iCloud Drive and syncs by itself. Nothing to set
  up beyond being signed in to iCloud.
- **Android, Windows and Linux:** these use a **Lunta** folder in your Google Drive. Sign in with your
  Google account in the app.
- **Mixing them (e.g. a Mac and an Android phone):** on the Mac, open **Lunta › Google Drive for
  Android…** and connect the same Google account. The Mac then keeps the iCloud library and the Google
  Drive folder in sync both ways: papers, highlights, notes, stars, Must Read and reading positions.
  The Mac passes changes on while Lunta is open on it (every few minutes), so changes made on the phone
  reach your other devices once the Mac has synced.
- **Only Android, Windows or Linux (no Mac):** Google Drive is all you need.
