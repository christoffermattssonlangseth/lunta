# Evaluation of Lunta's paper → reading-view conversion

A reproducible benchmark of how well Lunta turns a paper's **PDF** into a reading view, measured
against the publisher's **JATS XML** of the same article (PubMed Central open access). Results and
discussion: [RESULTS.md](RESULTS.md). Everything here is a script; downloaded and generated data
live in `data/` (git-ignored).

## What is compared

| System | What | How it is run |
|---|---|---|
| `swift` | Swift `Paper2EpubKit` CLI (macOS/iOS apps, PDFKit), current working tree | `p2e --offline article.pdf out/` |
| `base` | the same CLI at `38d2bdc^` (b813981, before the PDF fixes of 38d2bdc) | `p2e-base --offline article.pdf out/` |
| `rust` | Rust converter `core/src/bin/paper2epub.rs` (Android, Windows/Linux; PDFium chromium/8152) | `paper2epub --offline article.pdf out/` |
| `xml` | reading view built from the JATS XML (Swift) — target of the highlight test, and a control for the metrics | `p2e article.xml out/` |
| `pdftotext` | poppler `pdftotext` (26.10.0), all text of the PDF — outside reference point (floor) | `pdftotext -enc UTF-8 article.pdf -` (and `-layout`) |
| `grobid` | GROBID 0.9.1, CRF models (Docker `grobid/grobid:0.9.1-crf`) — outside reference point | `processFulltextDocument`; TEI abstract/body/back divs without references, figures, tables, formulas |
| `page-swift`, `page-rust` | reading view from the journal's article page ("Use this page's full text"), for the papers whose open-access page could be fetched | `p2e --page <url> page.html --images images/` / `paper2epub --page …` |
| `pdffig` | JATS XML whose figures are cut from the PDF (what the app does for a PDF whose DOI has an XML version without images, e.g. author manuscripts) | `p2e --pdf article.pdf article.xml out/` |

`--offline` makes the converters analyse the PDF itself rather than look up an XML version by the
PDF's DOI. `--pdf` is a small evaluation flag added to `p2e`
(`apple/Paper2EpubKit/Sources/p2e/main.swift`, public wrapper `Paper2Epub.convert(jats:pdf:into:)`
in `Converter.swift`); it calls the same internal `build(_:pdf:)` path the app uses and changes no
behaviour of the app.

## Requirements

- macOS with Xcode (Swift CLI), Rust toolchain, Node ≥ 18, `uv` (or any Python ≥ 3.10 + pip)
- PDFium for the Rust converter: `pdfium-mac-arm64.tgz` from
  [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases) (`chromium/8152` was used)
- Network access to `https://pmc-oa-opendata.s3.amazonaws.com` (public, no account)

## Reproduce

