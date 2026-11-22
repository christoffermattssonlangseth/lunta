// Check Citations in a .tex file (Windows, Linux), as on the Mac (apple/Mac/CheckCitations.swift):
// for each \cite, the sentence it is in, the cited papers' passages that best match it and, with the
// open model (Qwen3, when downloaded), a suggested verdict whose quotes are checked word for word
// against the paper (core/src/claims; src-tauri/src/claims.rs). Lunta proposes, the writer decides:
// "Looks right" / "Dismiss" per citation; nothing is written, nothing leaves this computer.
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog, message } from '@tauri-apps/plugin-dialog';
import { h, fill, status } from './ui';
import './claims.css';

interface Passage { text: string; section: string; score: number; link: string; }
interface Evidence { quote: string; section: string; link: string; }
interface Result {
  id: string; title: string; label: string; source: string; verdict: string; verdictLabel: string; byModel: boolean;
  reason: string; note: string; evidence: Evidence[]; passages: Passage[];
}
interface Report { claim: string; context: string; judged: boolean; note: string; results: Result[]; }
interface TexClaim { keys: string[]; ids: string[]; command: string; line: number; sentence: string; previous: string; claim: string; }
interface Item { tex: TexClaim; report: Report | null; error: string; checking: boolean; decision: '' | 'right' | 'dismissed'; }

const CLS: Record<string, string> = { supported: 'ok', partly: 'part', contradicted: 'bad', notFound: 'none', possible: 'maybe', noText: 'none' };
const ORDER = ['contradicted', 'notFound', 'partly', 'possible', 'noText', 'supported'];

/** Lunta's link to a passage: the paper opened and found there (`open(id, find)`). */
function parseLink(link: string): { id: string; find: string } | null {
  try {
    const u = new URL(link.replace(/^lunta:\/\/open/, 'https://x/open'));
    const id = u.searchParams.get('id');
    return id ? { id, find: u.searchParams.get('find') || '' } : null;
  } catch { return null; }
}

/** The counts line: "12 citations checked: 7 supported, …". */
export function claimsSummary(items: Item[]): string {
  const done = items.filter((i) => i.report);
  const n: Record<string, number> = {};
  for (const it of done) for (const r of it.report!.results) {
    const k = r.verdict === 'notFound' && !r.byModel ? 'noMatch' : r.verdict;
    n[k] = (n[k] || 0) + 1;
  }
  const names: [string, string][] = [['supported', 'supported'], ['partly', 'partly supported'], ['contradicted', 'contradicted'], ['notFound', 'not found in the paper'],
    ['noMatch', 'no matching passage'], ['possible', 'possible support'], ['noText', 'no text to check']];
  const parts = names.filter(([k]) => n[k]).map(([k, name]) => `${n[k]} ${name}`);
  const unknown = items.filter((i) => i.tex.ids.some((x) => !x)).length;
  if (unknown) parts.push(`${unknown} with keys not in your library`);
  const right = items.filter((i) => i.decision === 'right').length;
  return `${done.length === 1 ? '1 citation' : `${done.length} citations`} checked` + (parts.length ? ': ' + parts.join(', ') : '') + (right ? ` · ${right} marked Looks right` : '');
}

