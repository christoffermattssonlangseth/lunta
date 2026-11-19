"""Step 6b: the figure images a browser would have loaded with each saved article page.

For every page saved by s06, the <img> elements inside figures (a <figure>, or an element whose
class or id names a figure) are collected with their src / data-src / srcset addresses; each image
is fetched once (browser User-Agent, one at a time, DELAY seconds apart, at most MAX per page) into
data/pages/<PMCID>/images/, with urls.tsv mapping the file to the address(es), as `--images` of the
converters expects. Icons, logos and SVGs are skipped. A blocked image is recorded and skipped.
"""
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from lxml import html as LH

from s06_pages import HEADERS, PAGES

DELAY = 2.5
MAX = 20
SKIP = re.compile(r"logo|icon|badge|orcid|crossmark|altmetric|avatar|banner|sprite|\.svg|\.gif|"
                  r"data:|pixel|spacer|thumbnail_placeholder", re.I)
FIGCLASS = re.compile(r"fig", re.I)


def figure_imgs(doc, base):
    seen, out = set(), []
    for img in doc.iter("img"):
        anc = list(img.iterancestors())
        if not any(a.tag == "figure" or FIGCLASS.search((a.get("class") or "") + " " + (a.get("id") or ""))
                   for a in anc[:6]):
            continue
        cands = []
        for attr in ("data-src", "data-original", "data-large", "src", "data-lg-src"):
            v = img.get(attr)
            if v:
                cands.append(v)
        for attr in ("srcset", "data-srcset"):
            v = img.get(attr)
            if v:
                cands += [x.strip().split(" ")[0] for x in v.split(",") if x.strip()]
        pic = img.getparent()
        if pic is not None and pic.tag == "picture":
            for s in pic.iter("source"):
                v = s.get("srcset") or ""
                cands += [x.strip().split(" ")[0] for x in v.split(",") if x.strip()]
        cands = [urllib.parse.urljoin(base, c.replace("&amp;", "&")) for c in cands if not SKIP.search(c)]
        cands = [c for c in cands if c.startswith("http")]
        if not cands:
            continue
        key = cands[0]
        if key in seen:
            continue
        seen.add(key)
        out.append(cands)
    return out


def main():
    for d in sorted(PAGES.glob("PMC*")):
        f = d / "fetch.json"
        if not (d / "page.html").exists() or (d / "images.json").exists():
            continue
        rec = json.loads(f.read_text())
        base = rec["final_url"]
        doc = LH.fromstring((d / "page.html").read_bytes())
        figs = figure_imgs(doc, base)[:MAX]
        imgdir = d / "images"
        imgdir.mkdir(exist_ok=True)
        tsv, got, failed = [], 0, []
        for k, cands in enumerate(figs):
            # the browser shows one size; take the first address that loads
            for u in cands[:2]:
                try:
                    req = urllib.request.Request(u, headers=dict(HEADERS, Accept="image/avif,image/webp,image/*,*/*;q=0.8",
                                                                  Referer=base))
                    with urllib.request.urlopen(req, timeout=60) as r:
                        data, ctype = r.read(), r.headers.get("Content-Type", "")
                    time.sleep(DELAY)
                except Exception as e:
                    failed.append(f"{u}: {e}")
                    time.sleep(DELAY)
                    continue
                if not ctype.startswith("image/") or len(data) < 2000:
                    failed.append(f"{u}: {ctype} {len(data)} bytes")
                    continue
                ext = {"image/jpeg": ".jpg", "image/png": ".png", "image/webp": ".webp",
                       "image/gif": ".gif", "image/avif": ".avif"}.get(ctype.split(";")[0], ".img")
                name = re.sub(r"[^\w.-]", "_", Path(urllib.parse.urlsplit(u).path).name)[-60:] or f"fig{k}"
                if not name.lower().endswith(ext) and "." not in name:
                    name += ext
                if (imgdir / name).exists():
                    name = f"{k}-{name}"
                (imgdir / name).write_bytes(data)
                tsv.append(name + "\t" + "\t".join(cands))
                got += 1
                break
        (imgdir / "urls.tsv").write_text("\n".join(tsv) + "\n")
        (d / "images.json").write_text(json.dumps({"figures_seen": len(figs), "fetched": got, "failed": failed[:20]}, indent=1))
        print(f"{d.name}: {len(figs)} figure images in the page, {got} fetched, {len(failed)} failed", file=sys.stderr)


if __name__ == "__main__":
    main()
