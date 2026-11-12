// One paper: the reading view (the converted EPUB with the Apple apps' reader CSS and scripts,
// served by the lunta:// scheme into an iframe) or the original PDF (PDF.js). Mirrors
// apple/Mac/ReaderWindow.swift: contents on the left, highlights in four colours with notes,
// scroll or page mode, text size, and the reading position synced through the library folder.
//
// The page's scripts post messages through window.webkit.messageHandlers (src/reader/bridge.js
// turns those into postMessage to this window); `run()` is the apps' evaluateJavaScript.
import { convertFileSrc } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { api, type Annotation, type Paper, type PageSelection, type TocItem } from './api';
import { fill, h, $, status, copy } from './ui';
import { showPdf, closePdf, pdfZoom } from './pdf';

const COLORS = ['yellow', 'green', 'blue', 'pink'] as const;

let paper: Paper | null = null;
let mode: 'reading' | 'pdf' = 'reading';
let frame: HTMLIFrameElement | null = null;
let toc: TocItem[] = [];
let onClose: (() => void) | null = null;
let fontScale = Number(localStorage.getItem('readerFontScale') || '1');
let paged = localStorage.getItem('readerPaged') === '1';
let restored = false;
let saver: number | undefined;
let showingList = false;

export const readerPaper = () => paper?.id || null;

// ---- talking to the page ----

let seq = 0;
const pending = new Map<number, (v: unknown) => void>();

/** Runs a script in the page and resolves to its result (null on error or timeout). */
function run<T = unknown>(code: string): Promise<T | null> {
  return new Promise((resolve) => {
    const w = frame?.contentWindow;
    if (!w) return resolve(null);
    const id = ++seq;
    pending.set(id, resolve as (v: unknown) => void);
    w.postMessage({ lunta: 'eval', id, code }, '*');
    setTimeout(() => { if (pending.delete(id)) resolve(null); }, 5000);
  });
}

const pagedCall = () => `luntaSetPaged(${paged}, 30, 40, false)`;

window.addEventListener('message', (e) => {
  if (!frame || e.source !== frame.contentWindow) return;
  const d = e.data;
  if (!d || typeof d !== 'object') return;
  if (d.lunta === 'result') {
    const r = pending.get(d.id);
    if (r) { pending.delete(d.id); r(d.error ? null : d.result); }
    return;
  }
  if (d.lunta !== 'message') return;
  const body = d.body;
  switch (d.name) {
    case 'ann':      // the page is ready, or a highlight was clicked
      if (body === '__ready__') pageReady(); else editAnnotation(String(body));
      break;
    case 'page':     // pagedJS loaded: tell it the mode (PageMessageHandler in Swift)
      if (body === 'hello') run(pagedCall());
      break;
    case 'pos':      // the heading at the top of the screen
      markToc(String(body));
      break;
    case 'fig':
      showFigure(body.src, body.caption);
      break;
    case '__selection':
      selectionBar(body);
      break;
    case '__key':
      if (body === 'highlight') highlight('yellow');
      else if (body === 'escape') { if (!closeOverlay()) selectionBar(null); }
      else if (body === 'larger') scale(0.1);
      else if (body === 'smaller') scale(-0.1);
      break;
    case '__link':
      openUrl(String(body));
      break;
  }
});

// ---- opening and closing ----

export async function openReader(p: Paper, m: 'reading' | 'pdf', closed: () => void) {
  paper = p;
  onClose = closed;
  mode = m === 'reading' && !p.has_epub ? 'pdf' : m;
  if (mode === 'pdf' && !p.has_pdf && p.has_epub) mode = 'reading';
  $('#reader').hidden = false;
  render();
  if (!p.read) api.setRead(p.id, true).catch(() => {});
  await show();
}

export async function closeReader() {
  if (!paper) return;
  await savePosition();
  clearInterval(saver);
  closePdf();
  frame = null;
  paper = null;
  $('#reader').hidden = true;
  $('#reader').replaceChildren();
  onClose?.();
}

