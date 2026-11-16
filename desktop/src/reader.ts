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
import { fill, h, $, status, copy, menu, type MenuItem } from './ui';
import { showPdf, closePdf, pdfZoom, pdfFind, pdfFindStep, pdfFindClear } from './pdf';
import { closeFigureOverlay, figureList, figureOpen, showFigureGrid, showFigureViewer, type Fig } from './figures';
import { openFind, closeFind, findOpen, findStep, findUpdate, type FindResult, type FindTarget } from './find';

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
// highlight mode (Highlights.swift): every finished selection is highlighted at once in this colour
let hlMode = localStorage.getItem('highlightMode') === '1';
let hlColor = COLORS.includes(localStorage.getItem('highlightModeColor') as any) ? localStorage.getItem('highlightModeColor')! : 'yellow';
// figures in the text as cards (default) or in full (FigureSetting)
let figureCards = localStorage.getItem('readerFigureCards') !== '0';
// the reader's theme (ReaderTheme.swift)
const THEMES = ['automatic', 'light', 'paper', 'quiet', 'black'] as const;
let theme: string = THEMES.includes(localStorage.getItem('readerTheme') as any) ? localStorage.getItem('readerTheme')! : 'automatic';
// the paper's figures, as the page reported them when it loaded
let figures: Fig[] = [];

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
      if (body === '__ready__') pageReady();
      else if (typeof body === 'string' && body.startsWith('__hl__')) selectionDone(body.slice(6).replace(/^:/, ''));
      else editAnnotation(String(body));
      break;
    case 'page':     // pagedJS loaded: tell it the mode (PageMessageHandler in Swift)
      if (body === 'hello') run(pagedCall());
      break;
    case 'pos':      // the heading at the top of the screen
      markToc(String(body));
      break;
    case 'fig': {      // the page's figures as it loads, or a tapped mention, card or image
      const list = figureList(body);
      if (list) { figures = list; figuresButton(); break; }
      const o = figureOpen(body);
      if (o) { selectionBar(null); showFigureViewer(o, refocus, readerDark()); }
      break;
    }
    case '__selection':
      selectionBar(hlMode ? null : body);     // in highlight mode the selection is highlighted at once
      break;
    case '__key':
      if (body === 'highlight') highlight(hlColor);
      else if (body === 'hlmode') setHighlightMode(!hlMode);
      else if (body === 'figures') openFigureGrid();
      else if (body === 'escape') { if (!closeOverlay() && !closeAppearance() && !closeFind()) selectionBar(null); }
      else if (body === 'find') startFind();
      else if (body === 'findnext' || body === 'findprev') { if (findOpen()) findStep(body === 'findnext'); else startFind(); }
      else if (body === 'larger') scale(0.1);
      else if (body === 'smaller') scale(-0.1);
      break;
    case '__link':
      openUrl(String(body));
      break;
    case '__cite':     // the citation card's "Add to Lunta"
      citeFromPage(String(body));
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
  if (!p.read) api.markOpened(p.id).catch(() => {});
  await show();
}

export async function closeReader() {
  if (!paper) return;
  await savePosition();
  clearInterval(saver);
  closeFind();
  closeAppearance();
  closePdf();
  while (closeFigureOverlay());
  figures = [];
  frame = null;
  paper = null;
  $('#reader').hidden = true;
  $('#reader').replaceChildren();
  onClose?.();
}

