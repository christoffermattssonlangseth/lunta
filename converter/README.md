# paper2epub

Turn a scientific paper into a clean, reflowable EPUB for your e-reader or phone.

```bash
paper2epub 10.7554/eLife.09560          # DOI
paper2epub 2106.01234                   # arXiv ID
paper2epub PMC7042881                   # PubMed Central ID
paper2epub paper.pdf -o paper.epub      # local PDF
paper2epub ./latex-source/              # folder or .tar.gz with main.tex
```

## How it works

The tool tries the best available source first and falls back on its own:

| Order | Source | Where it comes from | Quality |
|---|---|---|---|
| 1 | **JATS XML** | Europe PMC / PMC open access, eLife, or a local `.xml`/`.nxml` | Best: real structure, MathML, real tables, linked references |
| 2 | **LaTeX source** | arXiv e-print or a local folder/archive | Very good: math as MathML, citeproc references, TikZ and algorithms rendered as images |
| 3 | **PDF + Claude** | arXiv PDF, Unpaywall open-access PDF, or a local PDF | Very good: each page goes to Claude with vision and comes back as Markdown with LaTeX math; figures are cropped from the PDF |
| 4 | **PDF, heuristic** | Same PDFs, when no API key is set | Decent: reading order, columns, headings, de-hyphenation and figure/table crops; equations and tables become images |

Every path ends in the same post-processing step. It converts and downscales images, adds metadata, a generated cover and the abstract, and fixes links. Pandoc then builds an EPUB3 with MathML, and `epubcheck` validates it.

## Install

```bash
# system: pandoc >= 3.0  (brew install pandoc / apt install pandoc)
# optional: java (for epubcheck), pdflatex (for TikZ/algorithm rendering), ghostscript (EPS figures)
pip install -r requirements.txt
```

## Options and environment

| | |
|---|---|
| `ANTHROPIC_API_KEY` | turns on Claude page transcription for PDFs (`--llm auto`, the default) |
| `--llm on/off` | force the Claude path on or off |
| `--model` / `PAPER2EPUB_MODEL` | Claude model (default `claude-sonnet-5-5`) |
| `PAPER2EPUB_EMAIL` | contact e-mail for Unpaywall lookups and polite API use |
| `--source jats/latex/pdf` | force a source type |
| `--keep` | keep the working folder (intermediate Markdown, crops) for debugging |
| `--no-cover`, `--no-validate` | skip cover generation or epubcheck |

Cost of the Claude path: about one vision call per page. A 20-page paper uses roughly 20 calls with up to 12k output tokens each.

## Use from Python

```python
from paper2epub import convert
report = convert("10.7554/eLife.09560", out="naledi.epub")
print(report["notes"], report["epubcheck"])
```

## Known limitations

- Many publishers block scripted PDF downloads. If a DOI fails, download the PDF in your browser and pass the file.
- bioRxiv/medRxiv: full text comes through Europe PMC when it is indexed there. Otherwise, use the PDF.
- Heuristic PDF mode keeps equations as images and can misread unusual layouts. Use the Claude mode for math-heavy papers.
- Very wide tables scroll horizontally. Some readers, notably Kindle, ignore this and shrink them instead.
- The JATS path skips eLife's decision letters and author responses (sub-articles).

## QA helper

`preview_epub.py book.epub shots/` renders every chapter at phone width (needs `playwright`). It's useful for checking a conversion visually.