function render() {
  const p = paper!;
  const seg = (m: 'reading' | 'pdf', label: string, enabled: boolean) =>
    h('button' + (mode === m ? '.on' : ''), { text: label, disabled: !enabled, onclick: () => { if (mode !== m) { savePosition(); mode = m; render(); show(); } } });
  const reading = mode === 'reading';
  const hlBtn: HTMLElement = h('button.icon', { text: '☰', title: 'Highlights and notes', onclick: () => toggleList() });
  $('#reader').replaceChildren(
    h('header.toolbar', {},
      h('button', { text: '‹ Library', onclick: closeReader, title: 'Back to the library (Esc)' }),
      h('div.title', { text: p.meta.title }),
      h('div.segmented', {}, seg('reading', 'Reading', p.has_epub), seg('pdf', 'PDF', p.has_pdf)),
      h('div.tools', {},
        reading ? h('button.icon.hl-yellow', { text: '🖍', title: 'Highlight the selected text (Ctrl+Shift+H)', onclick: () => highlight('yellow') }) : null,
        hlBtn,
        reading ? h('button.icon#paged-btn' + (paged ? '.on' : ''), { text: '📖', title: paged ? 'Scroll instead of turning pages' : 'Turn pages (arrow keys, space or swipe)', onclick: (e: Event) => togglePaged(e) }) : null,
        h('button.icon', { text: 'A−', title: 'Smaller text', onclick: () => (reading ? scale(-0.1) : pdfZoom(1 / 1.15)) }),
        h('button.icon', { text: 'A+', title: 'Larger text', onclick: () => (reading ? scale(0.1) : pdfZoom(1.15)) }))),
    h('div.reader-body', {},
      reading ? h('nav#toc') : null,
      h('div#page'),
      h('aside#hl-list', { hidden: !showingList })),
    h('div#resume', { hidden: true }),
    h('div#selbar', { hidden: true }, COLORS.map((c) => h(`button.swatch.c-${c}`, { title: `Highlight ${c}`, onmousedown: (e: Event) => { e.preventDefault(); highlight(c); } })),
      h('button', { text: 'Note…', onmousedown: (e: Event) => { e.preventDefault(); noteOnSelection(); } })),
  );
}

async function show() {
  const p = paper!;
  restored = false;
  clearInterval(saver);
  closePdf();
  const page = $('#page');
  if (mode === 'pdf') {
    frame = null;
    try { await showPdf(page, p.id); }
    catch (e) { page.replaceChildren(h('div.empty', {}, h('p', { text: `Couldn't open the PDF: ${e}` }))); }
    refreshList();
    return;
  }
  if (!p.has_epub) {
    page.replaceChildren(h('div.empty', {}, h('p', { text: 'No reading view yet. Use “Make Reading View” in the library.' })));
    return;
  }
  try {
    const book = await api.openBook(p.id);
    toc = book.toc;
    renderToc();
    const base = convertFileSrc('', 'lunta');   // lunta://localhost/ (http://lunta.localhost/ on Windows)
    frame = h('iframe', { src: `${base}${book.path}?scale=${fontScale}&paged=${paged ? 1 : 0}`, title: 'Reading view' }) as HTMLIFrameElement;
    frame.addEventListener('load', () => frame?.contentWindow?.focus());
    page.replaceChildren(frame);
  } catch (e) {
    page.replaceChildren(h('div.empty', {}, h('p', { text: `No reading view: ${e}` })));
  }
}

/** The page loaded: draw the highlights, then go to where the reader stopped (on any device). */
async function pageReady() {
  await refreshHighlights();
  if (restored || !paper) return;
  restored = true;
  const id = paper.id;
  const spot = await api.positionLatest(id).catch(() => null);
  if (spot && spot.offset > 300) {
    await new Promise((r) => setTimeout(r, 350));     // let figures and tables settle first
    await run(`luntaScrollToOffset(${Math.floor(spot.offset)})`);
    const myLabel = (await api.info()).device_label;
    resumeNote(spot.mine || spot.device === myLabel ? 'Continuing where you left off' : `Continuing where you left off on your ${spot.device}`);
  }
  saver = window.setInterval(savePosition, 6000);
}

async function savePosition() {
  if (!paper || mode !== 'reading' || !frame || !restored) return;
  const o = await run<number>('luntaReadingOffset()');
  if (typeof o === 'number' && paper) api.positionSave(paper.id, o).catch(() => {});
}

function resumeNote(text: string) {
  const n = $('#resume');
  if (!n) return;
  n.textContent = `🔖 ${text}`;
  n.hidden = false;
  setTimeout(() => { n.hidden = true; }, 3000);
}

// ---- contents ----

