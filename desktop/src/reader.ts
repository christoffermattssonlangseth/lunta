// One paper: the reading view (the converted EPUB with the Apple apps' reader CSS and scripts,
// served by the lunta:// scheme into an iframe) or the original PDF (PDF.js). Mirrors
// apple/Mac/ReaderWindow.swift: contents on the left, highlights in four colours with notes,
// scroll or page mode, text size, and the reading position synced through the library folder.
//
// The page's scripts post messages through window.webkit.messageHandlers (src/reader/bridge.js
// turns those into postMessage to this window); `run()` is the apps' evaluateJavaScript.
import { convertFileSrc } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ask } from '@tauri-apps/plugin-dialog';
import { api, type Annotation, type Paper, type PageSelection, type TocItem } from './api';
import { fill, h, $, status, copy, menu, type MenuItem } from './ui';
import { showPdf, closePdf, pdfZoom, pdfFind, pdfFindStep, pdfFindClear } from './pdf';
import { closeFigureOverlay, figMarks, figureList, figureOpen, refreshFigureMarks, shownCaption, showFigureGrid, showFigureViewer, type CaptionHighlights, type Fig } from './figures';
import { cropStyle, fileName, hashBytes, markTitle, placeMarks, regionOf, type FigureRegion, type Mark } from './figure-marks';
import { openFind, closeFind, findOpen, findStep, findUpdate, type FindResult, type FindTarget } from './find';
import { summaryPageReady, summaryAction, summaryClosed, openAsk, askPaperClosed } from './ai';
import { startNarration, attachReader, detachReader, narrationPaper, playFromId, playerBar, type NarrationItem, type ReaderLink } from './listen';

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
let showingRefs = false;
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
      if (o) { selectionBar(null); showFigureViewer(o, refocus, readerDark(), captionHighlights()); }
      break;
    }
    case 'summary':  // the Summary card's menu: regenerate, hide (ai.ts)
      summaryAction(String(body));
      break;
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
    case 'narr':       // a paragraph tapped while listening: read from there
      playFromId(String(body));
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
  if (narrationPaper() === paper.id) detachReader();
  await savePosition();
  clearInterval(saver);
  summaryClosed(paper.id);
  askPaperClosed(paper.id);
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
        reading ? h('button.icon#listen-btn', { text: '🎧', title: 'Listen: read the paper aloud', 'aria-label': 'Listen', onclick: () => startListening() }) : null,
        h('button.icon#find-btn', { text: '🔍', title: 'Find in paper (Ctrl+F)', 'aria-label': 'Find in paper', onclick: () => startFind() }),
        reading ? h('button.icon#goto-btn', { text: '#', title: 'Go to page (Ctrl+Alt+G)', 'aria-label': 'Go to page', onclick: () => goToPage() }) : null,
        modeBtn,
        reading ? h('button.icon.hl-more', { text: '▾', title: 'Highlight colour, and highlight the selection now', onclick: (e: MouseEvent) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); highlightMenu(r.left, r.bottom + 4);
        } }) : null,
        hlBtn,
        h('button.icon#ask-btn', { text: '✨', title: 'Ask about this paper (on-device AI)', 'aria-label': 'Ask about this paper', onclick: () => openAsk(p.id, p.meta.title) }),
        reading ? h('button.icon#refs-btn' + (showingRefs ? '.on' : ''), { text: '❝', title: 'References', 'aria-label': 'References', onclick: () => toggleRefs() }) : null,
        reading ? h('button.icon#appearance-btn', { text: 'Aa', title: 'Text size, theme and layout', 'aria-label': 'Text and theme', onclick: (e: MouseEvent) => toggleAppearance(e.currentTarget as HTMLElement) }) : null,
        reading ? null : h('button.icon', { text: '−', title: 'Zoom out (Ctrl+−)', onclick: () => pdfZoom(1 / 1.15) }),
        reading ? null : h('button.icon', { text: '+', title: 'Zoom in (Ctrl+=)', onclick: () => pdfZoom(1.15) }))),
    h('div.reader-body', {},
      reading ? h('nav#toc') : null,
      h('div#page'),
      h('aside#hl-list', { hidden: !showingList }),
      reading ? h('aside#refs-list', { hidden: !showingRefs }) : null),
    playerBar(),
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
    try { await showPdf(page, p.id, (id) => openRef(id)); }
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

