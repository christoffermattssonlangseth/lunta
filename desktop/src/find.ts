// Find in paper (apple/Shared/FindInPage.swift): a small bar over the reader with the search
// field, "3 of 17" / "No matches", previous / next and Done. It searches as you type (two
// characters or more, after a short pause). What is searched is given by a FindTarget: the
// reading view's page script (generated/find.js: luntaFind / luntaFindNext / luntaFindPrev /
// luntaFindClear) or PDF.js's find controller. Keys: Enter / Shift+Enter, F3 / Shift+F3 (and
// Ctrl+G / Ctrl+Shift+G), Esc closes.
import { h } from './ui';

export type FindResult = { count: number; index: number; capped?: boolean };

export interface FindTarget {
  /** Searches ('': takes the marks away). null: the answer comes later, through `findUpdate`. */
  find(query: string): Promise<FindResult | null>;
  step(forward: boolean): Promise<FindResult | null>;
  clear(): void;
}

let bar: (HTMLElement & { input?: HTMLInputElement }) | null = null;
let target: FindTarget | null = null;
let result: FindResult | null = null;
let searched = '';          // the query the result is for
let timer: number | undefined;
let seq = 0;
let closed: ((moved: boolean) => void) | null = null;
let moved = false;          // a match was shown (the reader may want to offer the way back)

const MIN = 2;
const PAUSE = 220;

export const findOpen = () => !!bar;

function label() {
  const l = bar?.querySelector<HTMLElement>('.fb-count');
  if (!l || !bar?.input) return;
  const q = bar.input.value.trim();
  const r = result;
  l.textContent = q.length < MIN ? '' : !r ? '…' : r.count === 0 ? 'No matches' : `${r.index + 1} of ${r.count}${r.capped ? '+' : ''}`;
  l.classList.toggle('none', q.length >= MIN && !!r && r.count === 0);
  bar.querySelectorAll<HTMLButtonElement>('.fb-step').forEach((b) => { b.disabled = !r || r.count === 0 || q.length < MIN; });
}

/** A result that arrives later (PDF.js reports while it searches the pages). */
export function findUpdate(r: FindResult | null) {
  result = r;
  if (r && r.index >= 0) moved = true;
  label();
}

async function search() {
  clearTimeout(timer);
  if (!bar?.input || !target) return;
  const q = bar.input.value.trim();
  const mine = ++seq;
  searched = q;
  if (q.length < MIN) { result = null; label(); await target.find(''); return; }
  result = null; label();
  const r = await target.find(q);
  if (mine !== seq) return;
  if (r) findUpdate(r);
}

/** The next or previous match (searching first if the text changed since). */
export async function findStep(forward: boolean) {
  if (!bar?.input || !target) return;
  if (bar.input.value.trim() !== searched || timer) { timer = undefined; await search(); return; }
  if (!result || !result.count) return;
  const r = await target.step(forward);
  if (r) findUpdate(r);
}

/** Opens the bar in `host` (or focuses it, with its text selected, if open). */
/** `query`: searched at once (Check Citations' passage). */
export function openFind(host: HTMLElement, t: FindTarget, onClose: (moved: boolean) => void, query?: string) {
  if (bar && bar.isConnected) {
    if (query) { bar.input!.value = query; search(); }
    bar.input!.focus(); bar.input!.select(); return;
  }
  target = t; closed = onClose; moved = false; result = null; searched = '';
  const input = h('input', { type: 'search', placeholder: 'Find in paper', 'aria-label': 'Find in paper', spellcheck: 'false' }) as HTMLInputElement;
  input.addEventListener('input', () => {
    clearTimeout(timer);
    timer = window.setTimeout(() => { timer = undefined; search(); }, input.value.trim().length < MIN ? 0 : PAUSE);
  });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === 'F3' || ((e.ctrlKey || e.metaKey) && e.code === 'KeyG')) { e.preventDefault(); findStep(!e.shiftKey); }
    else if (e.key === 'Escape') { e.preventDefault(); closeFind(); }
    else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.code === 'KeyF') { e.preventDefault(); input.select(); }
    else return;
    e.stopPropagation();
  });
  bar = h('div#findbar', { role: 'search' },
    input,
    h('span.fb-count', { 'aria-live': 'polite' }),
    h('button.icon.fb-step', { text: '‹', title: 'Previous match (Shift+Enter, Shift+F3)', 'aria-label': 'Previous match', onclick: () => findStep(false) }),
    h('button.icon.fb-step', { text: '›', title: 'Next match (Enter, F3)', 'aria-label': 'Next match', onclick: () => findStep(true) }),
    h('button', { text: 'Done', title: 'Close (Esc)', onclick: () => closeFind() })) as HTMLElement & { input?: HTMLInputElement };
  bar.input = input;
  host.append(bar);
  label();
  input.focus();
  if (query) { input.value = query; search(); }
}

/** Closes the bar and takes the marks out of the page. */
export function closeFind(): boolean {
  if (!bar) return false;
  clearTimeout(timer); timer = undefined;
  seq++;
  target?.clear();
  bar.remove();
  bar = null;
  target = null;
  const cb = closed, m = moved;
  closed = null;
  cb?.(m);
  return true;
}
