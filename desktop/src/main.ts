// The library window: sidebar (places, folders, smart folders, tags), paper list with search and
// sort, and the selected paper's details. Layout and wording follow apple/Mac/LibraryWindow.swift.
import { open as openDialog, ask } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { openUrl } from '@tauri-apps/plugin-opener';
import { api, smartApi, type Paper, type Row, type Sidebar, type Source, type CiteStyle, type Folder } from './api';
import { editDetails, editSmart, blankSmart } from './editors';
import { setupDrag, type Payload } from './drag';
import { fill, h, $, status, prompt, menu, menuBelow, copy, authorsShort, type MenuItem } from './ui';
import { openReader, readerPaper, closeReader } from './reader';

let source: Source = { kind: 'all' };
let query = '';
let sort = localStorage.getItem('sort') || 'added';
let rows: Row[] = [];
let selected: string | null = null;           // the paper shown in the details pane
let picked = new Set<string>();               // all selected papers (⌘/Ctrl-click, Shift-click)
let anchor: string | null = null;
let side: Sidebar | null = null;
let busy = 0;

// ---- first run ----

async function start() {
  const info = await api.info();
  if (info.opened) return showLibrary();
  $('#welcome').hidden = false;
  const path = $<HTMLInputElement>('#lib-path');
  if (info.library) path.value = info.library;
  if (info.error) showWelcomeError(`Couldn't open the library: ${info.error}`);
  $('#lib-browse').onclick = async () => {
    const dir = await openDialog({ directory: true, title: 'Library folder' });
    if (typeof dir === 'string') path.value = dir;
  };
  $('#lib-open').onclick = async () => {
    try { await api.chooseLibrary(path.value); $('#welcome').hidden = true; showLibrary(); }
    catch (e) { showWelcomeError(String(e)); }
  };
}

function showWelcomeError(t: string) { const e = $('#lib-error'); e.textContent = t; e.hidden = false; }

async function showLibrary() {
  $('#library').hidden = false;
  $<HTMLSelectElement>('#sort').value = sort;
  await reload();
  // changes synced from other devices (their op logs) show up within ten seconds
  setInterval(async () => { if (await api.refresh().catch(() => false)) reload(); }, 10000);
}

// ---- loading ----

async function reload() {
  side = await api.sidebar();
  renderSidebar();
  await reloadList();
}

let listSerial = 0;
async function reloadList() {
  const serial = ++listSerial;
  const r = await api.list(source, query, sort).catch((e) => { status(String(e), 'error'); return [] as Row[]; });
  if (serial !== listSerial) return;
  rows = r;
  if (selected && !rows.some((x) => x.paper.id === selected)) selected = null;
  picked = new Set([...picked].filter((id) => rows.some((x) => x.paper.id === id)));
  if (selected && !picked.has(selected)) picked = new Set(picked.size ? picked : [selected]);
  renderList();
  renderDetail();
}

async function after(p: Promise<unknown>, reloadAll = true) {
  try { await p; } catch (e) { status(String(e), 'error'); }
  if (reloadAll) await reload(); else await reloadList();
}

// ---- sidebar ----

function same(a: Source, b: Source) { return a.kind === b.kind && (a.id || '') === (b.id || ''); }

function place(label: string, s: Source, count?: number, icon = '', extra: Partial<{ depth: number; onmenu: (e: MouseEvent) => void }> = {}) {
  const item = h('button.place' + (same(s, source) ? '.on' : ''), {
    onclick: () => { source = s; selected = null; picked.clear(); renderSidebar(); reloadList(); },
    oncontextmenu: extra.onmenu ? (e: MouseEvent) => { e.preventDefault(); extra.onmenu!(e); } : undefined,
    'data-folder': s.kind === 'folder' ? s.id : undefined,
    'data-drag': s.kind === 'folder' ? `folder:${s.id}` : undefined,
    'data-drop': s.kind === 'folder' ? `folder:${s.id}` : s.kind === 'tag' ? `tag:${s.id}` : undefined,
  }, h('span.icon', { text: icon }), h('span.label', { text: label }), count !== undefined ? h('span.count', { text: String(count) }) : null);
  if (extra.depth) item.style.paddingLeft = `${12 + extra.depth * 14}px`;
  return item;
}

