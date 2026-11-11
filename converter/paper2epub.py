#!/usr/bin/env python3
"""
paper2epub — turn a scientific paper into a clean, reflowable EPUB.

Give it a DOI, an arXiv ID, a PMCID, a URL or a local file (PDF, JATS/NXML,
LaTeX source folder/archive). It looks for the best available source, converts
it, and builds an EPUB3 with MathML, figures, tables, references and a cover.

Source cascade (best first, falls back automatically):
  1. JATS XML full text (Europe PMC / PMC open access, eLife, local .xml/.nxml)
  2. LaTeX source (arXiv e-print or local source)
  3. PDF  -> Claude vision transcription per page (if ANTHROPIC_API_KEY is set)
          -> otherwise heuristic layout extraction with PyMuPDF

Requirements: python>=3.9, pandoc>=3.0, pymupdf, lxml, pillow.
Optional: anthropic (LLM repair), pdflatex (renders TikZ/algorithms), java+epubcheck.
"""
from __future__ import annotations

import argparse
import base64
import collections
import concurrent.futures as cf
import copy
import gzip
import html
import io
import json
import logging
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import textwrap
import urllib.parse
import urllib.request
import zipfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Iterator, Optional

log = logging.getLogger("paper2epub")
HERE = Path(__file__).resolve().parent
DEFAULT_MODEL = os.environ.get("PAPER2EPUB_MODEL", "claude-sonnet-5-5")
MATHML_NS = "http://www.w3.org/1998/Math/MathML"
XLINK = "{http://www.w3.org/1999/xlink}href"


class ConversionError(Exception):
    pass


# --------------------------------------------------------------------------- #
# Data model
# --------------------------------------------------------------------------- #
@dataclass
class Meta:
    title: str = ""
    authors: list = field(default_factory=list)
    date: str = ""
    doi: str = ""
    journal: str = ""
    abstract: str = ""
    keywords: list = field(default_factory=list)
    license: str = ""
    url: str = ""

    def merge(self, other: "Meta") -> "Meta":
        """Fill empty fields from other."""
        for k, v in vars(other).items():
            if v and not getattr(self, k):
                setattr(self, k, v)
        return self


@dataclass
class Source:
    kind: str                      # 'jats' | 'latex' | 'pdf'
    path: Path                     # main file
    meta: Meta = field(default_factory=Meta)
    label: str = ""
    extra: dict = field(default_factory=dict)  # e.g. pmcid, asset dirs, remote figure patterns


# --------------------------------------------------------------------------- #
# Small helpers
# --------------------------------------------------------------------------- #
def http_get(url: str, timeout: int = 60, accept: str | None = None) -> bytes:
    email = os.environ.get("PAPER2EPUB_EMAIL", "")
    ua = "paper2epub/0.1 (+https://github.com/; mailto:%s)" % email if email else "paper2epub/0.1"
    req = urllib.request.Request(url, headers={"User-Agent": ua, **({"Accept": accept} if accept else {})})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def http_json(url: str, timeout: int = 30) -> dict:
    return json.loads(http_get(url, timeout, accept="application/json"))


def run(cmd: list, cwd=None, timeout=300, check=True) -> subprocess.CompletedProcess:
    log.debug("run: %s", " ".join(map(str, cmd)))
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    if check and p.returncode != 0:
        raise ConversionError(f"{cmd[0]} failed: {p.stderr.strip()[:2000]}")
    return p


def slug(s: str, n: int = 60) -> str:
    s = re.sub(r"[^\w\s-]", "", s).strip().lower()
    return re.sub(r"[\s_-]+", "-", s)[:n].strip("-") or "paper"


def sniff(data: bytes) -> str:
    if data[:5] == b"%PDF-":
        return "pdf"
    if data[:2] == b"\x1f\x8b":
        return "gzip"
    if data[:4] == b"PK\x03\x04":
        return "zip"
    if len(data) > 262 and data[257:262] == b"ustar":
        return "tar"
    head = data[:2000].lower()
    if b"<article" in head or b"jats" in head or b"<!doctype article" in head:
        return "jats"
    if b"<html" in head or b"<!doctype html" in head:
        return "html"
    if b"\\documentclass" in data[:200000] or b"\\begin{document}" in data[:500000]:
        return "latex"
    return "unknown"


def extract_archive(data: bytes, dest: Path) -> None:
    dest.mkdir(parents=True, exist_ok=True)
    kind = sniff(data)
    if kind == "zip":
        with zipfile.ZipFile(io.BytesIO(data)) as z:
            for m in z.infolist():
                target = (dest / m.filename).resolve()
                if dest.resolve() in target.parents or target == dest.resolve():
                    z.extract(m, dest)
        return
    if kind == "gzip":
        raw = gzip.decompress(data)
        if len(raw) > 262 and raw[257:262] == b"ustar":
            data, kind = raw, "tar"
        else:  # single gzipped file (arXiv single .tex)
            (dest / "main.tex").write_bytes(raw)
            return
    if kind == "tar":
        with tarfile.open(fileobj=io.BytesIO(data)) as t:
            members = [m for m in t.getmembers()
                       if not (m.name.startswith("/") or ".." in Path(m.name).parts or m.issym() or m.islnk())]
            t.extractall(dest, members=members)
        return
    raise ConversionError(f"Unknown archive type: {kind}")


# --------------------------------------------------------------------------- #
# Source resolution (the "where do I get the best version of this paper" step)
# --------------------------------------------------------------------------- #
ARXIV_RE = re.compile(r"^(?:arxiv:)?(\d{4}\.\d{4,5}(?:v\d+)?|[a-z\-]+(?:\.[A-Z]{2})?/\d{7}(?:v\d+)?)$", re.I)
DOI_RE = re.compile(r"(10\.\d{4,9}/[^\s\"<>]+)", re.I)
PMCID_RE = re.compile(r"^PMC\d+$", re.I)


def candidates(inp: str, work: Path) -> Iterator[tuple[str, Callable[[], Source]]]:
    """Yield (label, thunk) pairs, best source first. Thunks may raise."""
    p = Path(inp).expanduser()
    if p.exists():
        yield from local_candidates(p, work)
        return

    s = inp.strip()
    m = re.search(r"arxiv\.org/(?:abs|pdf|html)/([^\s?#]+?)(?:\.pdf)?$", s)
    if m:
        s = m.group(1)
    if s.lower().startswith("10.48550/arxiv."):
        s = s.split("arxiv.", 1)[1] if "arxiv." in s.lower() else s
        s = re.sub(r"(?i)^10\.48550/arxiv\.", "", s)
    if ARXIV_RE.match(s):
        aid = ARXIV_RE.match(s).group(1)
        yield (f"arXiv LaTeX source {aid}", lambda: arxiv_source(aid, work))
        yield (f"arXiv PDF {aid}", lambda: arxiv_pdf(aid, work))
        return

    if PMCID_RE.match(s):
        yield (f"Europe PMC JATS {s}", lambda: europepmc_jats(s.upper(), work))
        return

    m = DOI_RE.search(urllib.parse.unquote(s))
    if m and ("doi.org" in s or s.startswith("10.") or s.lower().startswith("doi:")):
        doi = m.group(1).rstrip(".")
        yield from doi_candidates(doi, work)
        return

    if s.startswith("http"):
        yield (f"URL {s}", lambda: from_url(s, work))
        return
    raise ConversionError(f"Don't know how to interpret input: {inp}")


def local_candidates(p: Path, work: Path):
    if p.is_dir():
        yield (f"LaTeX folder {p}", lambda: latex_from_dir(p, Meta()))
        pdfs = sorted(p.glob("*.pdf"))
        xmls = sorted(list(p.glob("*.nxml")) + list(p.glob("*.xml")))
        if xmls:
            yield (f"JATS {xmls[0].name}", lambda: Source("jats", xmls[0], label="local JATS"))
        if pdfs:
            yield (f"PDF {pdfs[0].name}", lambda: Source("pdf", pdfs[0], label="local PDF"))
        return
    data = p.read_bytes()[:600000]
    kind = sniff(data)
    if p.suffix.lower() in (".md", ".markdown"):
        yield (f"Markdown {p.name}", lambda: Source("markdown", p, label="Markdown (edited)"))
        return
    if kind == "pdf":
        yield (f"PDF {p.name}", lambda: Source("pdf", p, label="local PDF"))
    elif kind == "jats" or p.suffix.lower() in (".nxml", ".xml"):
        yield (f"JATS {p.name}", lambda: Source("jats", p, label="local JATS", extra={"asset_dirs": [p.parent]}))
    elif kind == "latex" or p.suffix.lower() == ".tex":
        yield (f"LaTeX {p.name}", lambda: latex_from_dir(p.parent, Meta(), main=p))
    elif kind in ("zip", "gzip", "tar"):
        def unpack():
            d = work / "src"
            extract_archive(p.read_bytes(), d)
            nx = sorted(list(d.rglob("*.nxml")) + [x for x in d.rglob("*.xml") if sniff(x.read_bytes()[:3000]) == "jats"])
            if nx:
                return Source("jats", nx[0], label="JATS package", extra={"asset_dirs": [nx[0].parent]})
            return latex_from_dir(d, Meta())
        yield (f"archive {p.name}", unpack)
    else:
        raise ConversionError(f"Unrecognised file type: {p}")


def doi_candidates(doi: str, work: Path):
    info: dict = {}

    def epmc_lookup():
        if "r" not in info:
            q = urllib.parse.quote(f'DOI:"{doi}"')
            r = http_json(f"https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={q}&resultType=core&format=json")
            res = r.get("resultList", {}).get("result", [])
            info["r"] = res[0] if res else {}
        return info["r"]

    def via_epmc():
        r = epmc_lookup()
        ident = r.get("pmcid") or (r.get("id") if r.get("source") == "PPR" else None)
        if not ident:
            raise ConversionError("No full text in Europe PMC")
        src = europepmc_jats(ident, work)
        src.meta.merge(epmc_meta(r))
        return src

    yield (f"Europe PMC full text for {doi}", via_epmc)

    if doi.lower().startswith("10.7554/elife."):
        num = re.sub(r"\D", "", doi.split(".")[-1])
        yield (f"eLife JATS {num}", lambda: elife_jats(num, work))

    def via_unpaywall():
        email = os.environ.get("PAPER2EPUB_EMAIL") or os.environ.get("UNPAYWALL_EMAIL")
        if not email:
            raise ConversionError("Set PAPER2EPUB_EMAIL to enable Unpaywall lookups")
        r = http_json(f"https://api.unpaywall.org/v2/{urllib.parse.quote(doi)}?email={urllib.parse.quote(email)}")
        locs = [r.get("best_oa_location") or {}] + (r.get("oa_locations") or [])
        for loc in locs:
            url = loc.get("url_for_pdf")
            if not url:
                continue
            try:
                data = http_get(url, timeout=90)
            except Exception as e:  # publisher may block scripts
                log.info("  could not download %s (%s)", url, e)
                continue
            if sniff(data) == "pdf":
                f = work / "paper.pdf"
                f.write_bytes(data)
                meta = Meta(title=r.get("title") or "", doi=doi, journal=r.get("journal_name") or "",
                            date=str(r.get("published_date") or r.get("year") or ""),
                            authors=[" ".join(x for x in (a.get("given"), a.get("family")) if x)
                                     for a in (r.get("z_authors") or [])])
                return Source("pdf", f, meta=meta, label="Unpaywall PDF")
        raise ConversionError("No downloadable open-access PDF found")

    yield (f"Unpaywall open-access PDF for {doi}", via_unpaywall)


def epmc_meta(r: dict) -> Meta:
    authors = [a.get("fullName", "") for a in (r.get("authorList", {}) or {}).get("author", [])]
    journal = ((r.get("journalInfo") or {}).get("journal") or {}).get("title", "") or r.get("bookOrReportDetails", {}).get("publisher", "")
    return Meta(title=(r.get("title") or "").rstrip("."), authors=[a for a in authors if a],
                date=r.get("firstPublicationDate", ""), doi=r.get("doi", ""), journal=journal,
                abstract=re.sub(r"<[^>]+>", "", r.get("abstractText") or ""))


def europepmc_jats(ident: str, work: Path) -> Source:
    xml = http_get(f"https://www.ebi.ac.uk/europepmc/webservices/rest/{ident}/fullTextXML", timeout=90)
    if sniff(xml) != "jats":
        raise ConversionError("Europe PMC returned no JATS")
    d = work / "src"
    d.mkdir(parents=True, exist_ok=True)
    f = d / f"{ident}.xml"
    f.write_bytes(xml)
    asset_dirs = [d]
    try:  # zip of all article files (figures, supplements)
        z = http_get(f"https://www.ebi.ac.uk/europepmc/webservices/rest/{ident}/supplementaryFiles", timeout=180)
        if sniff(z) == "zip":
            extract_archive(z, d / "files")
            asset_dirs.append(d / "files")
    except Exception as e:
        log.info("  no supplementary-files bundle (%s); will try figure URLs", e)
    patterns = [f"https://europepmc.org/articles/{ident}/bin/{{stem}}.jpg",
                f"https://pmc.ncbi.nlm.nih.gov/articles/{ident}/bin/{{stem}}.jpg"]
    return Source("jats", f, label="Europe PMC JATS", extra={"asset_dirs": asset_dirs, "url_patterns": patterns})


def elife_jats(num: str, work: Path) -> Source:
    num = num.zfill(5)
    for v in (3, 2, 1):
        try:
            xml = http_get(f"https://raw.githubusercontent.com/elifesciences/elife-article-xml/master/articles/elife-{num}-v{v}.xml")
            break
        except Exception:
            continue
    else:
        raise ConversionError("eLife XML not found")
    d = work / "src"
    d.mkdir(parents=True, exist_ok=True)
    f = d / f"elife-{num}.xml"
    f.write_bytes(xml)
    pats = [f"https://iiif.elifesciences.org/lax:{num}%2F{{href}}/full/1500,/0/default.jpg",
            f"https://iiif.elifesciences.org/lax/{num}%2F{{href}}/full/1500,/0/default.jpg",
            f"https://cdn.elifesciences.org/articles/{num}/{{stem}}.jpg"]
    return Source("jats", f, label="eLife JATS", extra={"asset_dirs": [d], "url_patterns": pats})


