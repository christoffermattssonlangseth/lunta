"""Step 6c: reading views made from the saved article pages, as "Use this page's full text" makes them.

  page-swift   p2e --page <url> page.html --images images/      (Mac, iPhone, iPad)
  page-rust    paper2epub --page <url> page.html --images images/  (Android, Windows, Linux)

Output in data/out/<system>/<PMCID>/ like s02 (book.epub, epub/, log.txt, run.json; run.json also
holds the page reader the converter chose). One conversion at a time.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time
import zipfile

from common import DATA, OUT, save_json
from s06_pages import PAGES

BIN = DATA / "bin"


def run(system, d):
    rec = json.loads((d / "fetch.json").read_text())
    od = OUT / system / d.name
    if (od / "run.json").exists():
        return json.loads((od / "run.json").read_text())
    if od.exists():
        shutil.rmtree(od)
    tmp = od / "tmp"
    tmp.mkdir(parents=True)
    exe = BIN / ("p2e" if system == "page-swift" else "paper2epub")
    argv = [str(exe), "--page", rec["final_url"], str(d / "page.html")]
    if (d / "images" / "urls.tsv").exists():
        argv += ["--images", str(d / "images")]
    argv.append(str(tmp))
    t0 = time.perf_counter()
    p = subprocess.run(argv, capture_output=True, text=True, timeout=300,
                       env=dict(os.environ, PDFIUM_PATH=str(DATA / "pdfium")))
    wall = time.perf_counter() - t0
    log = p.stdout + p.stderr
    (od / "log.txt").write_text(log)
    epubs = list(tmp.glob("*.epub"))
    m = re.search(r"reader: ([^;]+);", log)
    out = {"system": system, "pmcid": d.name, "rc": p.returncode, "ok": p.returncode == 0 and len(epubs) == 1,
           "wall_s": round(wall, 3), "reader": m.group(1) if m else None, "group": rec["group"],
           "url": rec["final_url"]}
    if out["ok"]:
        os.replace(epubs[0], od / "book.epub")
        with zipfile.ZipFile(od / "book.epub") as z:
            z.extractall(od / "epub")
    else:
        out["error"] = log.strip().splitlines()[-1] if log.strip() else ""
    shutil.rmtree(tmp, ignore_errors=True)
    save_json(od / "run.json", out)
    return out


def main():
    for d in sorted(PAGES.glob("PMC*")):
        if not (d / "page.html").exists():
            continue
        for system in ("page-swift", "page-rust"):
            r = run(system, d)
            print(f"{system} {d.name} ok={r['ok']} reader={r['reader']} {r.get('error', '')}", file=sys.stderr)


if __name__ == "__main__":
    main()
