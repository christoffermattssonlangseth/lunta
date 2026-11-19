"""Step 6: journal article pages ("Use this page's full text") for a subset of the corpus.

Picks about 50 open-access papers of the main corpus whose publisher has a page reader in Lunta
(Springer Nature/BMC, Wiley, Silverchair/OUP, Cell Press/ScienceDirect) or is read by the general
reader (PLOS, Frontiers, MDPI, eLife, PeerJ), fetches each article page once as a browser would
(plain GET via https://doi.org/<DOI>, browser User-Agent, cookies kept, no JavaScript, no login),
one at a time, DELAY seconds apart, and saves it to data/pages/<PMCID>/page.html with
fetch.json (final URL, status, blocked or not). robots.txt is checked for the final URL
(user agent "*"); a disallowed path, an HTTP error or a bot check (Cloudflare, captcha) is recorded
as blocked and not retried. Nothing is done to get around a block.

A first draw (QUOTA) found the article pages of BMC and Springer (link.springer.com), Wiley and
Hindawi, MDPI, OUP, Elsevier and PeerJ behind bot checks; a second draw (--extra) therefore tries
every other main-corpus paper with a DOI whose publisher is not on those platforms.

Usage: s06_pages.py [--select-only] [--extra]
"""
import argparse
import csv
import http.cookiejar
import json
import random
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import urllib.robotparser
from pathlib import Path

from common import DATA, RESULTS, SEED, save_json

PAGES = DATA / "pages"
DELAY = 6
BROWSER_UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 "
              "(KHTML, like Gecko) Version/18.1 Safari/605.1.15")
HEADERS = {"User-Agent": BROWSER_UA,
           "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
           "Accept-Language": "en-US,en;q=0.9"}
BLOCK = re.compile(r"cf-chl|challenge-platform|just a moment\.\.\.|captcha|are you a robot|"
                   r"access denied|unusual traffic|enable javascript and cookies|<title>client challenge", re.I)

# publisher group -> how many to draw
QUOTA = [("BMC", 6), ("Nature", 6), ("Springer", 3), ("Wiley", 6), ("PLOS", 6), ("Frontiers", 6),
         ("MDPI", 6), ("OUP", 3), ("Elsevier", 4), ("eLife", 1), ("PeerJ", 1)]


def group_of(r):
    pub, doi = r["publisher"], r["doi"].lower()
    if pub == "BMC" or doi.startswith("10.1186/") and pub == "BMC":
        return "BMC"
    if doi.startswith("10.1038/"):
        return "Nature"
    if doi.startswith("10.1007/") and pub in ("Springer", "Springer New York", ""):
        return "Springer"
    if pub in ("Wiley", "John Wiley & Sons") and (doi.startswith("10.1002/") or doi.startswith("10.1111/")
                                                   or doi.startswith("10.1155/")):
        return "Wiley"
    if pub == "PLOS":
        return "PLOS"
    if pub.startswith("Frontiers"):
        return "Frontiers"
    if "MDPI" in pub:
        return "MDPI"
    if doi.startswith("10.1093/"):
        return "OUP"
    if doi.startswith("10.1016/"):
        return "Elsevier"
    if doi.startswith("10.7554/"):
        return "eLife"
    if doi.startswith("10.7717/"):
        return "PeerJ"
    return None


def select():
    rows = [r for r in csv.DictReader(open(RESULTS / "corpus.csv")) if r["group"] == "pub"]
    by = {}
    for r in rows:
        g = group_of(r)
        if g:
            by.setdefault(g, []).append(r)
    rnd = random.Random(SEED)
    out = []
    for g, n in QUOTA:
        cand = sorted(by.get(g, []), key=lambda r: r["pmcid"])
        rnd.shuffle(cand)
        for r in cand[:n]:
            out.append({"pmcid": r["pmcid"], "group": g, "year": r["year"], "journal": r["journal"],
                        "doi": r["doi"]})
    return out


BLOCKED_PREFIXES = ("10.1186/", "10.1007/", "10.1002/", "10.1111/", "10.1155/", "10.3390/", "10.1093/",
                    "10.1016/", "10.1038/", "10.7717/", "10.5402/", "10.1100/", "10.1120/")


def select_extra(first):
    done = {r["pmcid"] for r in first}
    out = []
    for r in csv.DictReader(open(RESULTS / "corpus.csv")):
        if r["group"] != "pub" or r["pmcid"] in done or not r["doi"] or r["doi"].lower().startswith(BLOCKED_PREFIXES):
            continue
        g = group_of(r) or (r["publisher"] or "unnamed")
        out.append({"pmcid": r["pmcid"], "group": g, "year": r["year"], "journal": r["journal"], "doi": r["doi"],
                    "draw": 2})
    return out