def arxiv_meta(aid: str) -> Meta:
    try:
        from lxml import etree
        x = etree.fromstring(http_get(f"https://export.arxiv.org/api/query?id_list={aid}"))
        ns = {"a": "http://www.w3.org/2005/Atom", "ar": "http://arxiv.org/schemas/atom"}
        e = x.find("a:entry", ns)
        return Meta(title=" ".join((e.findtext("a:title", "", ns) or "").split()),
                    authors=[a.findtext("a:name", "", ns) for a in e.findall("a:author", ns)],
                    date=(e.findtext("a:published", "", ns) or "")[:10],
                    abstract=" ".join((e.findtext("a:summary", "", ns) or "").split()),
                    doi=e.findtext("ar:doi", "", ns) or f"10.48550/arXiv.{aid}",
                    journal="arXiv", url=f"https://arxiv.org/abs/{aid}")
    except Exception as e:
        log.info("  arXiv metadata lookup failed: %s", e)
        return Meta(journal="arXiv", url=f"https://arxiv.org/abs/{aid}")


def arxiv_source(aid: str, work: Path) -> Source:
    data = http_get(f"https://arxiv.org/e-print/{aid}", timeout=120)
    if sniff(data) == "pdf":
        raise ConversionError("arXiv has no LaTeX source for this paper")
    d = work / "src"
    extract_archive(data, d)
    return latex_from_dir(d, arxiv_meta(aid))


def arxiv_pdf(aid: str, work: Path) -> Source:
    data = http_get(f"https://arxiv.org/pdf/{aid}", timeout=120)
    f = work / "paper.pdf"
    f.write_bytes(data)
    return Source("pdf", f, meta=arxiv_meta(aid), label="arXiv PDF")


def from_url(url: str, work: Path) -> Source:
    data = http_get(url, timeout=120)
    kind = sniff(data)
    if kind == "pdf":
        f = work / "paper.pdf"
        f.write_bytes(data)
        return Source("pdf", f, meta=Meta(url=url), label="PDF from URL")
    if kind == "jats":
        f = work / "paper.xml"
        f.write_bytes(data)
        return Source("jats", f, meta=Meta(url=url), label="JATS from URL")
    if kind in ("zip", "gzip", "tar"):
        f = work / "download.bin"
        f.write_bytes(data)
        label, thunk = next(local_candidates(f, work))
        return thunk()
    raise ConversionError(f"URL did not return a PDF or JATS XML (got {kind}). Try the DOI instead.")


# --------------------------------------------------------------------------- #
# Pandoc AST utilities
# --------------------------------------------------------------------------- #
def walk(node, fn):
    """Post-order walk; fn(node) may return a replacement node or a list (splice)."""
    if isinstance(node, list):
        out = []
        for x in node:
            r = walk(x, fn)
            if isinstance(r, _Splice):
                out.extend(r)
            else:
                out.append(r)
        return out
    if isinstance(node, dict):
        if "t" in node and "c" in node:
            node["c"] = walk(node["c"], fn)
        elif "t" not in node:
            for k in list(node):
                node[k] = walk(node[k], fn)
        r = fn(node) if "t" in node else None
        return node if r is None else r
    return node


class _Splice(list):
    pass


def stringify(node) -> str:
    out = []

    def f(n):
        if isinstance(n, dict) and "t" in n:
            t = n["t"]
            if t == "Str":
                out.append(n["c"])
            elif t in ("Space", "SoftBreak", "LineBreak"):
                out.append(" ")
            elif t == "Math":
                out.append(n["c"][1])
            elif t == "Code":
                out.append(n["c"][1])
            elif t in ("Para", "Plain", "Header"):
                out.append("\n")
        return None
    walk(copy.deepcopy(node), f)
    return re.sub(r"[ \t]+", " ", "".join(out)).strip()


def text_inlines(s: str) -> list:
    out = []
    for i, w in enumerate(s.split()):
        if i:
            out.append({"t": "Space"})
        out.append({"t": "Str", "c": w})
    return out


def pandoc_to_ast(path: Path, fmt: str, cwd: Path | None = None, extra: list | None = None) -> dict:
    cmd = ["pandoc", str(path), "-f", fmt, "-t", "json"] + (extra or [])
    p = run(cmd, cwd=cwd, timeout=600)
    if p.stderr.strip():
        log.debug("pandoc: %s", p.stderr.strip()[:3000])
    return json.loads(p.stdout)


