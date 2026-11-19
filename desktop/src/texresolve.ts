// Resolve Citations in a .tex file (Windows, Linux), as on the Mac (apple/Mac/TexResolve.swift):
// write \cite{?langseth2025} (or (Langseth et al. 2025), [Langseth 2025 spatial]) while writing,
// then Lunta lists the placeholders with its candidates (the library first, then Crossref /
// OpenAlex) and the writer confirms or skips each one. Lunta proposes, the writer decides: the
// file is written, and outside papers added, only on Write. The work: src-tauri/src/texresolve.rs
// on core/src/placeholders.
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog, message } from '@tauri-apps/plugin-dialog';
import { h, fill, status } from './ui';
import type { Folder, PaperMeta } from './api';

interface Cite { text: string; surnames: string[]; etAl: boolean; year: string; hints: string[]; key: string; }
interface Placeholder { start: number; end: number; text: string; kind: string; cites: Cite[]; }
interface Candidate {
  id: string; title: string; authors: string[]; families: string[]; year: string; journal: string; meta: PaperMeta;
  source: string; confidence: number; exact: boolean; inLibrary: boolean; citeKey: string;
}
interface Suggestions { candidates: Candidate[]; exact: boolean; note: string; }
type State = 'loading' | 'open' | 'confirmed' | 'skipped';
interface Part { cite: Cite; state: State; candidates: Candidate[]; pick: number; exact: boolean; note: string; searching: boolean; }
interface Item { ph: Placeholder; parts: Part[]; }

const decided = (it: Item) => it.parts.every((p) => p.state === 'confirmed' || p.state === 'skipped');
const confirmed = (it: Item) => decided(it) && it.parts.some((p) => p.state === 'confirmed');

function byline(c: Candidate) {
  const f = c.families.length ? c.families : c.authors;
  const who = f.length > 2 ? `${f[0]} et al.` : f.join(' & ');
  return [who, c.year, c.journal].filter(Boolean).join(' · ');
}
function source(c: Candidate) {
  const where = c.inLibrary ? `In your library · \\cite{${c.citeKey}}` : `${c.source === 'openalex' ? 'OpenAlex' : 'Crossref'} · added to your library when written`;
  return `${where} · ${Math.round(c.confidence * 100)}%${c.exact ? ' · exact match' : ''}`;
}

/**
 * Asks for the .tex file and shows its placeholders. `folders`: the library's folders, `kept`: the
 * folder ids whose .bib is kept updated, `folder`: the folder asked from (its menu), `changed`:
 * the library changed (reload, .bib files rewritten).
 */
