"""Step 9: summaries of the 2026-11-18 additions (s06-s08): outside reference points (pdftotext,
GROBID) on the 200 PDFs, and reading views made from the journal's article page.

Reads results/{corpus,per_paper,per_paper_extra,highlights,highlights_pages}.csv and data/pages/*/fetch.json;
writes results/summary_extra.json, results/summary_extra.md, results/pages.csv,
figures/f1_by_source.pdf and figures/highlight_survival_sources.pdf. Bootstrap as in s05 (B = 5000, seed 20261116).
"""
import csv
import json
from collections import Counter, defaultdict
from math import comb
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

from common import DATA, FIGS, OUT, RESULTS, SEED
from s05_aggregate import boot, boot_ratio, num, paired, read, style

PAGES = DATA / "pages"
BASE = ["pdftotext", "pdftotext-layout", "grobid", "swift", "rust", "xml"]


def stats(by, s, ids):
    rows = [by[s][p] for p in ids if p in by[s] and by[s][p]["ok"] == "True"]
    v = lambda k: [num(r.get(k)) for r in rows]
    out = {"n": len(rows)}
    for k in ("f1", "recall", "precision", "kendall_tau"):
        out[k] = boot(v(k), np.median)
    out["broken_per_1k"] = boot(v("broken_per_1k"))
    out["interleaving_papers"] = boot([float(x > 0) for x in v("interleavings")])
    out["leak_papers"] = boot([float(x > 0) for x in v("leak_total")])
    out["fig_identified"] = boot_ratio(v("fig_matched"), v("fig_gt"))
    out["table_identified"] = boot_ratio(v("table_matched"), v("table_gt"))
    ratio = [num(r["refs_out"]) / num(r["refs_gt"]) for r in rows if num(r.get("refs_gt")) > 0]
    out["refs_ratio_median"] = float(np.median(ratio)) if ratio else None
    out["refs_none_papers"] = int(sum(1 for r in rows if num(r.get("refs_gt")) > 0 and num(r["refs_out"]) == 0))
    return out


def wall_per_page(system, ids, by):
    t = []
    for p in ids:
        f = OUT / system / p / "run.json"
        if f.exists() and p in by["swift"]:
            t.append(json.loads(f.read_text())["wall_s"] / num(by["swift"][p]["pages"]))
    return float(np.median(t)) if t else None


def guard(rows, k):
    """Share of papers whose k highlights (drawn from the 20) would all be found (the replace rule)."""
    v = []
    for r in rows:
        n, found = int(r["n"]), int(r["n"]) - int(r["lost"])
        if n >= k:
            v.append(comb(found, k) / comb(n, k))
    return float(np.mean(v)) if v else None


def hl_summary(rows):
    tot = Counter()
    for r in rows:
        for k in ("n", "exact", "whitespace", "fold", "startend", "lost", "correct", "misplaced"):
            tot[k] += int(r[k])
    res = {k: tot[k] / tot["n"] for k in ("exact", "whitespace", "fold", "startend", "lost", "correct", "misplaced")}
    res["correct_ci"] = boot_ratio([int(r["correct"]) for r in rows], [int(r["n"]) for r in rows])
    res["n"], res["papers"] = tot["n"], len(rows)
    res["guard"] = {k: guard(rows, k) for k in (1, 3, 5, 10, 20)}
    return res


