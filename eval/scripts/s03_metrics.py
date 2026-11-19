"""Step 3: per-paper metrics of every converter output against the JATS ground truth.

Writes results/per_paper.csv (one row per paper x system) and results/pairs.csv (agreement
between two systems' outputs). Systems scored as PDF converters: swift, base, rust; the XML
reading view (xml) is scored the same way as a sanity check of the metrics (should be near-perfect).
"""
import csv
import re
import statistics
import sys
from collections import Counter, defaultdict
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import fitz
from rapidfuzz.distance import Indel, Levenshtein
from scipy.stats import kendalltau

import textutil as T
from common import CORPUS, DATA, OUT, RESULTS, load_json, save_json

SYSTEMS = ["swift", "base", "rust", "xml"]
SHINGLE = 5


# ------------------------------------------------------------------ helpers

def ids(toks, vocab):
    return [vocab.setdefault(t, len(vocab)) for t in toks]


def lcs_marks(a, b, inserts=None):
    """Boolean match marks of an LCS alignment of two token-id sequences. With `inserts` (a list),
    also collects pure insertions in b: (start, end) runs between two matched stretches."""
    ma, mb = [False] * len(a), [False] * len(b)
    ops = list(Indel.opcodes(a, b))
    for k, op in enumerate(ops):
        if op.tag == "equal":
            for i in range(op.src_end - op.src_start):
                ma[op.src_start + i] = True
                mb[op.dest_start + i] = True
        elif inserts is not None and op.tag == "insert" and 0 < k < len(ops) - 1 \
                and ops[k - 1].tag == "equal" and ops[k + 1].tag == "equal":
            inserts.append((op.dest_start, op.dest_end))
    return ma, mb


def f1(p, r):
    return 2 * p * r / (p + r) if p + r else 0.0


def pdf_margins(pdf):
    """Running headers/footers and printed page numbers of a PDF (lines in the top/bottom 8 % or
    side 6 % of the page that recur on at least two pages, digits masked)."""
    cache = pdf.parent / "margins.json"
    got = load_json(cache)
    if got is not None:
        return got
    doc = fitz.open(pdf)
    per_page = []
    pagenums = set()
    for page in doc:
        w, h = page.rect.width, page.rect.height
        found = set()
        for b in page.get_text("dict")["blocks"]:
            for l in b.get("lines", []):
                x0, y0, x1, y1 = l["bbox"]
                txt = " ".join(s["text"] for s in l["spans"]).strip()
                if not txt:
                    continue
                if y1 < 0.08 * h or y0 > 0.92 * h or x1 < 0.06 * w or x0 > 0.94 * w:
                    if re.fullmatch(r"(page\s*)?\d{1,4}(\s*(of|/)\s*\d{1,4})?", txt, re.I):
                        pagenums.add(int(re.search(r"\d+", txt).group()))
                    d = T.dense(txt)
                    if len(d) >= 8:
                        found.add(d)
        per_page.append(found)
    c = Counter(d for f in per_page for d in f)
    recurring = sorted(d for d, n in c.items() if n >= 2)
    res = {"pages": doc.page_count, "recurring": recurring, "pagenums": sorted(pagenums)}
    save_json(cache, res)
    return res


BOILER = re.compile(r"downloaded from|all rights reserved|©|\(c\) \d{4}|licensee |creative commons|"
                    r"this article is (an open access|licensed|distributed)|for personal use only|"
                    r"terms of use|legend continued on next page|continued on next page|"
                    r"\bpage \d+ of \d+\b|published by|reprints? and permissions", re.I)


# ------------------------------------------------------------------ per paper

def gt_of(pmcid):
    g = T.load_jats(CORPUS / pmcid / "article.xml")
    toks, part, para = [], [], []
    for i, (p, kind, txt) in enumerate(g["blocks"]):
        tt = T.tokens(txt)
        toks += tt
        part += [p] * len(tt)
        para += [i] * len(tt)
    g["toks"], g["part"], g["para"] = toks, part, para
    return g


def text_metrics(g, e, vocab):
    pdf_toks = [t for _, txt in e["blocks"] for t in T.tokens(txt)]
    a, b = ids(g["toks"], vocab), ids(pdf_toks, vocab)
    r = {"gt_tokens": sum(1 for p in g["part"] if p != "back"), "gt_ext_tokens": len(a), "out_tokens": len(b)}
    if not a or not b:
        return r, pdf_toks, None, None
    ins = []
    ma, mb = lcs_marks(a, b, ins)
    r["_inserts"] = ins
    main = [i for i, p in enumerate(g["part"]) if p != "back"]
    r["precision"] = sum(mb) / len(b)
    r["recall"] = sum(ma[i] for i in main) / max(1, len(main))
    r["recall_ext"] = sum(ma) / len(a)
    r["f1"] = f1(r["precision"], r["recall"])
    r["wer"] = Levenshtein.distance(a, b) / len(a)
    r["ned"] = Levenshtein.normalized_distance(a, b)
    ca, cb = Counter(a), Counter(b)
    cm = Counter(a[i] for i in main)
    r["bow_precision"] = sum((cb & ca).values()) / len(b)
    r["bow_recall"] = sum((cb & cm).values()) / max(1, len(main))
    r["bow_f1"] = f1(r["bow_precision"], r["bow_recall"])
    return r, pdf_toks, ma, mb


