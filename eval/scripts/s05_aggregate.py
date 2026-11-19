"""Step 5: aggregate per-paper results into summary statistics, tables and figures.

Reads results/{corpus,per_paper,pairs,pdffig,highlights}.csv (and timing runs in data/out/timing-*),
writes results/summary.json, results/summary_tables.md and figures/*.pdf.
Confidence intervals: percentile bootstrap over papers (B = 5000, fixed seed); paired differences
(current - baseline) are bootstrapped per paper and tested with the Wilcoxon signed-rank test.
"""
import csv
import json
from collections import Counter, defaultdict

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from scipy.stats import wilcoxon

from common import FIGS, OUT, RESULTS, SEED, load_json

B = 5000
rng = np.random.default_rng(SEED)
COL = {"swift": "#2a78d6", "rust": "#eb6834", "base": "#8a8984", "xml": "#1baf7a"}
NAME = {"swift": "Swift (current)", "base": "Swift (baseline 38d2bdc^)", "rust": "Rust (PDFium)",
        "xml": "XML reading view (control)"}
PDF_SYS = ["swift", "base", "rust"]


def read(name):
    with open(RESULTS / name) as fh:
        return list(csv.DictReader(fh))


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return np.nan


def boot(x, stat=np.mean):
    x = np.asarray([v for v in x if not np.isnan(v)])
    if len(x) == 0:
        return {"n": 0}
    idx = rng.integers(0, len(x), (B, len(x)))
    bs = stat(x[idx], axis=1)
    q1, med, q3 = np.percentile(x, [25, 50, 75])
    return {"n": int(len(x)), "mean": float(x.mean()), "median": float(med), "q1": float(q1), "q3": float(q3),
            "ci_lo": float(np.percentile(bs, 2.5)), "ci_hi": float(np.percentile(bs, 97.5)), "stat": stat.__name__}


def boot_ratio(num_, den):
    """Pooled ratio sum(num)/sum(den) with a bootstrap over papers."""
    num_, den = np.asarray(num_, float), np.asarray(den, float)
    keep = den > 0
    num_, den = num_[keep], den[keep]
    if len(den) == 0:
        return {"n": 0}
    idx = rng.integers(0, len(den), (B, len(den)))
    bs = num_[idx].sum(1) / den[idx].sum(1)
    return {"n": int(len(den)), "pooled": float(num_.sum() / den.sum()), "ci_lo": float(np.percentile(bs, 2.5)),
            "ci_hi": float(np.percentile(bs, 97.5)), "num": float(num_.sum()), "den": float(den.sum())}


def paired(a, b):
    d = np.asarray(a) - np.asarray(b)
    d = d[~np.isnan(d)]
    if len(d) == 0:
        return {"n": 0}
    idx = rng.integers(0, len(d), (B, len(d)))
    bs = d[idx].mean(1)
    try:
        p = float(wilcoxon(d).pvalue) if np.any(d != 0) else 1.0
    except ValueError:
        p = float("nan")
    return {"n": int(len(d)), "mean_diff": float(d.mean()), "median_diff": float(np.median(d)),
            "ci_lo": float(np.percentile(bs, 2.5)), "ci_hi": float(np.percentile(bs, 97.5)), "wilcoxon_p": p,
            "improved": int((d > 0).sum()), "worse": int((d < 0).sum()), "same": int((d == 0).sum())}


def fmt(s, pct=False, digits=3):
    if not s or s.get("n", 0) == 0:
        return "–"
    f = (lambda v: f"{100 * v:.1f}") if pct else (lambda v: f"{v:.{digits}f}")
    if "pooled" in s:
        return f"{f(s['pooled'])} [{f(s['ci_lo'])}, {f(s['ci_hi'])}]"
    if s.get("stat") == "median":
        return f"{f(s['median'])} [{f(s['ci_lo'])}, {f(s['ci_hi'])}] (IQR {f(s['q1'])}–{f(s['q3'])})"
    return f"{f(s['median'])} ({f(s['q1'])}–{f(s['q3'])}); mean {f(s['mean'])} [{f(s['ci_lo'])}, {f(s['ci_hi'])}]"


