"""Parsing of the ground truth (JATS XML) and of converter output (EPUB), and text normalisation."""
import re
import unicodedata
from pathlib import Path

from lxml import etree

# ------------------------------------------------------------------ normalisation

_DASH = dict.fromkeys(map(ord, "‐‑‒–—―−­"), "-")
_QUOTE = {ord("‘"): "'", ord("’"): "'", ord("“"): '"', ord("”"): '"', ord("´"): "'",
          ord("µ"): "μ"}


def norm(s):
    """Unicode NFKC (ligatures, full-width forms), unified dashes and quotes, lower case."""
    s = unicodedata.normalize("NFKC", s).translate(_DASH).translate(_QUOTE)
    s = s.replace("­", "")
    return s.lower()


WORD = re.compile(r"\w+", re.UNICODE)
RAWTOK = re.compile(r"\w+(?:-\w+)*", re.UNICODE)


def tokens(s):
    """Word tokens for text fidelity: case, punctuation and hyphenation are ignored."""
    return WORD.findall(norm(s))


def raw_tokens(s):
    """Tokens that keep internal hyphens (for the broken-word metrics)."""
    return RAWTOK.findall(norm(s))


def dense(s):
    """Letters and digits only, digits masked (for boilerplate matching)."""
    return re.sub(r"\d", "0", "".join(WORD.findall(norm(s))).replace("_", ""))


# ------------------------------------------------------------------ JATS ground truth

PARSER = etree.XMLParser(recover=True, huge_tree=True, resolve_entities=False, no_network=True)


def local(tag):
    return tag.split("}")[-1] if isinstance(tag, str) else ""


JATS_SKIP = {"fig", "fig-group", "table-wrap", "table-wrap-group", "disp-formula", "disp-formula-group",
             "supplementary-material", "graphic", "media", "alternatives", "tex-math", "fn", "ref-list",
             "object-id", "processing-meta"}
JATS_SKIP_INLINE = {"tex-math", "annotation"}   # inside inline-formula: keep MathML text, drop TeX source
JATS_NESTED_BLOCK = {"p", "list", "def-list", "sec", "boxed-text", "disp-quote", "statement", "title"}


def jats_text(el, skip_nested=True):
    out = []

    def rec(e, top):
        t = local(e.tag)
        if not top and (t in JATS_SKIP or t in JATS_SKIP_INLINE or (skip_nested and t in JATS_NESTED_BLOCK)):
            if e.tail:
                out.append(e.tail)
            return
        if e.text:
            out.append(e.text)
        for c in e:
            rec(c, False)
        if not top and e.tail:
            out.append(e.tail)

    rec(el, True)
    return " ".join("".join(out).split())


def jats_blocks(el, part, out, sec_path=()):
    """Paragraph-level blocks in document order: (part, kind, text)."""
    for c in el:
        t = local(c.tag)
        if t in JATS_SKIP:
            continue
        if t == "title":
            lab = c.getparent().find("label")
            txt = jats_text(c)
            if lab is not None and local(c.getparent().tag) in ("sec", "app"):
                txt = f"{jats_text(lab)} {txt}".strip()
            if txt:
                out.append((part, "title", txt))
        elif t == "p":
            txt = jats_text(c)
            if txt:
                out.append((part, "p", txt))
            jats_blocks(c, part, out)       # lists, quotes etc. nested in the paragraph
        elif t == "list-item" and c.find("p") is None:
            txt = jats_text(c)
            if txt:
                out.append((part, "p", txt))
        elif t == "label":
            continue
        else:
            jats_blocks(c, part, out)


def load_jats(path):
    root = etree.parse(str(path), PARSER).getroot()
    blocks = []
    meta = root.find(".//front/article-meta")
    if meta is not None:
        for ab in meta.findall("abstract"):
            if ab.get("abstract-type") in ("graphical", "teaser", "toc", "editor-summary", "author-highlights"):
                continue
            jats_blocks(ab, "abstract", blocks)
    body = root.find("body")
    if body is not None:
        jats_blocks(body, "body", blocks)
    back = root.find("back")
    if back is not None:
        for c in back:
            if local(c.tag) in ("ref-list",):
                continue
            tmp = []
            if local(c.tag) == "fn-group":
                for fn in c.iter("fn"):
                    txt = jats_text(fn, skip_nested=False)
                    if txt:
                        tmp.append(("back", "p", txt))
            else:
                jats_blocks(c, "back", tmp)
            blocks.extend(tmp)
    figs = []
    for f in root.iter("fig"):
        if any(local(a.tag) == "supplementary-material" for a in f.iterancestors()):
            continue
        if f.find(".//graphic") is None:
            continue
        cap = f.find("caption")
        figs.append({"label": jats_text(f.find("label")) if f.find("label") is not None else "",
                     "caption": jats_text(cap, skip_nested=False) if cap is not None else ""})
    tables = []
    for tw in root.iter("table-wrap"):
        cap = tw.find("caption")
        tables.append({"label": jats_text(tw.find("label")) if tw.find("label") is not None else "",
                       "caption": jats_text(cap, skip_nested=False) if cap is not None else ""})
    n_eq = sum(1 for _ in root.iter("disp-formula"))
    refs = root.find(".//back/ref-list")
    n_refs = len(root.findall(".//back//ref"))
    alltext = " ".join(" ".join(root.itertext()).split())
    return {"blocks": blocks, "figs": figs, "tables": tables, "n_eq": n_eq, "n_refs": n_refs,
            "alltext": alltext}


