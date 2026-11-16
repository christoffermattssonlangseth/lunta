// Figures in the reading view (apple/Shared/FigureViewer.swift): the page's figure script (in the
// generated style.js) links figure mentions, shows figures as cards and posts "fig" messages; here
// is the app's side: the figure on its own (image large and zoomable, label, "Panel B · 3 of 16",
// the caption below; ← → for the paper's other figures) and the grid of all figures.
// Both are drawn over the reader; the page underneath is never touched, so closing returns to
// exactly the same place.
import { h } from './ui';

export type FigMark = { t: string; c: string };
export type Fig = { index: number; srcs: string[]; label: string; title: string; caption: string; marks: FigMark[] };
/** What the viewer opens: figure `index` of `set`, at image `start`, pointing to `panel`. */
export type FigOpen = { set: Fig[]; index: number; start: number; panel: string };

const COLORS = ['yellow', 'green', 'blue', 'pink'];

function fig(d: any, i = 0): Fig {
  const s = (k: string) => String(d?.[k] ?? '').trim();
  const srcs: string[] = Array.isArray(d?.srcs) ? d.srcs.map(String) : d?.src ? [String(d.src)] : [];
  const marks: FigMark[] = Array.isArray(d?.marks)
    ? d.marks.filter((m: any) => typeof m?.t === 'string').map((m: any) => ({ t: m.t, c: COLORS.includes(m.c) ? m.c : 'yellow' }))
    : [];
  return { index: typeof d?.index === 'number' ? d.index : i, srcs, label: s('label'), title: s('title'), caption: s('caption'), marks };
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

/** Text with the reader's highlights marked (a caption's marks are found by their text). */
function marked(text: string, marks: FigMark[]): DocumentFragment {
  const spans: { at: number; end: number; c: string }[] = [];
  for (const m of marks) {
    if (m.t.length < 2) continue;
    for (let at = text.indexOf(m.t); at >= 0; at = text.indexOf(m.t, at + m.t.length)) spans.push({ at, end: at + m.t.length, c: m.c });
  }
  spans.sort((a, b) => a.at - b.at);
  const frag = document.createDocumentFragment();
  let cur = 0;
  for (const s of spans) {
    if (s.at < cur) continue;
    if (s.at > cur) frag.append(text.slice(cur, s.at));
    frag.append(h(`mark.c-${s.c}`, { text: text.slice(s.at, s.end) }));
    cur = s.end;
  }
  if (cur < text.length) frag.append(text.slice(cur));
  return frag;
}

/** Closes what `closeFigureOverlay` finds on top (the viewer, then the grid); false if none is open. */
export function closeFigureOverlay(): boolean {
  const all = document.querySelectorAll<HTMLElement>('#figure, #figures');
  const top = all[all.length - 1] as (HTMLElement & { close?: () => void }) | undefined;
  if (!top) return false;
  top.close ? top.close() : top.remove();
  return true;
}

let showCaption = true;

/** The figure viewer. `done` runs when it closes (to give the keyboard back). */
export function showFigureViewer(o: FigOpen, done: () => void, dark = matchMedia('(prefers-color-scheme: dark)').matches) {
  document.querySelector('#figure')?.remove();
  const figs = o.set;
  let current = Math.min(Math.max(0, o.index), figs.length - 1);
  let image = o.start;

  const label = h('div.fv-label');
  const sub = h('div.fv-sub');
  const prev = h('button.icon', { text: '‹', title: 'Previous figure (←)', onclick: () => go(current - 1) }) as HTMLButtonElement;
  const next = h('button.icon', { text: '›', title: 'Next figure (→)', onclick: () => go(current + 1) }) as HTMLButtonElement;
  const capBtn = h('button.icon', { onclick: () => { showCaption = !showCaption; draw(); } }) as HTMLButtonElement;
  const stage = h('div.fv-stage');
  const caption = h('div.fv-caption');
  const view = h('div#figure' + (dark ? '.dark' : '.light'), { tabindex: -1, role: 'dialog', 'aria-label': 'Figure' },
    h('div.fv-head', {},
      h('div.fv-titles', {}, label, sub),
      h('div.spacer'),
      figs.length > 1 ? prev : null, figs.length > 1 ? next : null,
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
  stage.addEventListener('mousedown', (e) => { if (s > 1.01 && e.button === 0) { drag = { x: e.clientX, y: e.clientY, tx, ty }; e.preventDefault(); } });
  const move = (e: MouseEvent) => { if (drag) { tx = drag.tx + e.clientX - drag.x; ty = drag.ty + e.clientY - drag.y; apply(); } };
  const up = () => { drag = null; };
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
    if (img) img.addEventListener('load', apply);
    stage.replaceChildren(...[img,
      f.srcs.length > 1 ? h('div.fv-images', {},
        h('button.icon', { text: '‹', title: 'Previous image', disabled: image === 0, onclick: () => { image--; draw(); } }),
        h('span', { text: `Image ${image + 1} of ${f.srcs.length}` }),
        h('button.icon', { text: '›', title: 'Next image', disabled: image === f.srcs.length - 1, onclick: () => { image++; draw(); } })) : null,
    ].filter((x): x is HTMLElement => !!x));
    stage.hidden = !f.srcs.length;

    const hasCaption = !!(f.caption || f.title);
    caption.hidden = !(showCaption && hasCaption);
    caption.classList.toggle('alone', !f.srcs.length);
    const body = h('div.fv-text');
    if (f.title) { body.append(h('strong', {}, marked(f.title, f.marks))); if (f.caption) body.append(' '); }
    body.append(marked(f.caption, f.marks));
    caption.replaceChildren(body);
    caption.scrollTop = 0;
  }

  view.addEventListener('keydown', (e) => {
    const k = e.key;
    if (k === 'Escape') close();
    else if (k === 'ArrowLeft') go(current - 1);
    else if (k === 'ArrowRight') go(current + 1);
    else if (k === '+' || k === '=') zoom(s * 1.5);
    else if (k === '-') zoom(s / 1.5);
    else if (k === '0') zoom(1);
    else return;
    e.preventDefault(); e.stopPropagation();
  });

  function close() {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    window.removeEventListener('resize', resize);
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