def main():
    corpus = {r["pmcid"]: r for r in read("corpus.csv")}
    per = read("per_paper.csv")
    by = defaultdict(dict)
    for r in per:
        by[r["system"]][r["pmcid"]] = r
    pub = [p for p, r in corpus.items() if r["group"] == "pub"]
    ms = [p for p, r in corpus.items() if r["group"] == "ms"]
    out = {"corpus": {}, "success": {}, "metrics": {}, "deltas": {}, "floats": {}, "highlights": {}, "speed": {},
           "agreement": {}, "pdffig": {}}

    # ---------------------------------------------------------- corpus
    def describe(ids):
        rows = [corpus[p] for p in ids]
        pages = [num(by["swift"].get(p, {}).get("pages")) for p in ids]
        return {"n": len(ids), "journals": len({r["journal"] for r in rows}),
                "publishers": Counter(r["publisher"] for r in rows).most_common(),
                "years": Counter(r["stratum"] for r in rows), "year_min": min(int(r["year"]) for r in rows),
                "year_max": max(int(r["year"]) for r in rows),
                "article_types": Counter(r["article_type"] for r in rows).most_common(),
                "licenses": Counter(r["license"] for r in rows).most_common(),
                "pages": boot(pages, np.median), "pages_total": float(np.nansum(pages)),
                "body_words": boot([num(r["body_words"]) for r in rows], np.median)}

    out["corpus"]["pub"] = describe(pub)
    if ms:
        out["corpus"]["ms"] = describe(ms)

    for s in PDF_SYS + ["xml", "pdffig"]:
        if s == "pdffig":
            continue
        ok = [by[s].get(p, {}).get("ok") == "True" for p in pub]
        out["success"][s] = {"ok": int(sum(ok)), "n": len(pub)}

    # paired analysis set: all three PDF converters produced an EPUB
    both = [p for p in pub if all(by[s].get(p, {}).get("ok") == "True" for s in PDF_SYS)]
    out["paired_n"] = len(both)

    metrics = [("f1", "Token F1 (LCS, vs XML)", False), ("precision", "Token precision", False),
               ("recall", "Token recall (abstract+body)", False), ("wer", "Word error rate", False),
               ("bow_f1", "Bag-of-words F1 (order-free)", False),
               ("kendall_tau", "Kendall's tau (paragraph order)", False),
               ("adjacent_ordered", "Adjacent paragraph pairs in order", False),
               ("interleavings", "Interleaving events / paper", False),
               ("broken_per_1k", "Broken words / 1000 tokens", False), ("split_per_1k", "  spurious splits / 1k", False),
               ("join_per_1k", "  spurious joins / 1k", False),
               ("spurious_hyphen_per_1k", "  leftover line-break hyphens / 1k", False),
               ("lost_hyphen_per_1k", "Lost compound hyphens / 1000 tokens", False),
               ("leak_total", "Boilerplate leaks / paper", False), ("leak_headerfooter", "  running headers/footers", False),
               ("leak_pagenums", "  page numbers", False), ("leak_keyword_paras", "  publisher/licence lines", False)]
    for s in PDF_SYS + ["xml"]:
        out["metrics"][s] = {}
        for m, _, _ in metrics:
            out["metrics"][s][m] = boot([num(by[s][p].get(m)) for p in both])
        out["metrics"][s]["any_leak"] = boot([float(num(by[s][p].get("leak_total")) > 0) for p in both])
        out["metrics"][s]["any_interleave"] = boot([float(num(by[s][p].get("interleavings")) > 0) for p in both])
        out["metrics"][s]["f1_lt_0.9"] = boot([float(num(by[s][p].get("f1")) < 0.9) for p in both])
    for m, _, _ in metrics + [("any_leak", "", 0)]:
        g = (lambda s, p: float(num(by[s][p].get("leak_total")) > 0)) if m == "any_leak" else \
            (lambda s, p, m=m: num(by[s][p].get(m)))
        out["deltas"][m] = {"swift-base": paired([g("swift", p) for p in both], [g("base", p) for p in both]),
                            "rust-swift": paired([g("rust", p) for p in both], [g("swift", p) for p in both])}

    # ---------------------------------------------------------- figures, tables, equations, references
    for s in PDF_SYS + ["xml"]:
        rows = [by[s][p] for p in both]
        g = lambda k: [num(r.get(k)) for r in rows]
        out["floats"][s] = {
            "fig_recall": boot_ratio(g("fig_matched"), g("fig_gt")),
            "fig_precision": boot_ratio(g("fig_out_matched"), g("fig_out")),
            "fig_caption": boot_ratio(g("fig_caption_ok"), g("fig_gt_with_caption")),
            "fig_recall_count": boot_ratio([min(a, b) for a, b in zip(g("fig_out"), g("fig_gt"))], g("fig_gt")),
            "fig_precision_count": boot_ratio([min(a, b) for a, b in zip(g("fig_out"), g("fig_gt"))], g("fig_out")),
            "table_recall_count": boot_ratio([min(a, b) for a, b in zip(g("table_out"), g("table_gt"))], g("table_gt")),
            "fig_count_exact": boot([float(a == b) for a, b in zip(g("fig_out"), g("fig_gt"))]),
            "table_recall": boot_ratio(g("table_matched"), g("table_gt")),
            "table_precision": boot_ratio(g("table_out_matched"), g("table_out")),
            "table_caption": boot_ratio(g("table_caption_ok"), g("table_gt_with_caption")),
            "eq_ratio": boot_ratio([min(a, b) for a, b in zip(g("eq_out"), g("eq_gt"))], g("eq_gt")),
            "eq_out_total": float(np.nansum(g("eq_out"))), "eq_gt_total": float(np.nansum(g("eq_gt"))),
            "papers_with_eq": int(sum(1 for v in g("eq_gt") if v > 0)),
            "refs_ratio": boot([a / b for a, b in zip(g("refs_out"), g("refs_gt")) if b > 0], np.median),
            "fig_gt_total": float(np.nansum(g("fig_gt"))), "fig_out_total": float(np.nansum(g("fig_out"))),
            "table_gt_total": float(np.nansum(g("table_gt"))), "table_out_total": float(np.nansum(g("table_out"))),
        }

    # ---------------------------------------------------------- figures from the PDF (JATS + PDF)
    pf = {r["pmcid"]: r for r in read("pdffig.csv")}
    for grp, ids in (("ms", ms), ("pub", pub)):
        rows = [pf[p] for p in ids if pf.get(p, {}).get("pdffig_ok") == "True"]
        if not rows:
            continue
        gt = [num(r["pdffig_gt"]) for r in rows]
        rec = [min(num(r["pdffig_recovered"]), num(r["pdffig_gt"])) for r in rows]
        out["pdffig"][grp] = {"papers": len(rows), "of": len(ids), "recovered": boot_ratio(rec, gt),
                              "all_recovered": boot([float(a >= b) for a, b in zip(rec, gt)]),
                              "none_recovered": boot([float(a == 0) for a in rec])}

    # ---------------------------------------------------------- highlights
    hl = read("highlights.csv")
    tiers = ["exact", "whitespace", "fold", "startend", "lost"]
    for grp, ids in (("pub", set(both)), ("ms", set(ms))):
        for (src, dst) in (("swift", "xml"), ("base", "xml"), ("rust", "xml"), ("base", "swift")):
            rows = [r for r in hl if r["source"] == src and r["target"] == dst and r["pmcid"] in ids]
            if not rows:
                continue
            n = [num(r["n"]) for r in rows]
            d = {t: boot_ratio([num(r[t]) for r in rows], n) for t in tiers}
            d["anchored"] = boot_ratio([num(r["n"]) - num(r["lost"]) for r in rows], n)
            d["correct"] = boot_ratio([num(r["correct"]) for r in rows], n)
            d["misplaced"] = boot_ratio([num(r["misplaced"]) for r in rows], n)
            d["papers"] = len(rows)
            out["highlights"].setdefault(grp, {})[f"{src}->{dst}"] = d

    # ---------------------------------------------------------- speed
    for s in ("swift", "rust", "base"):
        src = f"timing-{s}"
        recs = [load_json(OUT / src / p / "run.json") for p in pub if (OUT / src / p / "run.json").exists()]
        label = "isolated (one conversion at a time)"
        if not recs:
            recs = [load_json(OUT / s / p / "run.json") for p in pub if (OUT / s / p / "run.json").exists()]
            label = "batch (3 conversions in parallel)"
        rows = [(r, num(by["swift"].get(r["pmcid"], {}).get("pages"))) for r in recs if r.get("ok")]
        out["speed"][s] = {"setting": label,
                           "wall_s_per_page": boot([r["wall_s"] / pg for r, pg in rows if pg > 0], np.median),
                           "cpu_s_per_page": boot([r["cpu_s"] / pg for r, pg in rows if pg > 0], np.median),
                           "wall_s_per_paper": boot([r["wall_s"] for r, _ in rows], np.median),
                           "maxrss_mb": boot([r["maxrss_mb"] for r, _ in rows], np.median)}
    for s in PDF_SYS:
        out["speed"][f"{s}_batch"] = {
            "wall_s_per_page": boot([num(by[s][p]["wall_s"]) / num(by[s][p]["pages"]) for p in both], np.median),
            "cpu_s_per_page": boot([num(by[s][p]["cpu_s"]) / num(by[s][p]["pages"]) for p in both], np.median)}
    # breakdowns
    out["by_stratum"], out["by_publisher"] = {}, {}
    for key, field in (("by_stratum", "stratum"), ("by_publisher", "publisher")):
        groups = defaultdict(list)
        for p in both:
            groups[corpus[p][field] or "(none)"].append(p)
        for gname, ids in sorted(groups.items(), key=lambda kv: (-len(kv[1]), kv[0])):
            if key == "by_publisher" and len(ids) < 5:
                continue
            out[key][gname] = {"n": len(ids), **{s: {m: float(np.median([num(by[s][p][m]) for p in ids]))
                                                       for m in ("f1", "broken_per_1k")} for s in PDF_SYS}}

    # ---------------------------------------------------------- agreement
    pairs = read("pairs.csv")
    for a, b in (("swift", "rust"), ("swift", "base"), ("base", "rust")):
        v = [num(r["token_f1"]) for r in pairs if r["a"] == a and r["b"] == b and r["pmcid"] in set(both)]
        out["agreement"][f"{a}-{b}"] = boot(v, np.median)
    sw = np.array([num(by["swift"][p]["f1"]) for p in both])
    ru = np.array([num(by["rust"][p]["f1"]) for p in both])
    out["agreement"]["f1_corr_swift_rust"] = float(np.corrcoef(sw, ru)[0, 1])

    json.dump(out, open(RESULTS / "summary.json", "w"), indent=1, default=str)
    tables(out, metrics)
    figures(out, by, both, hl)