def main():
    corpus = {r["pmcid"]: r for r in read("corpus.csv")}
    pub = [p for p, r in corpus.items() if r["group"] == "pub"]
    by = defaultdict(dict)
    for r in read("per_paper.csv") + read("per_paper_extra.csv"):
        by[r["system"]][r["pmcid"]] = r
    o = {"outside": {}, "pages": {}, "highlights": {}}

    # ---------------------------------------------------------- outside reference points (n = 200)
    for s in BASE:
        o["outside"][s] = stats(by, s, pub)
        o["outside"][s]["s_per_page"] = wall_per_page(s, pub, by) if s in ("pdftotext", "grobid") else None
    for s in ("swift", "rust"):
        d = paired([num(by[s][p]["f1"]) for p in pub], [num(by["grobid"][p]["f1"]) for p in pub])
        o["outside"][f"{s}_minus_grobid_f1"] = d
    sep = [p for p in pub if num(by["swift"][p]["refs_out"]) > 0]
    nosep = [p for p in pub if num(by["swift"][p]["refs_out"]) == 0]
    o["outside"]["by_reference_split"] = {
        name: {"n": len(ids), **{s: float(np.nanmedian([num(by[s][p]["f1"]) for p in ids])) for s in ("swift", "rust", "grobid", "pdftotext")}}
        for name, ids in (("swift_found_reference_list", sep), ("swift_no_reference_items", nosep))}

    # ---------------------------------------------------------- journal pages
    fetched = []
    for f in sorted(PAGES.glob("PMC*/fetch.json")):
        r = json.loads(f.read_text())
        run = OUT / "page-swift" / r["pmcid"] / "run.json"
        rr = json.loads(run.read_text()) if run.exists() else {}
        r["converted"] = bool(rr.get("ok"))
        r["reader"] = rr.get("reader")
        r["refused"] = bool(rr) and not rr.get("ok") and "only shows the abstract" in (OUT / "page-swift" / r["pmcid"] / "log.txt").read_text()
        r["host"] = r["final_url"].split("/")[2] if r.get("final_url") else ""
        fetched.append(r)
    with open(RESULTS / "pages.csv", "w", newline="") as fh:
        keys = ["pmcid", "group", "year", "journal", "doi", "host", "status", "blocked", "converted", "refused", "reader"]
        w = csv.DictWriter(fh, keys, extrasaction="ignore")
        w.writeheader()
        for r in fetched:
            w.writerow(r)
    ok = [r["pmcid"] for r in fetched if r["converted"]]
    o["pages"]["attempted"] = len(fetched)
    o["pages"]["saved"] = sum(1 for r in fetched if not r["blocked"])
    o["pages"]["converted"] = len(ok)
    o["pages"]["refused_abstract_only"] = sum(1 for r in fetched if r["refused"])
    o["pages"]["blocked_by_host"] = Counter(r["host"].replace("www.", "") for r in fetched if r["blocked"]).most_common()
    o["pages"]["blocked_reasons"] = Counter((r["blocked"] or "").split(":")[0] for r in fetched if r["blocked"]).most_common()
    o["pages"]["readers"] = Counter(r["reader"] for r in fetched if r["converted"]).most_common()
    fam = lambda r: r["group"] if r["group"] in ("Nature", "PLOS", "Frontiers") else "other"
    o["pages"]["families"] = Counter(fam(r) for r in fetched if r["converted"]).most_common()
    for s in ("page-swift", "page-rust", "swift", "rust", "xml", "grobid", "pdftotext"):
        o["pages"][s] = stats(by, s, ok)
    o["pages"]["page_minus_pdf_f1"] = paired([num(by["page-swift"][p]["f1"]) for p in ok], [num(by["swift"][p]["f1"]) for p in ok])
    groups = defaultdict(list)
    for r in fetched:
        if r["converted"]:
            groups[fam(r)].append(r["pmcid"])
    o["pages"]["by_family"] = {g: {s: stats(by, s, ids) for s in ("page-swift", "swift")} for g, ids in groups.items()}

    # ---------------------------------------------------------- highlights
    hp = read("highlights_pages.csv")
    for src, dst in (("swift", "page-swift"), ("rust", "page-rust"), ("swift", "xml"), ("xml", "page-swift")):
        rows = [r for r in hp if r["source"] == src and r["target"] == dst and r["pmcid"] in ok]
        o["highlights"][f"{src}->{dst}"] = hl_summary(rows)
    rows = [r for r in read("highlights.csv") if r["group"] == "pub" and r["source"] == "swift" and r["target"] == "xml"]
    o["highlights"]["swift->xml (all 199)"] = hl_summary(rows)

    (RESULTS / "summary_extra.json").write_text(json.dumps(o, indent=1, default=float))
    write_md(o)
    figure(by, pub, ok)
    hl_figure(ok)


def f3(s, k):
    x = s[k]
    return f"{x['median']:.3f} ({x['q1']:.3f}–{x['q3']:.3f})"


