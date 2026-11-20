// Checks against the published record, as on the Mac (apple/Shared/Checks.swift): Check a Reference
// List (pasted, a .bib / .ris / text file, a folder's papers, a paper's own references) with its
// report (Save as Markdown or CSV), and retractions, corrections and published versions of
// preprints: a badge in the list, a banner in the details with the choices (Switch to the
// Published Version…, Link Both, Keep the Preprint), checked weekly in the background and on
// demand. The work is the core's (src-tauri/src/checks.rs → core/src/checks). Nothing in the
// library changes until a choice is made.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open as openDialog, save as saveDialog, ask } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { h, status, type MenuItem } from './ui';
import type { Paper, PaperMeta } from './api';

export interface EditorialNotice { kind: string; label: string; doi: string; date: string; source: string; }
export interface PublishedVersion { doi: string; title: string; journal: string; date: string; found_by: string; }
export interface PaperUpdates {
  notices: EditorialNotice[]; published: PublishedVersion | null; declined_doi: string;
  linked: PublishedVersion | null; preprint: PaperMeta | null; switched_at: number;
}
interface FieldDifference { field: string; given: string; found: string; }
type Verdict = 'verified' | 'minor_differences' | 'not_found' | 'wrong_doi' | 'not_checked' | 'failed';
interface RefCheckItem {
  number: number; key: string; reference: string; verdict: Verdict; found: PaperMeta | null; source: string; link: string;
  differences: FieldDifference[]; notices: EditorialNotice[]; note: string; suggested_doi: string; in_library: string | null;
}
interface RefCheckReport {
  items: RefCheckItem[]; verified: number; minor: number; not_found: number; wrong_doi: number; not_checked: number;
  failed: number; retracted: number; corrected: number; summary: string; markdown: string; csv: string;
}
interface UpdatesSummary {
  checked: number; retracted: string[]; concern: string[]; corrected: string[]; published: string[]; changed: string[]; errors: string[]; text: string;
}

let updates: Record<string, PaperUpdates> = {};
let onChange: () => void = () => {};
let running = false;

/** Called after a choice changed the library (the list and details reload). */
export function setChecksReload(f: () => void) { onChange = f; }

/** What the checks found, per paper: loaded with the list. */
export async function loadUpdates() {
  updates = await invoke<Record<string, PaperUpdates>>('paper_updates_all').catch(() => ({}));
}

// the weekly check found something new
listen<UpdatesSummary>('updates-checked', async (e) => {
  await loadUpdates();
  onChange();
  status(e.payload.text, 'info', true);
});

const RETRACTED = ['retraction', 'partial_retraction', 'withdrawal', 'removal'];
const CONCERN = ['expression_of_concern'];
const CORRECTED = ['correction', 'erratum', 'corrigendum', 'addendum', 'clarification'];

function level(notices: EditorialNotice[]): 'retracted' | 'concern' | 'corrected' | '' {
  const k = notices.map((n) => n.kind);
  if (k.some((x) => RETRACTED.includes(x))) return 'retracted';
  if (k.some((x) => CONCERN.includes(x))) return 'concern';
  if (k.some((x) => CORRECTED.includes(x))) return 'corrected';
  return '';
}

function mainNotice(notices: EditorialNotice[]): EditorialNotice | undefined {
  const rank = (n: EditorialNotice) => RETRACTED.includes(n.kind) ? 3 : CONCERN.includes(n.kind) ? 2 : CORRECTED.includes(n.kind) ? 1 : 0;
  return [...notices].filter((n) => rank(n) > 0).sort((a, b) => rank(b) - rank(a) || b.date.localeCompare(a.date))[0];
}

/** The list's badge for a paper the checks flagged. */
export function updatesBadge(id: string): HTMLElement | null {
  const u = updates[id];
  if (!u) return null;
  const l = level(u.notices);
  if (l === 'retracted') return h('span.badge.flag-red', { text: 'Retracted', title: 'Retracted, withdrawn or removed (see the details)' });
  if (l === 'concern') return h('span.badge.flag-orange', { text: 'Concern', title: 'An expression of concern (see the details)' });
  if (l === 'corrected') return h('span.badge.flag-orange', { text: 'Corrected', title: 'Corrected (see the details)' });
  if (u.published) return h('span.badge', { text: 'Published', title: `Published in ${u.published.journal || 'a journal'}` });
  return null;
}