/** A highlight to show once the reading view is ready (a summary's Open in Paper), instead of the saved place. */
let pendingHighlight: string | null = null;
export function showHighlightWhenReady(id: string) { pendingHighlight = id.replace(/[^A-Za-z0-9-]/g, ''); }

/** The page loaded: draw the highlights, then go to where the reader stopped (on any device). */
async function pageReady() {
  run(hlModeJS());
  if (paper) attachReader(paper.id, pageLink);
  if (showingRefs) loadRefs();
  await refreshHighlights();
  if (pendingHighlight) {
    const id = pendingHighlight;
    pendingHighlight = null;
    restored = true;
    setTimeout(() => run(`luntaShowAnnotation('${id}')`), 300);
  }
  if (paper) summaryPageReady(paper.id, paper.meta.title, paper.meta.abstract_text, run);
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
      run(`var e=document.getElementById('${safe}'); if(e){ if(window.luntaMarkJump) luntaMarkJump(); e.scrollIntoView({behavior:'smooth', block:'start'}); }`);
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

/** "Add to Lunta" on a citation card (lunta://cite?doi=…, ?arxiv=… or ?ref=<text>): the paper is found in the
 *  library or added, and the card says which (ExternalLinks.swift). */
async function citeFromPage(link: string) {
  let doi = '', arxiv = '', ref = '';
  try { const u = new URL(link); doi = u.searchParams.get('doi') || ''; arxiv = u.searchParams.get('arxiv') || ''; ref = u.searchParams.get('ref') || ''; } catch { /* not a link */ }
  const forPaper = paper?.id;
  let label = 'Couldn’t add it';
  try {
    const r = await api.citePaper(doi, arxiv, ref || undefined);
    label = r.label;
    if (r.label === 'Added to your library') status('Added to your library');
  } catch (e) { status(String(e), 'error'); }
  if (paper?.id !== forPaper || mode !== 'reading') return;
  run(`window.luntaCiteDone && luntaCiteDone(${JSON.stringify(label)})`);
}

// ---- Listen (listen.ts) ----

/** What the narration may do in the page: highlight the paragraph being read, the listening mode. */
const pageLink: ReaderLink = {
  highlight: (id) => { run(`window.luntaHighlight && luntaHighlight(${JSON.stringify(id)})`); },
  setListening: (on) => { run(`window.luntaListening && luntaListening(${on})`); },
};

/** Reads the paper aloud from the paragraph at the top of the screen (ReaderWindow.startListening). */
async function startListening() {
  if (!paper || mode !== 'reading' || !frame) return;
  if (attachReader(paper.id, pageLink)) return;     // already reading this paper: its player is back
  const json = await run<string>('window.luntaNarration ? luntaNarration(false) : "[]"');
  let items: NarrationItem[] = [];
  try { items = JSON.parse(json || '[]'); } catch { items = []; }
  const from = (await run<string>('window.luntaFirstVisible ? luntaFirstVisible() : ""')) || '';
  if (!paper) return;
  await startNarration(paper.id, paper.meta.title, items, from, pageLink, !!paper.has_audio);
}

/** Go to Page: the page's own panel (page number and slider; JumpBack.swift's luntaShowGoTo), with the keyboard. */
function goToPage() {
  frame?.contentWindow?.focus();
  run('window.luntaShowGoTo && luntaShowGoTo()');
}

// keys for the reader when the page doesn't have the keyboard
document.addEventListener('keydown', (e) => {
  if (!paper || $('#reader').hidden || (e.target as HTMLElement).closest?.('input, textarea')) return;
  if (document.querySelector('.modal-back, #figure, #figures')) { if (e.key === 'Escape') closeOverlay(); return; }
  const mod = e.metaKey || e.ctrlKey;
  if (e.key === 'Escape') { if (!closeAppearance() && !closeFind()) closeReader(); return; }
  if (mod && !e.shiftKey && !e.altKey && e.code === 'KeyF') { e.preventDefault(); startFind(); return; }
  if (mode === 'reading' && mod && e.altKey && !e.shiftKey && e.code === 'KeyG') { e.preventDefault(); goToPage(); return; }
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
    // regions marked on figures: a badge on each figure's card (FigureMarks.swift)
    placed = await placedMarks(list);
    const by: Record<string, string[]> = {};
    for (const a of list) { const p = placed.get(a.id); if (p) (by[p.figure] ??= []).push(a.color); }
    await run(`window.luntaFigures && luntaFigures.setRegions && luntaFigures.setRegions(${JSON.stringify(JSON.stringify(by))})`);
    refreshFigureMarks();   // an open figure's caption (and its marks) shows them too
  }
  renderList(list);
}

// ---- regions marked on figures (core/src/figmark.rs, figure-marks.ts) ----

const isMark = (a: Annotation) => a.location === 'figure' || a.kind === 'FigureMark';
/** Where the paper's marks are among the page's figures (mark id -> figure, image), as last drawn. */
let placed = new Map<string, { figure: number; image: number }>();

/** An image's content hash, as the core makes it (for finding a mark's image again after a remake). */
async function imageHash(src: string): Promise<string> {
  try {
    const r = await fetch(src);
    return r.ok ? hashBytes(new Uint8Array(await r.arrayBuffer())) : '';
  } catch { return ''; }
}

async function placedMarks(list: Annotation[]) {
  const marks = list.filter(isMark);
  if (!marks.length || !figures.length) return new Map<string, { figure: number; image: number }>();
  return placeMarks(marks, figures.map((f) => ({ label: f.label, srcs: f.srcs })), imageHash);
}

/** The start of a caption, on one line (core figmark::caption_context). */
function captionContext(c: string): string {
  const one = c.split(/\s+/).filter(Boolean).join(' ');
  if (one.length <= 240) return one;
  let cut = one.slice(0, 240);
  const sp = cut.lastIndexOf(' ');
  if (sp > 120) cut = cut.slice(0, sp);
  return cut.replace(/[,;:\s]+$/, '') + '…';
}

/** Opens the figure viewer at a mark (from the list). */
async function showMark(a: Annotation) {
  const p = placed.get(a.id);
  if (!p || mode !== 'reading') { status('This figure isn’t in this version of the reading view. The mark is kept, with its note.'); return; }
  const json = await run<string>(`window.luntaFigures ? luntaFigures.data(${p.figure}) : ''`);
  let o = null;
  try { o = json ? figureOpen(JSON.parse(json)) : null; } catch { o = null; }
  o = o || { set: figures, index: p.figure, start: 0, panel: '' };
  showFigureViewer({ ...o, start: p.image, focus: a.id }, refocus, readerDark(), captionHighlights());
}

/** A mark's editor: its region cropped from the image, colour, shape, note, Delete Mark. */
function openMarkEditor(a: Annotation) {
  const r = regionOf(a);
  const note = h('textarea', { rows: 4, placeholder: 'What is here (panel c, a cluster…)' }) as HTMLTextAreaElement;
  note.value = a.note;
  let color = a.color || 'yellow', ellipse = r?.shape === 'ellipse';
  const swatches = h('div.swatches', {}, COLORS.map((c) => h(`button.swatch.c-${c}` + (c === color ? '.on' : ''), {
    title: c, onclick: (e: Event) => {
      color = c;
      swatches.querySelectorAll('.swatch').forEach((s) => s.classList.remove('on'));
      (e.currentTarget as HTMLElement).classList.add('on');
    },
  })));
  const shapes = h('div.segmented', {}, ...([['Rectangle', false], ['Ellipse', true]] as const).map(([t, v]) => h('button' + (ellipse === v ? '.on' : ''), {
    text: t, onclick: (e: Event) => { ellipse = v; shapes.querySelectorAll('button').forEach((b) => b.classList.remove('on')); (e.currentTarget as HTMLElement).classList.add('on'); },
  })));
  const p = placed.get(a.id), src = p ? figures[p.figure]?.srcs[p.image] : undefined;
  const close = () => back.remove();
  const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('div.modal.wide', {},
      h('h3', { text: markTitle(r) }),
      src && r ? h('div.mark-crop', { style: cropStyle(src, r) }) : null,
      a.suffix ? h('p.hint', { text: a.suffix }) : null,
      h('h4', { text: 'Colour' }), swatches,
      h('h4', { text: 'Shape' }), shapes,
      h('h4', { text: 'Note' }), note,
      h('div.buttons', {},
        h('button.danger', { text: 'Delete Mark', onclick: async () => { close(); await api.deleteAnnotation(a.paper_id, a.id); refreshHighlights(); } }),
        h('div.spacer'),
        h('button', { text: 'Cancel', onclick: close }),
        h('button.primary', { text: 'Save', onclick: async () => {
          close();
          const pos = r ? JSON.stringify({ ...r, shape: ellipse ? 'ellipse' : 'rect' }) : a.position;
          localStorage.setItem('figureMarkColor', color);
          await api.saveAnnotation({ ...a, color, note: note.value.trim(), position: pos });
          refreshHighlights();
        } }))));
  if (document.querySelector('#figure')) back.classList.add('over-figure');
  document.body.append(back);
  note.focus();
}