function renderSidebar() {
  if (!side) return;
  const s = side;
  const kids = new Map<string | null, Folder[]>();
  for (const f of s.folders) {
    const p = f.parent && s.folders.some((x) => x.id === f.parent) ? f.parent : null;
    kids.set(p, [...(kids.get(p) || []), f]);
  }
  const tree = (parent: string | null, depth: number): HTMLElement[] =>
    (kids.get(parent) || []).flatMap((f) => [
      place(f.name, { kind: 'folder', id: f.id }, f.paper_count, '▸', { depth, onmenu: (e) => folderMenu(e, f) }),
      ...tree(f.id, depth + 1),
    ]);
  fill($('#sidebar'),
    h('h4', { text: 'Library' }),
    place('All Papers', { kind: 'all' }, s.all, '📚'),
    place('Starred', { kind: 'starred' }, s.starred, '★'),
    place('Must Read', { kind: 'must_read' }, s.must_read, '🔖'),
    place('Unread', { kind: 'unread' }, s.unread, '●'),
    h('h4', { 'data-drop': 'folders-root' }, 'Folders', h('button.mini', { title: 'New folder', text: '＋', onclick: () => newFolder(null) })),
    ...tree(null, 0),
    s.folders.length ? null : h('p.hint', { text: 'No folders yet' }),
    h('h4', {}, 'Smart Folders', h('button.mini', { title: 'New smart folder', text: '＋', onclick: () => newSmart() })),
    ...s.smart.map(([id, name]) => place(name, { kind: 'smart', id }, undefined, '⚙', { onmenu: (e) => smartMenu(e, id, name) })),
    s.tags.length ? h('h4', { text: 'Tags' }) : null,
    ...s.tags.map((t) => place(t, { kind: 'tag', id: t }, undefined, '#')),
  );
}

async function newSmart() {
  const id = await editSmart(blankSmart(), side?.tags || [], side?.folders || []);
  if (!id) return;
  source = { kind: 'smart', id }; selected = null; picked.clear();
  await reload();
}

async function smartMenu(e: MouseEvent, id: string, name: string) {
  menu(e.clientX, e.clientY, [
    { label: 'Edit Smart Folder…', action: async () => {
      const f = (await smartApi.list()).find((x) => x.id === id);
      if (f && await editSmart(f, side?.tags || [], side?.folders || [])) reload();
    } },
    { label: 'Delete Smart Folder', danger: true, action: async () => {
      if (!(await ask(`Delete the smart folder “${name}”? The papers stay in the library.`, { title: 'Delete Smart Folder', kind: 'warning' }))) return;
      if (same(source, { kind: 'smart', id })) source = { kind: 'all' };
      after(smartApi.remove(id));
    } },
  ]);
}

async function newFolder(parent: string | null) {
  const name = await prompt(parent ? 'New subfolder' : 'New folder', '', 'Name', 'Create');
  if (name) after(api.createFolder(name, parent));
}

function folderMenu(e: MouseEvent, f: Folder) {
  const items: MenuItem[] = [
    { label: 'New Subfolder…', action: () => newFolder(f.id) },
    { label: 'Rename…', action: async () => { const n = await prompt('Rename folder', f.name, '', 'Rename'); if (n) after(api.renameFolder(f.id, n)); } },
  ];
  if (f.parent) items.push({ label: 'Move to Top Level', action: () => after(api.moveFolder(f.id, null)) });
  const others = side!.folders.filter((x) => x.id !== f.id && x.id !== f.parent);
  if (others.length) items.push('-', ...others.map((o) => ({ label: `Move into “${o.name}”`, action: () => after(api.moveFolder(f.id, o.id)) })));
  items.push('-', {
    label: 'Delete Folder…', danger: true, action: async () => {
      if (await ask(`Delete the folder “${f.name}”? The papers in it stay in the library.`, { title: 'Delete Folder', kind: 'warning' })) {
        if (same(source, { kind: 'folder', id: f.id })) source = { kind: 'all' };
        after(api.deleteFolder(f.id));
      }
    },
  });
  menu(e.clientX, e.clientY, items);
}

// ---- paper list ----