# --------------------------------------------------------------------------- #
# JATS -> HTML (custom; pandoc's JATS reader drops front matter and mangles figures)
# --------------------------------------------------------------------------- #
class JatsConverter:
    BLOCK = {"fig", "fig-group", "table-wrap", "table-wrap-group", "disp-formula", "disp-formula-group", "list",
             "boxed-text", "disp-quote", "preformat", "code", "def-list", "supplementary-material", "statement",
             "graphic", "media", "array", "speech", "verse-group", "p", "sec", "chem-struct-wrap"}

    def __init__(self, src: Source, work: Path):
        from lxml import etree
        self.etree = etree
        self.src = src
        self.work = work
        parser = etree.XMLParser(recover=True, resolve_entities=False, load_dtd=False, no_network=True, huge_tree=True)
        self.tree = etree.parse(str(src.path), parser)
        self.root = self.tree.getroot()
        self._strip_ns_prefixes()
        self.tex = []                       # pending tex-math snippets
        self.asset_dirs = [Path(p) for p in src.extra.get("asset_dirs", [src.path.parent])]
        self.url_patterns = src.extra.get("url_patterns", [])
        self.img_dir = work / "jats_img"
        self.img_dir.mkdir(parents=True, exist_ok=True)
        self.missing = 0
        self.ref_ids = set()

    def _strip_ns_prefixes(self):
        # Keep MathML namespaced; strip any other namespaces (e.g. JATS 1.2+ with ali:, or default ns)
        for el in self.root.iter():
            if isinstance(el.tag, str) and el.tag.startswith("{") and not el.tag.startswith("{" + MATHML_NS):
                el.tag = el.tag.split("}", 1)[1]

    # ---------- metadata ----------
    def meta(self) -> Meta:
        r = self.root
        am = r.find(".//article-meta")
        m = Meta()
        if am is None:
            return m
        t = am.find(".//title-group/article-title")
        m.title = self.plain(t) if t is not None else ""
        for c in am.findall(".//contrib-group/contrib"):
            if c.get("contrib-type", "author") != "author":
                continue
            n = c.find("name")
            if n is not None:
                given = (n.findtext("given-names") or "").strip()
                sur = (n.findtext("surname") or "").strip()
                m.authors.append(f"{given} {sur}".strip())
            elif c.find("collab") is not None:
                m.authors.append(self.plain(c.find("collab")))
        for aid in am.findall("article-id"):
            if aid.get("pub-id-type") == "doi":
                m.doi = (aid.text or "").strip()
        jt = r.find(".//journal-meta//journal-title")
        m.journal = self.plain(jt) if jt is not None else ""
        dates = am.findall("pub-date")
        pick = None
        for d in dates:
            if d.get("pub-type") in ("epub", "ppub", "pub") or d.get("date-type") in ("pub", "publication"):
                pick = d
                break
        pick = pick if pick is not None else (dates[0] if dates else None)
        if pick is not None:
            y, mo, dd = pick.findtext("year") or "", pick.findtext("month") or "", pick.findtext("day") or ""
            m.date = "-".join(x.zfill(2) if i else x for i, x in enumerate([y, mo, dd]) if x)
        m.keywords = [self.plain(k) for k in am.findall(".//kwd-group/kwd")][:15]
        lic = am.find(".//permissions/license")
        if lic is not None:
            m.license = lic.get(XLINK) or self.plain(lic)[:300]
        ab = [a for a in am.findall("abstract") if not a.get("abstract-type")]
        if ab:
            m.abstract = self.plain(ab[0])
        return m

    def plain(self, el) -> str:
        return " ".join("".join(el.itertext()).split()) if el is not None else ""

    # ---------- body ----------
    def to_html(self) -> str:
        r = self.root
        out = ['<html><head><meta charset="utf-8"/><title>paper</title></head><body>']
        am = r.find(".//article-meta")
        if am is not None:
            affs = am.findall(".//aff")
            if affs:
                out.append('<div class="frontmatter"><p class="affiliations">')
                out.append("<br/>".join(self.inline(a, skip={"label"}) for a in affs))
                out.append("</p></div>")
            for ab in am.findall("abstract"):
                typ = ab.get("abstract-type") or ""
                if typ in ("toc", "graphical", "teaser", "web-summary", "short"):
                    continue
                title = ab.findtext("title") or ("Abstract" if not typ else typ.replace("-", " ").capitalize())
                if typ == "executive-summary" and not ab.findtext("title"):
                    title = "Summary"
                out.append(f'<section class="abstract"><h1>{html.escape(title.strip())}</h1>')
                out.append(self.blocks(ab, level=2, skip={"title"}))
                out.append("</section>")
        body = r.find("body")
        if body is not None:
            out.append(self.blocks(body, level=1))
        back = r.find("back")
        if back is not None:
            out.append(self.back(back))
        # sub-articles (decision letters, author responses) are skipped deliberately
        out.append("</body></html>")
        doc = "".join(out)
        return self.resolve_tex(doc)

    def back(self, back) -> str:
        out = []
        for ch in back:
            tag = ch.tag
            if tag == "ack":
                t = ch.findtext("title") or "Acknowledgements"
                out.append(f"<section><h1>{html.escape(t)}</h1>{self.blocks(ch, 2, skip={'title'})}</section>")
            elif tag == "sec":
                out.append(self.sec(ch, 1))
            elif tag == "app-group":
                for app in ch:
                    if app.tag in ("app", "sec"):
                        out.append(self.sec(app, 1))
            elif tag == "fn-group":
                t = ch.findtext("title") or "Notes"
                items = "".join(f'<li id="{html.escape(fn.get("id", ""))}">{self.blocks(fn, 3, skip={"label"})}</li>'
                                for fn in ch.findall("fn"))
                out.append(f'<section class="notes"><h1>{html.escape(t)}</h1><ol>{items}</ol></section>')
            elif tag == "notes":
                out.append(self.sec(ch, 1))
            elif tag == "glossary":
                out.append(self.sec(ch, 1))
        for rl in back.iter("ref-list"):
            out.append(self.ref_list(rl))
        return "".join(out)

    def sec(self, el, level: int) -> str:
        title_el = el.find("title")
        label = el.findtext("label") or ""
        title = (label + " " if label else "") + (self.inline(title_el) if title_el is not None else "")
        h = min(level, 6)
        sid = f' id="{html.escape(el.get("id"))}"' if el.get("id") else ""
        head = f"<h{h}>{title.strip()}</h{h}>" if title.strip() else ""
        return f"<section{sid}>{head}{self.blocks(el, level + 1, skip={'title', 'label'})}</section>"

    def blocks(self, el, level: int, skip=frozenset()) -> str:
        out = []
        if el.text and el.text.strip():
            out.append(f"<p>{html.escape(el.text)}</p>")
        for ch in el:
            if not isinstance(ch.tag, str) or ch.tag in skip:
                continue
            out.append(self.block(ch, level))
            if ch.tail and ch.tail.strip():
                out.append(f"<p>{html.escape(ch.tail)}</p>")
        return "".join(out)

    def block(self, ch, level: int) -> str:
        tag = ch.tag
        if tag == "sec":
            return self.sec(ch, level)
        if tag == "p":
            return self.para(ch)
        if tag in ("fig",):
            return self.fig(ch)
        if tag in ("fig-group", "table-wrap-group", "disp-formula-group", "boxed-text", "statement",
                   "chem-struct-wrap", "glossary"):
            title = ch.find("caption/title") if ch.find("caption/title") is not None else ch.find("title")
            label = ch.findtext("label") or ""
            head = ""
            if tag in ("boxed-text", "statement") and (title is not None or label):
                head = f"<p><strong>{html.escape(label)} {self.inline(title) if title is not None else ''}</strong></p>"
            cls = "box" if tag in ("boxed-text", "statement") else tag
            return f'<div class="{cls}">{head}{self.blocks(ch, level, skip={"label", "caption", "title"})}</div>'
        if tag == "table-wrap":
            return self.table_wrap(ch)
        if tag == "disp-formula":
            return self.formula(ch, display=True)
        if tag == "list":
            return self.lst(ch)
        if tag in ("disp-quote", "verse-group"):
            return f"<blockquote>{self.blocks(ch, level)}</blockquote>"
        if tag in ("preformat", "code"):
            return f"<pre><code>{html.escape(''.join(ch.itertext()))}</code></pre>"
        if tag == "def-list":
            items = []
            for di in ch.findall("def-item"):
                term = di.find("term")
                d = di.find("def")
                items.append(f"<dt>{self.inline(term) if term is not None else ''}</dt>"
                             f"<dd>{self.blocks(d, level) if d is not None else ''}</dd>")
            return f"<dl>{''.join(items)}</dl>"
        if tag == "supplementary-material":
            return self.supp(ch)
        if tag == "graphic":
            src = self.image(ch.get(XLINK, ""))
            return f'<figure><img src="{html.escape(src)}" alt=""/></figure>' if src else ""
        if tag in ("media", "alternatives"):
            return self.blocks(ch, level) if tag == "alternatives" else ""
        if tag in ("title", "label", "object-id", "permissions", "attrib"):
            return ""
        if tag == "ref-list":
            return self.ref_list(ch)
        if tag in ("fn-group", "fn"):
            return f'<div class="notes">{self.blocks(ch, level, skip={"label"})}</div>'
        # unknown: treat as paragraph-ish container
        inner = self.inline(ch)
        return f"<p>{inner}</p>" if inner.strip() else ""

    def para(self, el) -> str:
        out, buf = [], [html.escape(el.text or "")]

        def flush():
            s = "".join(buf).strip()
            if s:
                out.append(f"<p>{s}</p>")
            buf.clear()

        for ch in el:
            if not isinstance(ch.tag, str):
                buf.append(html.escape(ch.tail or ""))
                continue
            if ch.tag in self.BLOCK and ch.tag != "p" or ch.tag == "p":
                flush()
                out.append(self.block(ch, 3))
            else:
                buf.append(self.node(ch))
            buf.append(html.escape(ch.tail or ""))
        flush()
        return "".join(out)

    def inline(self, el, skip=frozenset()) -> str:
        if el is None:
            return ""
        out = [html.escape(el.text or "")]
        for ch in el:
            if isinstance(ch.tag, str) and ch.tag not in skip:
                out.append(self.node(ch))
            out.append(html.escape(ch.tail or ""))
        return "".join(out)

    def node(self, ch) -> str:
        tag = ch.tag
        if tag.startswith("{" + MATHML_NS):
            return self.mathml(ch, display=False)
        simple = {"italic": "em", "bold": "strong", "sup": "sup", "sub": "sub", "underline": "u",
                  "monospace": "code", "strike": "s", "overline": "span"}
        if tag in simple:
            t = simple[tag]
            return f"<{t}>{self.inline(ch)}</{t}>"
        if tag == "sc":
            return f'<span class="smallcaps">{self.inline(ch)}</span>'
        if tag in ("ext-link", "uri", "self-uri"):
            href = ch.get(XLINK) or "".join(ch.itertext())
            if ch.get("ext-link-type") == "doi" and not href.startswith("http"):
                href = "https://doi.org/" + href
            return f'<a href="{html.escape(href)}">{self.inline(ch)}</a>'
        if tag == "xref":
            rid = (ch.get("rid") or "").split()[0] if ch.get("rid") else ""
            txt = self.inline(ch) or html.escape(rid)
            return f'<a href="#{html.escape(rid)}">{txt}</a>' if rid else txt
        if tag == "inline-formula":
            return self.formula(ch, display=False)
        if tag == "disp-formula":
            return self.formula(ch, display=True)
        if tag == "tex-math":
            return self.texmath(ch, display=False)
        if tag == "inline-graphic":
            src = self.image(ch.get(XLINK, ""))
            return f'<img src="{html.escape(src)}" alt=""/>' if src else ""
        if tag == "break":
            return "<br/>"
        if tag in ("fn",):
            return f' <span class="fn">[{self.plain(ch)}]</span>'
        if tag in ("label",):
            return f"<strong>{self.inline(ch)}</strong> "
        if tag in ("email",):
            e = "".join(ch.itertext())
            return f'<a href="mailto:{html.escape(e)}">{html.escape(e)}</a>'
        if tag in ("object-id", "permissions", "media"):
            return ""
        if tag == "alternatives":
            return self.alternatives(ch, display=False)
        return self.inline(ch)

    # ---------- math ----------
    def alternatives(self, el, display: bool) -> str:
        m = el.find(f"{{{MATHML_NS}}}math")
        if m is not None:
            return self.mathml(m, display)
        t = el.find("tex-math")
        if t is not None:
            return self.texmath(t, display)
        g = el.find("graphic") if el.find("graphic") is not None else el.find("inline-graphic")
        if g is not None:
            src = self.image(g.get(XLINK, ""))
            return f'<img class="math" src="{html.escape(src)}" alt=""/>' if src else ""
        return self.inline(el)

    def formula(self, el, display: bool) -> str:
        inner = ""
        for ch in el:
            if not isinstance(ch.tag, str):
                continue
            if ch.tag == "alternatives":
                inner = self.alternatives(ch, display)
            elif ch.tag == f"{{{MATHML_NS}}}math":
                inner = self.mathml(ch, display)
            elif ch.tag == "tex-math":
                inner = self.texmath(ch, display)
            elif ch.tag == "graphic":
                src = self.image(ch.get(XLINK, ""))
                inner = f'<img class="math" src="{html.escape(src)}" alt=""/>' if src else ""
        if not inner:
            inner = self.inline(el, skip={"label"})
        if not display:
            return inner
        label = el.findtext("label") or ""
        lid = f' id="{html.escape(el.get("id"))}"' if el.get("id") else ""
        lab = f' <span class="eqlabel">{html.escape(label)}</span>' if label else ""
        return f'<div class="equation"{lid}><p>{inner}{lab}</p></div>'

    def mathml(self, m, display: bool) -> str:
        m = copy.deepcopy(m)
        for e in m.iter():
            if isinstance(e.tag, str) and e.tag.startswith("{"):
                e.tag = e.tag.split("}", 1)[1]
            for a in list(e.attrib):
                if a.startswith("{"):
                    del e.attrib[a]
        if display:
            m.set("display", "block")
        m.attrib.pop("id", None)
        self.etree.cleanup_namespaces(m)
        s = self.etree.tostring(m, encoding="unicode", with_tail=False)
        return s.replace("<math", f'<math xmlns="{MATHML_NS}"', 1)

    def texmath(self, el, display: bool) -> str:
        t = "".join(el.itertext())
        if "\\begin{document}" in t:
            t = t.split("\\begin{document}", 1)[1].rsplit("\\end{document}", 1)[0]
        t = t.strip().strip("$").strip()
        t = re.sub(r"^\\\[|\\\]$|^\\\(|\\\)$", "", t).strip()
        self.tex.append((t, display))
        return f"@@TEX{len(self.tex) - 1}@@"

    def resolve_tex(self, doc: str) -> str:
        if not self.tex:
            return doc
        parts = []
        for t, disp in self.tex:
            t1 = t.replace("\n", " ")
            parts.append(f"$${t1}$$" if disp else f"${t1}$")
        md = "\n\n<!--SEP-->\n\n".join(parts)
        try:
            out = subprocess.run(["pandoc", "-f", "markdown", "-t", "html", "--mathml"], input=md,
                               capture_output=True, text=True, timeout=300).stdout
            chunks = out.split("<!--SEP-->")
        except Exception:
            chunks = []
        rendered = []
        for i, (t, disp) in enumerate(self.tex):
            m = re.search(r"<math.*?</math>", chunks[i], re.S) if i < len(chunks) else None
            rendered.append(m.group(0) if m else f"<code>{html.escape(t)}</code>")
        return re.sub(r"@@TEX(\d+)@@", lambda mm: rendered[int(mm.group(1))], doc)

    # ---------- figures / tables ----------
    def caption(self, el) -> str:
        label = el.findtext("label") or ""
        cap = el.find("caption")
        parts = []
        if label:
            parts.append(f"<strong>{html.escape(label.strip())}</strong>")
        if cap is not None:
            t = cap.find("title")
            if t is not None:
                parts.append(f"<strong>{self.inline(t)}</strong>")
            for p in cap.findall("p"):
                txt = self.inline(p)
                if re.match(r"^\s*<strong>DOI:</strong>", txt):
                    continue
                parts.append(txt)
        return " ".join(parts)

    def fig(self, el) -> str:
        fid = el.get("id", "")
        graphics = el.findall(".//graphic")
        cap = self.caption(el)
        imgs = []
        for g in graphics:
            src = self.image(g.get(XLINK, ""))
            if src:
                imgs.append(f'<img src="{html.escape(src)}" alt="{html.escape((el.findtext("label") or "Figure").strip())}"/>')
        if not imgs:
            self.missing += 1
            imgs.append('<span class="missing">[Figure image not available]</span>')
        return f'<figure id="{html.escape(fid)}">{"".join(imgs)}<figcaption>{cap}</figcaption></figure>'

    def table_wrap(self, el) -> str:
        tid = el.get("id", "")
        cap = self.caption(el)
        tables = el.findall(".//table")
        body = []
        for t in tables:
            body.append(self.table(t))
        if not tables:
            for g in el.findall(".//graphic"):
                src = self.image(g.get(XLINK, ""))
                if src:
                    body.append(f'<img src="{html.escape(src)}" alt="table"/>')
        foot = el.find("table-wrap-foot")
        ft = f'<div class="tablefoot">{self.blocks(foot, 4)}</div>' if foot is not None else ""
        return f'<div class="table-wrap" id="{html.escape(tid)}"><p class="caption">{cap}</p>{"".join(body)}{ft}</div>'

    def table(self, t) -> str:
        def cell(c):
            attrs = "".join(f' {a}="{html.escape(c.get(a))}"' for a in ("rowspan", "colspan") if c.get(a))
            return f"<{c.tag}{attrs}>{self.inline(c)}</{c.tag}>"

        def rows(container):
            return "".join("<tr>" + "".join(cell(c) for c in tr if c.tag in ("td", "th")) + "</tr>"
                           for tr in container.iter("tr"))
        parts = ["<table>"]
        th = t.find("thead")
        if th is not None:
            parts.append(f"<thead>{rows(th)}</thead>")
        bodies = t.findall("tbody")
        if bodies:
            for b in bodies:
                parts.append(f"<tbody>{rows(b)}</tbody>")
        else:
            parts.append("<tbody>" + "".join("<tr>" + "".join(cell(c) for c in tr if c.tag in ("td", "th")) + "</tr>"
                                             for tr in t.findall("tr")) + "</tbody>")
        parts.append("</table>")
        return "".join(parts)

    def lst(self, el) -> str:
        typ = el.get("list-type", "bullet")
        tag = "ol" if typ in ("order", "alpha-lower", "alpha-upper", "roman-lower", "roman-upper") else "ul"
        style = {"alpha-lower": ' type="a"', "alpha-upper": ' type="A"', "roman-lower": ' type="i"',
                 "roman-upper": ' type="I"'}.get(typ, "")
        if typ == "simple":
            style = ' class="simple"'
        items = "".join(f"<li>{self.blocks(li, 4, skip={'label'})}</li>" for li in el.findall("list-item"))
        return f"<{tag}{style}>{items}</{tag}>"

    def supp(self, el) -> str:
        label = (el.findtext("label") or "Supplementary material").strip()
        cap = el.find("caption")
        txt = self.inline(cap.find("title")) if cap is not None and cap.find("title") is not None else ""
        href = el.get(XLINK) or (el.find(".//media").get(XLINK) if el.find(".//media") is not None else "")
        sid = f' id="{html.escape(el.get("id"))}"' if el.get("id") else ""
        return f'<p class="supp"{sid}><strong>{html.escape(label)}</strong> {txt} <em>({html.escape(href)})</em></p>'

    # ---------- references ----------
    def ref_list(self, rl) -> str:
        title = rl.findtext("title") or "References"
        items = []
        for ref in rl.findall("ref"):
            rid = ref.get("id", "")
            cit = None
            for tag in ("element-citation", "mixed-citation", "citation", "nlm-citation"):
                cit = ref.find(tag)
                if cit is not None:
                    break
            if cit is None:
                continue
            items.append(f'<li id="{html.escape(rid)}">{self.citation(cit)}</li>')
        return f'<section class="references"><h1>{html.escape(title)}</h1><ol class="refs">{"".join(items)}</ol></section>'

    def citation(self, c) -> str:
        if c.tag == "mixed-citation" and len("".join(t for t in [c.text or ""] + [x.tail or "" for x in c]).strip()) > 10:
            s = self.inline(c, skip={"pub-id"})
            doi = c.findtext(".//pub-id[@pub-id-type='doi']")
            if doi:
                s += f' <a href="https://doi.org/{html.escape(doi)}">doi:{html.escape(doi)}</a>'
            return s
        names = []
        for pg in c.findall("person-group") or [c]:
            for n in pg.findall("name") + pg.findall("string-name"):
                sur = n.findtext("surname") or ""
                giv = n.findtext("given-names") or ""
                names.append(f"{sur} {giv}".strip() or self.plain(n))
            for col in pg.findall("collab"):
                names.append(self.plain(col))
        if len(names) > 10:
            names = names[:10] + ["et al."]
        year = c.findtext("year") or ""
        title_el = c.find("article-title") if c.find("article-title") is not None else c.find("chapter-title")
        title = self.inline(title_el) if title_el is not None else ""
        src_el = c.find("source")
        src = self.inline(src_el) if src_el is not None else ""
        vol, fp, lp = c.findtext("volume") or "", c.findtext("fpage") or "", c.findtext("lpage") or ""
        eloc = c.findtext("elocation-id") or ""
        parts = []
        if names:
            parts.append(html.escape(", ".join(names)) + ".")
        if year:
            parts.append(f"{html.escape(year)}.")
        if title:
            parts.append(title.rstrip(".") + ".")
        if src:
            s = f"<em>{src}</em>"
            if vol:
                s += f" <strong>{html.escape(vol)}</strong>"
            if fp:
                s += f":{html.escape(fp)}" + (f"–{html.escape(lp)}" if lp else "")
            elif eloc:
                s += f":{html.escape(eloc)}"
            parts.append(s + ".")
        doi = c.findtext(".//pub-id[@pub-id-type='doi']")
        if doi:
            parts.append(f'<a href="https://doi.org/{html.escape(doi)}">doi:{html.escape(doi)}</a>')
        pmid = c.findtext(".//pub-id[@pub-id-type='pmid']")
        if pmid and not doi:
            parts.append(f'<a href="https://pubmed.ncbi.nlm.nih.gov/{html.escape(pmid)}">PMID {html.escape(pmid)}</a>')
        if not parts:
            return self.inline(c)
        return " ".join(parts)

    # ---------- images ----------
    def image(self, href: str) -> str:
        if not href:
            return ""
        stem = Path(href).stem
        name = Path(href).name
        for d in self.asset_dirs:
            if not d.exists():
                continue
            direct = d / href
            if direct.exists():
                return str(direct)
            for f in d.rglob("*"):
                if f.is_file() and (f.name == name or f.stem == stem) and f.suffix.lower() in IMAGE_EXTS:
                    return str(f)
        for pat in self.url_patterns:
            url = pat.format(href=urllib.parse.quote(href), stem=urllib.parse.quote(stem))
            try:
                data = http_get(url, timeout=60)
                if data[:3] == b"\xff\xd8\xff" or data[:8] == b"\x89PNG\r\n\x1a\n" or data[:4] in (b"II*\x00", b"MM\x00*"):
                    ext = ".png" if data[:4] == b"\x89PNG" else ".jpg"
                    out = self.img_dir / f"{slug(stem)}{ext}"
                    out.write_bytes(data)
                    return str(out)
            except Exception as e:
                log.debug("figure fetch failed %s: %s", url, e)
        log.debug("figure not found: %s", href)
        return ""


IMAGE_EXTS = {".png", ".jpg", ".jpeg", ".gif", ".tif", ".tiff", ".bmp", ".webp", ".svg", ".pdf", ".eps", ".ps"}


def jats_to_ast(src: Source, work: Path) -> tuple[dict, Meta, list]:
    conv = JatsConverter(src, work)
    meta = conv.meta()
    h = conv.to_html()
    hp = work / "jats.html"
    hp.write_text(h, encoding="utf-8")
    ast = pandoc_to_ast(hp, "html")
    notes = []
    if conv.missing:
        notes.append(f"{conv.missing} figure image(s) could not be fetched; captions kept with a placeholder.")
    return ast, meta, notes


# --------------------------------------------------------------------------- #
# LaTeX -> AST
# --------------------------------------------------------------------------- #
RENDER_ENVS = ["tikzpicture", "algorithm", "algorithm*", "pgfpicture", "circuitikz", "forest", "tikzcd"]


def find_main_tex(d: Path) -> Path:
    texs = [p for p in d.rglob("*.tex")]
    scored = []
    for p in texs:
        t = p.read_text(errors="replace")
        if "\\documentclass" in t and "\\begin{document}" in t:
            score = 10 + (5 if p.stem.lower() in ("main", "ms", "paper", "article", "manuscript") else 0) + len(t) / 1e5
            scored.append((score, p))
    if not scored:
        raise ConversionError("No main LaTeX file (with \\documentclass and \\begin{document}) found")
    return max(scored)[1]


