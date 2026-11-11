---
name: paper-to-epub
description: Convert a scientific paper (DOI, arXiv ID, PMCID, URL, PDF, JATS XML or LaTeX source) into a clean EPUB, then check and repair the result. Use when the user asks to turn a paper into an ebook/EPUB for an e-reader or phone.
---

# Paper → EPUB

You are the agent around the `paper2epub` tool. The tool does the mechanical conversion. Your job is to pick inputs, check the output and fix what the tool got wrong.

## 1. Convert

```bash
paper2epub "<input>" -o "<name>.epub" --workdir /tmp/p2e-<slug> -v
```

- Input can be a DOI, arXiv ID, PMCID, URL or local file. The tool tries JATS XML, then LaTeX source, then PDF, in that order.
- If `ANTHROPIC_API_KEY` is set, PDFs are transcribed page by page with Claude. Otherwise the heuristic extractor is used.
- If every source fails (paywall, blocked download), ask the user for the PDF. Do not try to get around the block.

Read the printed report: source used, word count, image count, notes and epubcheck result.

## 2. Inspect (always)

1. Run `python preview_epub.py <name>.epub /tmp/p2e-shots` and look at the chapter screenshots: the first body chapter, one with figures, one with equations or tables, and References.
2. Skim `/tmp/p2e-<slug>/paper.md` (PDF paths) or `jats.html` (JATS path).
3. Sanity checks:
   - Word count should be roughly 3–8k for a typical paper.
   - The title and authors should be right on the title page.
   - The abstract comes first.
   - Section headings should match the paper's structure.
   - Figures should have captions.
   - There should be no running headers, line numbers or "bioRxiv preprint" stamps in the text.

## 3. Repair (PDF paths)

Edit `/tmp/p2e-<slug>/paper.md` directly. It is Pandoc Markdown with a YAML header, and figures point to absolute paths of PDF crops. Typical fixes:

- Fix the metadata (`title`, `author`, `date`, `doi`, `journal`) in the YAML header.
- Promote or demote headings (`#` section, `##` subsection). Delete table cells that were misread as headings.
- Rejoin paragraphs split at page or column breaks.
- Move a caption next to its figure: `![Figure 2. caption](path)`.
- Replace an equation image with LaTeX (`$$...$$`) when you can read it from the page image (`pdftoppm -f N -l N -r 110 -png paper.pdf /tmp/pg`).
- Turn a simple table image into a pipe table.

Then rebuild:

```bash
paper2epub /tmp/p2e-<slug>/paper.md -o "<name>.epub" --workdir /tmp/p2e-<slug>-rebuild
```

For the JATS and LaTeX paths, problems are rare. If figures are missing (the report notes how many), say so. Fetching from publisher sites is often blocked.

## 4. Deliver

Send the EPUB. In one or two lines, say which source was used and anything still imperfect, such as "3 equations kept as images" or "figures unavailable from the publisher". Don't recap the steps.