/** Asks for the .tex file and checks its citations. `open(id, find)`: the paper in the reader, at those words. */
export async function checkTexCitations(open: (id: string, find: string) => void) {
  const path = await openDialog({ title: 'Choose the .tex file', filters: [{ name: 'LaTeX', extensions: ['tex'] }] });
  if (typeof path !== 'string') return;
  let found: TexClaim[];
  try { found = (await invoke<{ claims: TexClaim[] }>('tex_claims', { path })).claims; }
  catch (e) { status(String(e), 'error'); return; }
  const name = path.split(/[\\/]/).pop()!;
  if (!found.length) {
    await message('Lunta looks for \\cite{…}, \\citep{…}, \\citet{…}, \\parencite{…}, \\autocite{…} and the like, outside comments.', { title: `No citations in “${name}”` });
    return;
  }
  const model = await invoke<boolean>('claims_model').catch(() => false);
  const items: Item[] = found.map((tex) => ({ tex, report: null, error: '', checking: false, decision: '' }));
  let stopped = false, running = true, onlyOpen = false;

  const list = h('div.claims-list');
  const head = h('div.claims-head');
  const close = () => { stopped = true; back.remove(); };
  const back = h('div.modal-back', {},
    h('div.modal.claims-modal', {},
      h('h3', { text: `Check Citations: ${name}` }),
      h('p.hint', { text: 'Lunta proposes, you decide: for each citation, the passages of the cited paper that best match the sentence, and, with the on-device model, a suggested verdict whose quotes are checked word for word against the paper. It can be wrong: read the passages. Checked on this computer: nothing is sent anywhere.' }),
      model ? null : h('p.hint', { text: 'No on-device model downloaded (Ask Lunta’s model): the passages that share the most words with each sentence are shown, with their word overlap, for you to judge.' }),
      head, list,
      h('div.buttons', {}, h('button', { type: 'button', text: 'Done', onclick: close }))));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);

  const go = (link: string) => { const l = parseLink(link); if (l) { close(); open(l.id, l.find); } };

  function resultView(r: Result) {
    const label = r.verdictLabel || r.verdict;
    return h('div.claims-r', {},
      h('div.claims-rh', {}, h('span.claims-badge.' + (CLS[r.verdict] || 'none'), { text: label }),
        h('span.claims-paper', { text: `${r.label || 'Paper'}${r.title ? ' · ' + r.title : ''}` })),
      r.byModel ? h('div.claims-by', { text: 'Suggested by the on-device model' + (r.reason ? ': ' + r.reason : '.') }) : null,
      r.evidence.map((e) => h('button.link.claims-q', { type: 'button', title: 'Open the paper at this passage', text: `“${e.quote}”${e.section ? ' — ' + e.section : ''}`, onclick: () => go(e.link) })),
      r.note ? h('div.claims-note', { text: r.note }) : null,
      r.passages.length ? (() => {
        const d = h('details', {}, h('summary', { text: r.passages.length === 1 ? 'The best-matching passage' : `The ${r.passages.length} best-matching passages` }),
          r.passages.map((p) => h('button.claims-p', { type: 'button', title: 'Open the paper at this passage', onclick: () => go(p.link) },
            h('span.claims-pm', { text: `${p.section || 'Passage'} · ${Math.round(p.score * 100)} % word overlap` }), h('span', { text: p.text })))) as HTMLDetailsElement;
        d.open = !r.byModel;
        return d;
      })() : null);
  }

  function itemView(it: Item) {
    const decide = (d: Item['decision']) => { it.decision = d; render(); };
    const buttons = it.decision
      ? [h('span.claims-note', { text: it.decision === 'right' ? '✓ Looks right' : 'Dismissed' }), h('button.link', { type: 'button', text: 'Undo', onclick: () => decide('') })]
      : [h('button', { type: 'button', text: 'Looks right', disabled: !it.report, onclick: () => decide('right') }), h('button', { type: 'button', text: 'Dismiss', onclick: () => decide('dismissed') })];
    const missing = it.tex.keys.filter((_, k) => !it.tex.ids[k]);
    return h('div.claims-item' + (it.decision ? '.decided' : ''), {},
      h('div.claims-where', {}, h('span', { text: `Line ${it.tex.line}` }), h('code', { text: it.tex.command }), h('span.grow'), ...buttons),
      h('div.claims-claim', { text: it.tex.claim }),
      it.decision === 'dismissed' ? null : [
        it.checking ? h('div.hint', { text: 'Checking…' }) : null,
        it.error ? h('div.claims-note', { text: it.error }) : null,
        missing.length ? h('div.claims-note', { text: `${missing.map((k) => `\\cite{${k}}`).join(', ')}: not in your Lunta library (nor found by the .bib’s DOI or title), so it can’t be checked.` }) : null,
        ...(it.report ? it.report.results.map(resultView) : []),
      ]);
  }

  const rank = (it: Item) => !it.report ? 1 : Math.min(4, ...it.report.results.map((r) => { const i = ORDER.indexOf(r.verdict); return i < 0 ? 4 : i; }));
  function render() {
    if (!back.isConnected) return;
    const shown = items.filter((it) => !onlyOpen || (!it.decision && (!it.report || it.report.results.some((r) => r.verdict !== 'supported'))));
    if (!running) shown.sort((a, b) => rank(a) - rank(b));
    const only = h('input', { type: 'checkbox', checked: onlyOpen, onchange: (e: Event) => { onlyOpen = (e.target as HTMLInputElement).checked; render(); } });
    fill(head, h('div.buttons.left', {},
      h('strong', { text: claimsSummary(items) }),
      running ? h('button', { type: 'button', text: 'Stop', onclick: () => { stopped = true; } }) : null,
      h('label.chk', {}, only, ' Only those to look at')));
    fill(list, ...shown.map(itemView));
  }
  render();

  for (const it of items) {
    if (stopped) break;
    const ids = it.tex.ids.filter(Boolean);
    if (!ids.length) continue;
    it.checking = true; render();
    try { it.report = await invoke<Report>('check_claim', { claim: it.tex.claim, context: it.tex.previous, ids }); }
    catch (e) { it.error = String(e); }
    it.checking = false;
    render();
  }
  running = false;
  render();
}