def latex_from_dir(d: Path, meta: Meta, main: Path | None = None) -> Source:
    main = main or find_main_tex(d)
    return Source("latex", main, meta=meta, label="LaTeX source")


def strip_tex_comments(t: str) -> str:
    out = []
    for line in t.splitlines():
        m = re.search(r"(?<!\\)%", line)
        out.append(line[:m.start()] if m else line)
    return "\n".join(out)


def flatten_tex(path: Path, root: Path, depth=0) -> str:
    t = strip_tex_comments(path.read_text(errors="replace"))
    if depth > 6:
        return t

    def rep(m):
        name = m.group(2).strip()
        for cand in (root / name, root / (name + ".tex"), path.parent / name, path.parent / (name + ".tex")):
            if cand.is_file():
                return flatten_tex(cand, root, depth + 1)
        return m.group(0)
    return re.sub(r"\\(input|include|subfile)\s*\{([^}]+)\}", rep, t)


def find_env_spans(t: str, env: str) -> list[tuple[int, int]]:
    b, e = f"\\begin{{{env}}}", f"\\end{{{env}}}"
    spans, i = [], 0
    while True:
        s = t.find(b, i)
        if s < 0:
            break
        depth, j = 0, s
        while True:
            nb, ne = t.find(b, j), t.find(e, j)
            if ne < 0:
                return spans
            if 0 <= nb < ne:
                depth += 1
                j = nb + len(b)
            else:
                depth -= 1
                j = ne + len(e)
                if depth == 0:
                    break
        spans.append((s, j))
        i = j
    return spans


def crop_whitespace(png: Path, pad=12):
    from PIL import Image, ImageOps
    im = Image.open(png).convert("RGB")
    bbox = ImageOps.invert(im).getbbox()
    if bbox:
        l, t, r, b = bbox
        im = im.crop((max(0, l - pad), max(0, t - pad), min(im.width, r + pad), min(im.height, b + pad)))
    im.save(png)


def render_tex_snippet(snippet: str, preamble: str, docclass: str, cwd: Path, out_png: Path) -> bool:
    if not shutil.which("pdflatex"):
        return False
    import pymupdf
    pre = "\n".join(l for l in preamble.splitlines()
                    if not re.search(r"\\linenumbers|\{lineno\}|\{minted\}|\\addbibresource|\{biblatex\}|\\bibliographystyle|\{hyperref\}|\{geometry\}|\{fullpage\}", l))
    snippet = re.sub(r"\\begin\{(algorithm\*?)\}\s*(\[[^\]]*\])?", lambda m: f"\\begin{{{m.group(1)}}}[H]", snippet)
    snippet = re.sub(r"\\label\{[^}]*\}", "", snippet)
    for cls in ("\\documentclass[11pt]{article}", docclass):
        doc = (f"{cls}\n{pre}\n\\usepackage{{float}}\n\\pagestyle{{empty}}\n\\begin{{document}}\n"
               f"{snippet}\n\\end{{document}}\n")
        with tempfile.TemporaryDirectory() as td:
            tf = Path(td) / "snip.tex"
            tf.write_text(doc)
            try:
                p = subprocess.run(["pdflatex", "-interaction=nonstopmode", "-halt-on-error",
                                    f"-output-directory={td}", str(tf)], cwd=cwd, capture_output=True, timeout=90)
            except subprocess.TimeoutExpired:
                continue
            pdf = Path(td) / "snip.pdf"
            if p.returncode == 0 and pdf.exists():
                d = pymupdf.open(pdf)
                d[0].get_pixmap(dpi=220).save(out_png)
                crop_whitespace(out_png)
                return True
    return False


def latex_to_ast(src: Source, work: Path) -> tuple[dict, Meta, list]:
    root = src.path.parent
    t = flatten_tex(src.path, root)
    notes = []
    m = re.search(r"\\documentclass(\[[^\]]*\])?\{[^}]*\}", t)
    docclass = m.group(0) if m else "\\documentclass{article}"
    preamble = t[m.end() if m else 0: t.find("\\begin{document}")]

    # bibliography: .bib -> citeproc ; .bbl only -> inline it
    bibs = []
    for mm in re.finditer(r"\\(?:bibliography|addbibresource)(?:\[[^\]]*\])?\{([^}]+)\}", t):
        for name in mm.group(1).split(","):
            name = name.strip()
            for cand in (root / name, root / (name + ".bib")):
                if cand.is_file():
                    bibs.append(cand)
                    break
    if not bibs:
        bbls = list(root.glob("*.bbl"))
        bbl = next((b for b in bbls if b.stem == src.path.stem), bbls[0] if bbls else None)
        if bbl is not None:
            bbl_t = bbl.read_text(errors="replace")
            t = re.sub(r"\\bibliography\{[^}]*\}|\\printbibliography(\[[^\]]*\])?",
                       lambda _: bbl_t.replace("\\", "\\\\"), t, count=1)
            t = re.sub(r"\\bibliography\{[^}]*\}", "", t)

    # render environments pandoc can't handle (TikZ, algorithms) to images
    rdir = work / "tex_render"
    rdir.mkdir(exist_ok=True)
    n_ok = n_fail = 0
    body_start = t.find("\\begin{document}")
    for env in RENDER_ENVS:
        spans = [s for s in find_env_spans(t, env) if s[0] > body_start]
        for s, e in reversed(spans):
            snippet = t[s:e]
            out = rdir / f"{slug(env)}-{s}.png"
            if render_tex_snippet(snippet, preamble, docclass, root, out):
                n_ok += 1
                if env.startswith("algorithm"):
                    repl = f"\\begin{{figure}}\\includegraphics{{{out}}}\\end{{figure}}"
                else:
                    repl = f"\\includegraphics{{{out}}}"
                t = t[:s] + repl + t[e:]
            else:
                n_fail += 1
    if n_ok:
        notes.append(f"Rendered {n_ok} TikZ/algorithm block(s) as images.")
    if n_fail:
        notes.append(f"{n_fail} TikZ/algorithm block(s) could not be rendered (pdflatex missing or failed).")

    t = re.sub(r"\\(linenumbers|nolinenumbers|tableofcontents|listoffigures|listoftables)\b", "", t)
    t = re.sub(r"\\begin\{linenomath\*?\}|\\end\{linenomath\*?\}", "", t)

    # graphics search path for the image resolver
    gp = []
    mm = re.search(r"\\graphicspath\s*\{((?:\s*\{[^}]*\}\s*)+)\}", t)
    if mm:
        gp = [root / x for x in re.findall(r"\{([^}]*)\}", mm.group(1))]
    src.extra["image_dirs"] = [root] + gp

    flat = root / "_p2e_flat.tex"
    flat.write_text(t)
    extra = ["--resource-path", os.pathsep.join(map(str, [root] + gp))]
    if bibs:
        extra += ["--citeproc", "--metadata", "link-citations=true", "--metadata", "reference-section-title=References"]
        for b in bibs:
            extra += ["--bibliography", str(b)]
    try:
        ast = pandoc_to_ast(flat, "latex", cwd=root, extra=extra)
    except ConversionError:
        if bibs:  # broken .bib files are common; retry without citeproc
            notes.append("Bibliography could not be processed; citations left as keys.")
            ast = pandoc_to_ast(flat, "latex", cwd=root, extra=extra[:2])
        else:
            raise

    # quality check: did pandoc keep most of the text?
    body_txt = strip_tex_comments(t[body_start:])
    body_txt = re.sub(r"\$[^$]*\$|\\[a-zA-Z]+\*?(\[[^\]]*\])?|[{}]", " ", body_txt)
    approx = len(body_txt.split())
    got = len(stringify(ast["blocks"]).split())
    log.info("  LaTeX: pandoc kept ~%d of ~%d words", got, approx)
    if approx > 500 and got < 0.35 * approx:
        raise ConversionError(f"pandoc dropped too much text ({got}/{approx} words)")
    meta = meta_from_ast(ast)
    return ast, meta, notes


def meta_from_ast(ast: dict) -> Meta:
    m = Meta()
    mt = ast.get("meta", {})
    if "title" in mt:
        m.title = stringify(mt["title"])
    if "author" in mt:
        a = mt["author"]
        items = a["c"] if a.get("t") == "MetaList" else [a]
        names = [stringify(x) for x in items]
        if len(names) == 1 and names[0].count(",") >= 2:
            names = [n.strip() for n in re.split(r",|\band\b", names[0]) if n.strip()]
        m.authors = [re.sub(r"[\d*†‡§]+$", "", n).strip() for n in names if n]
    if "date" in mt:
        m.date = stringify(mt["date"])
    if "abstract" in mt:
        m.abstract = stringify(mt["abstract"])
    if "doi" in mt:
        m.doi = stringify(mt["doi"])
    if "journal" in mt:
        m.journal = stringify(mt["journal"])
    return m


# --------------------------------------------------------------------------- #
# PDF -> Markdown (heuristic layout analysis)
# --------------------------------------------------------------------------- #
MATH_FONT_RE = re.compile(r"CMMI|CMSY|CMEX|MSAM|MSBM|Math|Symbol|STIX|rsfs|EUFM|CMBSY|LMMath|esint|MTMI|MTSY|txsy|txmi|pxmi|Euclid", re.I)
KNOWN_SECTIONS = re.compile(
    r"^(?:\d+(?:\.\d+)*\.?\s+|[IVX]+\.\s+)?(abstract|summary|introduction|background|results?|discussion|"
    r"results and discussion|methods?|materials and methods|methods and materials|star\s*methods|online methods|"
    r"conclusions?|references|bibliography|acknowledge?ments?|funding|author contributions?|"
    r"competing interests?|declaration of interests?|conflicts? of interest|data availability|code availability|"
    r"data and code availability|supplementary (?:information|material|figures?|tables?)|supporting information|"
    r"appendix|keywords|significance|highlights|limitations|related work|experiments?|"
    r"figure legends|figures|tables|abbreviations|ethics statement|resource availability)\s*[:.]?$", re.I)
CAPTION_RE = re.compile(
    r"^(?:Supplementary\s+|Extended\s+Data\s+|Supporting\s+)?(Figure|Fig\.?|Table|Algorithm|Box)\s*S?\d+[A-Za-z]?\s*[.:|\-–—]?", re.I)


@dataclass
class Line:
    text: str
    bbox: tuple
    size: float
    bold: float
    spans: list
    page: int
    block: int
    math: float
    font: str


@dataclass
class Para:
    lines: list
    page: int
    kind: str = "text"            # text | heading | caption | front | title
    level: int = 0
    asset: Optional[str] = None   # image path for figures/equations

    @property
    def text(self):
        return join_lines([l.text for l in self.lines])

    @property
    def bbox(self):
        xs0 = [l.bbox[0] for l in self.lines]
        ys0 = [l.bbox[1] for l in self.lines]
        xs1 = [l.bbox[2] for l in self.lines]
        ys1 = [l.bbox[3] for l in self.lines]
        return (min(xs0), min(ys0), max(xs1), max(ys1))

    @property
    def size(self):
        c = collections.Counter()
        for l in self.lines:
            c[l.size] += len(l.text)
        return c.most_common(1)[0][0] if c else 0


def join_lines(parts: list[str]) -> str:
    out = ""
    for p in parts:
        p = p.strip()
        if not p:
            continue
        if not out:
            out = p
        elif re.search(r"[A-Za-z]-$", out) and p[:1].islower():
            out = out[:-1] + p
        elif out.endswith(("-", "/", "–")) and not out.endswith(" -"):
            out += p
        else:
            out += " " + p
    return out


def md_escape(s: str) -> str:
    s = re.sub(r"([\\`*_{}\[\]<>#$^~|@])", r"\\\1", s)
    return s


def md_para_start(s: str) -> str:
    s = re.sub(r"^(\d+)([.)])(\s)", r"\1\\\2\3", s)
    s = re.sub(r"^([-+*:])(\s)", r"\\\1\2", s)
    s = re.sub(r"^([A-Za-z])([.)])(\s)", r"\1\\\2\3", s)
    return s


def rect_union(rs):
    return (min(r[0] for r in rs), min(r[1] for r in rs), max(r[2] for r in rs), max(r[3] for r in rs))


def rect_inter(a, b, pad=0.0):
    return not (a[2] + pad < b[0] or b[2] + pad < a[0] or a[3] + pad < b[1] or b[3] + pad < a[1])


def rect_area(r):
    return max(0, r[2] - r[0]) * max(0, r[3] - r[1])


def rect_contains(outer, inner, tol=2.0):
    return (inner[0] >= outer[0] - tol and inner[1] >= outer[1] - tol and inner[2] <= outer[2] + tol
            and inner[3] <= outer[3] + tol)