function render() {
  const p = paper!;
  closeFind();
  closeAppearance();
  const seg = (m: 'reading' | 'pdf', label: string, enabled: boolean) =>
    h('button' + (mode === m ? '.on' : ''), { text: label, disabled: !enabled, onclick: () => { if (mode !== m) { savePosition(); mode = m; render(); show(); } } });
  const reading = mode === 'reading';
  const hlBtn: HTMLElement = h('button.icon', { text: '☰', title: 'Highlights and notes', onclick: () => toggleList() });
  const modeBtn = reading ? h('button.icon#hlmode-btn', {
    text: '🖍', onclick: () => setHighlightMode(!hlMode),
    oncontextmenu: (e: MouseEvent) => { e.preventDefault(); highlightMenu(e.clientX, e.clientY); },
  }) : null;
  $('#reader').replaceChildren(
    h('header.toolbar', {},
      h('button', { text: '‹ Library', onclick: closeReader, title: 'Back to the library (Esc)' }),
      h('div.title', { text: p.meta.title }),
      h('div.segmented', {}, seg('reading', 'Reading', p.has_epub), seg('pdf', 'PDF', p.has_pdf)),
      // figures · find · highlighter · highlights and notes · Aa (text size, theme, layout, figures)
      h('div.tools', {},
        reading ? h('button.icon#figures-btn', { text: '🖼', hidden: !figures.length, onclick: () => openFigureGrid() }) : null,
        h('button.icon#find-btn', { text: '🔍', title: 'Find in paper (Ctrl+F)', 'aria-label': 'Find in paper', onclick: () => startFind() }),
        modeBtn,
        reading ? h('button.icon.hl-more', { text: '▾', title: 'Highlight colour, and highlight the selection now', onclick: (e: MouseEvent) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); highlightMenu(r.left, r.bottom + 4);
        } }) : null,
        hlBtn,
        reading ? h('button.icon#appearance-btn', { text: 'Aa', title: 'Text size, theme and layout', 'aria-label': 'Text and theme', onclick: (e: MouseEvent) => toggleAppearance(e.currentTarget as HTMLElement) }) : null,
        reading ? null : h('button.icon', { text: '−', title: 'Zoom out (Ctrl+−)', onclick: () => pdfZoom(1 / 1.15) }),
        reading ? null : h('button.icon', { text: '+', title: 'Zoom in (Ctrl+=)', onclick: () => pdfZoom(1.15) }))),
    h('div.reader-body', {},
      reading ? h('nav#toc') : null,
      h('div#page'),
      h('aside#hl-list', { hidden: !showingList })),
    h('div#resume', { hidden: true }),
    h('div#selbar', { hidden: true }, COLORS.map((c) => h(`button.swatch.c-${c}`, { title: `Highlight ${c}`, onmousedown: (e: Event) => { e.preventDefault(); highlight(c); } })),
      h('button', { text: 'Note…', onmousedown: (e: Event) => { e.preventDefault(); noteOnSelection(); } })),
  );
  modeButton();
  figuresButton();
  $('#reader').dataset.theme = reading ? theme : 'automatic';
}

// ---- themes ----

function themeName(t: string) { return t[0].toUpperCase() + t.slice(1); }

/** Sets the theme in the page (and the space around it), keeping the place. */
function setTheme(t: string) {
  theme = t;
  localStorage.setItem('readerTheme', t);
  if (mode === 'reading') run(`window.luntaSetTheme && luntaSetTheme('${t}')`);
  $('#reader').dataset.theme = mode === 'reading' ? t : 'automatic';
}

/** Is the reader shown dark (the theme's appearance, or the system's)? */
export function readerDark() {
  if (theme === 'quiet' || theme === 'black') return true;
  if (theme === 'light' || theme === 'paper') return false;
  return matchMedia('(prefers-color-scheme: dark)').matches;
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
    figures = [];
    figuresButton();
    lastAuto = null;
    const base = convertFileSrc('', 'lunta');   // lunta://localhost/ (http://lunta.localhost/ on Windows)
    frame = h('iframe', { src: `${base}${book.path}?scale=${fontScale}&paged=${paged ? 1 : 0}&cards=${figureCards ? 1 : 0}&theme=${theme}`, title: 'Reading view' }) as HTMLIFrameElement;
    frame.addEventListener('load', () => frame?.contentWindow?.focus());
    page.replaceChildren(frame);
  } catch (e) {
    page.replaceChildren(h('div.empty', {}, h('p', { text: `No reading view: ${e}` })));
  }
}

