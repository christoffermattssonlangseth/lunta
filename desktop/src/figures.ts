// Figures in the reading view (apple/Shared/FigureViewer.swift): the page's figure script (in the
// generated style.js) links figure mentions, shows figures as cards and posts "fig" messages; here
// is the app's side: the figure on its own (image large and zoomable, label, "Panel B · 3 of 16",
// the caption below; ← → for the paper's other figures) and the grid of all figures.
// Both are drawn over the reader; the page underneath is never touched, so closing returns to
// exactly the same place.
import { h } from './ui';
import { figureRegion, markAt, regionBox, type Box, type Mark, type Rect } from './figure-marks';

/** A highlight in a caption: its text and colour, and (from the page) where it is in the shown caption
 *  (`s`…`e`, title + ' ' + caption), the reading view's highlight id and whether it has a note. */
export type FigMark = { t: string; c: string; s?: number; e?: number; id?: string; n?: boolean };
/** `inPage`: one of the page's figures (`index` its number there), so its caption can be highlighted. */
export type Fig = { index: number; srcs: string[]; label: string; title: string; caption: string; marks: FigMark[]; inPage: boolean;
  /** where the figure is in the page's text (a highlight's offset): a region marked on it is listed there */
  at: number };

/** The viewer's caption highlights (the reader's): they are the reading view's own (figureJS's
 *  luntaFigures.select turns a passage of the caption into the page's passage of those words). */
export type CaptionHighlights = {
  /** highlight mode's colour, or null when it is off */
  mode: () => string | null;
  /** highlight (or note on, `note`) the passage from…to of figure `fig`'s shown caption */
  highlight: (fig: number, from: number, to: number, color: string, note: boolean) => Promise<void>;
  /** a highlight in the caption clicked: its editor */
  edit: (id: string) => void;
  /** the highlights the page has in figure `fig`'s caption now */
  marks: (fig: number) => Promise<FigMark[] | null>;
  /** regions marked on figure `fig`'s images (figure-marks.ts), each with its image */
  regions?: (fig: number) => Promise<Mark[]>;
  /** a region drawn on image `image` of figure `fig` (0–1 of the image): saved as a mark */
  addRegion?: (fig: number, image: number, r: Rect) => Promise<void>;
};
/** What the viewer opens: figure `index` of `set`, at image `start`, pointing to `panel`. */
export type FigOpen = { set: Fig[]; index: number; start: number; panel: string;
  /** a region marked on the figure to show as it opens (zoomed to and flashed) */
  focus?: string };

const COLORS = ['yellow', 'green', 'blue', 'pink'];

/** The page's marks ({t, c, s, e, id, n}). */
export function figMarks(list: any): FigMark[] {
  return Array.isArray(list)
    ? list.filter((m: any) => typeof m?.t === 'string').map((m: any) => ({
      t: m.t, c: COLORS.includes(m.c) ? m.c : 'yellow',
      ...(typeof m.s === 'number' && typeof m.e === 'number' ? { s: m.s, e: m.e } : {}),
      ...(typeof m.id === 'string' ? { id: m.id } : {}), n: !!m.n,
    }))
    : [];
}

function fig(d: any, i = 0): Fig {
  const s = (k: string) => String(d?.[k] ?? '').trim();
  const srcs: string[] = Array.isArray(d?.srcs) ? d.srcs.map(String) : d?.src ? [String(d.src)] : [];
  return { index: typeof d?.index === 'number' ? d.index : i, srcs, label: s('label'), title: s('title'), caption: s('caption'),
    marks: figMarks(d?.marks), inPage: typeof d?.index === 'number', at: typeof d?.at === 'number' ? d.at : 0 };
}

/** The caption as the viewer shows it (and the page counts it, figureJS shownText). */
export function shownCaption(f: Fig): string {
  return f.title ? (f.caption ? `${f.title} ${f.caption}` : f.title) : f.caption;
}

/** The paper's figures, from the page's {list: […]} message. */
export function figureList(d: any): Fig[] | null {
  return d && Array.isArray(d.list) ? d.list.map((x: any, i: number) => fig(x, i)) : null;
}