function renderToc() {
  const nav = $('#toc');
  if (!nav) return;
  nav.replaceChildren(h('h4', { text: 'Contents' }), ...toc.map((t) => h('button.toc' + (t.level <= 1 ? '.top' : ''), {
    text: t.title, 'data-anchor': t.anchor, style: `padding-left:${10 + Math.max(0, t.level - 1) * 12}px`,
    onclick: () => {
      const safe = t.anchor.replace(/\\/g, '').replace(/'/g, "\\'");     // ReaderAssets.scrollJS
      run(`var e=document.getElementById('${safe}'); if(e) e.scrollIntoView({behavior:'smooth', block:'start'});`);
      markToc(t.anchor);
      frame?.contentWindow?.focus();
    },
  })));
}

function markToc(anchor: string) {
  document.querySelectorAll('#toc .toc').forEach((b) => b.classList.toggle('on', (b as HTMLElement).dataset.anchor === anchor));
}

// ---- text size and pages ----

function scale(d: number) {
  if (mode !== 'reading') { pdfZoom(d > 0 ? 1.15 : 1 / 1.15); return; }
  fontScale = Math.round(Math.min(1.8, Math.max(0.7, fontScale + d)) * 10) / 10;
  localStorage.setItem('readerFontScale', String(fontScale));
  run(`luntaSetTextScale(${fontScale})`);
}

function togglePaged(e?: Event) {
  paged = !paged;
  localStorage.setItem('readerPaged', paged ? '1' : '0');
  run(pagedCall());
  // only the button changes: re-attaching the iframe would reload the page and lose the place
  const b = (e?.currentTarget as HTMLElement) || document.querySelector('#paged-btn');
  if (b) { b.classList.toggle('on', paged); b.title = paged ? 'Scroll instead of turning pages' : 'Turn pages (arrow keys, space or swipe)'; }
  frame?.contentWindow?.focus();
}

// keys for the reader when the page doesn't have the keyboard
document.addEventListener('keydown', (e) => {
  if (!paper || $('#reader').hidden || (e.target as HTMLElement).closest('input, textarea')) return;
  if (document.querySelector('.modal-back, #figure')) { if (e.key === 'Escape') closeOverlay(); return; }
  const mod = e.metaKey || e.ctrlKey;
  if (e.key === 'Escape') { closeReader(); return; }
  if (mod && e.shiftKey && e.key.toLowerCase() === 'h') { e.preventDefault(); highlight('yellow'); return; }
  if (mod && (e.key === '=' || e.key === '+')) { e.preventDefault(); scale(0.1); return; }
  if (mod && e.key === '-') { e.preventDefault(); scale(-0.1); return; }
  if (mode === 'reading' && paged && !mod) {
    if (['ArrowRight', 'ArrowDown', 'PageDown', ' '].includes(e.key)) { e.preventDefault(); run(`luntaTurn(${e.shiftKey && e.key === ' ' ? -1 : 1})`); }
    if (['ArrowLeft', 'ArrowUp', 'PageUp'].includes(e.key)) { e.preventDefault(); run('luntaTurn(-1)'); }
  }
});

// ---- highlights ----

function selectionBar(r: { x: number; y: number; w: number; h: number } | null) {
  const bar = $('#selbar');
  if (!bar) return;
  if (!r || !frame || mode !== 'reading') { bar.hidden = true; return; }
  const f = frame.getBoundingClientRect();
  bar.hidden = false;
  const bw = bar.offsetWidth, bh = bar.offsetHeight;
  let top = f.top + r.y - bh - 8;
  if (top < f.top + 4) top = f.top + r.y + r.h + 8;
  bar.style.top = `${top}px`;
  bar.style.left = `${Math.max(f.left + 4, Math.min(f.left + r.x + r.w / 2 - bw / 2, f.right - bw - 4))}px`;
}

async function pageSelection(): Promise<PageSelection | null> {
  const json = await run<string>('luntaSelection()');
  if (!json) return null;
  try { return JSON.parse(json) as PageSelection; } catch { return null; }
}

async function highlight(color: string) {
  if (!paper || mode !== 'reading') return;
  const sel = await pageSelection();
  if (!sel) { status('Select some text first, then highlight it.'); return; }
  try {
    await api.addHighlight(paper.id, sel, color, '');
    run('window.getSelection().removeAllRanges()');
    selectionBar(null);
    await refreshHighlights();
  } catch (e) { status(String(e), 'error'); }
}

async function noteOnSelection() {
  if (!paper) return;
  const sel = await pageSelection();
  if (!sel) return;
  try {
    const a = await api.addHighlight(paper.id, sel, 'yellow', '');
    run('window.getSelection().removeAllRanges()');
    selectionBar(null);
    await refreshHighlights();
    openEditor(a);
  } catch (e) { status(String(e), 'error'); }
}

/** Draws the paper's highlights into the page (ReaderController.showAnnotations). */
async function refreshHighlights() {
  if (!paper) return;
  const list = await api.annotations(paper.id).catch(() => [] as Annotation[]);
  if (mode === 'reading' && frame) {
    const items = list.filter((a) => a.location === 'epub').map((a) => ({
      id: a.id, quote: a.quote, prefix: a.prefix, suffix: a.suffix, color: a.color, note: a.note, start: parseInt(a.position, 10) || 0,
    }));
    await run(`luntaApplyAnnotations(${JSON.stringify(JSON.stringify(items))})`);
  }
  renderList(list);
}

async function editAnnotation(id: string) {
  if (!paper) return;
  const a = (await api.annotations(paper.id)).find((x) => x.id === id);
  if (a) openEditor(a);
}

function openEditor(a: Annotation) {
  const note = h('textarea', { rows: 5, placeholder: 'Add a note' }) as HTMLTextAreaElement;
  note.value = a.note;
  let color = a.color || 'yellow';
  const swatches = h('div.swatches', {}, COLORS.map((c) => h(`button.swatch.c-${c}` + (c === color ? '.on' : ''), {
    title: c, onclick: (e: Event) => {
      color = c;
      swatches.querySelectorAll('.swatch').forEach((s) => s.classList.remove('on'));
      (e.currentTarget as HTMLElement).classList.add('on');
    },
  })));
  const close = () => back.remove();
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('div.modal.wide', {},
      h('h3', { text: a.note ? 'Note' : 'Highlight' }),
      h('blockquote', { text: `“${a.quote.trim()}”` }),
      h('h4', { text: 'Colour' }), swatches,
      h('h4', { text: 'Note' }), note,
      h('div.buttons', {},
        h('button', { text: 'Copy Quote', onclick: () => copy(a.quote.trim(), 'the quote') }),
        h('button.danger', { text: 'Delete Highlight', onclick: async () => { close(); await api.deleteAnnotation(a.paper_id, a.id); refreshHighlights(); } }),
        h('div.spacer'),
        h('button', { text: 'Cancel', onclick: close }),
        h('button.primary', { text: 'Save', onclick: async () => { close(); await api.saveAnnotation({ ...a, color, note: note.value.trim() }); refreshHighlights(); } }))));
  document.body.append(back);
  note.focus();
}

