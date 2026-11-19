"""Step 1: draw the corpus from the PMC Article Datasets on AWS (s3://pmc-oa-opendata).

Sampling frame: the bucket's daily S3 inventory (INVENTORY_DATE in common.py), which lists the
metadata JSON of every article version. Article versions are grouped by PMCID (newest version
kept), shuffled with a fixed seed, and their metadata JSON is read in that order until every
stratum is full.

Main corpus ("pub"): open-access, commercial-use licence (CC BY, CC BY-SA, CC BY-ND, CC0;
the former oa_comm subset), published journal version (no preprints, author manuscripts,
historical scans or retractions), with the publisher PDF in the bucket. Stratified by
publication year (5 strata x 40) with at most one article per journal, so the sample spreads
over journals and publishers.

Author-manuscript subset ("ms"): NIHMS author manuscripts (is_manuscript) with a PDF, 30 in total,
at most one per journal.

Every candidate's XML is checked after download (a research/review-type article with at least 800
body words and a <body>); ineligible ones are recorded with the reason and replaced by the next
candidate of the same stratum. Output: data/corpus/<PMCID>/ and results/corpus.csv.
"""
import csv
import gzip
import json
import random
import re
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from lxml import etree

from common import BUCKET, CORPUS, DATA, RESULTS, SEED, fetch, fetch_to, save_json

COMM = {"CC BY", "CC BY-SA", "CC BY-ND", "CC0"}
STRATA = [("<=2009", 0, 2009), ("2010-2014", 2010, 2014), ("2015-2018", 2015, 2018),
          ("2019-2021", 2019, 2021), ("2022-2026", 2022, 2100)]
PER_STRATUM = 40
N_MS = 30
OK_TYPES = {"research-article", "review-article", "brief-report", "case-report", "systematic-review",
            "rapid-communication", "article", "other", "methods-article", "data-paper"}
MAX_META = 16000  # metadata fetch budget
INV = DATA / "inventory"


def frame():
    """{pmcid: latest version} from the S3 inventory."""
    cache = DATA / "frame.json"
    if cache.exists():
        return json.loads(cache.read_text())
    latest = {}
    files = json.loads((INV / "manifest.json").read_text())["files"]
    for f in files:
        with gzip.open(INV / Path(f["key"]).name, "rt") as fh:
            for row in csv.reader(fh):
                m = re.match(r"metadata/(PMC\d+)\.(\d+)\.json$", row[1])
                if m:
                    pmc, v = m.group(1), int(m.group(2))
                    if v > latest.get(pmc, 0):
                        latest[pmc] = v
    cache.write_text(json.dumps(latest))
    return latest


def meta(key):
    p = DATA / "meta" / f"{key}.json"
    p.parent.mkdir(exist_ok=True)
    try:
        fetch_to(f"{BUCKET}/metadata/{key}.json", p)
        return json.loads(p.read_text())
    except Exception as e:
        return {"error": str(e)}


def year_of(citation):
    m = re.search(r"\b(1[89]\d\d|20\d\d)\b", citation or "")
    return int(m.group(1)) if m else None


def journal_of(citation):
    return (citation or "").split(".")[0].strip()


def check_xml(path):
    """(ok, reason, info) for a downloaded JATS file."""
    try:
        root = etree.parse(str(path), etree.XMLParser(recover=True, huge_tree=True)).getroot()
    except Exception as e:
        return False, f"xml parse: {e}", {}
    art_type = root.get("article-type", "")
    body = root.find(".//body")
    pub = root.findtext(".//publisher-name") or ""
    jt = root.findtext(".//journal-title") or ""
    info = {"article_type": art_type, "publisher": " ".join(pub.split()), "journal_title": " ".join(jt.split())}
    if body is None:
        return False, "no body", info
    words = len(" ".join(body.itertext()).split())
    info["body_words"] = words
    if art_type not in OK_TYPES:
        return False, f"article-type {art_type}", info
    if words < 800:
        return False, "body < 800 words", info
    if body.find(".//p") is None:
        return False, "no paragraphs", info
    return True, "", info