async function editAnnotation(id: string) {
  if (!paper) return;
  const a = (await api.annotations(paper.id)).find((x) => x.id === id);
  if (a) openEditor(a);
}

function openEditor(a: Annotation) {
  if (isMark(a)) { openMarkEditor(a); return; }
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
  if (document.querySelector('#figure')) back.classList.add('over-figure');   // a highlight in a figure's caption
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
  // in the paper's order (a region marked on a figure: where its figure is)
  const order = (a: Annotation) => (isMark(a) ? regionOf(a)?.start ?? 0 : parseInt(a.position, 10) || 0);
  const list = [...all].sort((a, b) => order(a) - order(b));
  const md = () => `## ${paper?.meta.title || 'Highlights'}\n\n` + list.map((a) => isMark(a)
    ? `> **${markTitle(regionOf(a))}**${a.suffix ? ` — ${a.suffix}` : ''}\n${a.note ? `\n${a.note}\n` : ''}\n`
    : `> ${a.quote.trim()}\n${a.note ? `\n${a.note}\n` : ''}\n`).join('');
  const markRow = (a: Annotation) => {
    const r = regionOf(a), p = placed.get(a.id), src = p ? figures[p.figure]?.srcs[p.image] : undefined;
    return h('div.hl-item.hl-mark', { onclick: () => showMark(a), oncontextmenu: (e: MouseEvent) => { e.preventDefault(); openEditor(a); } },
      h(`span.mark-thumb.c-${a.color || 'yellow'}`, src && r ? { style: cropStyle(src, r) } : { text: '⬚' }),
      h('div', {}, h('div.quote', { text: markTitle(r) }),
        a.note ? h('div.note', { text: `✎ ${a.note}` }) : a.suffix ? h('div.where', { text: a.suffix }) : null,
        p ? null : h('div.where', { text: 'Not in this reading view' })),
      h('button.icon.edit', { text: '…', title: 'Edit', onclick: (e: Event) => { e.stopPropagation(); openEditor(a); } }));
  };
  fill(box,
    h('h4', { text: 'Highlights & Notes' }),
    list.length ? null : h('p.hint', { text: 'No highlights yet. Select text in the reading view, then pick a colour or Note.' }),
    ...list.map((a) => isMark(a) ? markRow(a) : h('div.hl-item', {
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

// ---- references (ReferencesPanel.swift): the paper's reference list, In library or Add ----

interface Ref { index: number; label: string; text: string; doi: string; arxiv: string; pmid: string; pmcid: string; anchor: string; cited: number; }
let refs: Ref[] = [];
let refsFor = '';
let refsInLib = new Map<number, string>();     // reference index -> paper id
const refsAdding = new Set<number>();
const refsFailed = new Set<number>();
let refsQuery = '';
let refsBulk: { done: number; total: number; cancelled: boolean } | null = null;

const refInput = (r: Ref) => r.doi || (r.arxiv ? `arXiv:${r.arxiv}` : '') || r.pmcid || r.pmid || null;
const doiKey = (d: string) => d.trim().toLowerCase().replace(/^(https?:\/\/(dx\.)?doi\.org\/|doi:)/, '');
const arxivKey = (a: string) => a.trim().toLowerCase().replace(/^arxiv:/, '').replace(/v\d+$/, '');
const refsMissing = () => refs.filter((r) => refInput(r) && !refsInLib.has(r.index));

function toggleRefs() {
  showingRefs = !showingRefs;
  const l = $('#refs-list');
  if (l) l.hidden = !showingRefs;
  document.querySelector('#refs-btn')?.classList.toggle('on', showingRefs);
  if (showingRefs) loadRefs();
}

/** Reads the list from the page (once per paper), then which are in the library. */
async function loadRefs() {
  if (!paper || mode !== 'reading') return;
  if (refsFor !== paper.id) {
    const id = paper.id;
    const json = await run<string>('window.luntaReferences ? luntaReferences() : null');
    if (typeof json !== 'string' || paper?.id !== id) { renderRefs(); return; }
    try { refs = JSON.parse(json); } catch { refs = []; }
    refsFor = id;
    refsInLib = new Map();
    refsAdding.clear(); refsFailed.clear();
  }
  await refreshRefStatus();
}

/** Which references are in the library: by DOI, arXiv, PubMed and PMC ids (no network). */
async function refreshRefStatus() {
  const rows = await api.list({ kind: 'all' }, '', 'added').catch(() => [] as { paper: Paper }[]);
  const index = new Map<string, string>();
  for (const { paper: p } of rows) {
    const m = p.meta;
    if (m.doi) {
      const d = doiKey(m.doi);
      index.set(`doi:${d}`, p.id);
      if (d.startsWith('10.48550/arxiv.')) index.set(`arxiv:${arxivKey(d.slice(15))}`, p.id);
    }
    if (m.arxiv) index.set(`arxiv:${arxivKey(m.arxiv)}`, p.id);
    if (m.pmid) index.set(`pmid:${m.pmid.trim()}`, p.id);
    if (m.pmcid) index.set(`pmcid:${m.pmcid.trim().toUpperCase()}`, p.id);
  }
  refsInLib = new Map();
  for (const r of refs) {
    const keys = [r.doi && `doi:${doiKey(r.doi)}`, r.arxiv && `arxiv:${arxivKey(r.arxiv)}`, r.pmid && `pmid:${r.pmid}`, r.pmcid && `pmcid:${r.pmcid.toUpperCase()}`];
    const id = keys.map((k) => k && index.get(k)).find(Boolean);
    if (id) refsInLib.set(r.index, id);
  }
  renderRefs();
}

async function addRef(r: Ref) {
  const input = refInput(r);
  if (!input || refsInLib.has(r.index) || refsAdding.has(r.index)) return;
  const forPaper = refsFor;
  refsAdding.add(r.index); refsFailed.delete(r.index);
  renderRefs();
  try {
    const a = await api.addPaper(input);
    if (refsFor === forPaper) refsInLib.set(r.index, a.paper.id);
  } catch {
    if (refsFor === forPaper) refsFailed.add(r.index);
  }
  refsAdding.delete(r.index);
  renderRefs();
}

/** Add All Missing: three at a time, until done or cancelled. */
async function addAllRefs() {
  const todo = refsMissing();
  if (!todo.length || refsBulk) return;
  const skipped = refs.filter((r) => !refInput(r)).length;
  const ok = await ask(skipped ? `${skipped} without a DOI, arXiv or PubMed id are skipped.` : 'They are added a few at a time.',
    { title: `Add ${todo.length} ${todo.length === 1 ? 'reference' : 'references'} to your library?`, kind: 'info', okLabel: `Add ${todo.length}` });
  if (!ok || refsBulk) return;
  const bulk = { done: 0, total: todo.length, cancelled: false };
  refsBulk = bulk;
  renderRefs();
  for (let i = 0; i < todo.length && !bulk.cancelled; i += 3) {
    await Promise.all(todo.slice(i, i + 3).map((r) => addRef(r)));
    bulk.done = Math.min(todo.length, i + 3);
    renderRefs();
  }
  refsBulk = null;
  await refreshRefStatus();
}

/** Opens a paper from the list (the reader shows it instead of this one). */
async function openRef(id: string) {
  const p = await api.paper(id).catch(() => null);
  if (!p) return;
  const back = onClose;
  await closeReader();
  if (!back) return;
  $('#library').hidden = true;
  await openReader(p, 'reading', back);
}

function renderRefs() {
  const box = $('#refs-list');
  if (!box || !showingRefs) return;
  if (refsFor !== paper?.id) { fill(box, h('h4', { text: 'References' }), h('p.hint', { text: 'Loading…' })); return; }
  if (!refs.length) { fill(box, h('h4', { text: 'References' }), h('p.hint', { text: 'This reading view has no reference list Lunta can read.' })); return; }
  const q = refsQuery.trim().toLowerCase();
  const shown = q ? refs.filter((r) => r.text.toLowerCase().includes(q) || r.label === q || r.doi.toLowerCase().includes(q)) : refs;
  const missing = refsMissing().length;
  const filter = h('input.refs-filter', { type: 'search', placeholder: 'Filter', value: refsQuery }) as HTMLInputElement;
  filter.addEventListener('input', () => { refsQuery = filter.value; renderRefs(); const f = $('#refs-list .refs-filter') as HTMLInputElement | null; f?.focus(); f?.setSelectionRange(f.value.length, f.value.length); });
  const bulk = refsBulk;
  fill(box,
    h('h4', { text: 'References' }),
    h('p.hint', { text: `${refs.length} ${refs.length === 1 ? 'reference' : 'references'} · ${refsInLib.size} in your library` }),
    bulk ? h('div.refs-bulk', {}, h('progress', { max: String(bulk.total), value: String(bulk.done) }), h('span', { text: `${bulk.done} of ${bulk.total}` }),
      h('button', { text: 'Cancel', disabled: bulk.cancelled, onclick: () => { bulk.cancelled = true; renderRefs(); } }))
      : missing ? h('button', { text: 'Add All Missing', onclick: () => addAllRefs() }) : null,
    filter,
    ...shown.map((r) => {
      const inLib = refsInLib.get(r.index);
      const input = refInput(r);
      const state = inLib ? h('button.ref-in', { text: '✓ In library', title: 'In your library. Open it', onclick: () => openRef(inLib) })
        : refsAdding.has(r.index) ? h('span.ref-adding', { text: 'Adding…' })
        : input ? h('span', {}, refsFailed.has(r.index) ? h('span.ref-failed', { text: 'Couldn’t add ' }) : null,
            h('button.ref-add', { text: '+ Add', title: 'Add to your library', onclick: () => addRef(r) }))
        : h('button.ref-search', { text: 'Search', title: 'No DOI, arXiv or PubMed id: search Google Scholar',
            onclick: () => openUrl('https://scholar.google.com/scholar?q=' + encodeURIComponent(r.text.slice(0, 200))) });
      return h('div.ref-item' + (input || inLib ? '' : '.no-id'), {},
        h('span.ref-n', { text: r.label || '•' }),
        h('div', {},
          h('div.ref-text', { text: r.text, title: 'Show in the paper', onclick: () => { run(`window.luntaShowReference && luntaShowReference(${r.index})`); frame?.contentWindow?.focus(); } }),
          h('div.ref-row', {}, r.cited ? h('span.ref-cited', { text: `Cited ${r.cited}×` }) : h('span'), state)));
    }),
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

/** Highlights in the figure viewer's caption: the reading view's own, made from the page's passage of
 *  the words selected there (figureJS's luntaFigures.select), saved and drawn as a selection's are. */
function captionHighlights(): CaptionHighlights | undefined {
  if (!paper || mode !== 'reading') return undefined;
  return {
    mode: () => (hlMode ? hlColor : null),
    highlight: async (fig, from, to, color, note) => {
      if (!paper) return;
      const json = await run<string>(`window.luntaFigures && luntaFigures.select ? luntaFigures.select(${fig}, ${from}, ${to}) : ''`);
      let sel: PageSelection | null = null;
      try { sel = json ? JSON.parse(json) as PageSelection : null; } catch { sel = null; }
      if (!sel) { status('Those words weren’t found in the reading view.'); return; }
      try {
        const a = await api.addHighlight(paper.id, sel, color, '');
        await refreshHighlights();
        if (note) openEditor(a);
      } catch (e) { status(String(e), 'error'); }
    },
    edit: (id) => { editAnnotation(id); },
    regions: async (fig) => {
      if (!paper) return [];
      const list = await api.annotations(paper.id).catch(() => [] as Annotation[]);
      placed = await placedMarks(list);
      return list.filter((a) => placed.get(a.id)?.figure === fig).map((a): Mark => {
        const r = regionOf(a)!;
        return { id: a.id, x: r.x, y: r.y, w: r.w, h: r.h, ellipse: r.shape === 'ellipse', color: a.color, note: a.note, image: placed.get(a.id)!.image };
      });
    },
    addRegion: async (fig, image, r) => {
      const f = figures[fig];
      if (!paper || !f || !f.srcs[image]) return;
      const src = f.srcs[image];
      const region: FigureRegion = { figure: f.label, index: fig, image: fileName(src),
        hash: await imageHash(src), image_index: image, x: r.x, y: r.y, w: r.w, h: r.h, shape: 'rect', start: f.at };
      const color = hlMode ? hlColor : localStorage.getItem('figureMarkColor') || 'yellow';
      try {
        const a = await api.saveAnnotation({ id: '', paper_id: paper.id, kind: 'FigureMark', color, location: 'figure', position: JSON.stringify(region),
          quote: `${f.label || 'Figure'} (region)`, prefix: '', suffix: captionContext(shownCaption(f)), note: '', created: 0, modified: 0 });
        await refreshHighlights();
        if (!hlMode) openEditor(a);
      } catch (e) { status(String(e), 'error'); }
    },
    marks: async (fig) => {
      const json = await run<string>(`window.luntaFigures && luntaFigures.marks ? luntaFigures.marks(${fig}) : ''`);
      try { return json ? figMarks(JSON.parse(json)) : null; } catch { return null; }
    },
  };
}

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
      showFigureViewer(o || { set: figures, index: i, start: 0, panel: '' }, refocus, readerDark(), captionHighlights());
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