def tables(o, metrics):
    L = []
    c = o["corpus"]["pub"]
    L.append(f"## Corpus\n\nMain corpus: {c['n']} articles, {c['journals']} journals, "
             f"{len(c['publishers'])} publishers, {c['year_min']}–{c['year_max']}; pages median "
             f"{c['pages']['median']:.0f} (IQR {c['pages']['q1']:.0f}–{c['pages']['q3']:.0f}), {c['pages_total']:.0f} pages in total; "
             f"body words median {c['body_words']['median']:.0f}.\n")
    L.append("| Year stratum | n |\n|---|---|")
    for k in sorted(c["years"]):
        L.append(f"| {k} | {c['years'][k]} |")
    L.append("\n| Publisher | n |\n|---|---|")
    for p, n in c["publishers"][:15]:
        L.append(f"| {p or '(none)'} | {n} |")
    rest = sum(n for _, n in c["publishers"][15:])
    if rest:
        L.append(f"| {len(c['publishers']) - 15} other publishers | {rest} |")
    L.append("\n| Article type | n |\n|---|---|")
    for p, n in c["article_types"]:
        L.append(f"| {p} | {n} |")
    if "ms" in o["corpus"]:
        m = o["corpus"]["ms"]
        L.append(f"\nAuthor-manuscript subset: {m['n']} NIHMS manuscripts, {m['journals']} journals, "
                 f"{m['year_min']}–{m['year_max']}, pages median {m['pages']['median']:.0f}.\n")
    L.append("\n## Conversion success (main corpus)\n\n| System | EPUB produced |\n|---|---|")
    for s, v in o["success"].items():
        L.append(f"| {NAME[s]} | {v['ok']}/{v['n']} |")
    L.append(f"\nPaired analysis set (all three PDF converters succeeded): n = {o['paired_n']}.\n")
    L.append("## Text metrics (per paper: median (IQR); mean [95% bootstrap CI])\n")
    L.append("| Metric | " + " | ".join(NAME[s] for s in PDF_SYS + ["xml"]) + " |")
    L.append("|---|" + "---|" * 4)
    for m, label, _ in metrics:
        L.append(f"| {label} | " + " | ".join(fmt(o["metrics"][s][m]) for s in PDF_SYS + ["xml"]) + " |")
    for m, label in (("any_leak", "Papers with any boilerplate leak"), ("any_interleave", "Papers with any interleaving"),
                     ("f1_lt_0.9", "Papers with token F1 < 0.9")):
        L.append(f"| {label} (%) | " + " | ".join(
            f"{100 * o['metrics'][s][m]['mean']:.1f} [{100 * o['metrics'][s][m]['ci_lo']:.1f}, {100 * o['metrics'][s][m]['ci_hi']:.1f}]"
            for s in PDF_SYS + ["xml"]) + " |")
    L.append("\n## Current vs baseline (paired, current − baseline; and Rust − Swift)\n")
    L.append("| Metric | Δ mean [95% CI] (Swift current − baseline) | Wilcoxon p | better/worse/same | Δ mean [95% CI] (Rust − Swift) |")
    L.append("|---|---|---|---|---|")
    for m, label, _ in metrics + [("any_leak", "Papers with any leak (share)", 0)]:
        d, r = o["deltas"][m]["swift-base"], o["deltas"][m]["rust-swift"]
        if d.get("n", 0) == 0:
            continue
        lower_better = m not in ("f1", "precision", "recall", "bow_f1", "kendall_tau", "adjacent_ordered")
        better = d["worse"] if lower_better else d["improved"]
        worse = d["improved"] if lower_better else d["worse"]
        L.append(f"| {label} | {d['mean_diff']:+.4f} [{d['ci_lo']:+.4f}, {d['ci_hi']:+.4f}] | {d['wilcoxon_p']:.2g} | "
                 f"{better}/{worse}/{d['same']} | {r['mean_diff']:+.4f} [{r['ci_lo']:+.4f}, {r['ci_hi']:+.4f}] |")
    L.append("\n## Figures, tables, equations, references (pooled over papers [95% CI])\n")
    L.append("| | " + " | ".join(NAME[s] for s in PDF_SYS) + " |\n|---|---|---|---|")
    rows = [("fig_recall_count", "Figure images, count-based recall Σmin(out,xml)/Σxml", True),
            ("fig_precision_count", "Figure images, count-based precision Σmin(out,xml)/Σout", True),
            ("fig_recall", "Figure recall, identified (image whose caption/label matches an XML figure)", True),
            ("fig_precision", "Figure precision (output figures matching an XML figure)", True),
            ("fig_caption", "Figure captions kept (≥80 % of first 50 words)", True),
            ("table_recall_count", "Table images, count-based recall", True),
            ("table_recall", "Table recall, identified (kept as image, label/caption matches)", True), ("table_precision", "Table precision", True),
            ("table_caption", "Table captions kept", True), ("eq_ratio", "Display equations kept as images (min(out,xml)/xml)", True)]
    for k, label, pct in rows:
        L.append(f"| {label} | " + " | ".join(fmt(o["floats"][s][k], pct=True) for s in PDF_SYS) + " |")
    L.append("| Exact figure count (% papers) | " + " | ".join(
        f"{100 * o['floats'][s]['fig_count_exact']['mean']:.1f}" for s in PDF_SYS) + " |")
    L.append("| Figures: XML total / output total | " + " | ".join(
        f"{o['floats'][s]['fig_gt_total']:.0f} / {o['floats'][s]['fig_out_total']:.0f}" for s in PDF_SYS) + " |")
    L.append("| Tables: XML total / output total | " + " | ".join(
        f"{o['floats'][s]['table_gt_total']:.0f} / {o['floats'][s]['table_out_total']:.0f}" for s in PDF_SYS) + " |")
    L.append("| Display equations: XML total / output images | " + " | ".join(
        f"{o['floats'][s]['eq_gt_total']:.0f} / {o['floats'][s]['eq_out_total']:.0f}" for s in PDF_SYS) + " |")
    L.append("| Reference items out/XML (median ratio) | " + " | ".join(
        f"{o['floats'][s]['refs_ratio']['median']:.2f}" for s in PDF_SYS) + " |")
    if o["pdffig"]:
        L.append("\n## Figures cut from the PDF for an XML version without images\n")
        L.append("| Subset | Papers | Figures recovered (pooled [95% CI]) | Papers with all figures | Papers with none |\n|---|---|---|---|---|")
        for g, d in o["pdffig"].items():
            L.append(f"| {'author manuscripts' if g == 'ms' else 'main corpus'} | {d['papers']}/{d['of']} | "
                     f"{fmt(d['recovered'], pct=True)} | {100 * d['all_recovered']['mean']:.1f} % | {100 * d['none_recovered']['mean']:.1f} % |")
    L.append("\n## Highlight survival (20 simulated highlights per paper; % of highlights [95% CI])\n")
    for g, sc in o["highlights"].items():
        L.append(f"\n{'Main corpus (paired set)' if g == 'pub' else 'Author manuscripts'}\n")
        L.append("| Scenario | exact | whitespace | fold | start/end | lost | anchored correctly | anchored to wrong text |")
        L.append("|---|---|---|---|---|---|---|---|")
        for k, d in sc.items():
            L.append(f"| {k} (n={d['papers']} papers) | " + " | ".join(
                f"{100 * d[t]['pooled']:.1f}" for t in ("exact", "whitespace", "fold", "startend", "lost")) +
                f" | {fmt(d['correct'], pct=True)} | {100 * d['misplaced']['pooled']:.1f} |")
    L.append("\n## Speed\n\n| System | setting | s/page median (IQR) | CPU s/page | s/paper | peak RSS MB |\n|---|---|---|---|---|---|")
    for s in ("swift", "rust", "base"):
        d = o["speed"][s]
        f = lambda x: f"{x['median']:.3f} ({x['q1']:.3f}–{x['q3']:.3f})"
        L.append(f"| {NAME[s]} | {d['setting']} | {f(d['wall_s_per_page'])} | {f(d['cpu_s_per_page'])} | "
                 f"{d['wall_s_per_paper']['median']:.2f} | {d['maxrss_mb']['median']:.0f} |")
    L.append("\nBatch run (3 conversions in parallel, all 200 papers): " + "; ".join(
        f"{NAME[s]} {o['speed'][s + '_batch']['wall_s_per_page']['median']:.3f} s/page wall, "
        f"{o['speed'][s + '_batch']['cpu_s_per_page']['median']:.3f} s/page CPU" for s in PDF_SYS) + ".")
    for key, title in (("by_stratum", "year stratum"), ("by_publisher", "publisher (≥ 5 papers)")):
        L.append(f"\n## Median token F1 / broken words per 1k by {title}\n")
        L.append("| Group | n | F1 Swift current | F1 baseline | F1 Rust | broken/1k current | broken/1k baseline | broken/1k Rust |")
        L.append("|---|---|---|---|---|---|---|---|")
        for gname, d in o[key].items():
            L.append(f"| {gname} | {d['n']} | " + " | ".join(f"{d[s]['f1']:.3f}" for s in PDF_SYS) + " | " +
                     " | ".join(f"{d[s]['broken_per_1k']:.2f}" for s in PDF_SYS) + " |")
    L.append("\n## Swift vs Rust agreement (token F1 between the two outputs)\n")
    for k, v in o["agreement"].items():
        if isinstance(v, dict):
            L.append(f"- {k}: {fmt(v)}")
        else:
            L.append(f"- Pearson r of per-paper F1 vs XML (Swift, Rust): {v:.2f}")
    (RESULTS / "summary_tables.md").write_text("\n".join(L) + "\n")