class PdfAnalyzer:
    def __init__(self, pdf: Path, work: Path, dpi: int = 200):
        import pymupdf
        self.pm = pymupdf
        self.doc = pymupdf.open(pdf)
        self.work = work
        self.dpi = dpi
        self.asset_dir = work / "pdf_assets"
        self.asset_dir.mkdir(parents=True, exist_ok=True)
        self.pages_lines: list[list[Line]] = []
        self.regions: dict[int, list[dict]] = collections.defaultdict(list)  # page -> [{rect, kind, path, caption}]
        self.body_size = 10.0
        self.running = set()

    # ---- extraction ----
    def extract(self):
        flags = self.pm.TEXT_PRESERVE_WHITESPACE | self.pm.TEXT_MEDIABOX_CLIP
        sizes = collections.Counter()
        for pno, page in enumerate(self.doc):
            d = page.get_text("dict", flags=flags)
            lines = []
            for bi, b in enumerate(d["blocks"]):
                if b.get("type") != 0:
                    continue
                for l in b["lines"]:
                    if abs(l["dir"][1]) > 0.1 or l["dir"][0] < 0:
                        continue  # rotated margin text (e.g. bioRxiv/arXiv stamps)
                    spans = [s for s in l["spans"] if s["text"].strip() or s["text"] == " "]
                    if not spans or not "".join(s["text"] for s in spans).strip():
                        continue
                    n = sum(len(s["text"].strip()) for s in spans) or 1
                    size_c = collections.Counter()
                    bold = math = 0
                    for s in spans:
                        k = len(s["text"].strip())
                        size_c[round(s["size"] * 2) / 2] += k
                        if s["flags"] & 16 or re.search(r"bold|black|heavy|semibold", s["font"], re.I):
                            bold += k
                        if MATH_FONT_RE.search(s["font"]):
                            math += k
                    size = size_c.most_common(1)[0][0]
                    text = "".join(s["text"] for s in spans)
                    ln = Line(text=text, bbox=tuple(l["bbox"]), size=size, bold=bold / n, spans=spans,
                              page=pno, block=bi, math=math / n, font=spans[0]["font"])
                    lines.append(ln)
                    sizes[size] += n
            self.pages_lines.append(lines)
        if sizes:
            self.body_size = sizes.most_common(1)[0][0]
        self._find_running()

    def _norm(self, s):
        return re.sub(r"\d+", "#", s.strip().lower())

    def _find_running(self):
        n = len(self.doc)
        if n < 3:
            return
        c = collections.Counter()
        for pno, lines in enumerate(self.pages_lines):
            h = self.doc[pno].rect.height
            seen = set()
            for l in lines:
                if l.bbox[1] < 0.09 * h or l.bbox[3] > 0.91 * h:
                    seen.add(self._norm(l.text))
            c.update(seen)
        self.running = {k for k, v in c.items() if v >= max(2, 0.3 * n)}

    def is_noise(self, l: Line, page) -> bool:
        t = l.text.strip()
        h = page.rect.height
        in_margin = l.bbox[1] < 0.09 * h or l.bbox[3] > 0.91 * h
        if in_margin and (self._norm(t) in self.running or re.fullmatch(r"[\divxlcIVXLC\s\-–/of|]+", t)):
            return True
        if re.fullmatch(r"\d{1,4}", t) and (l.bbox[2] - l.bbox[0]) < 25:
            return True  # line numbers / page numbers
        if re.match(r"(?i)^(bioRxiv|medRxiv) preprint doi", t) or "copyright holder for this preprint" in t:
            return True
        if re.match(r"(?i)^(downloaded from|arxiv:\d{4}\.\d+)", t) and in_margin:
            return True
        return False

    # ---- figure / table / equation regions ----
    def find_regions(self, pno: int, lines: list[Line]):
        page = self.doc[pno]
        pr = page.rect
        parea = pr.width * pr.height
        margin_top, margin_bot = 0.07 * pr.height, 0.93 * pr.height
        rects = []
        for info in page.get_image_info():
            r = tuple(info["bbox"])
            if (r[2] - r[0] < 1 and r[3] - r[1] < 1) or rect_area(r) > 0.95 * parea:
                continue  # keep thin strips: some PDFs store one picture as hundreds of 1-px scanline images
            rects.append(("img", r))
        try:
            drawings = page.get_drawings()
        except Exception:
            drawings = []
        thin = []
        for dr in drawings:
            r = tuple(dr["rect"])
            w, h = r[2] - r[0], r[3] - r[1]
            if w < 2.5 or h < 2.5:
                thin.append(r)
            elif rect_area(r) < 0.9 * parea:
                rects.append(("draw", r))
        # cluster (sorted sweep keeps this fast when a page has thousands of fragments)
        rects.sort(key=lambda kr: (kr[1][1], kr[1][0]))
        clusters = []
        for kind, r in rects:
            if clusters and rect_inter(clusters[-1]["rect"], r, pad=6):
                c = clusters[-1]
                c["rect"] = rect_union([c["rect"], r])
                c["kinds"].add(kind)
                c["n"] += 1
                continue
            placed = False
            for c in clusters:
                if rect_inter(c["rect"], r, pad=6):
                    c["rect"] = rect_union([c["rect"], r])
                    c["kinds"].add(kind)
                    c["n"] += 1
                    placed = True
                    break
            if not placed:
                clusters.append({"rect": r, "kinds": {kind}, "n": 1})
        merged = True
        while merged:
            merged = False
            for i in range(len(clusters)):
                for j in range(i + 1, len(clusters)):
                    if rect_inter(clusters[i]["rect"], clusters[j]["rect"], pad=6):
                        clusters[i]["rect"] = rect_union([clusters[i]["rect"], clusters[j]["rect"]])
                        clusters[i]["kinds"] |= clusters[j]["kinds"]
                        clusters[i]["n"] += clusters[j]["n"]
                        del clusters[j]
                        merged = True
                        break
                if merged:
                    break
        # second pass: join nearby pieces of one figure (panels, tiles) unless body text sits between them
        prose = [l.bbox for l in lines if len(l.text.strip()) > 50 and l.size >= self.body_size - 0.5]

        def prose_between(a, b):
            u = rect_union([a, b])
            return any(rect_contains(u, t, tol=0) and not rect_inter(a, t) and not rect_inter(b, t) for t in prose)
        merged = True
        while merged:
            merged = False
            for i in range(len(clusters)):
                for j in range(i + 1, len(clusters)):
                    a, b = clusters[i]["rect"], clusters[j]["rect"]
                    if rect_inter(a, b, pad=22) and not prose_between(a, b):
                        clusters[i]["rect"] = rect_union([a, b])
                        clusters[i]["kinds"] |= clusters[j]["kinds"]
                        clusters[i]["n"] += clusters[j]["n"]
                        del clusters[j]
                        merged = True
                        break
                if merged:
                    break
        # attach thin strokes touching a cluster (axes, ticks)
        for c in clusters:
            for r in thin:
                if rect_inter(c["rect"], r, pad=2):
                    c["rect"] = rect_union([c["rect"], r])
        regions = []
        for c in clusters:
            r = c["rect"]
            if rect_area(r) < 2500 or (r[3] < margin_top or r[1] > margin_bot):
                continue
            inside_chars = sum(len(l.text) for l in lines if rect_contains(r, l.bbox))
            if "img" not in c["kinds"] and c["n"] <= 3 and inside_chars > 250:
                continue  # shaded text box / background panel, keep as text
            if "img" not in c["kinds"] and c["n"] <= 2 and (r[3] - r[1]) < 30:
                continue
            regions.append({"rect": r, "kind": "figure"})

        # tables: grid tables via find_tables, rule tables via caption + horizontal rules
        try:
            for tb in page.find_tables().tables:
                r = tuple(tb.bbox)
                if tb.row_count >= 2 and tb.col_count >= 2 and not any(rect_inter(r, g["rect"]) for g in regions):
                    regions.append({"rect": r, "kind": "table"})
        except Exception:
            pass
        caps = [l for l in lines if re.match(r"(?i)^(Supplementary\s+)?Table\s*S?\d+", l.text.strip())]
        hrules = sorted([r for r in thin if (r[2] - r[0]) > 80 and (r[3] - r[1]) < 2.5], key=lambda r: r[1])
        for cap in caps:
            if any(rect_inter(cap.bbox, g["rect"], pad=30) for g in regions):
                continue
            below = [r for r in hrules if r[1] > cap.bbox[1] - 2]
            above = [r for r in hrules if r[3] < cap.bbox[1] + 2]
            grp = None
            if below and below[0][1] - cap.bbox[3] < 60:
                grp = [below[0]]
                for r in below[1:]:
                    if r[1] - grp[-1][3] < 250:
                        grp.append(r)
                    else:
                        break
            elif above and cap.bbox[1] - above[-1][3] < 60:
                grp = [above[-1]]
                for r in reversed(above[:-1]):
                    if grp[-1][1] - r[3] < 250:
                        grp.append(r)
                    else:
                        break
            if grp and len(grp) >= 2:
                regions.append({"rect": rect_union(grp), "kind": "table"})
            else:
                tr = self.text_table_region(cap, lines, pr)
                if tr:
                    regions.append({"rect": tr, "kind": "table"})

        # expand regions with nearby short labels (axis labels, panel letters)
        for g in regions:
            r = g["rect"]
            for _ in range(2):
                for l in lines:
                    if CAPTION_RE.match(l.text.strip()):
                        continue
                    vert_overlap = min(r[3], l.bbox[3]) - max(r[1], l.bbox[1]) > 0.6 * (l.bbox[3] - l.bbox[1])
                    if rect_inter(r, l.bbox, pad=8) and len(l.text.strip()) < 70 and not rect_contains(r, l.bbox) \
                            and (l.size < self.body_size - 0.4 or vert_overlap):
                        r = rect_union([r, l.bbox])
            g["rect"] = r
        return regions

    def text_table_region(self, cap: Line, lines: list[Line], pr):
        """Borderless table: rows with several horizontally separated cells, next to a 'Table N' caption."""
        x0, x1 = (cap.bbox[0] - 5, cap.bbox[2] + 5) if (cap.bbox[2] - cap.bbox[0]) < 0.6 * pr.width else (0, pr.width)
        rows = collections.defaultdict(list)
        for l in lines:
            if l is cap or l.bbox[0] < x0 - 40 or l.bbox[2] > x1 + 40:
                continue
            rows[round(l.bbox[1] / 3)].append(l)
        ys = sorted(rows)
        multi = {y: len(rows[y]) >= 2 for y in ys}

        def run_from(start_y, step):
            idx = [i for i, y in enumerate(ys) if (y * 3 >= start_y if step > 0 else y * 3 <= start_y)]
            if not idx:
                return None
            seq = idx if step > 0 else list(reversed(idx))
            got, last, n_multi = [], None, 0
            for i in seq:
                y = ys[i]
                ry = min(l.bbox[1] for l in rows[y]) if step > 0 else max(l.bbox[3] for l in rows[y])
                if last is not None and abs(ry - last) > 40:
                    break
                txtlen = sum(len(l.text) for l in rows[y])
                if not multi[y] and txtlen > 90:
                    break  # a prose line ends the table
                got.append(y)
                n_multi += multi[y]
                last = max(l.bbox[3] for l in rows[y]) if step > 0 else min(l.bbox[1] for l in rows[y])
            if len(got) >= 3 and n_multi >= 0.5 * len(got):
                return rect_union([l.bbox for y in got for l in rows[y]])
            return None
        r = run_from(cap.bbox[1] - 1, -1) if cap.bbox[1] > pr.height * 0.25 else None
        r2 = run_from(cap.bbox[3] + 1, +1)
        cands = [x for x in (r, r2) if x]
        return max(cands, key=rect_area) if cands else None

    def save_clip(self, pno: int, rect, name: str, photo: bool = False) -> str:
        page = self.doc[pno]
        r = self.pm.Rect(rect) + (-4, -4, 4, 4)
        r = r & page.rect
        pix = page.get_pixmap(clip=r, dpi=self.dpi)
        out = self.asset_dir / f"{name}.{'jpg' if photo else 'png'}"
        if photo:
            try:
                pix.save(out, jpg_quality=85)
            except TypeError:
                pix.save(out)
        else:
            pix.save(out)
        return str(out)

    # ---- assembly ----
    def analyze(self):
        self.extract()
        all_paras: list[Para] = []
        for pno, lines in enumerate(self.pages_lines):
            page = self.doc[pno]
            lines = [l for l in lines if not self.is_noise(l, page)]
            regions = self.find_regions(pno, lines)
            # render regions
            for i, g in enumerate(regions):
                photo = any(rect_contains(g["rect"], tuple(info["bbox"]), tol=5) and rect_area(tuple(info["bbox"])) > 0.3 * rect_area(g["rect"])
                            for info in page.get_image_info())
                g["path"] = self.save_clip(pno, g["rect"], f"p{pno + 1}-{g['kind']}{i + 1}", photo=photo)
            self.regions[pno] = regions
            free = []
            for l in lines:
                host = next((g for g in regions if rect_contains(g["rect"], l.bbox, tol=3)
                             or (rect_area(l.bbox) and rect_inter(g["rect"], l.bbox) and
                                 rect_area((max(g["rect"][0], l.bbox[0]), max(g["rect"][1], l.bbox[1]),
                                            min(g["rect"][2], l.bbox[2]), min(g["rect"][3], l.bbox[3]))) > 0.6 * rect_area(l.bbox))), None)
                if host is None:
                    free.append(l)
            paras = self.paragraphs(pno, free, regions)
            all_paras.extend(paras)
        return self.classify(all_paras)

    def paragraphs(self, pno: int, lines: list[Line], regions: list[dict]) -> list[Para]:
        """Order single lines (and figure regions) by reading order, then glue lines into paragraphs."""
        # merge fragments on the same baseline within a block
        lines = sorted(lines, key=lambda l: (l.block, round(l.bbox[1]), l.bbox[0]))
        merged: list[Line] = []
        for l in lines:
            m = merged[-1] if merged else None
            if m and m.block == l.block and abs(m.bbox[1] - l.bbox[1]) < 2 and l.bbox[0] >= m.bbox[2] - 1 \
                    and l.bbox[0] - m.bbox[2] < 3 * l.size:
                m.text = m.text + ("" if m.text.endswith(" ") else " ") + l.text
                m.spans = m.spans + [{**l.spans[0], "text": " " + l.spans[0]["text"]}] + l.spans[1:]
                m.bbox = rect_union([m.bbox, l.bbox])
            else:
                merged.append(copy.copy(l))
        units = [Para([l], pno) for l in merged]
        for g in regions:
            units.append(Para(lines=[Line("", g["rect"], 0, 0, [], pno, -1, 0, "")], page=pno,
                              kind=g["kind"], asset=g["path"]))
        units = self.order(pno, units)
        # right edge of the text column each line belongs to
        txt = [u.lines[0] for u in units if u.kind == "text"]

        def right_edge(l):
            ov = [o.bbox[2] for o in txt if min(o.bbox[2], l.bbox[2]) - max(o.bbox[0], l.bbox[0]) > 0.3 * (l.bbox[2] - l.bbox[0])]
            return max(ov) if ov else l.bbox[2]
        abbrev = re.compile(r"(\b(e\.g|i\.e|et al|cf|Fig|Figs|Eq|Eqs|vs|approx|ca|resp|Ref|Refs|No|Dr|Prof|al)\.|\b[A-Z]\.)$")
        paras: list[Para] = []
        cur: list[Line] = []

        def flush():
            if cur:
                paras.append(Para(list(cur), pno))
                cur.clear()
        for u in units:
            if u.kind != "text":
                flush()
                paras.append(u)
                continue
            l = u.lines[0]
            if not cur:
                cur.append(l)
                continue
            a = cur[-1]
            left = min(x.bbox[0] for x in cur)
            gap = l.bbox[1] - a.bbox[3]
            jumped = l.bbox[1] < a.bbox[1] - 2 or abs(l.bbox[0] - left) > 0.3 * self.doc[pno].rect.width
            re_a = right_edge(a)
            width = max(1.0, re_a - left)
            at = a.text.rstrip()
            short_a = (re_a - a.bbox[2]) > 0.18 * width and at.endswith((".", ":", "?", "!")) and not abbrev.search(at)
            indent = (l.bbox[0] - left > 8 and all(abs(x.bbox[0] - left) < 3 for x in cur[1:])
                      and at.endswith((".", ":", "?", "!")) and not abbrev.search(at))
            hanging = len(cur) >= 2 and l.bbox[0] < a.bbox[0] - 6 and cur[0].bbox[0] < a.bbox[0] - 6
            size_change = abs(l.size - a.size) >= 1.0
            style_break = (a.bold > 0.9) != (l.bold > 0.9) and (len(at) < 120 or len(l.text) < 120)
            cap = CAPTION_RE.match(l.text.strip())
            if jumped or gap > 0.75 * max(l.size, a.size) or short_a or indent or hanging or size_change \
                    or style_break or cap or (len(cur) == 1 and l.bbox[0] > a.bbox[0] + 8 and gap > 0.3 * l.size):
                if jumped and not (gap > 0.75 * l.size) and not short_a and not size_change and not cap \
                        and l.text.strip()[:1].islower():
                    cur.append(l)  # column/page continuation of the same paragraph
                    continue
                flush()
            cur.append(l)
        flush()
        return paras

    def order(self, pno: int, paras: list[Para]) -> list[Para]:
        page = self.doc[pno]
        W = page.rect.width
        textual = [p for p in paras if p.kind == "text"]
        narrow = [p for p in textual if (p.bbox[2] - p.bbox[0]) < 0.55 * W]
        chars_n = sum(len(p.text) for p in narrow)
        chars_all = sum(len(p.text) for p in textual) or 1
        left = sum(len(p.text) for p in narrow if (p.bbox[0] + p.bbox[2]) / 2 < W / 2)
        right = chars_n - left
        two_col = chars_n > 0.5 * chars_all and left > 0.2 * chars_n and right > 0.2 * chars_n
        if not two_col:
            return sorted(paras, key=lambda p: (round(p.bbox[1] / 4), p.bbox[0]))
        wide = sorted([p for p in paras if (p.bbox[2] - p.bbox[0]) >= 0.55 * W], key=lambda p: p.bbox[1])
        rest = [p for p in paras if (p.bbox[2] - p.bbox[0]) < 0.55 * W]
        out, prev_y = [], -1
        for wp in wide + [None]:
            y = wp.bbox[1] if wp else 1e9
            band = [p for p in rest if prev_y <= p.bbox[1] < y]
            lcol = sorted([p for p in band if (p.bbox[0] + p.bbox[2]) / 2 < W / 2], key=lambda p: p.bbox[1])
            rcol = sorted([p for p in band if (p.bbox[0] + p.bbox[2]) / 2 >= W / 2], key=lambda p: p.bbox[1])
            out += lcol + rcol
            if wp:
                out.append(wp)
                prev_y = wp.bbox[1]
        return out

    def classify(self, paras: list[Para]) -> list[Para]:
        body = self.body_size

        # glue wrapped large-font lines (titles, long headings) that were split by centring
        glued: list[Para] = []
        for p in paras:
            q = glued[-1] if glued else None
            if (q and p.kind == q.kind == "text" and p.page == q.page and p.size == q.size
                    and p.size >= body + 1.0 and 0 <= p.bbox[1] - q.bbox[3] < 0.9 * p.size
                    and len(q.text) + len(p.text) < 400):
                glued[-1] = Para(q.lines + p.lines, q.page)
            else:
                glued.append(p)
        paras = glued

        # title: biggest text on page 1
        p1 = [p for p in paras if p.page == 0 and p.kind == "text"]
        if p1:
            big = max(p1, key=lambda p: p.size)
            if big.size >= body + 2.0 and len(big.text) < 400:
                big.kind = "title"
        heading_sizes = set()
        for p in paras:
            if p.kind != "text":
                continue
            t = p.text.strip()
            if CAPTION_RE.match(t):
                p.kind = "caption"
                continue
            n = len(t)
            boldish = sum(l.bold * len(l.text) for l in p.lines) / max(1, sum(len(l.text) for l in p.lines))
            numbered = re.match(r"^(\d+(?:\.\d+)*)\.?\s+[A-Z]", t)
            ends_sentence = t.endswith(".") and n > 60
            if n <= 160 and len(p.lines) <= 3 and not ends_sentence:
                if p.size >= body + 1.0 or KNOWN_SECTIONS.match(t) or (boldish > 0.85 and n < 110 and p.size >= body - 0.6) \
                        or (numbered and boldish > 0.5):
                    if p.size < body - 1.5:
                        continue
                    p.kind = "heading"
                    heading_sizes.add(p.size)

        # runs of 3+ consecutive "headings" are usually table cells or figure labels: demote
        i = 0
        while i < len(paras):
            j = i
            while j < len(paras) and paras[j].kind == "heading":
                j += 1
            if j - i >= 3:
                for p in paras[i:j]:
                    if not (KNOWN_SECTIONS.match(p.text.strip()) or re.match(r"^[A-Z]?\d+(\.\d+)*\.?\s+[A-Z][a-z]", p.text.strip())):
                        p.kind = "text"
            i = max(j, i + 1)
        for p in paras:
            if p.kind == "heading" and (re.match(r"^[\(\[\d·.,\s]+$", p.text.strip()) or p.text.strip().startswith(("(", "["))):
                p.kind = "text"

        # front matter: page-1 material before the first real section heading
        boundary = None
        for i, p in enumerate(paras):
            if p.page > 1:
                break
            t = p.text.strip()
            if p.kind == "heading" and (KNOWN_SECTIONS.match(t) or re.match(r"^\d+\.?\s+[A-Z]", t)):
                boundary = i
                break
        if boundary is None:
            boundary = next((i for i, p in enumerate(paras) if p.page > 1 or (p.page == 1 and p.kind == "heading")), len(paras))
        abstract_started = False
        for p in paras[:boundary]:
            if p.kind in ("title", "figure", "table"):
                continue
            affil = len(re.findall(r"(?i)universit|institut|department|centre|center|hospital|laborator|school of|faculty", p.text))
            if abstract_started and p.kind == "text":
                continue  # body text that precedes the first heading (e.g. an unlabelled introduction)
            if re.match(r"(?i)^abstract\b", p.text.strip()) or (len(p.text) >= 500 and affil < 2) \
                    or (abstract_started and p.kind == "text" and len(p.text) > 200 and affil < 2 and False):
                p.kind = "abstract"
                abstract_started = True
            elif p.kind in ("text", "heading"):
                p.kind = "front"

        ranks = sorted(heading_sizes, reverse=True)
        for p in paras:
            if p.kind != "heading":
                continue
            t = p.text.strip()
            m = re.match(r"^(\d+(?:\.\d+)*)\.?\s", t)
            if m:
                p.level = min(3, m.group(1).count(".") + 1)
            else:
                big = [s for s in ranks if s >= body + 1.0]
                p.level = min(3, (big.index(p.size) + 1) if p.size in big else len(big) + 1)

        # display equations: mostly math-font lines; merge stacked pieces (fractions, multi-line)
        def mathfrac(p):
            nchar = sum(len(l.text) for l in p.lines)
            return sum(l.math * len(l.text) for l in p.lines) / max(1, nchar), nchar
        out: list[Para] = []
        for p in paras:
            if p.kind == "text" and p.lines:
                mf, nchar = mathfrac(p)
                if mf > 0.3 and nchar < 400:
                    p.kind = "equation"
            prev = out[-1] if out else None
            if prev is not None and prev.page == p.page and p.kind in ("equation", "text") and prev.kind == "equation" \
                    and p.bbox[1] - prev.bbox[3] < 1.6 * self.body_size and (p.kind == "equation" or len(p.text) < 30):
                out[-1] = Para(prev.lines + p.lines, p.page, kind="equation")
                continue
            if prev is not None and prev.page == p.page and p.kind == "equation" and prev.kind == "text" \
                    and len(prev.text) < 30 and p.bbox[1] - prev.bbox[3] < 1.0 * self.body_size \
                    and not re.search(r"[a-z]{4}", prev.text):
                out[-1] = Para(prev.lines + p.lines, p.page, kind="equation")
                continue
            out.append(p)
        for p in out:
            if p.kind == "equation":
                rect = p.bbox
                p.asset = self.save_clip(p.page, rect, f"p{p.page + 1}-eq{int(rect[1])}")
        return out

    def styled(self, p: Para) -> str:
        """Paragraph text as Markdown with italics/bold/superscripts."""
        segs = []
        for li, l in enumerate(p.lines):
            for s in l.spans:
                st = ("i" if s["flags"] & 2 or re.search(r"ital|oblique", s["font"], re.I) else "") + \
                     ("b" if s["flags"] & 16 or re.search(r"bold", s["font"], re.I) else "") + \
                     ("^" if s["flags"] & 1 else "")
                segs.append([s["text"], st])
            segs.append(["\n", ""])
        # merge
        merged = []
        for t, st in segs:
            if merged and (merged[-1][1] == st or t.strip() == "") and t != "\n":
                merged[-1][0] += t
            else:
                merged.append([t, st])
        para_bold = all(st in ("b", "bi", "ib", "") for _, st in merged) and sum(len(t) for t, st in merged if "b" in st) > 0.85 * len(p.text)
        out = []
        for t, st in merged:
            if t == "\n":
                out.append("\n")
                continue
            lead = t[:len(t) - len(t.lstrip())]
            trail = t[len(t.rstrip()):]
            core = md_escape(t.strip())
            if not core:
                out.append(t)
                continue
            if "^" in st and len(core) < 25:
                core = "^" + core.replace(" ", "\\ ") + "^"
            else:
                if "b" in st and not para_bold:
                    core = f"**{core}**"
                if "i" in st:
                    core = f"*{core}*"
            out.append(lead + core + trail)
        txt = "".join(out)
        return md_para_start(join_lines(txt.split("\n")))