function snippet(s: string) {
  // the core marks the hit as [word]
  const out = h('div.snippet');
  s.split(/(\[[^\]]*\])/).forEach((part) => out.append(part.startsWith('[') && part.endsWith(']') ? h('mark', { text: part.slice(1, -1) }) : part));
  return out;
}

function renderList() {
  const list = $('#list');
  if (!rows.length) {
    list.replaceChildren(h('div.empty', {},
      h('p', { text: query ? `Nothing in this place matches “${query}”.` : 'No papers here yet.' }),
      side?.all ? null : h('button.primary', { text: 'Add a Paper…', onclick: addPaper })));
    return;
  }
  list.replaceChildren(...rows.map(({ paper: p, snippet: sn }) => {
    const m = p.meta;
    return h('div.row' + (picked.has(p.id) ? '.on' : '') + (p.read ? '' : '.unread'), {
      'data-id': p.id,
      'data-drag': 'paper',
      onclick: (e: MouseEvent) => click(e, p.id),
      ondblclick: () => read(p, p.has_epub ? 'reading' : 'pdf'),
      oncontextmenu: (e: MouseEvent) => {
        e.preventDefault();
        if (!picked.has(p.id)) select(p.id);
        if (picked.size > 1) pickedMenu(e); else paperMenu(e, p);
      },
    },
      h('div.marks', {}, h('span.dot', { title: p.read ? '' : 'Unread' }),
        h('span.star' + (p.starred ? '.on' : ''), { text: p.starred ? '★' : '' }),
        h('span.mustread', { text: p.must_read ? '🔖' : '', title: p.must_read ? 'Must Read' : '' })),
      h('div.main', {},
        h('div.title', { text: m.title || '(untitled)' }),
        h('div.sub', { text: [authorsShort(m.authors), m.journal, p.year].filter(Boolean).join(' · ') }),
        sn ? snippet(sn) : null),
      h('div.files', {},
        p.has_epub ? h('span.badge', { text: 'Reading view', title: 'Has a reading view' }) : null,
        p.has_pdf ? h('span.badge', { text: 'PDF', title: 'Has a PDF' }) : null,
        p.annotation_count ? h('span.badge.hl', { text: `✎${p.annotation_count}`, title: 'Highlights and notes' }) : null));
  }));
}

function click(e: MouseEvent, id: string) {
  if (e.shiftKey && anchor) {
    const a = rows.findIndex((r) => r.paper.id === anchor), b = rows.findIndex((r) => r.paper.id === id);
    if (a >= 0 && b >= 0) {
      const [lo, hi] = a < b ? [a, b] : [b, a];
      const range = rows.slice(lo, hi + 1).map((r) => r.paper.id);
      picked = e.metaKey || e.ctrlKey ? new Set([...picked, ...range]) : new Set(range);
      selected = id;
      return paintSelection();
    }
  }
  if (e.metaKey || e.ctrlKey) {
    if (picked.has(id)) { picked.delete(id); selected = picked.size ? [...picked][picked.size - 1] : null; }
    else { picked.add(id); selected = id; }
    anchor = id;
    return paintSelection();
  }
  select(id);
}

function select(id: string) {
  selected = id; anchor = id; picked = new Set([id]);
  paintSelection();
}

function paintSelection() {
  document.querySelectorAll('#list .row').forEach((r) => r.classList.toggle('on', picked.has((r as HTMLElement).dataset.id!)));
  document.querySelector(`#list .row[data-id="${selected}"]`)?.scrollIntoView({ block: 'nearest' });
  renderDetail();
}

/** The papers a drag or menu on this selection acts on, in list order. */
function pickedPapers(): Paper[] { return rows.filter((r) => picked.has(r.paper.id)).map((r) => r.paper); }

function pickedMenu(e: MouseEvent) {
  const ps = pickedPapers();
  const n = ps.length;
  const all = (f: (p: Paper) => boolean) => ps.every(f);
  menu(e.clientX, e.clientY, [
    { label: all((p) => p.starred) ? `Unstar ${n} Papers` : `Star ${n} Papers`, action: () => after(Promise.all(ps.map((p) => api.setStarred(p.id, !all((q) => q.starred))))) },
    { label: all((p) => p.must_read) ? `Remove ${n} from Must Read` : `Add ${n} to Must Read`, action: () => after(Promise.all(ps.map((p) => api.setMustRead(p.id, !all((q) => q.must_read))))) },
    { label: all((p) => p.read) ? `Mark ${n} as Unread` : `Mark ${n} as Read`, action: () => after(Promise.all(ps.map((p) => api.setRead(p.id, !all((q) => q.read))))) },
    '-',
    ...citeItems(ps.map((p) => p.id)),
    '-',
    { label: `Delete ${n} Papers…`, danger: true, action: () => deletePapers(ps) },
  ]);
}

