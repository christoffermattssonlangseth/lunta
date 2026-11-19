"""Shared helpers for the Lunta conversion evaluation (paths, polite HTTP)."""
import json
import os
import time
import urllib.error
import urllib.request
from pathlib import Path

EVAL = Path(__file__).resolve().parent.parent
DATA = EVAL / "data"
CORPUS = DATA / "corpus"          # PMCID/{article.xml, article.pdf, meta.json}
OUT = DATA / "out"                # converter outputs: OUT/<system>/<PMCID>/...
RESULTS = EVAL / "results"
FIGS = EVAL / "figures"
BUCKET = "https://pmc-oa-opendata.s3.amazonaws.com"
# generic, descriptive, no personal information
UA = "lunta-converter-eval/0.1 (reproducible research evaluation; +https://registry.opendata.aws/ncbi-pmc)"
SEED = 20261116
INVENTORY_DATE = "2026-11-15T01-00Z"

for d in (DATA, CORPUS, OUT, RESULTS, FIGS):
    d.mkdir(parents=True, exist_ok=True)


def fetch(url, tries=4, timeout=60):
    """GET with retries and exponential back-off. Returns bytes or raises."""
    last = None
    for i in range(tries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code == 404:
                raise
            last = e
        except Exception as e:  # network hiccup
            last = e
        time.sleep(1.5 * 2 ** i)
    raise last


def fetch_to(url, path, **kw):
    path = Path(path)
    if path.exists() and path.stat().st_size > 0:
        return path
    data = fetch(url, **kw)
    tmp = path.with_suffix(path.suffix + ".part")
    tmp.write_bytes(data)
    os.replace(tmp, path)
    return path


def load_json(p, default=None):
    p = Path(p)
    if not p.exists():
        return default
    return json.loads(p.read_text())


def save_json(p, obj):
    Path(p).write_text(json.dumps(obj, indent=1, ensure_ascii=False))