def pdf_meta_guess(an: PdfAnalyzer, paras: list[Para]) -> Meta:
    m = Meta()
    md = an.doc.metadata or {}
    t = (md.get("title") or "").strip()
    if len(t) > 12 and not re.search(r"(?i)microsoft word|untitled|\.docx?$|\.pdf$|^paper$", t):
        m.title = t
    title_p = next((p for p in paras if p.kind == "title"), None)
    if not m.title and title_p:
        m.title = title_p.text
    a = (md.get("author") or "").strip()
    if a and not re.search(r"(?i)administrator|user|owner|^admin", a):
        m.authors = [x.strip() for x in re.split(r";|,|\band\b", a) if x.strip()]
    if not m.authors:
        fronts = [p for p in paras if p.page == 0 and p.kind == "front"]
        for p in fronts[:3]:
            raw = "".join(s["text"] for l in p.lines for s in l.spans if not s["flags"] & 1)
            raw = re.sub(r"[\d*†‡§¶#✉]+", "", raw)
            parts = [x.strip() for x in re.split(r",|\band\b|&|;", raw) if x.strip()]
            good = [x for x in parts if 1 < len(x.split()) <= 5 and all(w[:1].isupper() for w in x.split() if w[0].isalpha())]
            if len(parts) >= 1 and len(good) >= max(1, 0.7 * len(parts)):
                m.authors = good
                break
    dm = re.search(r"(?:D:)?(\d{4})(\d{2})?(\d{2})?", md.get("creationDate") or "")
    if dm:
        m.date = dm.group(1)
    first_text = " ".join(p.text for p in paras[:40] if p.page < 2)
    dd = DOI_RE.search(first_text)
    if dd:
        m.doi = dd.group(1).rstrip(".,;)")
    return m


def _similar(a: str, b: str) -> bool:
    na, nb = re.sub(r"\W+", "", a.lower()), re.sub(r"\W+", "", b.lower())
    return bool(na) and bool(nb) and (na == nb or (len(na) > 20 and (na in nb or nb in na)))


def pdf_heuristic_markdown(pdf: Path, work: Path) -> tuple[str, Meta, list]:
    an = PdfAnalyzer(pdf, work)
    paras = an.analyze()
    meta = pdf_meta_guess(an, paras)
    out = []
    # merge paragraphs split across columns/pages
    merged: list[Para] = []
    for p in paras:
        if (merged and p.kind == "text" and merged[-1].kind == "text" and p.text[:1].islower()
                and not merged[-1].text.rstrip().endswith((".", "?", "!", ":"))):
            merged[-1] = Para(merged[-1].lines + p.lines, merged[-1].page)
            continue
        merged.append(p)
    pending_caption: dict = {}
    # attach captions to the nearest figure/table on the same page
    for i, p in enumerate(merged):
        if p.kind in ("figure", "table"):
            best, bd = None, 1e9
            for j in range(max(0, i - 4), min(len(merged), i + 5)):
                c = merged[j]
                if c.kind == "caption" and c.page == p.page and j not in pending_caption.values():
                    is_tab = re.match(r"(?i)^(supplementary\s+)?table", c.text)
                    if (p.kind == "table") != bool(is_tab):
                        continue
                    d = min(abs(c.bbox[1] - p.bbox[3]), abs(p.bbox[1] - c.bbox[3]))
                    if d < bd and d < 120:
                        best, bd = j, d
            if best is not None:
                pending_caption[i] = best
    used_caps = set(pending_caption.values())
    abstract_done = any(p.kind == "heading" and p.text.strip().lower().startswith("abstract") for p in merged[:30])
    has_intro = any(p.kind == "heading" and re.search(r"(?i)introduction|background", p.text) for p in merged)
    intro_done, last_kind = False, None
    for i, p in enumerate(merged):
        if p.kind in ("figure", "table"):
            cap = an.styled(merged[pending_caption[i]]) if i in pending_caption else ""
            rel = Path(p.asset).as_posix()
            out.append(f"![{cap}]({rel})\n" if cap else f"![]({rel}){{.unlabelled}}\n")
        elif i in used_caps:
            continue
        elif p.kind == "title":
            continue
        elif p.kind == "heading":
            out.append("#" * p.level + " " + md_escape(p.text.strip()) + "\n")
        elif p.kind == "front" and meta.title and _similar(p.text, meta.title):
            continue
        elif p.kind == "front":
            out.append("::: frontmatter\n" + an.styled(p) + "\n:::\n")
        elif p.kind == "abstract":
            if not abstract_done:
                out.append("# Abstract\n")
                abstract_done = True
            out.append(re.sub(r"^(\*\*)?Abstract(\*\*)?[.:]?(\*\*)?\s*", "", an.styled(p), flags=re.I) + "\n")
        elif p.kind == "equation":
            wem = (p.bbox[2] - p.bbox[0] + 8) / max(an.body_size, 6) * 0.95
            out.append(f"![]({Path(p.asset).as_posix()}){{.equation width={wem:.1f}em}}\n")
        elif p.kind == "caption":
            out.append(f"::: caption\n{an.styled(p)}\n:::\n")
        else:
            if abstract_done and not intro_done and last_kind == "abstract" and not has_intro:
                out.append("# Introduction\n")
            intro_done = intro_done or last_kind == "heading"
            out.append(an.styled(p) + "\n")
        last_kind = p.kind if p.kind not in ("front",) else last_kind
    md = "\n".join(out)
    md = re.sub(r"(:::\n)\n::: frontmatter\n", r"\n", md)  # merge consecutive front blocks
    notes = ["PDF converted with heuristic layout analysis (no LLM). Equations and tables are kept as images."]
    return md, meta, notes