function paperMenu(e: MouseEvent, p: Paper) {
  menu(e.clientX, e.clientY, [
    { label: 'Read', action: () => read(p, 'reading'), disabled: !p.has_epub },
    { label: 'Edit Details…', action: () => editPaper(p) },
    { label: 'Open PDF', action: () => read(p, 'pdf'), disabled: !p.has_pdf },
    '-',
    { label: p.starred ? 'Unstar' : 'Star', action: () => after(api.setStarred(p.id, !p.starred)) },
    { label: p.must_read ? 'Remove from Must Read' : 'Add to Must Read', action: () => after(api.setMustRead(p.id, !p.must_read)) },
    { label: p.read ? 'Mark as Unread' : 'Mark as Read', action: () => after(api.setRead(p.id, !p.read)) },
    '-',
    ...citeItems([p.id]),
    '-',
    { label: p.has_pdf ? 'Replace PDF…' : 'Add PDF…', action: () => choosePdf(p) },
    { label: 'Find PDF', action: () => findPdf(p), disabled: p.has_pdf },
    { label: 'Make Reading View', action: () => makeReadingView(p), disabled: p.has_epub },
    '-',
    { label: 'Delete', danger: true, action: () => deletePaper(p) },
  ]);
}

function citeItems(ids: string[]): MenuItem[] {
  const c = (style: CiteStyle, label: string) => ({ label: `Copy ${label}`, action: async () => copy(await api.cite(ids, style), `${label} citation`) });
  return [c('apa', 'APA'), c('harvard', 'Harvard'), c('chicago', 'Chicago'), c('cell', 'Cell'), c('mla', 'MLA'),
    c('vancouver', 'Vancouver'), c('ieee', 'IEEE'), c('ama', 'AMA'), c('nature', 'Nature'), c('science', 'Science'),
    c('pnas', 'PNAS'), c('acs', 'ACS'), c('bibtex', 'BibTeX'), c('ris', 'RIS')];
}

// ---- details ----