/** A tapped mention, card or image ({all, index, start, panel}), or a lone image ({srcs, label, …}). */
export function figureOpen(d: any): FigOpen | null {
  if (!d || typeof d !== 'object') return null;
  if (Array.isArray(d.all) && typeof d.index === 'number' && d.all[d.index]) {
    const set = d.all.map((x: any, i: number) => fig(x, i));
    const n = set[d.index].srcs.length;
    return { set, index: d.index, start: Math.min(Math.max(0, Number(d.start) || 0), Math.max(0, n - 1)), panel: String(d.panel || '').trim() };
  }
  const f = fig(d);
  if (!f.srcs.length && !f.caption && !f.title) return null;
  return { set: [f], index: 0, start: 0, panel: '' };
}

/** The shown caption (title in bold, a space, the caption) with the reader's highlights marked: where the
 *  page says they are, or (marks without a place) found by their text. */
function captionBody(f: Fig, marks: FigMark[]): HTMLElement {
  const text = shownCaption(f), tl = f.title.length;
  const spans: { at: number; end: number; c: string; id: string; n: boolean }[] = [];
  for (const m of marks) {
    if (m.s != null && m.e != null) { if (m.e > m.s) spans.push({ at: m.s, end: Math.min(m.e, text.length), c: m.c, id: m.id || '', n: !!m.n }); continue; }
    if (m.t.length < 2) continue;
    for (let at = text.indexOf(m.t); at >= 0; at = text.indexOf(m.t, at + m.t.length)) spans.push({ at, end: at + m.t.length, c: m.c, id: '', n: false });
  }
  spans.sort((a, b) => a.at - b.at);
  // pieces between the marks' ends and the title's end, each bold (in the title) and/or marked
  const cuts = new Set([0, text.length, ...(tl ? [tl] : [])]);
  const used = spans.filter((s, i) => !spans.slice(0, i).some((p) => p.end > s.at));
  for (const s of used) { cuts.add(s.at); cuts.add(s.end); }
  const at = [...cuts].filter((x) => x >= 0 && x <= text.length).sort((a, b) => a - b);
  const body = h('div.fv-text');
  for (let i = 0; i + 1 < at.length; i++) {
    const a = at[i], b = at[i + 1];
    if (b <= a) continue;
    let node: Node = document.createTextNode(text.slice(a, b));
    const m = used.find((s) => s.at <= a && b <= s.end);
    if (m) {
      const mk = h(`mark.c-${m.c}` + (m.n ? '.note' : ''));
      if (m.id && m.id !== '__pending') mk.setAttribute('data-ann', m.id);
      mk.append(node); node = mk;
    }
    if (a < tl) { const st = h('strong'); st.append(node); node = st; }
    body.append(node);
  }
  return body;
}

/** The offset in `root`'s text of a point of a selection in it. */
function textOffset(root: Node, node: Node, off: number): number {
  const r = document.createRange();
  r.setStart(root, 0);
  r.setEnd(node, off);
  return r.toString().length;
}

let refreshOpen: (() => void) | null = null;
/** The page's highlights were drawn again: the open viewer's caption shows them again. */
export function refreshFigureMarks() { refreshOpen?.(); }

/** Closes what `closeFigureOverlay` finds on top (the viewer, then the grid); false if none is open. */
export function closeFigureOverlay(): boolean {
  const all = document.querySelectorAll<HTMLElement>('#figure, #figures');
  const top = all[all.length - 1] as (HTMLElement & { close?: () => void }) | undefined;
  if (!top) return false;
  top.close ? top.close() : top.remove();
  return true;
}

let showCaption = true;

/** The figure viewer. `done` runs when it closes (to give the keyboard back); with `hl` its caption
 *  can be highlighted like the text (select: Highlight colours and Note; highlight mode: at once). */