def write_md(o):
    L = ["# Outside reference points and journal-page reading views (2026-11-18)", ""]
    L += ["## PDF conversion on the 200 PDFs: pdftotext, GROBID and Lunta", "",
          "| | pdftotext | pdftotext -layout | GROBID 0.9.1 (CRF) | Lunta Swift | Lunta Rust | XML view (control) |",
          "|---|---|---|---|---|---|---|"]
    S = [o["outside"][s] for s in BASE]
    L.append("| Token F1, median (IQR) | " + " | ".join(f3(s, "f1") for s in S) + " |")
    L.append("| Token F1, mean [CI] | " + " | ".join(f"{s['f1']['mean']:.3f}" for s in S) + " |")
    for k, lab in (("recall", "Recall, median"), ("precision", "Precision, median"), ("kendall_tau", "Kendall's τ, median")):
        L.append(f"| {lab} | " + " | ".join(f"{s[k]['median']:.3f}" for s in S) + " |")
    L.append("| Broken words / 1000, mean | " + " | ".join(f"{s['broken_per_1k']['mean']:.2f}" for s in S) + " |")
    L.append("| Papers with interleaving, % | " + " | ".join(f"{100 * s['interleaving_papers']['mean']:.1f}" for s in S) + " |")
    L.append("| Papers with any boilerplate leak, % | " + " | ".join(f"{100 * s['leak_papers']['mean']:.1f}" for s in S) + " |")
    L.append("| Figures identified (image + label), % | " + " | ".join(f"{100 * s['fig_identified']['pooled']:.1f}" for s in S) + " |")
    L.append("| Reference items / XML refs, median | " + " | ".join(f"{s['refs_ratio_median']:.2f}" for s in S) + " |")
    L.append("| Papers with no reference items | " + " | ".join(str(s["refs_none_papers"]) for s in S) + " |")
    L.append("| s per page, median | " + " | ".join(f"{s['s_per_page']:.3f}" if s.get("s_per_page") else "" for s in S) + " |")
    for s in ("swift", "rust"):
        d = o["outside"][f"{s}_minus_grobid_f1"]
        L.append(f"\n{s} − GROBID token F1: mean {d['mean_diff']:+.3f} [{d['ci_lo']:+.3f}, {d['ci_hi']:+.3f}], "
                 f"Wilcoxon p = {d['wilcoxon_p']:.1e}; {s} better in {d['improved']}, worse in {d['worse']}")
    for name, v in o["outside"]["by_reference_split"].items():
        L.append(f"\n{name} (n = {v['n']}): median F1 swift {v['swift']:.3f}, rust {v['rust']:.3f}, grobid {v['grobid']:.3f}, pdftotext {v['pdftotext']:.3f}")
    P = o["pages"]
    L += ["", "## Reading views from the journal's article page", "",
          f"Attempted {P['attempted']}, saved {P['saved']}, converted {P['converted']}, refused as abstract-only {P['refused_abstract_only']}.",
          f"Blocked by host: {P['blocked_by_host']}", f"Reasons: {P['blocked_reasons']}", f"Readers: {P['readers']}",
          f"Families: {P['families']}", "",
          "| same papers | page view (Swift) | page view (Rust) | PDF view (Swift) | PDF view (Rust) | XML view | GROBID | pdftotext |",
          "|---|---|---|---|---|---|---|---|"]
    S = [P[s] for s in ("page-swift", "page-rust", "swift", "rust", "xml", "grobid", "pdftotext")]
    L.append("| n | " + " | ".join(str(s["n"]) for s in S) + " |")
    L.append("| Token F1, median (IQR) | " + " | ".join(f3(s, "f1") for s in S) + " |")
    for k, lab in (("recall", "Recall, median"), ("precision", "Precision, median")):
        L.append(f"| {lab} | " + " | ".join(f"{s[k]['median']:.3f}" for s in S) + " |")
    L.append("| Broken words / 1000, mean | " + " | ".join(f"{s['broken_per_1k']['mean']:.2f}" for s in S) + " |")
    L.append("| Papers with interleaving, % | " + " | ".join(f"{100 * s['interleaving_papers']['mean']:.1f}" for s in S) + " |")
    L.append("| Papers with any boilerplate leak, % | " + " | ".join(f"{100 * s['leak_papers']['mean']:.1f}" for s in S) + " |")
    L.append("| Figures identified, % | " + " | ".join(f"{100 * s['fig_identified']['pooled']:.1f}" for s in S) + " |")
    L.append("| Tables identified, % | " + " | ".join(f"{100 * s['table_identified']['pooled']:.1f}" if s['table_identified'].get('n') else "–" for s in S) + " |")
    L.append("| Reference items / XML refs, median | " + " | ".join(f"{s['refs_ratio_median']:.2f}" for s in S) + " |")
    d = P["page_minus_pdf_f1"]
    L.append(f"\npage − PDF view (Swift) token F1: mean {d['mean_diff']:+.3f} [{d['ci_lo']:+.3f}, {d['ci_hi']:+.3f}], p = {d['wilcoxon_p']:.1e}; better in {d['improved']}, worse in {d['worse']}")
    L += ["", "| family | n | page F1 median | PDF F1 median | page figures % | PDF figures % | page refs ratio |", "|---|---|---|---|---|---|---|"]
    for g, v in P["by_family"].items():
        a, b = v["page-swift"], v["swift"]
        L.append(f"| {g} | {a['n']} | {a['f1']['median']:.3f} | {b['f1']['median']:.3f} | {100 * a['fig_identified']['pooled']:.1f} | "
                 f"{100 * b['fig_identified']['pooled']:.1f} | {a['refs_ratio_median']:.2f} |")
    L += ["", "## Highlights", "", "| from → to | n | exact | spacing | folded | ends | lost | correct [CI] | wrong place | guard pass k=1/3/5/10/20 |",
          "|---|---|---|---|---|---|---|---|---|---|"]
    for k, h in o["highlights"].items():
        c = h["correct_ci"]
        L.append(f"| {k} | {h['n']} ({h['papers']} papers) | " + " | ".join(f"{100 * h[t]:.1f}" for t in ("exact", "whitespace", "fold", "startend", "lost")) +
                 f" | {100 * c['pooled']:.1f} [{100 * c['ci_lo']:.1f}, {100 * c['ci_hi']:.1f}] | {100 * h['misplaced']:.1f} | " +
                 " / ".join(f"{h['guard'][k]:.2f}" for k in (1, 3, 5, 10, 20)) + " |")
    (RESULTS / "summary_extra.md").write_text("\n".join(L) + "\n")