/** The page loaded: draw the highlights, then go to where the reader stopped (on any device). */
async function pageReady() {
  run(hlModeJS());
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

/** Scroll or turn pages (the page is told; re-attaching the iframe would reload it and lose the place). */
function setPaged(on: boolean) {
  if (paged === on) return;
  paged = on;
  localStorage.setItem('readerPaged', paged ? '1' : '0');
  run(pagedCall());
}

function setFigureCards(on: boolean) {
  figureCards = on;
  localStorage.setItem('readerFigureCards', on ? '1' : '0');
  run(`window.luntaFigureCards && luntaFigureCards(${on})`);
}

// ---- Text & Theme (the iPhone's ReaderAppearanceSheet): everything about how the paper looks ----

function closeAppearance(): boolean {
  const p = document.querySelector('#appearance');
  if (!p) return false;
  p.remove();
  document.removeEventListener('mousedown', outsideAppearance, true);
  document.querySelector('#appearance-btn')?.classList.remove('on');
  return true;
}
function outsideAppearance(e: MouseEvent) {
  if (!(e.target as HTMLElement).closest('#appearance, #appearance-btn')) closeAppearance();
}

function toggleAppearance(anchor: HTMLElement) {
  if (closeAppearance()) return;
  const pct = h('span.ap-pct');
  const smaller = h('button', { text: 'A−', title: 'Smaller text (Ctrl+−)', 'aria-label': 'Smaller text', onclick: () => { scale(-0.1); sync(); } }) as HTMLButtonElement;
  const larger = h('button.ap-larger', { text: 'A+', title: 'Larger text (Ctrl+=)', 'aria-label': 'Larger text', onclick: () => { scale(0.1); sync(); } }) as HTMLButtonElement;
  const themes = h('div.ap-themes', {}, THEMES.map((t) => h('button.ap-theme', { 'data-theme': t, title: themeName(t), onclick: () => { setTheme(t); sync(); } },
    h('span.ap-swatch', { text: 'Aa' }), h('span.ap-name', { text: themeName(t) }))));
  const seg = (a: [string, () => void], b: [string, () => void]) => h('div.segmented', {},
    h('button', { text: a[0], onclick: () => { a[1](); sync(); } }), h('button', { text: b[0], onclick: () => { b[1](); sync(); } }));
  const layout = seg(['Scroll', () => setPaged(false)], ['Turn Pages', () => setPaged(true)]);
  const figs = seg(['As Cards', () => setFigureCards(true)], ['In Full', () => setFigureCards(false)]);
  function sync() {
    pct.textContent = `${Math.round(fontScale * 100)}%`;
    smaller.disabled = fontScale <= 0.7 + 1e-6;
    larger.disabled = fontScale >= 1.8 - 1e-6;
    themes.querySelectorAll<HTMLElement>('.ap-theme').forEach((b) => { b.classList.toggle('on', b.dataset.theme === theme); b.setAttribute('aria-pressed', String(b.dataset.theme === theme)); });
    const [s1, s2] = layout.querySelectorAll('button'); s1.classList.toggle('on', !paged); s2.classList.toggle('on', paged);
    const [f1, f2] = figs.querySelectorAll('button'); f1.classList.toggle('on', figureCards); f2.classList.toggle('on', !figureCards);
  }
  const panel = h('div#appearance', { role: 'dialog', 'aria-label': 'Text and theme' },
    h('h4', { text: 'Text Size' }), h('div.ap-size', {}, smaller, pct, larger),
    h('h4', { text: 'Theme' }), themes,
    h('h4', { text: 'Layout' }), layout,
    h('h4', { text: 'Figures in the Text' }), figs);
  panel.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeAppearance(); frame?.contentWindow?.focus(); } });
  sync();
  document.body.append(panel);
  const r = anchor.getBoundingClientRect();
  panel.style.top = `${r.bottom + 6}px`;
  panel.style.left = `${Math.max(8, Math.min(r.right - panel.offsetWidth, innerWidth - panel.offsetWidth - 8))}px`;
  anchor.classList.add('on');
  setTimeout(() => document.addEventListener('mousedown', outsideAppearance, true), 0);
}

// ---- find in paper ----

/** The reading view's page script (generated/find.js). */
const pageFind: FindTarget = {
  find: (q) => run<FindResult>(`window.luntaFind ? luntaFind(${JSON.stringify(q)}, false) : null`),
  step: (forward) => run<FindResult>(forward ? 'window.luntaFindNext ? luntaFindNext() : null' : 'window.luntaFindPrev ? luntaFindPrev() : null'),
  clear: () => { run('window.luntaFindClear && luntaFindClear()'); },
};
/** PDF.js's find controller (its answers come as it searches the pages). */
const pdfFinder: FindTarget = {
  find: async (q) => { pdfFind(q, findUpdate); return null; },
  step: async (forward) => { pdfFindStep(forward); return null; },
  clear: () => pdfFindClear(),
};