def style(ax):
    for sp in ("top", "right"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color("#b5b4ae")
    ax.spines["bottom"].set_color("#b5b4ae")
    ax.tick_params(colors="#52514e", labelsize=8)
    ax.yaxis.grid(True, color="#e8e7e2", lw=0.6)
    ax.set_axisbelow(True)


def figures(o, by, both, hl):
    plt.rcParams.update({"font.size": 8, "font.family": "sans-serif", "axes.titlesize": 9,
                         "axes.labelcolor": "#52514e", "text.color": "#0b0b0b"})
    # 1. text fidelity distributions
    fig, axes = plt.subplots(1, 3, figsize=(7.2, 2.6))
    for ax, (m, lab) in zip(axes, (("f1", "Token F1 vs XML"), ("kendall_tau", "Kendall's τ (paragraph order)"),
                                   ("broken_per_1k", "Broken words / 1000 tokens"))):
        data = [[num(by[s][p][m]) for p in both if not np.isnan(num(by[s][p][m]))] for s in PDF_SYS]
        bp = ax.boxplot(data, widths=0.5, patch_artist=True, showfliers=False,
                        medianprops={"color": "#0b0b0b", "lw": 1.2}, whiskerprops={"color": "#8a8984"},
                        capprops={"color": "#8a8984"}, boxprops={"lw": 0})
        for patch, s in zip(bp["boxes"], PDF_SYS):
            patch.set_facecolor(COL[s])
            patch.set_alpha(0.85)
        for i, (d, s) in enumerate(zip(data, PDF_SYS)):
            x = np.random.default_rng(1).normal(i + 1, 0.06, len(d))
            ax.scatter(x, d, s=3, color="#0b0b0b", alpha=0.25, lw=0)
        ax.set_xticks([1, 2, 3], ["Swift\ncurrent", "Swift\nbaseline", "Rust"])
        ax.set_title(lab)
        if m == "broken_per_1k":
            ax.set_yscale("symlog", linthresh=1)
            ax.set_ylim(bottom=0)
        style(ax)
    fig.tight_layout()
    fig.savefig(FIGS / "text_fidelity.pdf")
    plt.close(fig)

    # 2. paired current vs baseline F1
    fig, ax = plt.subplots(figsize=(3.4, 3.2))
    x = np.array([num(by["base"][p]["f1"]) for p in both])
    y = np.array([num(by["swift"][p]["f1"]) for p in both])
    ax.plot([0, 1], [0, 1], color="#b5b4ae", lw=0.8, ls="--")
    ax.scatter(x, y, s=10, color=COL["swift"], alpha=0.7, lw=0.4, edgecolor="#fcfcfb")
    lo = max(0, min(np.nanmin(x), np.nanmin(y)) - 0.02)
    ax.set_xlim(lo, 1.005)
    ax.set_ylim(lo, 1.005)
    ax.set_xlabel("Token F1, baseline (38d2bdc^)")
    ax.set_ylabel("Token F1, current")
    ax.set_title("Per-paper text fidelity, current vs baseline")
    style(ax)
    fig.tight_layout()
    fig.savefig(FIGS / "f1_current_vs_baseline.pdf")
    plt.close(fig)

    # 3. highlight survival stacked bars
    sc = o["highlights"].get("pub", {})
    if sc:
        tiers = [("exact", "exact", "#1c5aa6"), ("whitespace", "whitespace-insensitive", "#2a78d6"),
                 ("fold", "quote/dash/case-insensitive", "#6fa3e6"), ("startend", "start/end fallback", "#b4cff2"),
                 ("lost", "lost", "#d9d8d2")]
        fig, ax = plt.subplots(figsize=(6.4, 2.7))
        names = list(sc)
        left = np.zeros(len(names))
        for t, lab, c in tiers:
            v = np.array([100 * sc[k][t]["pooled"] for k in names])
            ax.barh(range(len(names)), v, left=left, color=c, label=lab, height=0.6, edgecolor="#fcfcfb", lw=1)
            left += v
        ax.set_yticks(range(len(names)), [k.replace("->", " → ").replace("base", "baseline PDF view")
                                          .replace("swift", "current PDF view").replace("rust", "Rust PDF view")
                                          .replace("xml", "XML view") for k in names])
        ax.invert_yaxis()
        ax.set_xlim(0, 100)
        ax.set_xlabel("% of simulated highlights")
        ax.legend(ncol=3, fontsize=6.5, frameon=False, loc="lower center", bbox_to_anchor=(0.4, 1.0))
        style(ax)
        ax.yaxis.grid(False)
        ax.xaxis.grid(True, color="#e8e7e2", lw=0.6)
        fig.tight_layout()
        fig.savefig(FIGS / "highlight_survival.pdf")
        plt.close(fig)

    # 4. speed: seconds per page vs pages
    fig, ax = plt.subplots(figsize=(3.6, 2.8))
    for s in ("swift", "rust"):
        pts = []
        for p in both:
            src = OUT / f"timing-{s}" / p / "run.json"
            r = load_json(src) if src.exists() else None
            if r and r.get("ok"):
                pts.append((num(by[s][p]["pages"]), r["wall_s"]))
        if pts:
            a = np.array(pts)
            ax.scatter(a[:, 0], a[:, 1], s=10, color=COL[s], alpha=0.75, lw=0.4, edgecolor="#fcfcfb", label=NAME[s])
    ax.set_xlabel("PDF pages")
    ax.set_ylabel("Conversion time (s, wall clock)")
    ax.set_title("Conversion time per paper")
    ax.legend(frameon=False, fontsize=7)
    style(ax)
    fig.tight_layout()
    fig.savefig(FIGS / "speed.pdf")
    plt.close(fig)

    # 5. Swift vs Rust F1 per paper
    fig, ax = plt.subplots(figsize=(3.4, 3.2))
    x = np.array([num(by["swift"][p]["f1"]) for p in both])
    y = np.array([num(by["rust"][p]["f1"]) for p in both])
    ax.plot([0, 1], [0, 1], color="#b5b4ae", lw=0.8, ls="--")
    ax.scatter(x, y, s=10, color=COL["rust"], alpha=0.7, lw=0.4, edgecolor="#fcfcfb")
    lo = max(0, min(np.nanmin(x), np.nanmin(y)) - 0.02)
    ax.set_xlim(lo, 1.005)
    ax.set_ylim(lo, 1.005)
    ax.set_xlabel("Token F1, Swift (PDFKit)")
    ax.set_ylabel("Token F1, Rust (PDFium)")
    ax.set_title("Cross-implementation agreement")
    style(ax)
    fig.tight_layout()
    fig.savefig(FIGS / "swift_vs_rust.pdf")
    plt.close(fig)


if __name__ == "__main__":
    main()