export function showFigureViewer(o: FigOpen, done: () => void, dark = matchMedia('(prefers-color-scheme: dark)').matches, hl?: CaptionHighlights) {
  document.querySelector('#figure')?.remove();
  const figs = o.set.map((f) => ({ ...f, marks: [...f.marks] }));
  let current = Math.min(Math.max(0, o.index), figs.length - 1);
  let image = o.start;

  const label = h('div.fv-label');
  const sub = h('div.fv-sub');
  const prev = h('button.icon', { text: '‹', title: 'Previous figure (←)', onclick: () => go(current - 1) }) as HTMLButtonElement;
  const next = h('button.icon', { text: '›', title: 'Next figure (→)', onclick: () => go(current + 1) }) as HTMLButtonElement;
  const capBtn = h('button.icon', { onclick: () => { showCaption = !showCaption; draw(); } }) as HTMLButtonElement;
  // the Mark tool: a drag on the image marks a region (also Alt-drag, and any drag in highlight mode)
  let marking = false;
  const markBtn = h('button.icon.fv-markbtn', { text: '⬚', onclick: () => { marking = !marking; markState(); } }) as HTMLButtonElement;
  const stage = h('div.fv-stage');
  const layer = h('div.fv-marks');
  const caption = h('div.fv-caption');
  const view = h('div#figure' + (dark ? '.dark' : '.light'), { tabindex: -1, role: 'dialog', 'aria-label': 'Figure' },
    h('div.fv-head', {},
      h('div.fv-titles', {}, label, sub),
      h('div.spacer'),
      figs.length > 1 ? prev : null, figs.length > 1 ? next : null,
      hl?.addRegion ? markBtn : null,
      figs.some((f) => f.caption || f.title) ? capBtn : null,
      h('button.icon.fv-close', { text: '✕', title: 'Back to the text (Esc)', onclick: () => close() })),
    stage, caption) as HTMLElement & { close?: () => void };

  // zoom: double-click, pinch or Ctrl+wheel, + and −; drag to move about while zoomed
  let s = 1, tx = 0, ty = 0;
  let img: HTMLImageElement | null = null;
  function apply() {
    if (!img) return;
    const mx = Math.max(0, (img.offsetWidth * s - stage.clientWidth) / 2);
    const my = Math.max(0, (img.offsetHeight * s - stage.clientHeight) / 2);
    tx = Math.max(-mx, Math.min(mx, tx));
    ty = Math.max(-my, Math.min(my, ty));
    img.style.transform = `translate(${tx}px, ${ty}px) scale(${s})`;
    stage.classList.toggle('zoomed', s > 1.01);
    placeRegions();
  }
  /** Zoom to `to`, keeping the point (x, y) (in the stage) where it is. */
  function zoom(to: number, x = stage.clientWidth / 2, y = stage.clientHeight / 2) {
    to = Math.max(1, Math.min(8, to));
    const qx = x - stage.clientWidth / 2, qy = y - stage.clientHeight / 2;
    const px = (qx - tx) / s, py = (qy - ty) / s;
    s = to; tx = qx - s * px; ty = qy - s * py;
    if (s <= 1.01) { s = 1; tx = 0; ty = 0; }
    apply();
  }
  const at = (e: MouseEvent) => { const r = stage.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] as const; };
  stage.addEventListener('dblclick', (e) => { const [x, y] = at(e); zoom(s > 1.01 ? 1 : 3, x, y); });
  stage.addEventListener('wheel', (e) => {
    e.preventDefault();
    if (e.ctrlKey) { const [x, y] = at(e); zoom(s * Math.exp(-e.deltaY * 0.01), x, y); }
    else if (s > 1.01) { tx -= e.deltaX; ty -= e.deltaY; apply(); }
  }, { passive: false });
  let drag: { x: number; y: number; tx: number; ty: number } | null = null;
  // a region being drawn: from where the drag started (on screen), the dashed box following the mouse
  let drawing: { x: number; y: number; live: HTMLElement } | null = null, drewAt = 0;
  const drawsNow = (e: MouseEvent) => canMark() && !!img && (marking || e.altKey || !!hl?.mode());
  stage.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest?.('.fv-region, .fv-images')) return;
    if (drawsNow(e)) {
      const b = imgBox();
      if (!b || e.clientX < b.left || e.clientX > b.left + b.width || e.clientY < b.top || e.clientY > b.top + b.height) return;
      const live = h('div.fv-live.c-' + (hl?.mode() || markColor()));
      layer.append(live);
      drawing = { x: e.clientX, y: e.clientY, live };
      e.preventDefault();
      return;
    }
    if (s > 1.01) { drag = { x: e.clientX, y: e.clientY, tx, ty }; e.preventDefault(); }
  });
  const move = (e: MouseEvent) => {
    if (drawing) {
      const b = imgBox();
      if (!b) return;
      const r = regionBox(figureRegion(drawing, { x: e.clientX, y: e.clientY }, b), b), st = stage.getBoundingClientRect();
      Object.assign(drawing.live.style, { left: `${r.left - st.left}px`, top: `${r.top - st.top}px`, width: `${r.width}px`, height: `${r.height}px` });
      return;
    }
    if (drag) { tx = drag.tx + e.clientX - drag.x; ty = drag.ty + e.clientY - drag.y; apply(); }
  };
  const up = (e: MouseEvent) => {
    drag = null;
    if (!drawing) return;
    const d = drawing, b = imgBox();
    drawing = null;
    drewAt = Date.now();
    d.live.remove();
    if (!b || Math.abs(e.clientX - d.x) < 8 || Math.abs(e.clientY - d.y) < 8) return;
    const r = figureRegion(d, { x: e.clientX, y: e.clientY }, b), f = figs[current];
    hl?.addRegion?.(f.index, image, r).then(loadRegions);
  };
  const resize = () => apply();
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
  window.addEventListener('resize', resize);

  function go(i: number) {
    if (i < 0 || i >= figs.length || i === current) return;
    current = i; image = 0; draw();
  }

  function draw() {
    const f = figs[current];
    const panel = current === o.index ? o.panel : '';
    label.textContent = f.label || 'Figure';
    const bits = [panel ? `Panel ${panel}` : '', figs.length > 1 ? `${current + 1} of ${figs.length}` : ''].filter(Boolean);
    sub.textContent = bits.join(' · ');
    sub.hidden = !bits.length;
    prev.disabled = current === 0;
    next.disabled = current === figs.length - 1;
    capBtn.textContent = showCaption ? '▤' : '▭';
    capBtn.title = showCaption ? 'Hide the caption' : 'Show the caption';
    capBtn.classList.toggle('on', showCaption);

    s = 1; tx = 0; ty = 0;
    image = Math.min(Math.max(0, image), Math.max(0, f.srcs.length - 1));
    img = f.srcs.length ? h('img', { src: f.srcs[image], alt: f.label, draggable: 'false' }) as HTMLImageElement : null;
    if (img) img.addEventListener('load', () => { apply(); placeRegions(); });
    stage.replaceChildren(...[img, layer,
      f.srcs.length > 1 ? h('div.fv-images', {},
        h('button.icon', { text: '‹', title: 'Previous image', disabled: image === 0, onclick: () => { image--; draw(); } }),
        h('span', { text: `Image ${image + 1} of ${f.srcs.length}` }),
        h('button.icon', { text: '›', title: 'Next image', disabled: image === f.srcs.length - 1, onclick: () => { image++; draw(); } })) : null,
    ].filter((x): x is HTMLElement => !!x));
    stage.hidden = !f.srcs.length;

    const hasCaption = !!(f.caption || f.title);
    caption.hidden = !(showCaption && hasCaption);
    caption.classList.toggle('alone', !f.srcs.length);
    drawCaption();
    caption.scrollTop = 0;
    if (canHighlight()) loadMarks();
    regions = [];
    layer.replaceChildren();
    markState();
    loadRegions();
  }

  // ---- regions marked on the image (figure-marks.ts) ----
  const canMark = () => !!hl?.addRegion && figs[current].inPage && figs[current].srcs.length > 0;
  const markColor = () => localStorage.getItem('figureMarkColor') || 'yellow';
  let regions: Mark[] = [];
  let focus = o.focus || '';
  function markState() {
    markBtn.hidden = !canMark();
    markBtn.classList.toggle('on', marking);
    markBtn.title = marking ? 'Done marking' : 'Mark a region: drag on the image (or Alt-drag)';
    stage.classList.toggle('marking', canMark() && marking);
  }
  /** Where the image is drawn now (zoomed, moved). */
  function imgBox(): Box | null {
    if (!img || !img.complete || !img.naturalWidth) return null;
    // the picture itself, inside the element's box (object-fit: contain)
    const r = img.getBoundingClientRect(), ar = img.naturalWidth / img.naturalHeight;
    let w = r.width, hh = r.height;
    if (w / hh > ar) w = hh * ar; else hh = w / ar;
    return { left: r.left + (r.width - w) / 2, top: r.top + (r.height - hh) / 2, width: w, height: hh };
  }
  async function loadRegions() {
    const i = current, list = canMark() && hl?.regions ? await hl.regions(figs[i].index) : [];
    if (i !== current || !view.isConnected) return;
    regions = list;
    layer.replaceChildren(...regions.filter((m) => (m.image ?? 0) === image).map((m) => {
      const el = h(`div.fv-region.c-${m.color || 'yellow'}` + (m.ellipse ? '.ellipse' : '') + (m.note ? '.note' : ''),
        { title: m.note || 'Marked region', 'data-mark': m.id, onclick: (e: Event) => { e.stopPropagation(); hl?.edit(m.id); } });
      return el;
    }));
    placeRegions();
    if (focus && regions.some((m) => m.id === focus)) showRegion(focus);
  }
  /** The marks drawn where their regions are on the image as it is drawn now. */
  function placeRegions() {
    const b = imgBox();
    if (!b) return;
    const st = stage.getBoundingClientRect();
    layer.querySelectorAll<HTMLElement>('.fv-region').forEach((el) => {
      const m = regions.find((x) => x.id === el.getAttribute('data-mark'));
      if (!m) return;
      const r = regionBox(m, b);
      Object.assign(el.style, { left: `${r.left - st.left}px`, top: `${r.top - st.top}px`, width: `${r.width}px`, height: `${r.height}px` });
    });
  }
  /** Zoomed to a small mark, which flashes. */
  function showRegion(id: string) {
    focus = '';
    const m = regions.find((x) => x.id === id);
    if (!m || !img) return;
    if (m.w < 0.6 && m.h < 0.6) {
      const b = imgBox();
      if (b) {
        // the mark's middle to the stage's middle (the picture is centred, scaled about its centre),
        // zoomed so it fills about half of it
        const pw = b.width / s, ph = b.height / s, to = Math.max(1, Math.min(8, 0.5 / Math.max(m.w, m.h)));
        s = to;
        tx = -(m.x + m.w / 2 - 0.5) * pw * to;
        ty = -(m.y + m.h / 2 - 0.5) * ph * to;
        apply();
      }
    }
    const el = layer.querySelector(`[data-mark="${CSS.escape(id)}"]`);
    el?.classList.add('flash');
    setTimeout(() => el?.classList.remove('flash'), 1800);
  }

  // ---- highlights in the caption ----
  const canHighlight = () => !!hl && figs[current].inPage;
  function drawCaption() {
    const keep = caption.scrollTop;
    selBar.remove();
    caption.replaceChildren(captionBody(figs[current], figs[current].marks));
    caption.scrollTop = keep;
  }
  async function loadMarks() {
    const i = current, m = await hl?.marks(figs[i].index);
    if (!m || i !== current || !view.isConnected) return;
    figs[i].marks = m;
    drawCaption();
  }
  refreshOpen = () => { if (canHighlight()) loadMarks(); loadRegions(); };
  /** The caption's selection as offsets in the shown caption, if it is in it. */
  function captionSelection(): [number, number] | null {
    const body = caption.querySelector('.fv-text'), sel = getSelection();
    if (!body || !sel || sel.isCollapsed || !sel.rangeCount) return null;
    const r = sel.getRangeAt(0);
    if (!body.contains(r.startContainer) || !body.contains(r.endContainer)) return null;
    const a = textOffset(body, r.startContainer, r.startOffset), b = textOffset(body, r.endContainer, r.endOffset);
    return shownCaption(figs[current]).slice(a, b).trim().length >= 2 ? [a, b] : null;
  }
  async function mark(from: number, to: number, color: string, note: boolean) {
    if (!hl) return;
    const f = figs[current];
    if (!note) { f.marks.push({ t: '', c: color, s: from, e: to, id: '__pending' }); }   // coloured at once; the page's redraw replaces it
    getSelection()?.removeAllRanges();
    drawCaption();
    await hl.highlight(f.index, from, to, color, note);
    loadMarks();
  }
  // the selection's bar: the colours and Note (as the reading view's)
  const selBar = h('div.fv-selbar');
  function showBar(range: [number, number]) {
    const r = getSelection()?.getRangeAt(0).getBoundingClientRect();
    if (!r) return;
    selBar.replaceChildren(
      ...COLORS.map((c) => h(`button.swatch.c-${c}`, { title: `Highlight ${c}`, onmousedown: (e: Event) => e.preventDefault(), onclick: () => mark(range[0], range[1], c, false) })),
      h('button', { text: 'Note', title: 'Add a note', onmousedown: (e: Event) => e.preventDefault(), onclick: () => mark(range[0], range[1], 'yellow', true) }));
    view.append(selBar);
    const bw = selBar.offsetWidth, bh = selBar.offsetHeight;
    let top = r.top - bh - 8;
    if (top < 4) top = r.bottom + 8;
    selBar.style.top = `${top}px`;
    selBar.style.left = `${Math.max(4, Math.min(r.left + r.width / 2 - bw / 2, innerWidth - bw - 4))}px`;
  }
  caption.addEventListener('mouseup', () => setTimeout(() => {
    if (!canHighlight()) return;
    const range = captionSelection();
    if (!range) { selBar.remove(); return; }
    const color = hl!.mode();
    if (color) mark(range[0], range[1], color, false);
    else showBar(range);
  }, 0));
  caption.addEventListener('mousedown', (e) => { if (!selBar.contains(e.target as Node)) selBar.remove(); });
  // a click on a mark (with the Mark tool off, or when it wasn't a drag): its editor
  stage.addEventListener('click', (e) => {
    if (!canMark() || drawing || Date.now() - drewAt < 300) return;
    const b = imgBox();
    const id = b && !(e.target as HTMLElement).closest?.('.fv-region') ? markAt({ x: e.clientX, y: e.clientY }, b, regions.filter((m) => (m.image ?? 0) === image)) : null;
    if (id) hl?.edit(id);
  });
  caption.addEventListener('click', (e) => {
    if (!canHighlight() || !getSelection()?.isCollapsed) return;
    const id = (e.target as HTMLElement).closest?.('mark[data-ann]')?.getAttribute('data-ann');
    if (id) hl!.edit(id);
  });

  view.addEventListener('keydown', (e) => {
    const k = e.key;
    if (k === 'Escape') close();
    else if (k === 'ArrowLeft') go(current - 1);
    else if (k === 'ArrowRight') go(current + 1);
    else if (k === '+' || k === '=') zoom(s * 1.5);
    else if (k === '-') zoom(s / 1.5);
    else if (k === '0') zoom(1);
    else if ((k === 'm' || k === 'M') && canMark() && !e.metaKey && !e.ctrlKey) { marking = !marking; markState(); }
    else return;
    e.preventDefault(); e.stopPropagation();
  });

  function close() {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    window.removeEventListener('resize', resize);
    refreshOpen = null;
    view.remove();
    done();
  }
  view.close = close;
  draw();
  document.body.append(view);
  view.focus();
}

