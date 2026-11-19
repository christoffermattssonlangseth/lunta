"""Crude split of the highlights lost from the current PDF view to the XML view (RESULTS section 7):
no counterpart = fewer than 20 % of the quote's 3-word shingles in the XML view's text; else garbled = a
word missing from it. Usage (in eval/): .venv/bin/python scripts/s10_lost_split.py data/highlights.jsonl"""
import sys, json
sys.path.insert(0, 'scripts')
import textutil as T
from pathlib import Path
def run(path):
    rows = [json.loads(l) for l in open(path)]
    lost = [r for r in rows if r['source'] == 'swift' and r['target'] == 'xml' and r['tier'] == 'lost']
    cache = {}
    nc = gar = 0
    for r in lost:
        p = r['pmcid']
        if p not in cache:
            import re as _re
            txt = ' '.join(_re.sub(r'<[^>]+>', ' ', f.read_text()) for f in T.spine(Path('data/out/xml') / p / 'epub'))
            import html as _h
            toks = T.tokens(_h.unescape(txt))
            cache[p] = (set(zip(toks, toks[1:], toks[2:])), set(toks))
        sh, voc = cache[p]
        q = T.tokens(r['quote'])
        qs = list(zip(q, q[1:], q[2:]))
        if qs and sum(s in sh for s in qs) / len(qs) < 0.2:
            nc += 1
        elif any(t not in voc for t in q):
            gar += 1
    n = len(lost)
    print(path, 'lost', n, 'no counterpart %.0f%%' % (100 * nc / n), 'garbled %.0f%%' % (100 * gar / n))
for p in sys.argv[1:]:
    run(p)