function renderDetail() {
  const box = $('#detail');
  const p = rows.find((r) => r.paper.id === selected)?.paper;
  if (p && picked.size > 1) {
    box.replaceChildren(h('div.multi', {}, h('h3', { text: `${picked.size} papers selected` }),
      h('p', { text: 'Drag them onto a folder in the sidebar to add them to it, or onto a tag to tag them.' }),
      h('button.danger', { text: `Delete ${picked.size} Papers…`, onclick: () => deletePapers(pickedPapers()) })));
    return;
  }
  if (!p) {
    box.replaceChildren(h('div.empty', {}, h('p', { text: rows.length ? 'Select a paper' : '' })));
    return;
  }
  const m = p.meta;
  const ids = [m.doi && `DOI ${m.doi}`, m.pmid && `PMID ${m.pmid}`, m.pmcid, m.arxiv && `arXiv ${m.arxiv}`].filter(Boolean) as string[];
  const link = m.doi ? `https://doi.org/${m.doi}` : m.url || (m.arxiv ? `https://arxiv.org/abs/${m.arxiv}` : '');
  const folders = side?.folders || [];
  const tagInput = h('input', { type: 'text', placeholder: 'Add tag', spellcheck: 'false' }) as HTMLInputElement;
  tagInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && tagInput.value.trim()) after(api.setTag(p.id, tagInput.value.trim(), true));
  });
  const citeBtn: HTMLElement = h('button', { text: 'Cite ▾', onclick: () => menuBelow(citeBtn, citeItems([p.id])) });
  const pdfBtn: HTMLElement = h('button', {
    text: p.has_pdf ? 'PDF ▾' : 'Add PDF…',
    onclick: () => p.has_pdf
      ? menuBelow(pdfBtn, [{ label: 'Open PDF', action: () => read(p, 'pdf') }, { label: 'Replace PDF…', action: () => choosePdf(p) }])
      : choosePdf(p),
  });
  box.replaceChildren(h('div.detail', { 'data-id': p.id },
    h('div.actions', {},
      p.has_epub ? h('button.primary', { text: 'Read', onclick: () => read(p, 'reading') })
        : h('button', { text: 'Make Reading View', onclick: () => makeReadingView(p) }),
      pdfBtn,
      p.has_pdf ? null : h('button', { text: 'Find PDF', onclick: () => findPdf(p) }),
      citeBtn,
      h('button', { text: 'Edit…', title: 'Edit title, authors, journal, date, DOI, abstract', onclick: () => editPaper(p) }),
      h('div.spacer'),
      h('button.icon' + (p.starred ? '.starred' : ''), { text: p.starred ? '★' : '☆', title: p.starred ? 'Unstar' : 'Star', onclick: () => after(api.setStarred(p.id, !p.starred)) }),
      h('button.icon' + (p.must_read ? '.starred' : ''), { text: '🔖', title: p.must_read ? 'Remove from Must Read' : 'Must Read: to read soon', style: p.must_read ? '' : 'opacity:.35', onclick: () => after(api.setMustRead(p.id, !p.must_read)) }),
      h('button.icon', { text: p.read ? '◯' : '●', title: p.read ? 'Mark as unread' : 'Mark as read', onclick: () => after(api.setRead(p.id, !p.read)) })),
    h('h2', { text: m.title || '(untitled)' }),
    h('p.authors', { text: m.authors.join(', ') }),
    h('p.venue', { text: [m.journal, [m.volume, m.issue && `(${m.issue})`].filter(Boolean).join(''), m.pages, m.date || p.year].filter(Boolean).join(' · ') }),
    ids.length ? h('p.ids', {}, link ? h('a', { href: '#', text: ids.join(' · '), onclick: (e: Event) => { e.preventDefault(); openUrl(link); } }) : ids.join(' · ')) : null,
    h('h4', { text: 'Folders' }),
    folders.length
      ? h('div.folders', {}, folders.map((f) => h('label.check', {},
        h('input', { type: 'checkbox', checked: p.collections.includes(f.id), onchange: (e: Event) => after(api.setMembership(f.id, p.id, (e.target as HTMLInputElement).checked)) }),
        folderPath(f, folders))))
      : h('p.hint', {}, 'No folders yet. ', h('a', { href: '#', text: 'New folder…', onclick: (e: Event) => { e.preventDefault(); newFolder(null); } })),
    h('h4', { text: 'Tags' }),
    h('div.tags', {}, p.tags.map((t) => h('span.tag', {}, `#${t}`, h('button', { text: '×', title: 'Remove tag', onclick: () => after(api.setTag(p.id, t, false)) }))), tagInput),
    m.abstract_text ? h('h4', { text: 'Abstract' }) : null,
    m.abstract_text ? h('p.abstract', { text: m.abstract_text }) : null,
    h('p.hint.drop', { text: p.has_pdf ? 'Drop a PDF here to replace this paper’s PDF.' : 'Drop the PDF onto this paper to add it.' }),
    h('button.danger.delete', { text: 'Delete Paper…', onclick: () => deletePaper(p) }),
  ));
}

function folderPath(f: Folder, all: Folder[]): string {
  const names = [f.name];
  let cur = f.parent;
  for (let i = 0; cur && i < 20; i++) { const x = all.find((y) => y.id === cur); if (!x) break; names.unshift(x.name); cur = x.parent; }
  return names.join(' › ');
}

// ---- actions ----

async function work<T>(label: string, p: () => Promise<T>): Promise<T | undefined> {
  busy++;
  status(label, 'busy');
  try { return await p(); }
  catch (e) { status(String(e), 'error'); return undefined; }
  finally { busy--; if (busy === 0 && $('#status').classList.contains('busy')) status(''); }
}

async function addPaper() {
  const input = await prompt('Add a paper', '', 'DOI, PMCID, PubMed ID, arXiv id or link', 'Add');
  if (!input) return;
  const r = await work(`Adding ${input}…`, () => api.addPaper(input));
  if (!r) return;
  source = { kind: 'all' };
  query = '';
  $<HTMLInputElement>('#search').value = '';
  selected = r.paper.id;
  await reload();
  select(r.paper.id);
  if (r.note) status(`Added “${r.paper.meta.title}”, without a reading view: ${r.note}`, 'error');
  else status(`Added “${r.paper.meta.title}”`);
}