/** The details' banner: a notice, or a preprint's published version and what can be done. */
export function updatesBanner(p: Paper): HTMLElement | null {
  const u = updates[p.id];
  if (!u) return null;
  const link = (text: string, url: string) => h('a', { href: '#', text, onclick: (e: Event) => { e.preventDefault(); openUrl(url); } });
  const l = level(u.notices);
  if (l) {
    const n = mainNotice(u.notices);
    const box = h('div.check-banner.' + (l === 'retracted' ? 'red' : 'orange'), {}, h('strong', { text: '' }));
    invoke<string>('updates_headline', { updates: u }).then((t) => { (box.firstChild as HTMLElement).textContent = t; }).catch(() => {});
    if (n?.doi) box.append(' ', link('See Notice', `https://doi.org/${n.doi}`));
    return box;
  }
  if (u.published) {
    const v = u.published;
    return h('div.check-banner', {},
      h('strong', { text: `Published in ${v.journal || 'a journal'}${v.date ? ` (${v.date.slice(0, 4)})` : ''}.`, title: `${v.title} · ${v.doi} · found by ${v.found_by}` }), ' ',
      h('a', { href: '#', text: 'Switch to the Published Version…', onclick: (e: Event) => { e.preventDefault(); switchTo(p, v); } }), ' · ',
      h('a', { href: '#', text: 'Link Both', title: 'Keep the preprint and show its published version with it', onclick: (e: Event) => { e.preventDefault(); choice('link_published_version', p.id); } }), ' · ',
      h('a', { href: '#', text: 'Keep the Preprint', onclick: (e: Event) => { e.preventDefault(); choice('keep_preprint', p.id); } }), ' · ',
      link('Open', `https://doi.org/${v.doi}`));
  }
  if (u.linked) {
    return h('div.check-banner.quiet', {}, `Published version: ${u.linked.journal || u.linked.doi}. `, link('Open', `https://doi.org/${u.linked.doi}`),
      ' · ', h('a', { href: '#', text: 'Switch…', onclick: (e: Event) => { e.preventDefault(); switchTo(p, u.linked!); } }));
  }
  if (u.preprint && u.switched_at > Date.now() - 7 * 86400_000) {
    return h('div.check-banner.quiet', {}, `Switched from the preprint (${u.preprint.journal || u.preprint.doi}). `,
      h('a', { href: '#', text: 'Undo', onclick: (e: Event) => { e.preventDefault(); choice('undo_published_version', p.id); } }));
  }
  return null;
}

async function choice(command: string, id: string) {
  try { await invoke(command, { id }); } catch (e) { status(String(e), 'error'); }
  await loadUpdates();
  onChange();
}

/** Switch to the Published Version…, after a confirm (also Tidy Library's Published preprints, tidy.ts). */
export async function switchTo(p: Paper, v: PublishedVersion) {
  const ok = await ask(`The paper's details become the published version's (${v.journal || v.doi}). Highlights, notes, tags and the citation key stay. You can undo this from the details.`,
    { title: 'Switch to the published version?', kind: 'info', okLabel: 'Switch', cancelLabel: 'Cancel' });
  if (!ok) return;
  status('Switching to the published version…', 'busy');
  try { await invoke('use_published_version', { id: p.id }); status('Switched to the published version. Make Reading View or Find PDF gets its full text when it is open access.', 'info', true); }
  catch (e) { status(`Couldn't switch: ${e}`, 'error'); }
  await loadUpdates();
  onChange();
}

// ---- checking ----

/** Items for the import menu: Check a Reference List…, Check Library…. */
export function checkMenuItems(): MenuItem[] {
  return [
    { label: 'Check a Reference List…', action: askForList },
    { label: 'Check Library for Retractions and Published Versions', action: () => checkUpdates([]) },
  ];
}

/** Items for a paper's menu. */
export function paperCheckItems(p: Paper): MenuItem[] {
  return [
    { label: 'Check for Retractions and Published Version', action: () => checkUpdates([p.id]) },
    { label: "Check This Paper's References", action: () => run('Reading the reference list…', 'check_paper_references', { id: p.id }, `the references of “${p.meta.title.slice(0, 60)}”`), disabled: !p.has_epub && !p.has_pdf },
  ];
}

/** The item for a folder's menu (null: All Papers). */
export function folderCheckItem(folder: string | null, name: string): MenuItem {
  return { label: 'Check References', action: () => run(`Checking the references in “${name}”…`, 'check_folder_references', { folder }, `“${name}”`) };
}

async function run(busy: string, command: string, args: Record<string, unknown>, title: string) {
  if (running) { status('A check is already running.', 'error'); return; }
  running = true;
  status(`${busy} (about a second per reference)`, 'busy');
  try {
    const r = await invoke<RefCheckReport>(command, args);
    status(r.summary, 'info', true);
    showReport(r, title);
  } catch (e) { status(String(e), 'error'); }
  finally { running = false; }
}

async function checkUpdates(ids: string[]) {
  if (running) { status('A check is already running.', 'error'); return; }
  running = true;
  status('Checking for retractions, corrections and published versions…', 'busy');
  try {
    const s = await invoke<UpdatesSummary>('check_updates', { ids });
    await loadUpdates();
    onChange();
    status(s.text, 'info', true);
    if (ids.length !== 1) showUpdates(s);
  } catch (e) { status(String(e), 'error'); }
  finally { running = false; }
}

function modal(title: string, body: (HTMLElement | string | null)[], buttons: HTMLElement[]): () => void {
  const close = () => back.remove();
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('div.modal.wide.check-modal', {}, h('h3', { text: title }), ...body.filter(Boolean) as HTMLElement[], h('div.buttons', {}, buttons)));
  back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') close(); });
  document.body.append(back);
  return close;
}