def order_metrics(g, pdf_toks):
    """Paragraph order via unique 5-gram shingles (independent of the LCS alignment)."""
    gt = g["toks"]
    sh_gt = Counter(tuple(gt[i:i + SHINGLE]) for i in range(len(gt) - SHINGLE + 1))
    pos_pdf = defaultdict(list)
    for i in range(len(pdf_toks) - SHINGLE + 1):
        pos_pdf[tuple(pdf_toks[i:i + SHINGLE])].append(i)
    para_hits = defaultdict(list)
    label_at = {}
    for i in range(len(gt) - SHINGLE + 1):
        s = tuple(gt[i:i + SHINGLE])
        if sh_gt[s] == 1 and len(pos_pdf.get(s, ())) == 1 and g["para"][i] == g["para"][i + SHINGLE - 1]:
            j = pos_pdf[s][0]
            para_hits[g["para"][i]].append(j)
            label_at[j] = g["para"][i]
    sizes = Counter(g["para"])
    order, positions = [], []
    eligible = 0
    for pid in sorted(sizes):
        n_sh = sizes[pid] - SHINGLE + 1
        if sizes[pid] < 15:
            continue
        eligible += 1
        hits = para_hits.get(pid, [])
        if len(hits) >= max(3, 0.3 * n_sh):
            order.append(pid)
            positions.append(statistics.median(hits))
    r = {"paras_eligible": eligible, "paras_matched": len(order)}
    if len(order) >= 3:
        tau = kendalltau(range(len(order)), positions).statistic
        r["kendall_tau"] = float(tau)
        adj = [positions[k] < positions[k + 1] for k in range(len(positions) - 1)]
        r["adjacent_ordered"] = sum(adj) / len(adj)
    # interleaving: runs of shingles from one XML paragraph; a paragraph resumed after another
    # paragraph's text has intervened counts as one interleaving event
    seq = [label_at[j] for j in sorted(label_at)]
    runs = []
    for lab in seq:
        if runs and runs[-1][0] == lab:
            runs[-1][1] += 1
        else:
            runs.append([lab, 1])
    runs = [x for x in runs if x[1] >= 3]
    merged = []
    for lab, n in runs:
        if merged and merged[-1] == lab:
            continue
        merged.append(lab)
    seen, events = set(), 0
    for lab in merged:
        if lab in seen:
            events += 1
        seen.add(lab)
    r["interleavings"] = events
    return r


def broken_words(g, e):
    vocab_tokens = T.raw_tokens(g["alltext"])
    V = set(vocab_tokens) | set(T.tokens(g["alltext"]))
    bigrams = set(zip(vocab_tokens, vocab_tokens[1:]))
    dehyph = {h.replace("-", ""): h for h in V if "-" in h}
    toks = [t for _, txt in e["blocks"] for t in T.raw_tokens(txt)]
    split = join = sp_hyph = lost_hyph = 0
    ex = defaultdict(list)
    for k, t in enumerate(toks):
        if "-" in t:
            if t not in V and t.replace("-", "").isalpha() and t.replace("-", "") in V:
                sp_hyph += 1
                ex["spurious_hyphen"].append(t)
            continue
        if t not in V and t.isalpha():
            if t in dehyph:
                lost_hyph += 1
                ex["lost_hyphen"].append(f"{t} ({dehyph[t]})")
                continue
            if len(t) >= 6 and not (k + 1 < len(toks) and (t + toks[k + 1]) in V) and \
                    not (k > 0 and (toks[k - 1] + t) in V):
                for c in range(2, len(t) - 1):
                    if (t[:c], t[c:]) in bigrams:
                        join += 1
                        ex["join"].append(f"{t} ({t[:c]} {t[c:]})")
                        break
        if k + 1 < len(toks):
            u = toks[k + 1]
            if t.isalpha() and u.isalpha() and (t not in V or u not in V) and (t + u) in V and len(t + u) >= 4:
                split += 1
                ex["split"].append(f"{t} {u}")
            elif t.isalpha() and u.isalpha() and f"{t}-{u}" in V and (t, u) not in bigrams:
                lost_hyph += 1
                ex["lost_hyphen"].append(f"{t} {u} ({t}-{u})")
    n = max(1, len(toks))
    r = {"split_per_1k": 1000 * split / n, "join_per_1k": 1000 * join / n,
         "spurious_hyphen_per_1k": 1000 * sp_hyph / n, "lost_hyphen_per_1k": 1000 * lost_hyph / n,
         "broken_per_1k": 1000 * (split + join + sp_hyph) / n,
         "n_split": split, "n_join": join, "n_spurious_hyphen": sp_hyph, "n_lost_hyphen": lost_hyph}
    return r, {k: v[:15] for k, v in ex.items()}


