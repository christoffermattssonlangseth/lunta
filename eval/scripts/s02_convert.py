"""Step 2: run every converter on every paper in results/corpus.csv.

Systems (binaries in data/bin, see README):
  swift   current Swift Paper2EpubKit CLI, PDF only:            p2e --offline article.pdf
  base    the same CLI at 38d2bdc^ (before the PDF fixes):       p2e-base --offline article.pdf
  rust    Rust converter (core/src/bin/paper2epub.rs, PDFium):   paper2epub --offline article.pdf
  xml     reading view from the JATS XML (Swift):                p2e article.xml
  pdffig  JATS XML with figures cut from the PDF (Swift):        p2e --pdf article.pdf article.xml

Each run writes data/out/<system>/<PMCID>/{book.epub, epub/, log.txt, run.json}; run.json holds the
exit code, wall-clock and CPU time (user+sys of the child) and peak memory. Existing results are
kept (delete the folder to redo). Usage: s02_convert.py [--workers N] [--systems a,b] [--timing]
--timing re-runs swift and rust one at a time (no other conversions in parallel) on the first 60
papers of the main corpus into data/out/timing-<system>/ for clean speed numbers.
"""
import argparse
import csv
import os
import shutil
import subprocess
import sys
import threading
import time
import zipfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from common import CORPUS, DATA, OUT, RESULTS, load_json, save_json

BIN = DATA / "bin"
PDFIUM = DATA / "pdfium"
TIMEOUT = 300


def cmd(system, d):
    pdf, xml = str(d / "article.pdf"), str(d / "article.xml")
    return {
        "swift": [BIN / "p2e", "--offline", pdf],
        "base": [BIN / "p2e-base", "--offline", pdf],
        "rust": [BIN / "paper2epub", "--offline", pdf],
        "xml": [BIN / "p2e", xml],
        "pdffig": [BIN / "p2e", "--pdf", pdf, xml],
    }[system]


def run_one(system, pmcid, outroot=OUT):
    d = CORPUS / pmcid
    od = outroot / system / pmcid
    if (od / "run.json").exists():
        return load_json(od / "run.json")
    if od.exists():
        shutil.rmtree(od)
    tmp = od / "tmp"
    tmp.mkdir(parents=True)
    argv = [str(a) for a in cmd(system.replace("timing-", ""), d)] + [str(tmp)]
    env = dict(os.environ, PDFIUM_PATH=str(PDFIUM))
    log = open(od / "log.txt", "w")
    t0 = time.perf_counter()
    p = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, env=env, cwd=str(tmp))
    killer = threading.Timer(TIMEOUT, p.kill)
    killer.start()
    _, status, ru = os.wait4(p.pid, 0)
    killer.cancel()
    wall = time.perf_counter() - t0
    log.close()
    rc = os.waitstatus_to_exitcode(status)
    epubs = list(tmp.glob("*.epub"))
    rec = {"system": system, "pmcid": pmcid, "rc": rc, "timeout": wall >= TIMEOUT - 1,
           "wall_s": round(wall, 3), "cpu_s": round(ru.ru_utime + ru.ru_stime, 3),
           "maxrss_mb": round(ru.ru_maxrss / 2**20, 1), "ok": rc == 0 and len(epubs) == 1}
    if rec["ok"]:
        os.replace(epubs[0], od / "book.epub")
        with zipfile.ZipFile(od / "book.epub") as z:
            z.extractall(od / "epub")
        rec["epub_bytes"] = (od / "book.epub").stat().st_size
    shutil.rmtree(tmp, ignore_errors=True)
    save_json(od / "run.json", rec)
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--systems", default="swift,base,rust,xml,pdffig")
    ap.add_argument("--timing", action="store_true")
    ap.add_argument("--downloaded", action="store_true", help="every paper with a meta.json (while sampling runs)")
    a = ap.parse_args()
    if a.downloaded:
        rows = [load_json(p) for p in sorted(CORPUS.glob("*/meta.json"))]
    else:
        rows = list(csv.DictReader(open(RESULTS / "corpus.csv")))
    if a.timing:
        ids = [r["pmcid"] for r in rows if r["group"] == "pub"][:60]
        for system in ("timing-swift", "timing-rust"):
            for i, pmcid in enumerate(ids):
                r = run_one(system, pmcid)
                print(f"{system} {i + 1}/{len(ids)} {pmcid} {r['wall_s']}s ok={r['ok']}", file=sys.stderr)
        return
    jobs = [(s, r["pmcid"]) for r in rows for s in a.systems.split(",")]
    done = 0
    with ThreadPoolExecutor(a.workers) as ex:
        for r in ex.map(lambda j: run_one(*j), jobs):
            done += 1
            if done % 25 == 0 or not r["ok"]:
                print(f"{done}/{len(jobs)} {r['system']} {r['pmcid']} ok={r['ok']} {r['wall_s']}s", file=sys.stderr)


if __name__ == "__main__":
    main()