```sh
cd eval
uv venv -p 3.12 .venv && uv pip install -p .venv/bin/python -r requirements.txt
mkdir -p data/bin data/pdfium

# 1. converters
(cd ../apple/Paper2EpubKit && xcodebuild -scheme p2e -configuration Release -destination 'platform=macOS' \
   -derivedDataPath ../build/dd-eval-pkg build) && cp ../apple/build/dd-eval-pkg/Build/Products/Release/p2e data/bin/p2e
git worktree add --detach ../../Lunta-eval-base 38d2bdc^
(cd ../../Lunta-eval-base/apple/Paper2EpubKit && xcodebuild -scheme p2e -configuration Release \
   -destination 'platform=macOS' -derivedDataPath ../build/dd-eval build) \
   && cp ../../Lunta-eval-base/apple/build/dd-eval/Build/Products/Release/p2e data/bin/p2e-base
git worktree remove --force ../../Lunta-eval-base
(cd ../core && cargo build --release --bin paper2epub) && cp ../core/target/release/paper2epub data/bin/
curl -L https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8152/pdfium-mac-arm64.tgz | tar xz -C data/pdfium \
   && mv data/pdfium/lib/libpdfium.dylib data/pdfium/

# 2. corpus (S3 inventory of 2026-11-15 as sampling frame; seed 20261116)
mkdir -p data/inventory && cd data/inventory
curl -sO https://pmc-oa-opendata.s3.amazonaws.com/inventory-reports/pmc-oa-opendata/metadata/2026-11-15T01-00Z/manifest.json
python3 -c "import json;[print(f['key']) for f in json.load(open('manifest.json'))['files']]" \
   | while read k; do curl -sO "https://pmc-oa-opendata.s3.amazonaws.com/$k"; done
cd ../../scripts
../.venv/bin/python s01_sample.py          # → data/corpus/<PMCID>/, results/corpus.csv

# 3.–6.
../.venv/bin/python s02_convert.py --workers 3   # all systems, all papers (~30 min on an M-series Mac)
../.venv/bin/python s02_convert.py --timing      # 60 papers, one conversion at a time (speed)
../.venv/bin/python s03_metrics.py               # → results/per_paper.csv, pairs.csv, pdffig.csv
node s04_highlights.mjs                          # → results/highlights.csv
../.venv/bin/python s05_aggregate.py             # → results/summary.json, summary_tables.md, figures/*.pdf

# 7.–9. (added 2026-11-18) outside reference points and journal-page reading views
brew install poppler colima docker && colima start --cpu 4 --memory 7 --vm-type vz
docker run -d --name grobid-eval --memory 6g --cpus 3 -p 8070:8070 grobid/grobid:0.9.1-crf
../.venv/bin/python s07_baselines.py pdftotext && ../.venv/bin/python s07_baselines.py grobid
../.venv/bin/python s06_pages.py --extra          # article pages (plain GET, browser UA, 6 s apart; blocks recorded, not bypassed)
../.venv/bin/python s06b_page_images.py           # figure images shown on the pages (2.5 s apart)
../.venv/bin/python s06b2_page_images.py <PMCID…> # second pass for pages whose view still lacked figure images
../.venv/bin/python s06c_convert_pages.py         # → data/out/page-swift, page-rust
../.venv/bin/python s08_extra_metrics.py          # → results/per_paper_extra.csv
node s04_highlights.mjs --scenarios swift:page-swift,rust:page-rust,swift:xml,xml:page-swift --out highlights_pages
../.venv/bin/python s09_extra_aggregate.py        # → results/summary_extra.{json,md}, pages.csv, figures/f1_by_source.pdf
```

Pages are fetched as a browser would (one plain GET through doi.org with a Safari User-Agent, no
JavaScript, no login), one at a time; a site that answers with a bot check, an HTTP error or a
robots.txt that disallows or cannot be read is recorded in `data/pages/<PMCID>/fetch.json` and
skipped. The second pass (`s06b2`) was run for the 13 pages whose views still lacked figure images:
PMC10898353 PMC10999593 PMC3869048 PMC4032908 PMC4273299 PMC4333867 PMC6123364 PMC7509473
PMC7577192 PMC8282988 PMC8669635 PMC8789677 PMC9852304 (Frontiers, Beilstein, Gates Open Research).

The corpus is fully determined by the inventory date, the seed and the eligibility rules in
`s01_sample.py`; `results/corpus.csv` lists the PMCIDs and article versions actually used, so the same
articles can also be fetched directly (`https://pmc-oa-opendata.s3.amazonaws.com/<PMCID>.<v>/<PMCID>.<v>.{xml,pdf}`).
HTTP requests use a generic descriptive User-Agent, at most 6 concurrent small metadata requests,
sequential file downloads, and retries with back-off.

## Files