# --------------------------------------------------------------------------- #
# PDF -> Markdown with Claude (vision, page by page)
# --------------------------------------------------------------------------- #
LLM_SYSTEM = """You transcribe pages of scientific papers into clean Pandoc Markdown for an e-book.
Rules:
- Output ONLY the Markdown for this page's main content, in correct reading order (handle multi-column layouts).
- Omit running headers/footers, page numbers, line numbers, journal stamps, watermarks and preprint notices.
- On the first page, omit the title, author list, affiliations and correspondence details (they go in metadata),
  but DO keep the abstract under a heading '# Abstract'.
- Headings: '#' for main sections (Introduction, Results, Methods...), '##' for subsections, '###' below that.
  Drop section numbers only if they are purely decorative; keep them otherwise.
- Join hyphenated words split across lines. Keep paragraphs as single lines separated by blank lines.
- Math: write inline math as $...$ and display equations as $$...$$ using LaTeX. Keep equation numbers as \\tag{n}.
- Use *italic* and **bold** as in the original. Citation superscripts like ¹² become ^1,2^. Keep bracketed citations as text.
- Figures and tables provided as image crops are listed below with IDs. Where one appears, write a separate paragraph
  '[[FIG:<id>]] <full caption text>' (caption in Markdown, starting with its label, e.g. 'Figure 2. ...').
  If a table is simple (<= 7 columns, no merged cells), you MAY transcribe it as a Markdown pipe table followed by
  'Table: <caption>' instead of using its ID. Never invent IDs.
- If a caption appears on this page but its figure is not in the list, write it as '[[CAPTION]] <caption text>'.
- If the page starts in the middle of a paragraph continued from the previous page, begin your output with '<<CONT>>'.
- Reference lists: one reference per paragraph, keep numbering.
- Do not summarise, add commentary, or wrap the output in code fences."""


def pdf_llm_markdown(pdf: Path, work: Path, model: str, workers: int = 6) -> tuple[str, Meta, list]:
    import anthropic
    client = anthropic.Anthropic(max_retries=5)
    an = PdfAnalyzer(pdf, work)
    paras = an.analyze()  # gives us figure/table crops + fallback text + metadata guess
    meta = pdf_meta_guess(an, paras)
    by_page = collections.defaultdict(list)
    for p in paras:
        by_page[p.page].append(p)

    def page_job(pno: int) -> str:
        page = an.doc[pno]
        zoom = min(1568 / max(page.rect.width, page.rect.height), 2.2)
        pix = page.get_pixmap(matrix=an.pm.Matrix(zoom, zoom))
        img_b64 = base64.b64encode(pix.tobytes("png")).decode()
        regions = an.regions.get(pno, [])
        W, H = page.rect.width, page.rect.height
        items = []
        for i, g in enumerate(regions):
            r = g["rect"]
            items.append(f"- id `{Path(g['path']).stem}` ({g['kind']}) at x={r[0] / W:.2f}-{r[2] / W:.2f}, "
                         f"y={r[1] / H:.2f}-{r[3] / H:.2f} of the page")
        hint = "\n".join(p.text for p in by_page.get(pno, []) if p.kind not in ("figure", "table"))[:12000]
        prompt = (f"Page {pno + 1} of {len(an.doc)}.\n\nImage crops available on this page:\n"
                  + ("\n".join(items) if items else "(none)")
                  + "\n\nRaw text extracted from the PDF (may have ordering/symbol errors; use it to get spelling right):\n<raw>\n"
                  + hint + "\n</raw>")
        msg = client.messages.create(
            model=model, max_tokens=12000, system=LLM_SYSTEM,
            messages=[{"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": img_b64}},
                {"type": "text", "text": prompt}]}])
        return "".join(b.text for b in msg.content if getattr(b, "type", "") == "text").strip()

    pages_md: list[str] = [""] * len(an.doc)
    log.info("  sending %d pages to %s ...", len(an.doc), model)
    with cf.ThreadPoolExecutor(max_workers=workers) as ex:
        futs = {ex.submit(page_job, i): i for i in range(len(an.doc))}
        for f in cf.as_completed(futs):
            i = futs[f]
            try:
                pages_md[i] = f.result()
            except Exception as e:
                log.warning("  page %d failed (%s); using heuristic text", i + 1, e)
                pages_md[i] = "\n\n".join(an.styled(p) for p in by_page.get(i, []) if p.kind == "text")
            log.info("  page %d/%d done", i + 1, len(an.doc))

    asset_by_id = {Path(g["path"]).stem: g["path"] for regs in an.regions.values() for g in regs}
    out = ""
    used = set()
    for md in pages_md:
        md = re.sub(r"^```(?:markdown|md)?\s*|\s*```$", "", md.strip())
        if md.startswith("<<CONT>>"):
            md = md[len("<<CONT>>"):].lstrip()
            out = out.rstrip()
            if re.search(r"[A-Za-z]-$", out) and md[:1].islower():
                out = out[:-1] + md
            else:
                out += " " + md
        else:
            out += "\n\n" + md
        out += "\n"

    def fig(m):
        fid, cap = m.group(1).strip("` "), m.group(2).strip()
        path = asset_by_id.get(fid)
        if not path:
            return f"::: caption\n{cap}\n:::" if cap else ""
        used.add(fid)
        return f"![{cap}]({Path(path).as_posix()})" if cap else f"![]({Path(path).as_posix()}){{.unlabelled}}"
    out = re.sub(r"^\[\[FIG:([^\]]+)\]\][ \t]*(.*)$", fig, out, flags=re.M)
    out = re.sub(r"^\[\[CAPTION\]\][ \t]*(.*)$", lambda m: f"::: caption\n{m.group(1)}\n:::", out, flags=re.M)
    # metadata from page 1
    try:
        first = "\n".join(p.text for p in by_page.get(0, []))[:6000]
        r = client.messages.create(model=model, max_tokens=1500, messages=[{"role": "user", "content":
            "Extract bibliographic metadata from this first page of a paper. Reply with JSON only: "
            '{"title": str, "authors": [str], "date": str, "doi": str, "journal": str}. Use "" when unknown.\n\n' + first}])
        txt = "".join(b.text for b in r.content if getattr(b, "type", "") == "text")
        j = json.loads(re.search(r"\{.*\}", txt, re.S).group(0))
        meta = Meta(title=j.get("title", ""), authors=j.get("authors") or [], date=j.get("date", ""),
                    doi=j.get("doi", ""), journal=j.get("journal", "")).merge(meta)
    except Exception as e:
        log.info("  LLM metadata step failed (%s); using heuristics", e)
    notes = [f"PDF transcribed page-by-page with {model}; figures/tables cropped from the PDF."]
    unused = len(asset_by_id) - len(used)
    if unused > 0:
        notes.append(f"{unused} detected image region(s) were not placed by the model (often decorative or duplicates).")
    return out, meta, notes


def markdown_to_ast(md: str, work: Path) -> dict:
    f = work / "paper.md"
    f.write_text(md, encoding="utf-8")
    return pandoc_to_ast(f, "markdown+tex_math_dollars+superscript+subscript+pipe_tables+fenced_divs+implicit_figures-citations",
                         cwd=work)


# --------------------------------------------------------------------------- #
# Post-processing: images, abstract, metadata
# --------------------------------------------------------------------------- #
def normalise_images(ast: dict, search_dirs: list[Path], out_dir: Path, max_px: int = 1600) -> tuple[int, int]:
    """Resolve, convert (tif/pdf/eps -> png/jpg) and downscale every image; drop unresolvable ones."""
    from PIL import Image
    import pymupdf
    out_dir.mkdir(parents=True, exist_ok=True)
    cache: dict[str, Optional[str]] = {}
    stats = [0, 0]

    def locate(target: str) -> Optional[Path]:
        p = Path(urllib.parse.unquote(target))
        cands = [p] if p.is_absolute() else [d / p for d in search_dirs]
        for c in list(cands):
            if c.suffix == "":
                cands += [c.with_suffix(e) for e in (".png", ".pdf", ".jpg", ".jpeg", ".eps", ".tif", ".tiff", ".svg")]
        for c in cands:
            if c.is_file():
                return c
        return None

    def convert(src: Path) -> Optional[str]:
        ext = src.suffix.lower()
        base = out_dir / f"img{len(cache):03d}-{slug(src.stem, 30)}"
        try:
            if ext == ".svg":
                dst = base.with_suffix(".svg")
                shutil.copy(src, dst)
                return str(dst)
            if ext in (".pdf", ".eps", ".ps"):
                if ext != ".pdf":
                    if not shutil.which("gs"):
                        return None
                    pdfp = base.with_suffix(".pdf")
                    subprocess.run(["gs", "-q", "-dNOPAUSE", "-dBATCH", "-dEPSCrop", "-sDEVICE=pdfwrite",
                                    f"-sOutputFile={pdfp}", str(src)], check=True, timeout=120)
                    src = pdfp
                d = pymupdf.open(src)
                pg = d[0]
                zoom = min(3.0, max_px / max(pg.rect.width, 1))
                dst = base.with_suffix(".png")
                pg.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom), alpha=False).save(dst)
                crop_whitespace(dst, pad=6)
                return str(dst)
            im = Image.open(src)
            im.load()
            if im.mode in ("P", "LA", "RGBA", "I;16", "I", "F", "CMYK", "1", "L") and ext in (".tif", ".tiff", ".bmp", ".webp"):
                im = im.convert("RGBA" if "A" in im.mode else "RGB")
            if max(im.size) > max_px:
                im.thumbnail((max_px, max_px))
            photo = im.mode == "RGB" and (src.stat().st_size > 400_000 or ext in (".jpg", ".jpeg", ".tif", ".tiff"))
            if photo:
                dst = base.with_suffix(".jpg")
                im.convert("RGB").save(dst, quality=85, optimize=True)
            else:
                dst = base.with_suffix(".png")
                if im.mode not in ("RGB", "RGBA", "L", "P"):
                    im = im.convert("RGBA")
                im.save(dst, optimize=True)
            return str(dst)
        except Exception as e:
            log.debug("image conversion failed for %s: %s", src, e)
            return None

    def fn(node):
        if node["t"] != "Image":
            return None
        target = node["c"][2][0]
        if target.startswith(("http://", "https://", "data:")):
            stats[1] += 1
            return {"t": "Emph", "c": text_inlines("[image omitted]")}
        if target not in cache:
            loc = locate(target)
            cache[target] = convert(loc) if loc else None
        new = cache[target]
        if not new:
            stats[1] += 1
            return {"t": "Emph", "c": text_inlines(f"[image not available: {Path(target).name}]")}
        stats[0] += 1
        node["c"][2][0] = new
        return None

    ast["blocks"] = walk(ast["blocks"], fn)
    return stats[0], stats[1]


def meta_value(v):
    if isinstance(v, list):
        return {"t": "MetaList", "c": [meta_value(x) for x in v]}
    return {"t": "MetaInlines", "c": text_inlines(str(v))}


def finalise_ast(ast: dict, meta: Meta) -> None:
    m = ast.setdefault("meta", {})

    # unwrap section-like Divs containing headers: pandoc's EPUB splitter only sees top-level headers
    def unwrap(node):
        if node["t"] == "Div" and any(isinstance(b, dict) and b.get("t") == "Header" for b in node["c"][1]):
            blocks = node["c"][1]
            div_id = node["c"][0][0]
            for b in blocks:
                if b.get("t") == "Header":
                    if div_id and not b["c"][1][0]:
                        b["c"][1][0] = div_id
                    break
            return _Splice(blocks)
        return None
    ast["blocks"] = walk(ast["blocks"], unwrap)

    # clean TeX math that MathML conversion chokes on (labels, trailing line breaks)
    def cleanmath(n):
        if n["t"] == "Math":
            t = n["c"][1]
            t = re.sub(r"\\label\{[^}]*\}|\\nonumber|\\notag", "", t)
            t = re.sub(r"(\\\\\s*)+$", "", t.strip())
            n["c"][1] = t.strip()
        return None
    walk(ast["blocks"], cleanmath)
    # every header needs a unique id, otherwise pandoc's EPUB nav gets dangling links
    seen = set()

    def hid(node):
        if node["t"] == "Header":
            ident = node["c"][1][0] or slug(stringify(node), 40) or "section"
            base, i = ident, 1
            while ident in seen:
                i += 1
                ident = f"{base}-{i}"
            seen.add(ident)
            node["c"][1][0] = ident
        return None
    # collect existing non-header ids first so header ids do not collide with figure/ref ids
    walk(ast["blocks"], lambda n: seen.add(n["c"][0][0]) if n["t"] in ("Div", "Span", "Figure", "Table", "CodeBlock")
         and isinstance(n["c"], list) and n["c"] and isinstance(n["c"][0], list) and n["c"][0] and n["c"][0][0] else None)
    ast["blocks"] = walk(ast["blocks"], hid)

    # pandoc 3.1's EPUB writer does not rewrite cross-chapter links to Figure/Table ids: move ids onto a Div
    def figdiv(n):
        if n["t"] in ("Figure", "Table") and n["c"][0][0]:
            ident = n["c"][0][0]
            n["c"][0][0] = ""
            return {"t": "Div", "c": [[ident, [], []], [n]]}
        return None
    ast["blocks"] = walk(ast["blocks"], figdiv)

    # drop internal links whose target id does not exist (keeps epubcheck happy)
    ids = set()

    def collect(n):
        c = n.get("c")
        if isinstance(c, list) and c and isinstance(c[0], list) and len(c[0]) == 3 and isinstance(c[0][0], str):
            ids.add(c[0][0])
        if n["t"] == "Header":
            ids.add(n["c"][1][0])
        return None
    walk(ast["blocks"], collect)

    def fixlink(n):
        if n["t"] == "Link":
            tgt = n["c"][2][0]
            if tgt.startswith("#") and tgt[1:] not in ids:
                return _Splice(n["c"][1])
        return None
    ast["blocks"] = walk(ast["blocks"], fixlink)
    # move abstract from metadata into the body if the body has none
    has_abs_heading = any(b["t"] == "Header" and stringify(b).lower().startswith(("abstract", "summary"))
                          for b in ast["blocks"][:60])
    abs_blocks = None
    if "abstract" in m:
        v = m.pop("abstract")
        if v["t"] == "MetaBlocks":
            abs_blocks = v["c"]
        elif v["t"] == "MetaInlines":
            abs_blocks = [{"t": "Para", "c": v["c"]}]
    elif meta.abstract:
        abs_blocks = [{"t": "Para", "c": text_inlines(meta.abstract)}]
    if abs_blocks and not has_abs_heading:
        ast["blocks"] = [{"t": "Header", "c": [1, ["abstract", ["abstract"], []], text_inlines("Abstract")]}] \
            + abs_blocks + ast["blocks"]
    if meta.title:
        m["title"] = meta_value(meta.title)
    if meta.authors:
        m["author"] = meta_value(meta.authors)
    if meta.date:
        m["date"] = meta_value(meta.date)
    if meta.journal:
        m["publisher"] = meta_value(meta.journal)
    if meta.doi:
        m["identifier"] = {"t": "MetaList", "c": [{"t": "MetaMap", "c": {
            "scheme": meta_value("DOI"), "text": meta_value("doi:" + meta.doi)}}]}
    if meta.license:
        m["rights"] = meta_value(meta.license)
    if meta.keywords:
        m["subject"] = meta_value(meta.keywords)
    m.setdefault("lang", meta_value("en"))
    m.pop("bibliography", None)
    # pandoc's epub title page shows these; empty title breaks epubcheck
    if "title" not in m:
        m["title"] = meta_value("Untitled paper")
    if meta.abstract and "description" not in m:
        m["description"] = meta_value(meta.abstract[:1500])