def figure(by, pub, ok):
    plt.rcParams.update({"font.size": 8, "font.family": "sans-serif", "axes.titlesize": 9,
                         "axes.labelcolor": "#52514e", "text.color": "#0b0b0b"})
    sysl = [("pdftotext", pub, "pdftotext", "#b5b4ae"), ("grobid", pub, "GROBID", "#8a8984"),
            ("swift", pub, "Lunta,\nPDF", "#2a78d6"), ("page-swift", ok, "Lunta,\npage", "#1baf7a")]
    fig, ax = plt.subplots(figsize=(3.4, 2.6))
    data = [[num(by[s][p]["f1"]) for p in ids if p in by[s] and by[s][p]["ok"] == "True"] for s, ids, _, _ in sysl]
    bp = ax.boxplot(data, widths=0.5, patch_artist=True, showfliers=False,
                    medianprops={"color": "#0b0b0b", "lw": 1.2}, whiskerprops={"color": "#8a8984"},
                    capprops={"color": "#8a8984"}, boxprops={"lw": 0})
    for patch, (_, _, _, c) in zip(bp["boxes"], sysl):
        patch.set_facecolor(c)
        patch.set_alpha(0.85)
    for i, d in enumerate(data):
        x = np.random.default_rng(1).normal(i + 1, 0.07, len(d))
        ax.scatter(x, d, s=3, color="#0b0b0b", alpha=0.3, lw=0)
    ax.set_xticks(range(1, len(sysl) + 1), [f"{lab}\n(n = {len(d)})" for (_, _, lab, _), d in zip(sysl, data)])
    ax.set_ylabel("Token F1 against the XML")
    ax.set_ylim(0, 1.02)
    style(ax)
    fig.tight_layout()
    fig.savefig(FIGS / "f1_by_source.pdf")
    plt.close(fig)


def hl_figure(ok):
    main = read("highlights.csv")
    hp = read("highlights_pages.csv")
    sc = [("PDF view → XML", [r for r in main if r["group"] == "pub" and r["source"] == "swift" and r["target"] == "xml"]),
          ("Rust PDF view → XML", [r for r in main if r["group"] == "pub" and r["source"] == "rust" and r["target"] == "xml"]),
          ("PDF view → journal's page\n(30 papers)", [r for r in hp if r["source"] == "swift" and r["target"] == "page-swift" and r["pmcid"] in ok]),
          ("Previous converter's view\n→ current PDF view", [r for r in main if r["group"] == "pub" and r["source"] == "base" and r["target"] == "swift"])]
    tiers = [("exact", "exact", "#1c5aa6"), ("whitespace", "whitespace-insensitive", "#2a78d6"),
             ("fold", "quote/dash/case-insensitive", "#6fa3e6"), ("startend", "start/end fallback", "#b4cff2"),
             ("lost", "lost", "#d9d8d2")]
    fig, ax = plt.subplots(figsize=(6.4, 2.7))
    left = np.zeros(len(sc))
    for t, lab, c in tiers:
        v = np.array([100 * sum(int(r[t]) for r in rows) / sum(int(r["n"]) for r in rows) for _, rows in sc])
        ax.barh(range(len(sc)), v, left=left, color=c, label=lab, height=0.6, edgecolor="#fcfcfb", lw=1)
        left += v
    ax.set_yticks(range(len(sc)), [n for n, _ in sc])
    ax.invert_yaxis()
    ax.set_xlim(0, 100)
    ax.set_xlabel("% of simulated highlights")
    ax.legend(ncol=3, fontsize=6.5, frameon=False, loc="lower center", bbox_to_anchor=(0.4, 1.0))
    style(ax)
    ax.yaxis.grid(False)
    ax.xaxis.grid(True, color="#e8e7e2", lw=0.6)
    fig.tight_layout()
    fig.savefig(FIGS / "highlight_survival_sources.pdf")
    plt.close(fig)


if __name__ == "__main__":
    main()