function toggleList() {
  showingList = !showingList;
  const l = $('#hl-list');
  if (l) l.hidden = !showingList;
  refreshList();
}

async function refreshList() {
  if (!paper || !showingList) return;
  renderList(await api.annotations(paper.id).catch(() => []));
}

function renderList(all: Annotation[]) {
  const box = $('#hl-list');
  if (!box || !showingList || !paper) return;
  const list = [...all].sort((a, b) => (parseInt(a.position, 10) || 0) - (parseInt(b.position, 10) || 0));
  const md = () => `## ${paper?.meta.title || 'Highlights'}\n\n` + list.map((a) => `> ${a.quote.trim()}\n${a.note ? `\n${a.note}\n` : ''}\n`).join('');
  fill(box,
    h('h4', { text: 'Highlights & Notes' }),
    list.length ? null : h('p.hint', { text: 'No highlights yet. Select text in the reading view, then pick a colour or Note.' }),
    ...list.map((a) => h('div.hl-item', {
      onclick: () => {
        if (a.location !== 'epub') { status('This highlight was made in the PDF on another device; PDF highlights show on the Mac and iPhone for now.'); return; }
        if (mode !== 'reading') return;
        const safe = a.id.replace(/[^A-Za-z0-9-]/g, '');
        run(`luntaShowAnnotation('${safe}')`);
      },
      oncontextmenu: (e: MouseEvent) => { e.preventDefault(); openEditor(a); },
    },
      h(`span.bar.c-${a.color || 'yellow'}`),
      h('div', {}, h('div.quote', { text: a.quote.trim() }), a.note ? h('div.note', { text: `✎ ${a.note}` }) : null,
        a.location === 'pdf' ? h('div.where', { text: 'in the PDF' }) : null),
      h('button.icon.edit', { text: '…', title: 'Edit', onclick: (e: Event) => { e.stopPropagation(); openEditor(a); } }))),
    list.length ? h('button', { text: 'Copy All as Markdown', onclick: () => copy(md(), 'highlights as Markdown') }) : null,
  );
}

// ---- figures ----

function showFigure(src: string, caption: string) {
  const back = h('div#figure.modal-back', { onclick: () => back.remove() },
    h('figure.figure-view', {}, h('img', { src }), caption ? h('figcaption', { text: caption }) : null));
  document.body.append(back);
}

function closeOverlay(): boolean {
  const o = document.querySelector('#figure, .modal-back');
  if (o) { o.remove(); return true; }
  return false;
}