/** All the paper's figures as a grid; `open(i)` shows figure i (over the grid). */
export function showFigureGrid(list: Fig[], opts: {
  open: (i: number) => void; cards: boolean; setCards: (on: boolean) => void; done: () => void;
  /** the reader's theme: the grid is drawn in its colours (Automatic: the system's) */
  theme?: string;
}) {
  document.querySelector('#figures')?.remove();
  const setting = h('select', { title: 'Show figures in the text as cards or in full' },
    h('option', { value: '1', text: 'As cards' }), h('option', { value: '0', text: 'In full' })) as HTMLSelectElement;
  setting.value = opts.cards ? '1' : '0';
  setting.addEventListener('change', () => opts.setCards(setting.value === '1'));
  const back = h(`div#figures.modal-back.theme-${opts.theme || 'automatic'}`, { tabindex: -1, onmousedown: (e: MouseEvent) => { if (e.target === back) close(); } },
    h('div.modal.figure-grid', {},
      h('div.fg-head', {},
        h('h3', { text: `Figures (${list.length})` }), h('div.spacer'),
        h('label.fg-setting', {}, 'Figures in the text: ', setting),
        h('button.primary', { text: 'Done', onclick: () => close() })),
      h('div.fg-grid', {}, list.map((f, i) => h('button.fg-cell', { title: f.label || 'Figure', onclick: () => opts.open(i) },
        h('div.fg-thumb', {}, f.srcs.length ? h('img', { src: f.srcs[0], alt: '', loading: 'lazy', draggable: 'false' }) : h('span', { text: '🖼' })),
        h('div.fg-label', { text: f.label || 'Figure' }),
        f.title || f.caption ? h('div.fg-title', { text: f.title || f.caption }) : null))))) as HTMLElement & { close?: () => void };
  back.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); } });
  function close() { back.remove(); opts.done(); }
  back.close = close;
  document.body.append(back);
  back.focus();
}