/** Paste a list or choose a file. */
function askForList() {
  const text = h('textarea', { rows: 12, placeholder: '1. Smith J, Jones K. A title. Journal. 2020;12:34-56.\n2. …', spellcheck: 'false' }) as HTMLTextAreaElement;
  const close = modal('Check a Reference List', [
    h('p.hint', { text: 'Paste a reference list (numbered or author–year, from a manuscript or one you are reviewing), BibTeX or RIS, or choose a file. Each reference is looked up in Crossref, DataCite, OpenAlex and PubMed and its details compared with the record. Nothing is added to your library.' }),
    text,
  ], [
    h('button', { type: 'button', text: 'Cancel', onclick: () => close() }),
    h('button', { type: 'button', text: 'Choose File…', onclick: async () => {
      const f = await openDialog({ title: 'A reference list', filters: [{ name: 'BibTeX, RIS or text', extensions: ['bib', 'ris', 'txt', 'md'] }] });
      if (typeof f !== 'string') return;
      close();
      run(`Checking ${f.split(/[\\/]/).pop()}…`, 'check_references_file', { path: f }, f.split(/[\\/]/).pop() || 'the file');
    } }),
    h('button.primary', { type: 'button', text: 'Check', onclick: () => {
      if (!text.value.trim()) return;
      const t = text.value;
      close();
      run('Checking the pasted list…', 'check_references', { text: t }, 'the pasted list');
    } }),
  ]);
  text.focus();
}

const SECTIONS: [string, (i: RefCheckItem) => boolean][] = [
  ['Retracted or with notices', (i) => level(i.notices) !== ''],
  ['DOI points to a different work', (i) => i.verdict === 'wrong_doi'],
  ['Could not be found in Crossref, OpenAlex or PubMed', (i) => i.verdict === 'not_found'],
  ['Minor differences', (i) => i.verdict === 'minor_differences'],
  ["Couldn't be checked", (i) => i.verdict === 'failed'],
  ['Verified', (i) => i.verdict === 'verified'],
  ['Not looked up (not papers)', (i) => i.verdict === 'not_checked'],
];

function showReport(r: RefCheckReport, title: string) {
  const link = (text: string, url: string) => h('a', { href: '#', text, onclick: (e: Event) => { e.preventDefault(); openUrl(url); } });
  const row = (i: RefCheckItem) => h('div.check-item', {},
    h('div.check-key', { text: i.key && i.key !== String(i.number) ? `[${i.number}] ${i.key}` : `[${i.number}]` }, i.in_library ? h('span.badge', { text: 'In library' }) : null),
    h('div', { text: i.reference }),
    level(i.notices) ? h('div.check-red', { text: `${mainNotice(i.notices)?.label || 'Notice'}${mainNotice(i.notices)?.date ? ` (${mainNotice(i.notices)!.date})` : ''}` }) : null,
    i.note ? h('div.hint', { text: i.note }) : null,
    ...i.differences.map((d) => h('div.hint', { text: `${d.field}: ${d.given ? `“${d.given}”` : 'not in the reference'} → “${d.found}” in ${i.source}` })),
    h('div.check-links', {}, i.link ? link(`Open (${i.source})`, i.link) : null,
      ...i.notices.filter((n) => n.doi).map((n) => link(`${n.label} notice`, `https://doi.org/${n.doi}`)),
      i.suggested_doi ? link(`Open ${i.suggested_doi}`, `https://doi.org/${i.suggested_doi}`) : null));
  const sections = SECTIONS.map(([name, f]) => {
    const items = r.items.filter(f);
    return items.length ? h('div.check-section', {}, h('h4', { text: `${name} (${items.length})` }), ...items.map(row)) : null;
  });
  const save = async (text: string, ext: string) => {
    const path = await saveDialog({ defaultPath: `Reference check.${ext}`, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
    if (typeof path !== 'string') return;
    invoke('save_report', { path, text }).then(() => status(`Saved ${path.split(/[\\/]/).pop()}.`)).catch((e) => status(String(e), 'error'));
  };
  const close = modal(`Checked ${title}`, [
    h('p', { text: r.summary }),
    h('p.hint', { text: 'A reference that could not be found may still exist (books, reports, theses and very new papers are often missing): check it by hand.' }),
    h('div.check-list', {}, ...sections),
  ], [
    h('button', { type: 'button', text: 'Save as Markdown…', onclick: () => save(r.markdown, 'md') }),
    h('button', { type: 'button', text: 'Save as CSV…', onclick: () => save(r.csv, 'csv') }),
    h('button.primary', { type: 'button', text: 'Done', onclick: () => close() }),
  ]);
}

function showUpdates(s: UpdatesSummary) {
  const close = modal('Retractions and Published Versions', [
    h('p', { text: s.text }),
    s.retracted.length || s.concern.length || s.corrected.length || s.published.length
      ? h('p.hint', { text: 'The papers carry a badge in the list; the smart folders “Retracted or corrected” and “Published since you added them” collect them (New Smart Folder › Start from). Select a paper for the notice or the published version and what to do.' })
      : null,
  ], [h('button.primary', { type: 'button', text: 'Done', onclick: () => close() })]);
}