export async function resolveTexCitations(folders: Folder[], kept: string[], folder: string | null, changed: () => void) {
  const path = await openDialog({ title: 'Choose the .tex file', filters: [{ name: 'LaTeX', extensions: ['tex'] }] });
  if (typeof path !== 'string') return;
  let found: Placeholder[];
  try { found = (await invoke<{ placeholders: Placeholder[] }>('tex_open', { path })).placeholders; }
  catch (e) { status(String(e), 'error'); return; }
  const name = path.split(/[\\/]/).pop()!;
  if (!found.length) {
    await message('Write \\cite{?name2025} (or \\cite{name2025?}, with words after the year as hints: \\cite{?name2025spatial}), (Name et al. 2025) or [Name 2025 hint words] where a citation goes, then resolve them here.', { title: `No placeholders in “${name}”` });
    return;
  }
  const items: Item[] = found.map((ph) => ({ ph, parts: ph.cites.map((cite) => ({ cite, state: 'loading', candidates: [], pick: 0, exact: false, note: '', searching: false })) }));
  let target = folder ?? folders.find((f) => kept.includes(f.id))?.id ?? '';
  let report = '', writing = false;

  const list = h('div.tex-list');
  const foot = h('div.tex-foot');
  const close = () => back.remove();
  const back = h('div.modal-back', {},
    h('div.modal.tex-resolve', {},
      h('h3', { text: `Resolve Citations: ${name}` }),
      h('p.hint', { text: 'Lunta proposes, you decide: each placeholder is rewritten as \\cite{key} only when you confirm it. Papers not in your library are added when the file is written.' }),
      list, foot));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);

  function fillPart(p: Part, s: Suggestions) {
    p.candidates = s.candidates || [];
    p.exact = !!s.exact && !!p.candidates[0]?.exact && !!p.candidates[0]?.inLibrary;
    p.note = s.note || (p.candidates.length ? '' : 'No paper found. Search, or skip it.');
    p.pick = 0; p.state = 'open';
  }
  async function search(p: Part, q: string) {
    if (!q.trim()) return;
    p.state = 'loading'; render();
    try { fillPart(p, await invoke<Suggestions>('tex_suggest', { cite: null, query: q.trim() })); p.exact = false; p.searching = false; }
    catch (e) { p.state = 'open'; p.note = String(e); }
    render();
  }
  async function write() {
    if (writing) return;
    writing = true; render();
    const todo = items.filter(confirmed);
    try {
      const r = await invoke<{ written: number; added: string[]; papers: number; backup: string }>('tex_write', {
        path, folder: target || null,
        choices: todo.map((it) => ({ start: it.ph.start, end: it.ph.end, picks: it.parts.map((p) => (p.state === 'confirmed' ? p.candidates[p.pick] : null)) })),
      });
      const f = folders.find((x) => x.id === target);
      report = `${r.written === 1 ? '1 placeholder' : `${r.written} placeholders`} resolved in “${name}” (the original is ${r.backup.split(/[\\/]/).pop()})` +
        (items.length > r.written ? `; ${items.length - r.written} left as written` : '') + '. ' +
        (r.added.length ? `${r.added.length === 1 ? '1 paper' : `${r.added.length} papers`} added to your library. ` : '') +
        (f ? (kept.includes(f.id) ? `The papers are in “${f.name}”, whose BibTeX file Lunta keeps updated.` : `The papers are in “${f.name}”: keep its BibTeX file updated (the folder’s menu) to use them in the LaTeX project.`)
          : 'Keep a BibTeX file updated for a folder with these papers (the folder’s menu) to use them in the LaTeX project.');
      // the added papers' open-access PDFs, as when adding by DOI
      for (const id of r.added) invoke('find_pdf', { id }).catch(() => {});
      changed();
    } catch (e) {
      report = `Nothing was written to “${name}”: ${e}`;
    }
    writing = false; render();
  }

  function partView(it: Item, p: Part) {
    const label = it.parts.length > 1 ? h('div.tex-label', { text: p.cite.text }) : null;
    const change = h('button.link', { text: 'Change', onclick: () => { p.state = 'open'; render(); } });
    if (p.state === 'loading') return h('div.tex-part', {}, label, h('div.hint', { text: 'Looking…' }));
    if (p.state === 'confirmed') return h('div.tex-part', {}, label, h('div.tex-chosen', {}, `✓ ${p.candidates[p.pick].title || 'Untitled'} `, change));
    if (p.state === 'skipped') return h('div.tex-part', {}, label, h('div.tex-chosen', {}, 'Skipped: stays as written ', change));
    const q = h('input', { type: 'search', placeholder: 'Title words, authors, a DOI…', value: [...p.cite.surnames, ...p.cite.hints].join(' ') }) as HTMLInputElement;
    q.addEventListener('keydown', (e) => { if (e.key === 'Enter') search(p, q.value); });
    return h('div.tex-part', {}, label,
      p.candidates.map((c, k) => h('div.tex-cand' + (k === p.pick ? '.on' : ''), { onclick: () => { p.pick = k; render(); } },
        h('div.t', { text: c.title || 'Untitled' }), h('div.m', { text: byline(c) }), h('div.m', { text: source(c) }))),
      p.note ? h('div.hint', { text: p.note }) : null,
      h('div.buttons.left', {},
        h('button.primary', { type: 'button', text: 'Confirm', disabled: !p.candidates.length, onclick: () => { p.state = 'confirmed'; render(); } }),
        h('button', { type: 'button', text: 'Skip', onclick: () => { p.state = 'skipped'; render(); } }),
        h('button', { type: 'button', text: 'Search…', onclick: () => { p.searching = !p.searching; render(); } })),
      p.searching ? h('div.buttons.left', {}, q, h('button', { type: 'button', text: 'Search', onclick: () => search(p, q.value) })) : null);
  }

  function render() {
    const exact = items.reduce((n, it) => n + it.parts.filter((p) => p.state === 'open' && p.exact).length, 0);
    fill(list, ...items.map((it) => h('div.tex-ph', {}, h('div.tex-text', { text: it.ph.text }), it.parts.map((p) => partView(it, p)))));
    const select = h('select', { onchange: (e: Event) => { target = (e.target as HTMLSelectElement).value; } },
      h('option', { value: '', text: 'No folder' }),
      folders.map((f) => h('option', { value: f.id, text: kept.includes(f.id) ? `${f.name} (.bib kept)` : f.name }))) as HTMLSelectElement;
    select.value = target;
    const n = items.filter(confirmed).length;
    fill(foot,
      h('label.tex-folder', {}, 'Put the papers in ', select),
      exact ? h('button', { type: 'button', text: `Confirm All Exact Matches (${exact})`, onclick: () => {
        items.forEach((it) => it.parts.forEach((p) => { if (p.state === 'open' && p.exact) { p.pick = 0; p.state = 'confirmed'; } }));
        render();
      } }) : null,
      report ? h('p', { text: report }) : null,
      h('p.hint', { text: 'Overleaf: download the project (or use its Git or Dropbox sync), resolve the .tex here, and upload it with the kept .bib.' }),
      h('div.buttons', {},
        h('button', { type: 'button', text: report ? 'Done' : 'Cancel', onclick: close }),
        h('button.primary', { type: 'button', text: n === 1 ? 'Write 1 Citation' : `Write ${n} Citations`, disabled: writing || !n || !!report, onclick: write })));
  }
  render();

  // two at a time: Crossref is asked for the parts the library can't settle
  const jobs = items.flatMap((it) => it.parts);
  const next = async (): Promise<void> => {
    const p = jobs.shift();
    if (!p) return;
    try { fillPart(p, await invoke<Suggestions>('tex_suggest', { cite: p.cite, query: null })); }
    catch (e) { p.state = 'open'; p.note = String(e); }
    if (back.isConnected) render();
    return next();
  };
  await Promise.all([next(), next()]);
}
