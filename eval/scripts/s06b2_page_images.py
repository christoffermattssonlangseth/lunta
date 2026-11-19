"""Step 6b, second pass: for pages whose reading view still lacked figure images after s06b, every
<img> inside a <figure> of the page is looked up with the same candidate addresses the converter
uses (src, data-src, data-lazy-src, data-original, data-hi-res-src, data-large-src, srcset,
<picture> sources, enclosing link, hi-res links in the figure); the first image address among them
is fetched once (browser User-Agent, one at a time, DELAY s apart) and listed in urls.tsv with all
of its candidates, so the converter finds it by address. Usage: s06b2_page_images.py PMCID ...
"""
import json
import re
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

from lxml import html as LH

from s06_pages import HEADERS, PAGES

DELAY = 2.5
MAX = 25
HIRES = re.compile(r"(?i)(_lrg|large|full|original|hi-?res)|\.(jpe?g|png|gif|webp|tiff?)(\?|$)")
IMGURL = re.compile(r"(?i)\.(jpe?g|png|gif|webp)(\?|$)|/api/ipx/|image\?|/figure/|images?/")


def srcset(v):
    return [p.strip().split(" ")[0] for p in v.split(",") if p.strip()]


def candidates(img):
    c = [img.get(k) for k in ("src", "data-src", "data-lazy-src", "data-original", "data-hi-res-src", "data-large-src")]
    c = [x for x in c if x and not x.startswith("data:")]
    for k in ("srcset", "data-srcset"):
        if img.get(k):
            c += list(reversed(srcset(img.get(k))))
    p = img.getparent()
    if p is not None and p.tag == "picture":
        for s in p.iter("source"):
            if s.get("srcset"):
                c += srcset(s.get("srcset"))
    a = next((x for x in img.iterancestors() if x.tag == "a" and x.get("href")), None)
    if a is not None:
        c.append(a.get("href"))
    fig = next((x for x in img.iterancestors() if x.tag == "figure"), None)
    if fig is not None:
        c = [x.get("href") for x in fig.iter("a") if x.get("href") and HIRES.search(x.get("href"))] + c
    return c


def main():
    for pm in sys.argv[1:]:
        d = PAGES / pm
        base = json.loads((d / "fetch.json").read_text())["final_url"]
        doc = LH.fromstring((d / "page.html").read_bytes())
        imgdir = d / "images"
        imgdir.mkdir(exist_ok=True)
        tsv = (imgdir / "urls.tsv").read_text().splitlines() if (imgdir / "urls.tsv").exists() else []
        have = {u for line in tsv for u in line.split("\t")[1:]}
        got = tried = 0
        for k, img in enumerate(x for x in doc.iter("img") if any(a.tag == "figure" or re.search(r"\bfig", a.get("class") or "") for a in x.iterancestors())):
            cands = [urllib.parse.urljoin(base, c.strip()) for c in candidates(img)]
            cands = [c for c in cands if c.startswith("http")]
            if not cands or have & set(cands) or tried >= MAX:
                continue
            u = next((c for c in cands if IMGURL.search(c)), cands[0])
            tried += 1
            try:
                req = urllib.request.Request(u, headers=dict(HEADERS, Accept="image/avif,image/webp,image/*,*/*;q=0.8", Referer=base))
                with urllib.request.urlopen(req, timeout=60) as r:
                    data, ctype = r.read(), r.headers.get("Content-Type", "")
            except Exception as e:
                print(f"  {u}: {e}", file=sys.stderr)
                time.sleep(DELAY)
                continue
            time.sleep(DELAY)
            if not ctype.startswith("image/") or len(data) < 2000:
                continue
            name = f"v2-{k}-" + re.sub(r"[^\w.-]", "_", Path(urllib.parse.urlsplit(u).path).name)[-50:]
            (imgdir / name).write_bytes(data)
            tsv.append(name + "\t" + "\t".join(dict.fromkeys(cands)))
            have |= set(cands)
            got += 1
        (imgdir / "urls.tsv").write_text("\n".join(tsv) + "\n")
        info = json.loads((d / "images.json").read_text()) if (d / "images.json").exists() else {}
        info["second_pass_fetched"] = got
        (d / "images.json").write_text(json.dumps(info, indent=1))
        print(f"{pm}: second pass fetched {got} of {tried}", file=sys.stderr)


if __name__ == "__main__":
    main()