```
scripts/common.py         paths, seed, polite HTTP
scripts/s01_sample.py     sampling frame, eligibility, stratified sample, downloads
scripts/s02_convert.py    runs the converters, records wall/CPU time and memory
scripts/textutil.py       JATS ground truth; EPUB parsing; normalisation and tokenisation
scripts/s03_metrics.py    per-paper metrics (text, order, broken words, boilerplate, floats, agreement)
scripts/s04_highlights.mjs  highlight survival with the reader's own locate() from apple/Shared/Highlights.swift
scripts/s05_aggregate.py  bootstrap CIs, paired tests, tables, figures
scripts/s06_pages.py, s06b_page_images.py, s06b2_page_images.py, s06c_convert_pages.py
                          journal article pages: fetch, figure images, conversion
scripts/s07_baselines.py  pdftotext and GROBID outputs as one-chapter EPUB folders
scripts/s08_extra_metrics.py, s09_extra_aggregate.py  s03 metrics and summaries for the added systems
scripts/s10_lost_split.py  crude split of the lost highlights (no counterpart / garbled), RESULTS section 7
results/                  corpus.csv, corpus_excluded.csv, sampling.json, per_paper.csv, pairs.csv,
                          pdffig.csv, highlights.csv, summary.json, summary_tables.md
figures/                  text_fidelity.pdf, f1_current_vs_baseline.pdf, highlight_survival.pdf,
                          speed.pdf, swift_vs_rust.pdf
data/ (git-ignored)       inventory, metadata cache, corpus (XML+PDF), binaries, converter outputs,
                          examples/<PMCID>.json (broken-word and leak examples), highlights.jsonl
```

## Metric definitions (short)

All text is NFKC-normalised, dashes/quotes unified, lower-cased, and tokenised into `\w+` words
(hyphenated compounds count as their parts; punctuation is ignored).

- **Ground truth** (from the JATS XML): abstract + body paragraphs and section titles (with section
  labels), excluding figures, tables, display formulas, supplementary material and footnotes;
  back matter other than the reference list (acknowledgements, declarations, appendices) is added
  as "extended" ground truth so that PDF text from those sections is not counted as noise.
- **Output text**: the reading view without title page, figure and table captions, equation images
  and the references section.
- **Token precision/recall/F1**: LCS alignment of the token sequences (rapidfuzz Indel);
  precision over all output tokens (vs extended GT), recall over abstract+body tokens.
  **WER**: token Levenshtein distance / GT length. **Bag-of-words F1**: multiset overlap (order-free).
- **Reading order**: XML paragraphs (≥ 15 tokens) located in the output by unique 5-gram shingles
  (≥ 30 % of their shingles, median position); Kendall's τ between XML and output order, share of
  consecutive matched paragraphs in order; **interleaving events**: a paragraph's text resumes after
  text from another paragraph intervened (runs of ≥ 3 shingles).
- **Broken words** (per 1000 output tokens, vocabulary = every word in the XML): spurious split
  (two adjacent tokens, at least one not in the vocabulary, whose concatenation is), spurious join
  (an out-of-vocabulary token that splits into an XML bigram), leftover line-break hyphen
  ("transfor-mation"), lost compound hyphen ("longterm"/"long term" where the XML has "long-term").
- **Boilerplate leakage** (per paper): lines in the PDF's top/bottom 8 % or side 6 % margins that
  recur on ≥ 2 pages (digits masked) found in the output text more often than in the XML; printed
  page numbers inserted into running text (pure 1–3-token insertions in the alignment); paragraphs
  with publisher/licence/download boilerplate not present in the XML.
- **Figures/tables**: XML `fig` (with a graphic) and `table-wrap` matched to output figures/tables by
  label number ("Figure 3") or caption text; recall = XML items matched to an output item with an
  image; precision = output images matched to an XML item; caption kept = the XML caption's first 50
  words found (≥ 80 %, in order) in an output caption. Display equations: `disp-formula` vs equation
  images. References: output reference paragraphs / XML `ref`s.
- **Highlight survival**: see the header of `s04_highlights.mjs`.
- **Speed**: wall-clock and CPU (user+sys) time of the CLI process per PDF page (PyMuPDF page count).
- **Agreement**: token F1 (LCS) between two converters' output texts.