async function choosePdf(p: Paper) {
  const file = await openDialog({ title: p.has_pdf ? 'Replace PDF' : 'Add PDF', filters: [{ name: 'PDF', extensions: ['pdf'] }] });
  if (typeof file === 'string') attachPdf(p, file);
}

async function attachPdf(p: Paper, path: string) {
  if (p.has_pdf && !(await ask(`Replace the PDF of “${p.meta.title}”?`, { title: 'Replace PDF', kind: 'warning' }))) return;
  const note = await work('Adding the PDF…', () => api.attachPdf(p.id, path));
  await reload();
  if (note === undefined) return;
  if (note) status(`Added the PDF. No reading view: ${note}`, 'error');
  else status('Added the PDF');
}

async function findPdf(p: Paper) {
  const r = await work('Looking for an open-access PDF…', () => api.findPdf(p.id));
  if (r === undefined) return;
  status(r ? `Found the PDF (${r})` : 'No open-access PDF found');
  reload();
}

async function makeReadingView(p: Paper) {
  const ok = await work('Making the reading view…', async () => { await api.makeReadingView(p.id); return true; });
  if (ok) status('Made the reading view');
  reload();
}

async function editPaper(p: Paper) {
  if (await editDetails(p)) { status('Saved the details'); await reload(); }
}

async function deletePapers(ps: Paper[]) {
  if (!(await ask(`Delete ${ps.length} papers? Their files, highlights and notes go too, on every device.`, { title: 'Delete Papers', kind: 'warning' }))) return;
  selected = null; picked.clear();
  after(Promise.all(ps.map((p) => api.deletePaper(p.id))));
}

async function deletePaper(p: Paper) {
  if (!(await ask(`Delete “${p.meta.title}”? Its files, highlights and notes go too, on every device.`, { title: 'Delete Paper', kind: 'warning' }))) return;
  if (selected === p.id) selected = null;
  picked.delete(p.id);
  after(api.deletePaper(p.id));
}

async function read(p: Paper, mode: 'reading' | 'pdf') {
  $('#library').hidden = true;
  await openReader(p, mode, async () => { $('#library').hidden = false; await reload(); });
}

// ---- wiring ----

listen<string>('status', (e) => { if (busy) status(e.payload, 'busy'); });

$('#add').onclick = addPaper;
$<HTMLSelectElement>('#sort').onchange = (e) => { sort = (e.target as HTMLSelectElement).value; localStorage.setItem('sort', sort); reloadList(); };
let searchTimer: number | undefined;
$<HTMLInputElement>('#search').oninput = (e) => {
  clearTimeout(searchTimer);
  searchTimer = window.setTimeout(() => { query = (e.target as HTMLInputElement).value; reloadList(); }, 150);
};
$('#settings').onclick = async () => {
  const info = await api.info();
  menuBelow($('#settings'), [
    { label: `Library: ${info.library}`, action: () => {}, disabled: true },
    { label: `This device: ${info.device_id}`, action: () => {}, disabled: true },
    '-',
    { label: 'Use Another Library Folder…', action: async () => {
      const dir = await openDialog({ directory: true, title: 'Library folder' });
      if (typeof dir !== 'string') return;
      try { await api.chooseLibrary(dir); source = { kind: 'all' }; selected = null; reload(); status(`Library: ${dir}`); }
      catch (e) { status(String(e), 'error'); }
    } },
  ]);
};

document.addEventListener('keydown', (e) => {
  if (!$('#reader').hidden || document.querySelector('.modal-back')) return;
  const mod = e.metaKey || e.ctrlKey;
  if (mod && e.key === 'n') { e.preventDefault(); addPaper(); return; }
  if (mod && e.key === 'f') { e.preventDefault(); $('#search').focus(); return; }
  if ((e.target as HTMLElement).closest?.('input, textarea, select')) return;
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    const i = rows.findIndex((r) => r.paper.id === selected);
    const j = Math.max(0, Math.min(rows.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)));
    if (!rows[j]) return;
    if (e.shiftKey && anchor) click({ shiftKey: true, metaKey: false, ctrlKey: false } as MouseEvent, rows[j].paper.id);
    else select(rows[j].paper.id);
  } else if (mod && e.key === 'a') {
    e.preventDefault();
    picked = new Set(rows.map((r) => r.paper.id));
    paintSelection();
  } else if (e.key === 'Enter' && selected) {
    const p = rows.find((r) => r.paper.id === selected)?.paper;
    if (p) read(p, p.has_epub ? 'reading' : 'pdf');
  }
});