# ------------------------------------------------------------------ EPUB (converter output)

XH = "{http://www.w3.org/1999/xhtml}"
REF_HEAD = re.compile(r"^\s*(\d+\.?\s*)?(references?( and notes)?|bibliography|literature cited|works cited|"
                      r"reference list|citations?)\s*$", re.I)


def spine(epub_dir):
    epub_dir = Path(epub_dir)
    opf = next(epub_dir.glob("**/content.opf"))
    o = etree.parse(str(opf), PARSER).getroot()
    ns = {"o": "http://www.idpf.org/2007/opf"}
    items = {i.get("id"): i.get("href") for i in o.findall(".//o:manifest/o:item", ns)}
    return [opf.parent / items[r.get("idref")] for r in o.findall(".//o:spine/o:itemref", ns)]


def classes(e):
    return set((e.get("class") or "").split())


def _text(e, skip):
    out = []

    def rec(x, top):
        if not top and skip(x):
            if x.tail:
                out.append(x.tail)
            return
        if x.text:
            out.append(x.text)
        for c in x:
            rec(c, False)
        if not top and x.tail:
            out.append(x.tail)

    rec(e, True)
    return " ".join("".join(out).split())


def load_epub(epub_dir):
    """Body blocks (no title page, captions, tables, equations or references), figures, tables,
    equations and reference items of a converted paper."""
    blocks, figs, tables = [], [], []
    n_eq = n_refitems = 0
    ref_text = []
    for f in spine(epub_dir):
        name = f.name
        if name in ("cover.xhtml", "title.xhtml", "nav.xhtml"):
            continue
        root = etree.parse(str(f), PARSER).getroot()
        body = root.find(f"{XH}body")
        if body is None:
            continue
        in_refs = False

        def is_float(x):
            t = local(x.tag)
            c = classes(x)
            return t == "figure" or (t == "div" and ({"table-wrap", "equation"} & c)) or \
                (t == "p" and "caption" in c) or (t == "section" and "references" in c) or \
                (t == "ol" and "refs" in c)

        for x in body.iter():
            t = local(x.tag)
            c = classes(x)
            if t == "figure":
                img = x.find(f".//{XH}img")
                cap = x.find(f"{XH}figcaption")
                if img is not None and "equation" not in classes(img):
                    figs.append({"caption": _text(cap, lambda y: False) if cap is not None else "", "has_img": True})
                elif cap is not None:
                    figs.append({"caption": _text(cap, lambda y: False), "has_img": False})
                continue
            if t == "div" and "table-wrap" in c:
                cap = x.find(f"{XH}p[@class='caption']")
                if cap is None:     # reading views made from article pages
                    cap = x.find(f"{XH}p[@class='table-caption']")
                tables.append({"caption": _text(cap, lambda y: False) if cap is not None else "",
                               "has_img": x.find(f".//{XH}img") is not None,
                               "has_table": x.find(f".//{XH}table") is not None})
                continue
            if t == "div" and "equation" in c:
                n_eq += 1
                continue
            if t == "p" and "caption" in c and "table-wrap" not in classes(x.getparent()):
                # a caption whose figure / table image was not found
                cap = _text(x, lambda y: False)
                (tables if re.match(r"\s*table", cap, re.I) else figs).append(
                    {"caption": cap, "has_img": False, "has_table": False})
        # blocks in order, skipping floats; a references heading switches to reference mode
        def walk(e):
            nonlocal in_refs, n_refitems
            for ch in e:
                t = local(ch.tag)
                if not isinstance(ch.tag, str):
                    continue
                if t in ("h1", "h2", "h3", "h4", "h5", "h6"):
                    txt = _text(ch, lambda y: False)
                    lvl = int(t[1])
                    if REF_HEAD.match(txt):
                        in_refs = lvl
                    elif in_refs and lvl <= in_refs:
                        in_refs = False
                    if not in_refs:
                        blocks.append(("title", txt))
                    continue
                if is_float(ch):
                    if t == "section" and "references" in classes(ch):
                        n_refitems += len(ch.findall(f".//{XH}li"))
                    if t == "ol" and "refs" in classes(ch):
                        n_refitems += len(ch.findall(f"{XH}li"))
                    continue
                if t == "p" or (t == "li" and ch.find(f"{XH}p") is None):
                    txt = _text(ch, is_float)
                    if in_refs:
                        n_refitems += 1
                        ref_text.append(txt)
                    elif txt:
                        blocks.append(("p", txt))
                    continue
                walk(ch)

        walk(body)
    return {"blocks": blocks, "figs": figs, "tables": tables, "n_eq": n_eq, "n_refitems": n_refitems}