def boilerplate(g, e, pdf_toks, mb, margins, inserts):
    body_dense = T.dense(" ".join(txt for _, txt in e["blocks"]))
    gt_dense = T.dense(g["alltext"])
    hf = 0
    leaked = []
    for s in margins["recurring"]:
        n = body_dense.count(s) - gt_dense.count(s)
        if n > 0:
            hf += n
            leaked.append(s[:60])
    kw = sum(1 for _, txt in e["blocks"] if BOILER.search(txt))
    kw_gt = sum(1 for _, _, txt in g["blocks"] if BOILER.search(txt))
    kw = max(0, kw - kw_gt)
    pn = 0
    if mb is not None:
        pages = set(margins["pagenums"])
        # a printed page number inside running text: a pure insertion of 1-3 tokens holding it
        pn = sum(1 for (i, j) in inserts if j - i <= 3 and any(t.isdigit() and int(t) in pages for t in pdf_toks[i:j]))
        pn = min(pn, margins["pages"])
    return {"leak_headerfooter": hf, "leak_keyword_paras": kw, "leak_pagenums": pn,
            "leak_total": hf + kw + pn}, leaked[:10]


def label_num(s):
    m = re.match(r"\s*(fig(?:ure)?s?\.?|scheme|plate|chart|table|box)\s*(s?\d+[a-z]?)", T.norm(s))
    if not m:
        return None
    kind = "table" if m.group(1).startswith("table") else ("box" if m.group(1) == "box" else "fig")
    return kind, m.group(2)


def caption_score(gt_cap, out_cap, vocab):
    a = ids(T.tokens(gt_cap)[:50], vocab)
    b = ids(T.tokens(out_cap)[:80], vocab)
    if not a or not b:
        return 0.0
    ma, _ = lcs_marks(a, b)
    return sum(ma) / len(a)


def float_metrics(gt_items, out_items, vocab, kind):
    """Matches XML figures/tables to output ones by label number, else by caption text."""
    used = set()
    matched = cap_ok = 0
    for gi in gt_items:
        gl = label_num(gi["label"] + " ") if gi["label"] else None
        best, best_s = None, 0.0
        for k, oi in enumerate(out_items):
            if k in used:
                continue
            ol = label_num(oi["caption"])
            s = caption_score(gi["caption"], oi["caption"], vocab) if gi["caption"] else 0.0
            if gl and ol and gl[1] == ol[1] and ol[0] == kind:
                s = max(s, 0.99)
            if oi["has_img"] or oi.get("has_table"):
                s += 0.05           # a duplicate caption without image loses ties
            if s > best_s:
                best, best_s = k, s
        if best is not None and best_s >= 0.6:
            used.add(best)
            if out_items[best]["has_img"] or out_items[best].get("has_table"):
                matched += 1

    # caption kept: the XML caption's first 50 words found (>= 80 %, in order) in some output caption
    for gi in gt_items:
        if gi["caption"] and any(caption_score(gi["caption"], oi["caption"], vocab) >= 0.8 for oi in out_items):
            cap_ok += 1
    n_out_img = sum(1 for o in out_items if o["has_img"] or o.get("has_table"))
    out_matched_img = sum(1 for k in used if out_items[k]["has_img"] or out_items[k].get("has_table"))
    return {f"{kind}_gt": len(gt_items), f"{kind}_out": n_out_img, f"{kind}_matched": matched,
            f"{kind}_out_matched": out_matched_img, f"{kind}_caption_ok": cap_ok,
            f"{kind}_gt_with_caption": sum(1 for g in gt_items if g["caption"])}