/** Opens the find bar (Ctrl+F, the toolbar). Closing it after going to a match offers the way back. */
async function startFind() {
  if (!paper || !$('#reader') || document.querySelector('.modal-back, #figure, #figures')) return;
  closeAppearance();
  if (findOpen()) { openFind($('#reader'), pageFind, () => {}); return; }   // focuses it
  const reading = mode === 'reading';
  if (reading && !frame) return;
  const origin = reading ? await run<number>('luntaReadingOffset()') : null;
  if (findOpen()) return;
  document.querySelector('#findback')?.remove();
  openFind($('#reader'), reading ? pageFind : pdfFinder, (moved) => {
    if (reading && frame) frame.contentWindow?.focus();
    if (!moved || !reading || typeof origin !== 'number' || !paper) return;
    const back = h('button#findback', { text: '↩ Back to where you were', onclick: () => { back.remove(); run(`luntaScrollToOffset(${Math.floor(origin)})`); frame?.contentWindow?.focus(); } });
    $('#reader')?.append(back);
    setTimeout(() => back.remove(), 6000);
  });
}

// ---- citation cards ----

/** "Add to Lunta" on a citation card (lunta://cite?doi=… or ?arxiv=…): the paper is found in the
 *  library or added, and the card says which (ExternalLinks.swift). */
async function citeFromPage(link: string) {
  let doi = '', arxiv = '';
  try { const u = new URL(link); doi = u.searchParams.get('doi') || ''; arxiv = u.searchParams.get('arxiv') || ''; } catch { /* not a link */ }
  const forPaper = paper?.id;
  let label = 'Couldn’t add it';
  try {
    const r = await api.citePaper(doi, arxiv);
    label = r.label;
    if (r.label === 'Added to your library') status('Added to your library');
  } catch (e) { status(String(e), 'error'); }
  if (paper?.id !== forPaper || mode !== 'reading') return;
  run(`window.luntaCiteDone && luntaCiteDone(${JSON.stringify(label)})`);
}