def opener():
    cj = http.cookiejar.CookieJar()
    return urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))


ROBOTS = {}


def allowed(url):
    p = urllib.parse.urlsplit(url)
    base = f"{p.scheme}://{p.netloc}"
    if base not in ROBOTS:
        rp = urllib.robotparser.RobotFileParser()
        try:
            req = urllib.request.Request(base + "/robots.txt", headers=HEADERS)
            with urllib.request.urlopen(req, timeout=30) as r:
                rp.parse(r.read().decode("utf-8", "replace").splitlines())
        except urllib.error.HTTPError as e:
            rp.parse([] if e.code in (404, 410) else ["User-agent: *", "Disallow: /"]) if e.code != 403 else rp.parse([])
            ROBOTS[base + "#status"] = e.code
        except Exception as e:
            rp.parse([])
            ROBOTS[base + "#status"] = str(e)
        ROBOTS[base] = rp
        time.sleep(2)
    return ROBOTS[base].can_fetch("*", url)


def get(op, url):
    req = urllib.request.Request(url, headers=HEADERS)
    try:
        with op.open(req, timeout=60) as r:
            return r.status, r.geturl(), r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.geturl() or url, e.read() or b""
    except Exception as e:      # DNS, TLS, timeout
        return None, url, str(e).encode()


def follow_html_redirect(html, base):
    """Elsevier's linkinghub sends the browser on with a hidden form field or a meta refresh."""
    m = re.search(r'name="redirectURL"[^>]*value="([^"]+)"', html) or \
        re.search(r'http-equiv="refresh"[^>]*content="\d+;\s*url=([^"]+)"', html, re.I)
    if m:
        return urllib.parse.urljoin(base, urllib.parse.unquote(m.group(1).replace("&amp;", "&")))
    return None


def fetch_one(r):
    d = PAGES / r["pmcid"]
    d.mkdir(parents=True, exist_ok=True)
    if (d / "fetch.json").exists():
        return json.loads((d / "fetch.json").read_text())
    op = opener()
    rec = dict(r)
    url = "https://doi.org/" + r["doi"]
    status, final, body = get(op, url)
    html = body.decode("utf-8", "replace")
    for _ in range(2):
        nxt = follow_html_redirect(html, final) if status == 200 and len(html) < 60000 else None
        if not nxt:
            break
        time.sleep(DELAY)
        if not allowed(nxt):
            rec.update(final_url=nxt, status=None, blocked="robots.txt")
            save_json(d / "fetch.json", rec)
            return rec
        status, final, body = get(op, nxt)
        html = body.decode("utf-8", "replace")
    rec.update(final_url=final, status=status, bytes=len(body))
    if not allowed(final):
        rec["blocked"] = "robots.txt"
    elif status is None:
        rec["blocked"] = "network error: " + body.decode()[:200]
    elif status != 200:
        rec["blocked"] = f"HTTP {status}"
    elif body[:5] == b"%PDF-":
        rec["blocked"] = "not an HTML page (the DOI leads to a PDF)"
    elif BLOCK.search(html[:20000]) and len(re.findall(r"<p[ >]", html)) < 10:
        rec["blocked"] = "bot check"
    else:
        rec["blocked"] = None
        (d / "page.html").write_bytes(body)
    if rec["blocked"]:
        (d / "blocked.html").write_bytes(body[:200000])
    save_json(d / "fetch.json", rec)
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--select-only", action="store_true")
    ap.add_argument("--extra", action="store_true")
    a = ap.parse_args()
    sel = select()
    if a.extra:
        sel = sel + select_extra(sel)
    save_json(PAGES / "selection.json", sel)
    print(f"{len(sel)} papers selected", file=sys.stderr)
    if a.select_only:
        for r in sel:
            print(r["group"], r["pmcid"], r["year"], r["doi"], r["journal"])
        return
    for i, r in enumerate(sel):
        if not (PAGES / r["pmcid"] / "fetch.json").exists():
            rec = fetch_one(r)
            print(f"{i + 1}/{len(sel)} {r['group']} {r['pmcid']} {rec.get('status')} {rec.get('blocked')} "
                  f"{rec.get('final_url')}", file=sys.stderr)
            time.sleep(DELAY)


if __name__ == "__main__":
    main()