def score(pmcid, system, g, margins, vocab):
    od = OUT / system / pmcid
    run = load_json(od / "run.json", {})
    row = {"pmcid": pmcid, "system": system, "ok": bool(run.get("ok")), "rc": run.get("rc"),
           "wall_s": run.get("wall_s"), "cpu_s": run.get("cpu_s"), "maxrss_mb": run.get("maxrss_mb"),
           "pages": margins["pages"]}
    if not row["ok"]:
        return row, None
    e = T.load_epub(od / "epub")
    tm, pdf_toks, ma, mb = text_metrics(g, e, vocab)
    inserts = tm.pop("_inserts", [])
    row.update(tm)
    row.update(order_metrics(g, pdf_toks))
    bw, bw_ex = broken_words(g, e)
    row.update(bw)
    bp, leaked = boilerplate(g, e, pdf_toks, mb, margins, inserts)
    row.update(bp)
    row.update(float_metrics(g["figs"], e["figs"], vocab, "fig"))
    row.update(float_metrics(g["tables"], e["tables"], vocab, "table"))
    row.update({"eq_gt": g["n_eq"], "eq_out": e["n_eq"], "refs_gt": g["n_refs"], "refs_out": e["n_refitems"]})
    return row, {"broken_examples": bw_ex, "leaked": leaked, "tokens": pdf_toks}


def pdffig_metrics(pmcid, g):
    """JATS XML + PDF: XML figures whose image was cut from the PDF."""
    od = OUT / "pdffig" / pmcid
    run = load_json(od / "run.json", {})
    row = {"pmcid": pmcid, "pdffig_ok": bool(run.get("ok"))}
    if not run.get("ok"):
        return row
    n_img = n_missing = 0
    for f in T.spine(od / "epub"):
        s = f.read_text(errors="replace")
        for fig in re.findall(r"<figure[^>]*>.*?</figure>", s, re.S):
            if "../images/" in fig:
                n_img += 1
            elif "missing" in fig:
                n_missing += 1
    log = (od / "log.txt").read_text(errors="replace")
    m = re.search(r"(\d+) figure image\(s\) taken from the PDF", log)
    row.update({"pdffig_gt": len(g["figs"]), "pdffig_recovered": int(m.group(1)) if m else 0,
                "pdffig_figures_with_image": n_img, "pdffig_figures_missing": n_missing,
                "pdffig_wall_s": run.get("wall_s")})
    return row


def do_paper(pmcid):
    try:
        g = gt_of(pmcid)
        margins = pdf_margins(CORPUS / pmcid / "article.pdf")
        vocab = {}
        rows, extra = [], {}
        for s in SYSTEMS:
            r, x = score(pmcid, s, g, margins, vocab)
            rows.append(r)
            extra[s] = x
        # agreement between pairs of PDF converters
        pairs = []
        for a, b in (("swift", "rust"), ("swift", "base"), ("base", "rust")):
            if extra.get(a) and extra.get(b):
                ta, tb = ids(extra[a]["tokens"], vocab), ids(extra[b]["tokens"], vocab)
                if ta and tb:
                    ma, mb = lcs_marks(ta, tb)
                    p, rr = sum(mb) / len(tb), sum(ma) / len(ta)
                    pairs.append({"pmcid": pmcid, "a": a, "b": b, "token_f1": f1(p, rr),
                                  "ned": Levenshtein.normalized_distance(ta, tb)})
        ex = {s: {k: v for k, v in (x or {}).items() if k != "tokens"} for s, x in extra.items()}
        save_json(DATA / "examples" / f"{pmcid}.json", ex)
        return rows, pairs, pdffig_metrics(pmcid, g)
    except Exception as err:
        import traceback
        traceback.print_exc()
        print(f"FAILED {pmcid}: {err}", file=sys.stderr)
        return [], [], {"pmcid": pmcid}


def main():
    corpus = list(csv.DictReader(open(RESULTS / "corpus.csv")))
    (DATA / "examples").mkdir(exist_ok=True)
    group = {r["pmcid"]: r["group"] for r in corpus}
    rows, pairs, pf = [], [], []
    with ProcessPoolExecutor(4) as ex:
        for i, (r, p, f) in enumerate(ex.map(do_paper, [c["pmcid"] for c in corpus])):
            rows += r
            pairs += p
            pf.append(f)
            if (i + 1) % 20 == 0:
                print(f"{i + 1}/{len(corpus)}", file=sys.stderr)
    for r in rows + pairs + pf:
        r["group"] = group[r["pmcid"]]
    for name, data in (("per_paper.csv", rows), ("pairs.csv", pairs), ("pdffig.csv", pf)):
        keys = []
        for r in data:
            keys += [k for k in r if k not in keys]
        with open(RESULTS / name, "w", newline="") as fh:
            w = csv.DictWriter(fh, keys)
            w.writeheader()
            for r in data:
                w.writerow({k: (round(v, 5) if isinstance(v, float) else v) for k, v in r.items()})
    print(f"wrote {len(rows)} rows", file=sys.stderr)


if __name__ == "__main__":
    main()