// keys for the reader when the page doesn't have the keyboard
document.addEventListener('keydown', (e) => {
  if (!paper || $('#reader').hidden || (e.target as HTMLElement).closest?.('input, textarea')) return;
  if (document.querySelector('.modal-back, #figure, #figures')) { if (e.key === 'Escape') closeOverlay(); return; }
  const mod = e.metaKey || e.ctrlKey;
  if (e.key === 'Escape') { if (!closeAppearance() && !closeFind()) closeReader(); return; }
  if (mod && !e.shiftKey && !e.altKey && e.code === 'KeyF') { e.preventDefault(); startFind(); return; }
  if (e.key === 'F3' || (mod && !e.altKey && e.code === 'KeyG')) { e.preventDefault(); if (findOpen()) findStep(!e.shiftKey); else startFind(); return; }
  if (mode === 'reading' && e.ctrlKey && e.altKey && !e.shiftKey && e.code === 'KeyH') { e.preventDefault(); setHighlightMode(!hlMode); return; }
  if (mod && e.shiftKey && !e.altKey && e.code === 'KeyH') { e.preventDefault(); highlight(hlColor); return; }
  if (mode === 'reading' && mod && e.shiftKey && !e.altKey && e.code === 'KeyF') { e.preventDefault(); openFigureGrid(); return; }
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

// ---- highlight mode ----

function setHighlightMode(on: boolean) {
  hlMode = on;
  localStorage.setItem('highlightMode', on ? '1' : '0');
  if (mode === 'reading') run(hlModeJS());
  if (on) selectionBar(null);
  modeButton();
}

function setHighlightColor(c: string) {
  hlColor = c;
  localStorage.setItem('highlightModeColor', c);
  if (mode === 'reading') run(hlModeJS());
  modeButton();
}

/** Highlight mode and its colour in the page (the selection itself shows in the colour while on). */
function hlModeJS() {
  return `window.luntaHLMode = ${hlMode}; window.luntaHLColor = '${hlColor}';` +
    `document.documentElement.classList.toggle('m-hlmode', ${hlMode}); document.documentElement.setAttribute('data-hlcolor', '${hlColor}');`;
}

/** The toggle shows the mode: tinted with the chosen colour while on. */
function modeButton() {
  const b = document.querySelector<HTMLElement>('#hlmode-btn');
  if (!b) return;
  b.classList.toggle('on', hlMode);
  b.style.setProperty('--hl', `var(--${hlColor})`);
  b.setAttribute('aria-pressed', String(hlMode));
  b.title = hlMode
    ? `Highlight mode is on: selections are highlighted in ${hlColor} at once (Ctrl+Alt+H). Right-click for the colour.`
    : 'Highlight mode: highlight every selection at once (Ctrl+Alt+H). Right-click for the colour; Ctrl+Shift+H highlights one selection.';
}

function highlightMenu(x: number, y: number) {
  const items: MenuItem[] = [
    { label: 'Highlight Mode (Ctrl+Alt+H)', checked: hlMode, action: () => setHighlightMode(!hlMode) },
    { label: 'Highlight Selection (Ctrl+Shift+H)', action: () => highlight(hlColor) },
    '-',
    ...COLORS.map((c) => ({ label: c[0].toUpperCase() + c.slice(1), checked: c === hlColor, action: () => setHighlightColor(c) })),
  ];
  menu(x, y, items);
}

// the highlight just made in highlight mode (AutoHighlighter in Highlights.swift): a selection
// overlapping it soon after (the selection was adjusted) replaces it instead of adding another
let lastAuto: { id: string; start: number; end: number; at: number } | null = null;
let autoQueue: Promise<void> = Promise.resolve();

/** Highlight mode: the page coloured a finished selection (mouse released) and sends it to be saved. */
function selectionDone(json: string) {
  autoQueue = autoQueue.then(async () => {
    if (!hlMode || mode !== 'reading' || !paper) return;
    let sel: PageSelection | null = null;
    try { sel = json ? JSON.parse(json) as PageSelection : null; } catch { sel = null; }
    if (!sel) sel = await pageSelection();
    if (!sel || !paper) { await refreshHighlights(); return; }
    const id = paper.id;
    const end = sel.start + sel.quote.length;
    try {
      const l = lastAuto;
      if (l && Date.now() - l.at < 60_000 && sel.start < l.end && end > l.start) {
        const old = (await api.annotations(id)).find((a) => a.id === l.id);
        if (old && !old.note.trim()) {
          if (sel.start === l.start && end === l.end) { run('window.getSelection().removeAllRanges()'); await refreshHighlights(); return; }
          await api.deleteAnnotation(id, old.id);
        }
      }
      const a = await api.addHighlight(id, sel, hlColor, '');
      lastAuto = { id: a.id, start: sel.start, end, at: Date.now() };
      run('window.getSelection().removeAllRanges()');
      selectionBar(null);
      await refreshHighlights();
    } catch (e) { status(String(e), 'error'); }
  });
}

// ---- figures ----

function figuresButton() {
  const b = document.querySelector<HTMLElement>('#figures-btn');
  if (!b) return;
  b.hidden = !figures.length || mode !== 'reading';
  b.title = `Figures (${figures.length}) — Ctrl+Shift+F`;
}

/** Gives the keyboard back to the page (closing a figure or the grid). */
function refocus() {
  if (document.querySelector('#figures')) (document.querySelector('#figures') as HTMLElement).focus();
  else frame?.contentWindow?.focus();
}

function openFigureGrid() {
  if (mode !== 'reading' || !figures.length) return;
  showFigureGrid(figures, {
    cards: figureCards,
    setCards: setFigureCards,
    theme,
    open: async (i) => {
      // as the page shows it now (with the highlights in its caption), else as it was listed
      const json = await run<string>(`window.luntaFigures ? luntaFigures.data(${i}) : ''`);
      let o = null;
      try { o = json ? figureOpen(JSON.parse(json)) : null; } catch { o = null; }
      showFigureViewer(o || { set: figures, index: i, start: 0, panel: '' }, refocus, readerDark());
    },
    done: refocus,
  });
}

function closeOverlay(): boolean {
  if (closeFigureOverlay()) return true;
  const all = document.querySelectorAll('.modal-back');
  const o = all[all.length - 1];
  if (o) { o.remove(); return true; }
  return false;
}

