"""Step 8: s03's per-paper metrics for the systems added on 2026-11-18.

  pdftotext, pdftotext-layout, grobid   outside reference points on the 200 main-corpus PDFs (s07)
  page-swift, page-rust                 reading views made from the journal's article page (s06 +
                                        `p2e --page` / `paper2epub --page`), on the papers whose page
                                        could be fetched

Scored with exactly the functions of s03 (same ground truth, alignment and detectors). Writes
results/per_paper_extra.csv (one row per paper x system) and results/pairs_extra.csv.
"""
import csv
import sys
from concurrent.futures import ProcessPoolExecutor

import s03_metrics as M
from common import CORPUS, OUT, RESULTS, save_json, DATA

PDF_SYSTEMS = ["pdftotext", "pdftotext-layout", "grobid"]
PAGE_SYSTEMS = ["page-swift", "page-rust"]


def do_paper(pmcid):
    try:
        g = M.gt_of(pmcid)
        margins = M.pdf_margins(CORPUS / pmcid / "article.pdf")
        vocab = {}
        rows = []
        for s in PDF_SYSTEMS + PAGE_SYSTEMS:
            if not (OUT / s / pmcid / "run.json").exists():
                continue
            r, _ = M.score(pmcid, s, g, margins, vocab)
            rows.append(r)
        return rows
    except Exception as err:
        import traceback
        traceback.print_exc()
        print(f"FAILED {pmcid}: {err}", file=sys.stderr)
        return []


def main():
    corpus = [r for r in csv.DictReader(open(RESULTS / "corpus.csv")) if r["group"] == "pub"]
    rows = []
    with ProcessPoolExecutor(3) as ex:
        for r in ex.map(do_paper, [c["pmcid"] for c in corpus]):
            rows += r
    keys = []
    for r in rows:
        keys += [k for k in r if k not in keys]
    with open(RESULTS / "per_paper_extra.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, keys)
        w.writeheader()
        for r in rows:
            w.writerow({k: (round(v, 5) if isinstance(v, float) else v) for k, v in r.items()})
    print(f"wrote {len(rows)} rows", file=sys.stderr)


if __name__ == "__main__":
    main()