def download(pmcid, key):
    d = CORPUS / pmcid
    d.mkdir(parents=True, exist_ok=True)
    fetch_to(f"{BUCKET}/{key}/{key}.xml", d / "article.xml")
    return d


def main():
    latest = frame()
    ids = sorted(latest)
    rng = random.Random(SEED)
    rng.shuffle(ids)
    print(f"frame: {len(ids)} PMCIDs", file=sys.stderr)

    pub = {s[0]: [] for s in STRATA}
    ms = []
    journals_pub, journals_ms = set(), set()
    excluded = []
    log = []
    pos = 0
    pool = ThreadPoolExecutor(6)

    def full():
        return all(len(v) >= PER_STRATUM for v in pub.values()) and len(ms) >= N_MS

    while not full() and pos < MAX_META:
        batch = ids[pos:pos + 120]
        keys = [f"{p}.{latest[p]}" for p in batch]
        metas = list(pool.map(meta, keys))
        for pmcid, key, m in zip(batch, keys, metas):
            pos += 1
            if "error" in m or not m.get("pdf_url") or m.get("is_historical_ocr") or m.get("is_retracted"):
                continue
            cit = m.get("citation") or ""
            if "Preprint" in cit:
                continue
            y, j = year_of(cit), journal_of(cit)
            if y is None:
                continue
            if m.get("is_manuscript"):
                if len(ms) >= N_MS or j in journals_ms:
                    continue
                group = "ms"
            else:
                if not m.get("is_pmc_openaccess") or m.get("license_code") not in COMM:
                    continue
                st = next(s[0] for s in STRATA if s[1] <= y <= s[2])
                if len(pub[st]) >= PER_STRATUM or j in journals_pub:
                    continue
                group = st
            try:
                d = download(pmcid, key)
            except Exception as e:
                excluded.append({"pmcid": pmcid, "group": group, "reason": f"download: {e}"})
                continue
            ok, why, info = check_xml(d / "article.xml")
            if group == "ms" and ok:
                root = etree.parse(str(d / "article.xml"), etree.XMLParser(recover=True, huge_tree=True)).getroot()
                if not [f for f in root.iter("fig") if f.find(".//graphic") is not None]:
                    ok, why = False, "manuscript without figures"
            if not ok:
                excluded.append({"pmcid": pmcid, "group": group, "reason": why})
                continue
            try:
                fetch_to(f"{BUCKET}/{key}/{key}.pdf", d / "article.pdf", timeout=180)
            except Exception as e:
                excluded.append({"pmcid": pmcid, "group": group, "reason": f"pdf download: {e}"})
                continue
            rec = {"pmcid": pmcid, "version": latest[pmcid], "group": "ms" if group == "ms" else "pub",
                   "stratum": group, "year": y, "journal": j, "doi": m.get("doi") or "",
                   "license": m.get("license_code") or "", "citation": cit, "draw_index": pos, **info}
            save_json(d / "meta.json", rec)
            (ms if group == "ms" else pub[group]).append(rec)
            (journals_ms if group == "ms" else journals_pub).add(j)
            log.append(rec)
        print(f"  read {pos} metadata; " + ", ".join(f"{k}:{len(v)}" for k, v in pub.items()) + f", ms:{len(ms)}",
              file=sys.stderr)

    rows = [r for v in pub.values() for r in v] + ms
    fields = ["pmcid", "version", "group", "stratum", "year", "journal", "journal_title", "publisher", "doi",
              "license", "article_type", "body_words", "draw_index", "citation"]
    with open(RESULTS / "corpus.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fields, extrasaction="ignore")
        w.writeheader()
        w.writerows(rows)
    with open(RESULTS / "corpus_excluded.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, ["pmcid", "group", "reason"])
        w.writeheader()
        w.writerows(excluded)
    save_json(RESULTS / "sampling.json", {"seed": SEED, "frame_size": len(ids), "metadata_read": pos,
                                          "selected": len(rows), "excluded": len(excluded)})
    print(f"selected {len(rows)}; excluded {len(excluded)}; metadata read {pos}", file=sys.stderr)


if __name__ == "__main__":
    main()
