"""Step 7: outside reference points for PDF conversion on the same 200 PDFs.

  pdftotext   poppler `pdftotext` (reading order, no layout), all text of the PDF
  pdftotext-layout   the same with `-layout`
  grobid      GROBID processFulltextDocument (TEI): abstract, body and back-matter divs (heads and
              paragraphs) without the reference list, figures, tables or formulas; run against a
              local GROBID server (Docker, see README), one PDF at a time

Each output is written as a minimal one-chapter EPUB folder (data/out/<system>/<PMCID>/epub/) with
<h2> headings and <p> paragraphs, so that s03's scoring reads it exactly like a converter's output;
run.json holds the wall-clock time. GROBID's reference entries are written as an <ol class="refs">
(counted as reference items, not as text), its figure and table captions as <figure>/<div
class="table-wrap"> captions without images.

Usage: s07_baselines.py pdftotext|pdftotext-layout|grobid [--url http://localhost:8070]
"""
import argparse
import csv
import html
import os
import re
import subprocess
import sys
import time
import urllib.request
import uuid
from pathlib import Path

from lxml import etree

from common import CORPUS, OUT, RESULTS, load_json, save_json

OPF = """<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><manifest>
<item id="c1" href="text.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="c1"/></spine></package>"""


def write_epub(od, blocks, extra=""):
    """blocks: [(kind, text)] with kind 'h' or 'p'."""
    d = od / "epub" / "EPUB"
    d.mkdir(parents=True, exist_ok=True)
    (d / "content.opf").write_text(OPF)
    body = "\n".join(f"<h2>{html.escape(t)}</h2>" if k == "h" else f"<p>{html.escape(t)}</p>"
                     for k, t in blocks if t.strip())
    (d / "text.xhtml").write_text('<?xml version="1.0" encoding="utf-8"?>\n<html xmlns="http://www.w3.org/1999/xhtml">'
                                  f"<head><title>t</title></head><body>\n{body}\n{extra}</body></html>")


def run_pdftotext(pmcid, layout):
    system = "pdftotext-layout" if layout else "pdftotext"
    od = OUT / system / pmcid
    if (od / "run.json").exists():
        return load_json(od / "run.json")
    od.mkdir(parents=True, exist_ok=True)
    argv = ["pdftotext", "-enc", "UTF-8"] + (["-layout"] if layout else []) + \
        [str(CORPUS / pmcid / "article.pdf"), "-"]
    t0 = time.perf_counter()
    p = subprocess.run(argv, capture_output=True)
    wall = time.perf_counter() - t0
    txt = p.stdout.decode("utf-8", "replace")
    paras = [" ".join(x.split()) for x in re.split(r"\n\s*\n|\f", txt)]
    write_epub(od, [("p", x) for x in paras if x])
    rec = {"system": system, "pmcid": pmcid, "rc": p.returncode, "ok": p.returncode == 0,
           "wall_s": round(wall, 3)}
    save_json(od / "run.json", rec)
    return rec


TEI = "{http://www.tei-c.org/ns/1.0}"


def tei_text(e):
    """Text of a TEI element without its <ref> markers' brackets dropped (keeps the citation text)."""
    return " ".join("".join(e.itertext()).split())


def tei_blocks(root):
    blocks, figs = [], []
    prof = root.find(f".//{TEI}profileDesc/{TEI}abstract")
    if prof is not None:
        for e in prof.iter(f"{TEI}head", f"{TEI}p"):
            blocks.append(("h" if e.tag == f"{TEI}head" else "p", tei_text(e)))
    text = root.find(f"{TEI}text")
    for part in (text.find(f"{TEI}body"), text.find(f"{TEI}back")):
        if part is None:
            continue
        for div in part.iter(f"{TEI}div"):
            if div.get("type") == "references" or div.find(f"{TEI}listBibl") is not None:
                continue
            for e in div:
                if e.tag == f"{TEI}head":
                    n = e.get("n")
                    blocks.append(("h", f"{n} {tei_text(e)}" if n else tei_text(e)))
                elif e.tag in (f"{TEI}p", f"{TEI}list"):
                    blocks.append(("p", tei_text(e)))
                elif e.tag == f"{TEI}formula":
                    pass                       # display formulas: left out, as in the XML ground truth
        for f in part.iter(f"{TEI}figure"):
            head = f.find(f"{TEI}head")
            desc = f.find(f"{TEI}figDesc")
            cap = " ".join(x for x in (tei_text(head) if head is not None else "",
                                       tei_text(desc) if desc is not None else "") if x)
            figs.append((f.get("type") == "table", cap))
    refs = root.findall(f".//{TEI}listBibl/{TEI}biblStruct")
    return blocks, figs, len(refs)


def run_grobid(pmcid, url):
    od = OUT / "grobid" / pmcid
    if (od / "run.json").exists():
        return load_json(od / "run.json")
    od.mkdir(parents=True, exist_ok=True)
    pdf = (CORPUS / pmcid / "article.pdf").read_bytes()
    bnd = uuid.uuid4().hex
    parts = []
    for k, v in (("consolidateHeader", "0"), ("consolidateCitations", "0"), ("includeRawCitations", "0")):
        parts.append(f'--{bnd}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode())
    parts.append(f'--{bnd}\r\nContent-Disposition: form-data; name="input"; filename="article.pdf"\r\n'
                 f"Content-Type: application/pdf\r\n\r\n".encode() + pdf + b"\r\n")
    parts.append(f"--{bnd}--\r\n".encode())
    req = urllib.request.Request(url + "/api/processFulltextDocument", data=b"".join(parts),
                                 headers={"Content-Type": f"multipart/form-data; boundary={bnd}"})
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=600) as r:
            tei = r.read()
            rc = 0
    except Exception as e:
        tei, rc = str(e).encode(), 1
    wall = time.perf_counter() - t0
    (od / "tei.xml").write_bytes(tei)
    rec = {"system": "grobid", "pmcid": pmcid, "rc": rc, "ok": False, "wall_s": round(wall, 3)}
    if rc == 0:
        try:
            root = etree.fromstring(tei, etree.XMLParser(recover=True, huge_tree=True))
            blocks, figs, nrefs = tei_blocks(root)
            extra = "".join(
                f'<div class="table-wrap"><p class="caption">{html.escape(c)}</p></div>' if is_tab
                else f"<figure><figcaption>{html.escape(c)}</figcaption></figure>" for is_tab, c in figs)
            extra += '<ol class="refs">' + "<li>r</li>" * nrefs + "</ol>"
            write_epub(od, blocks, extra)
            rec["ok"] = True
        except Exception as e:
            rec["error"] = str(e)
    save_json(od / "run.json", rec)
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("system", choices=["pdftotext", "pdftotext-layout", "grobid"])
    ap.add_argument("--url", default="http://localhost:8070")
    a = ap.parse_args()
    ids = [r["pmcid"] for r in csv.DictReader(open(RESULTS / "corpus.csv")) if r["group"] == "pub"]
    for i, pmcid in enumerate(ids):
        r = run_pdftotext(pmcid, a.system == "pdftotext-layout") if a.system.startswith("pdftotext") \
            else run_grobid(pmcid, a.url)
        if not r["ok"] or (i + 1) % 20 == 0:
            print(f"{a.system} {i + 1}/{len(ids)} {pmcid} ok={r['ok']} {r['wall_s']}s", file=sys.stderr)


if __name__ == "__main__":
    main()