# --------------------------------------------------------------------------- #
# Cover + EPUB build
# --------------------------------------------------------------------------- #
def make_cover(meta: Meta, out: Path) -> Path:
    from PIL import Image, ImageDraw, ImageFont
    W, H = 1200, 1800
    im = Image.new("RGB", (W, H), "#f6f3ec")
    d = ImageDraw.Draw(im)

    def font(names, size):
        for n in names:
            for base in ("/usr/share/fonts/truetype/dejavu/", "/Library/Fonts/", "/System/Library/Fonts/Supplemental/",
                         "C:/Windows/Fonts/", ""):
                try:
                    return ImageFont.truetype(base + n, size)
                except Exception:
                    continue
        return ImageFont.load_default()
    serif_b = ["DejaVuSerif-Bold.ttf", "Georgia Bold.ttf", "georgiab.ttf"]
    sans = ["DejaVuSans.ttf", "Arial.ttf", "arial.ttf"]
    d.rectangle([0, 0, W, 360], fill="#1f4e5f")
    top = (meta.journal or "Scientific paper").upper()
    year = (meta.date or "")[:4]
    f_top = font(sans, 38)
    d.text((90, 150), top[:48], font=f_top, fill="#e8eef0")
    if year:
        d.text((90, 215), year, font=font(sans, 34), fill="#a9c3cc")
    title = meta.title or "Untitled paper"
    size = 82
    while size > 40:
        f_t = font(serif_b, size)
        wrapped = textwrap.wrap(title, width=max(10, int(1020 / (size * 0.55))))
        if len(wrapped) * size * 1.22 < 820:
            break
        size -= 6
    y = 460
    for line in wrapped[:12]:
        d.text((90, y), line, font=f_t, fill="#1d1d1f")
        y += int(size * 1.22)
    d.line([90, y + 30, 330, y + 30], fill="#c2703d", width=6)
    y += 80
    authors = meta.authors[:8]
    a = ", ".join(authors) + (" et al." if len(meta.authors) > 8 else "")
    f_a = font(sans, 36)
    for line in textwrap.wrap(a, width=48)[:6]:
        d.text((90, y), line, font=f_a, fill="#3a3a3c")
        y += 50
    if meta.doi:
        d.text((90, H - 130), "doi:" + meta.doi, font=font(sans, 28), fill="#6e6e73")
    im.save(out)
    return out


CSS = """
body { font-family: serif; line-height: 1.45; margin: 0 0.4em; }
h1, h2, h3, h4 { font-family: sans-serif; line-height: 1.25; page-break-after: avoid; }
h1 { font-size: 1.35em; margin-top: 1.6em; }
h2 { font-size: 1.15em; margin-top: 1.3em; }
h3 { font-size: 1.0em; margin-top: 1.1em; }
p { margin: 0 0 0.7em 0; text-align: left; }
figure, .figure { margin: 1.2em 0; text-align: center; page-break-inside: avoid; }
figure img, .figure img { max-width: 100%; height: auto; }
figcaption, .caption, p.caption { font-size: 0.85em; text-align: left; line-height: 1.35; margin-top: 0.4em; }
img { max-width: 100%; height: auto; }
img.math { vertical-align: middle; }
img.equation { display: block; margin: 0.8em auto; }
.equation { text-align: center; margin: 0.8em 0; }
.equation img { max-width: 100%; height: auto; }
.eqlabel { float: right; }
math[display="block"] { display: block; margin: 0.6em auto; max-width: 100%; overflow-x: auto; }
table { border-collapse: collapse; font-size: 0.75em; margin: 0.6em 0; display: block; overflow-x: auto; }
th, td { border-bottom: 1px solid #bbb; padding: 0.2em 0.4em; vertical-align: top; text-align: left; }
thead th { border-bottom: 2px solid #555; }
.table-wrap { margin: 1.2em 0; }
.tablefoot { font-size: 0.75em; }
.frontmatter { font-size: 0.82em; color: #444; }
.box { border: 1px solid #ccc; padding: 0.5em 0.8em; margin: 1em 0; background: #f7f7f7; }
.references, #refs, .refs { font-size: 0.85em; }
ol.refs li, #refs > div { margin-bottom: 0.45em; }
.csl-entry { margin-bottom: 0.45em; padding-left: 1.5em; text-indent: -1.5em; }
.missing, .unlabelled { color: #888; font-style: italic; }
.smallcaps { font-variant: small-caps; }
a { color: inherit; }
pre, code { font-size: 0.85em; white-space: pre-wrap; }
nav#toc ol { list-style-type: none; }
"""


def build_epub(ast: dict, meta: Meta, out: Path, work: Path, cover: bool = True) -> None:
    jf = work / "final.json"
    jf.write_text(json.dumps(ast))
    css = work / "style.css"
    css.write_text(CSS)
    cmd = ["pandoc", str(jf), "-f", "json", "-t", "epub3", "-o", str(out), "--mathml", "--toc", "--toc-depth=2",
           "--split-level=1", "--css", str(css), "--resource-path", str(work)]
    if cover:
        cmd += ["--epub-cover-image", str(make_cover(meta, work / "cover.png"))]
    p = run(cmd, timeout=600)
    if p.stderr.strip():
        log.debug("pandoc epub: %s", p.stderr.strip()[:3000])
    fix_epub_markup(out)


def fix_epub_markup(path: Path) -> None:
    """Remove MathML attributes texmath emits that EPUB's schema rejects (e.g. displaystyle on mfrac)."""
    bad = re.compile(r'(<(?!math\b|mstyle\b|mtable\b)m[a-z]+\b[^>]*?)\s+displaystyle="[^"]*"')
    tmp = path.with_suffix(".tmp.epub")
    changed = False
    with zipfile.ZipFile(path) as zin, zipfile.ZipFile(tmp, "w") as zout:
        for item in zin.infolist():
            data = zin.read(item.filename)
            if item.filename.endswith(".xhtml") and b"displaystyle" in data:
                txt = data.decode("utf-8")
                new = txt
                while True:
                    nxt = bad.sub(r"\1", new)
                    if nxt == new:
                        break
                    new = nxt
                if new != txt:
                    data, changed = new.encode("utf-8"), True
            ctype = zipfile.ZIP_STORED if item.filename == "mimetype" else zipfile.ZIP_DEFLATED
            zout.writestr(item, data, compress_type=ctype)
    if changed:
        tmp.replace(path)
    else:
        tmp.unlink()


def epubcheck(path: Path) -> Optional[list[str]]:
    try:
        from epubcheck import EpubCheck
    except Exception:
        return None
    try:
        r = EpubCheck(str(path))
        return [f"{m.level}: {m.message} ({m.location})" for m in r.messages if m.level in ("ERROR", "FATAL")]
    except Exception as e:
        log.debug("epubcheck failed: %s", e)
        return None


# --------------------------------------------------------------------------- #
# Orchestration
# --------------------------------------------------------------------------- #
def convert(inp: str, out: Optional[Path] = None, llm: str = "auto", model: str = DEFAULT_MODEL,
            workdir: Optional[Path] = None, keep: bool = False, cover: bool = True,
            force: Optional[str] = None, validate: bool = True) -> dict:
    """Convert a paper to EPUB. Returns a report dict."""
    tmp = Path(tempfile.mkdtemp(prefix="paper2epub-")) if workdir is None else Path(workdir)
    tmp.mkdir(parents=True, exist_ok=True)
    use_llm = llm == "on" or (llm == "auto" and os.environ.get("ANTHROPIC_API_KEY"))
    if use_llm:
        try:
            import anthropic  # noqa: F401
        except ImportError:
            if llm == "on":
                raise ConversionError("--llm on needs the 'anthropic' package")
            use_llm = False
    report = {"input": inp, "attempts": [], "notes": []}
    try:
        ast = meta = None
        for label, thunk in candidates(inp, tmp):
            log.info("Trying %s", label)
            try:
                src = thunk()
                if force and src.kind != force:
                    raise ConversionError(f"skipped (forced {force})")
                if src.kind == "jats":
                    ast, m2, notes = jats_to_ast(src, tmp)
                    search = src.extra.get("asset_dirs", []) + [src.path.parent, tmp]
                elif src.kind == "latex":
                    ast, m2, notes = latex_to_ast(src, tmp)
                    search = src.extra.get("image_dirs", [src.path.parent]) + [tmp]
                elif src.kind == "pdf":
                    if use_llm:
                        md, m2, notes = pdf_llm_markdown(src.path, tmp, model)
                    else:
                        md, m2, notes = pdf_heuristic_markdown(src.path, tmp)
                    md_meta = src.meta.merge(m2)
                    yaml_head = "---\n" + "".join(
                        f"{k}: {json.dumps(v, ensure_ascii=False)}\n" for k, v in
                        (("title", md_meta.title), ("author", md_meta.authors), ("date", md_meta.date),
                         ("doi", md_meta.doi), ("journal", md_meta.journal)) if v) + "---\n\n"
                    ast = markdown_to_ast(yaml_head + md, tmp)
                    search = [tmp]
                elif src.kind == "markdown":
                    ast = markdown_to_ast(src.path.read_text(encoding="utf-8"), tmp)
                    m2, notes = meta_from_ast(ast), []
                    search = [src.path.parent, tmp]
                else:
                    raise ConversionError(f"unsupported source kind {src.kind}")
                meta = src.meta.merge(m2)
                report["attempts"].append((label, "ok"))
                report["source"] = src.label or label
                report["notes"] += notes
                break
            except Exception as e:
                log.info("  -> %s", e)
                log.debug("traceback", exc_info=True)
                report["attempts"].append((label, f"failed: {e}"))
        if ast is None:
            raise ConversionError("No usable source found:\n" + "\n".join(f"  {a}: {b}" for a, b in report["attempts"]))

        ok, missing = normalise_images(ast, search, tmp / "img")
        report["images"] = ok
        if missing:
            report["notes"].append(f"{missing} image(s) missing or remote; replaced with a placeholder.")
        finalise_ast(ast, meta)
        if out is None:
            first = (meta.authors[0].split()[-1] if meta.authors else "paper")
            out = Path(f"{slug(first, 20)}-{(meta.date or '')[:4] or 'nd'}-{slug(meta.title, 50)}.epub")
        out = Path(out)
        build_epub(ast, meta, out, tmp, cover=cover)
        report["output"] = str(out)
        report["title"] = meta.title
        report["words"] = len(stringify(ast["blocks"]).split())
        if validate:
            errs = epubcheck(out)
            report["epubcheck"] = "not run" if errs is None else (errs or "valid")
        return report
    finally:
        if not keep and workdir is None:
            shutil.rmtree(tmp, ignore_errors=True)
        elif keep:
            report["workdir"] = str(tmp)


def main(argv=None):
    ap = argparse.ArgumentParser(prog="paper2epub", description=__doc__.split("\n\n")[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter,
                                 epilog=textwrap.dedent("""\
        examples:
          paper2epub 10.7554/eLife.09560
          paper2epub 2106.01234            # arXiv ID
          paper2epub PMC7042881
          paper2epub paper.pdf -o paper.epub
          paper2epub ./latex-source/       # folder with main.tex
        env:
          ANTHROPIC_API_KEY   enables Claude page transcription for PDFs (--llm auto)
          PAPER2EPUB_MODEL    model id (default %s)
          PAPER2EPUB_EMAIL    contact e-mail for Unpaywall / polite API use
        """ % DEFAULT_MODEL))
    ap.add_argument("input", help="DOI, arXiv ID, PMCID, URL, or path to PDF / JATS XML / LaTeX source")
    ap.add_argument("-o", "--output", type=Path)
    ap.add_argument("--llm", choices=["auto", "on", "off"], default="auto",
                    help="use Claude to transcribe PDFs (auto = if ANTHROPIC_API_KEY is set)")
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--source", choices=["jats", "latex", "pdf"], help="force a source type")
    ap.add_argument("--no-cover", action="store_true")
    ap.add_argument("--no-validate", action="store_true")
    ap.add_argument("--keep", action="store_true", help="keep the working directory (for debugging)")
    ap.add_argument("--workdir", type=Path)
    ap.add_argument("-v", "--verbose", action="count", default=0)
    a = ap.parse_args(argv)
    logging.basicConfig(level=logging.DEBUG if a.verbose > 1 else logging.INFO, format="%(message)s")
    if not shutil.which("pandoc"):
        sys.exit("pandoc not found. Install it from https://pandoc.org/installing.html")
    try:
        rep = convert(a.input, a.output, llm=a.llm, model=a.model, workdir=a.workdir, keep=a.keep,
                      cover=not a.no_cover, force=a.source, validate=not a.no_validate)
    except ConversionError as e:
        sys.exit(f"error: {e}")
    print(f"\n✓ {rep['output']}")
    print(f"  source: {rep.get('source')}   words: {rep.get('words')}   images: {rep.get('images')}")
    for n in rep["notes"]:
        print(f"  note: {n}")
    ec = rep.get("epubcheck")
    if ec and ec != "not run":
        print("  epubcheck: " + ("valid" if ec == "valid" else f"{len(ec)} error(s)"))
        if ec != "valid":
            for e in ec[:10]:
                print("    " + e)
    if rep.get("workdir"):
        print(f"  workdir: {rep['workdir']}")


if __name__ == "__main__":
    main()
