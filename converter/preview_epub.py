"""Render EPUB spine documents as phone-sized screenshots for visual QA.
usage: python3 epubshot.py book.epub outdir [max_docs] [height]"""
import sys, zipfile, tempfile, re
from pathlib import Path
from playwright.sync_api import sync_playwright

epub, outdir = Path(sys.argv[1]), Path(sys.argv[2])
maxdocs = int(sys.argv[3]) if len(sys.argv) > 3 else 99
height = int(sys.argv[4]) if len(sys.argv) > 4 else 2400
outdir.mkdir(parents=True, exist_ok=True)
tmp = Path(tempfile.mkdtemp())
zipfile.ZipFile(epub).extractall(tmp)
opf = next(tmp.rglob("*.opf"))
o = opf.read_text()
items = dict(re.findall(r'<item id="([^"]+)" href="([^"]+)"', o))
spine = re.findall(r'<itemref idref="([^"]+)"', o)
with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page(viewport={"width": 400, "height": 700}, device_scale_factor=1.5)
    for i, idref in enumerate(spine[:maxdocs]):
        f = opf.parent / items[idref]
        pg.goto(f.as_uri())
        pg.wait_for_timeout(150)
        h = min(height, pg.evaluate("document.body.scrollHeight"))
        pg.set_viewport_size({"width": 400, "height": max(300, h)})
        pg.screenshot(path=str(outdir / f"{i:02d}-{Path(f).stem}.png"))
        pg.set_viewport_size({"width": 400, "height": 700})
    b.close()
print("\n".join(sorted(x.name for x in outdir.iterdir())))