// ---- dragging papers and folders onto the sidebar (LibraryWindow.swift's dropDestination) ----

function descendantOf(folder: string, ancestor: string): boolean {
  for (let cur: string | null | undefined = folder, i = 0; cur && i < 50; i++) {
    if (cur === ancestor) return true;
    cur = side?.folders.find((f) => f.id === cur)?.parent;
  }
  return false;
}

setupDrag({
  begin(el) {
    if (el.dataset.drag === 'paper') {
      const id = el.dataset.id!;
      if (!picked.has(id)) select(id);
      const ps = pickedPapers();
      return { payload: { kind: 'papers', ids: ps.map((p) => p.id) },
        label: ps.length > 1 ? `${ps.length} papers` : ps[0]?.meta.title || '1 paper' };
    }
    const id = el.dataset.drag!.replace(/^folder:/, '');
    return { payload: { kind: 'folder', id }, label: `📁 ${side?.folders.find((f) => f.id === id)?.name || 'Folder'}` };
  },
  accepts(p: Payload, t) {
    const [kind, ...rest] = t.dataset.drop!.split(':');
    const id = rest.join(':');
    if (p.kind === 'papers') return kind === 'folder' || kind === 'tag';
    // a folder goes into another folder (not itself or its own subfolder), or to the top level
    if (kind === 'folders-root') return !!side?.folders.find((f) => f.id === p.id)?.parent;
    return kind === 'folder' && !descendantOf(id, p.id) && side?.folders.find((f) => f.id === p.id)?.parent !== id;
  },
  drop(p: Payload, t) {
    const [kind, ...rest] = t.dataset.drop!.split(':');
    const id = rest.join(':');
    if (p.kind === 'folder') {
      after(api.moveFolder(p.id, kind === 'folders-root' ? null : id));
      return;
    }
    const n = p.ids.length, what = n === 1 ? '1 paper' : `${n} papers`;
    if (kind === 'folder') {
      const name = side?.folders.find((f) => f.id === id)?.name;
      after(Promise.all(p.ids.map((pid) => api.setMembership(id, pid, true))).then(() => status(`Added ${what} to “${name}”`)));
    } else {
      after(Promise.all(p.ids.map((pid) => api.setTag(pid, id, true))).then(() => status(`Tagged ${what} #${id}`)));
    }
  },
});

// Dropping PDFs: onto a paper in the list or onto the details pane gives that paper its PDF; in
// the reader, onto the paper being read.
getCurrentWebview().onDragDropEvent(async (ev) => {
  const t = ev.payload;
  document.querySelectorAll('.drop-target').forEach((x) => x.classList.remove('drop-target'));
  if (t.type === 'leave') return;
  const target = (): { el: HTMLElement | null; id: string | null } => {
    if (!('position' in t)) return { el: null, id: null };
    const x = t.position.x / devicePixelRatio, y = t.position.y / devicePixelRatio;
    if (!$('#reader').hidden) return { el: $('#reader'), id: readerPaper() };
    const el = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest('#list .row, #detail .detail') as HTMLElement | null;
    return { el, id: el?.dataset.id || null };
  };
  const { el, id } = target();
  if (t.type === 'over' || t.type === 'enter') { el?.classList.add('drop-target'); return; }
  if (t.type !== 'drop') return;
  const pdf = t.paths.find((x) => x.toLowerCase().endsWith('.pdf'));
  if (!pdf) { status('Only PDFs can be dropped here.', 'error'); return; }
  if (!id) { status('Drop the PDF onto a paper to give it that PDF.', 'error'); return; }
  const p = await api.paper(id);
  if (!p) return;
  if (!$('#reader').hidden) { closeReader(); }
  await attachPdf(p, pdf);
});

start();
