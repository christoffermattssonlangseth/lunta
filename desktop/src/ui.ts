// Small DOM helpers: element builder, status line, text prompt, context menu.
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

type Attrs = Record<string, string | number | boolean | ((e: any) => void) | undefined | null>;
type Child = Node | string | null | undefined | false;

/** h('div.cls#id', {attrs, onclick}, children…) */
export function h<K extends keyof HTMLElementTagNameMap>(spec: K | string, attrs: Attrs = {}, ...kids: (Child | Child[])[]): HTMLElement {
  const m = spec.match(/^([a-z0-9]+)?((?:[.#][\w-]+)*)$/i);
  const node = document.createElement(m?.[1] || 'div');
  for (const part of (m?.[2] || '').match(/[.#][\w-]+/g) || []) {
    if (part[0] === '.') node.classList.add(part.slice(1)); else node.id = part.slice(1);
  }
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith('on') && typeof v === 'function') node.addEventListener(k.slice(2), v as EventListener);
    else if (k === 'text') node.textContent = String(v);
    else if (v === true) node.setAttribute(k, '');
    else node.setAttribute(k, String(v));
  }
  for (const k of kids.flat()) if (k !== null && k !== undefined && k !== false) node.append(k);
  return node;
}

/** replaceChildren that skips null / false entries. */
export function fill(el: HTMLElement, ...kids: Child[]) {
  el.replaceChildren(...(kids.filter((k) => k !== null && k !== undefined && k !== false) as (Node | string)[]));
}

export const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector(sel) as T;

let statusTimer: number | undefined;
/** Shows a line in the status bar; `sticky` keeps it until the next message. */
export function status(text: string, kind: 'info' | 'busy' | 'error' = 'info', sticky = false) {
  const bar = $('#status');
  bar.textContent = text;
  bar.className = `status ${kind}`;
  bar.hidden = !text;
  clearTimeout(statusTimer);
  if (!sticky && kind !== 'busy') statusTimer = window.setTimeout(() => { bar.hidden = true; }, kind === 'error' ? 12000 : 4000);
}

export async function copy(text: string, what: string) {
  try { await writeText(text); status(`Copied ${what}`); }
  catch (e) { status(`Couldn't copy: ${e}`, 'error'); }
}

/** A modal with a text field; resolves to the text, or null when cancelled. */
export function prompt(title: string, value = '', placeholder = '', okLabel = 'OK'): Promise<string | null> {
  return new Promise((resolve) => {
    const input = h('input', { type: 'text', value, placeholder }) as HTMLInputElement;
    const done = (v: string | null) => { back.remove(); resolve(v); };
    const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) done(null); } },
      h('form.modal', { onsubmit: (e: Event) => { e.preventDefault(); done(input.value.trim() || null); } },
        h('h3', { text: title }), input,
        h('div.buttons', {}, h('button', { type: 'button', text: 'Cancel', onclick: () => done(null) }),
          h('button.primary', { type: 'submit', text: okLabel }))));
    back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') done(null); });
    document.body.append(back);
    input.focus(); input.select();
  });
}

export type MenuItem = { label: string; action: () => void; danger?: boolean; disabled?: boolean; checked?: boolean } | '-';

/** A context / drop-down menu at a point. */
export function menu(x: number, y: number, items: MenuItem[]) {
  closeMenu();
  const m = h('div.menu#menu', {}, items.map((it) =>
    it === '-' ? h('div.sep') : h('button' + (it.danger ? '.danger' : ''), {
      disabled: it.disabled, onclick: () => { closeMenu(); it.action(); },
    }, it.checked !== undefined ? h('span.check', { text: it.checked ? '✓' : '' }) : null, it.label)));
  document.body.append(m);
  const r = m.getBoundingClientRect();
  m.style.left = `${Math.max(4, Math.min(x, innerWidth - r.width - 4))}px`;
  m.style.top = `${Math.max(4, Math.min(y, innerHeight - r.height - 4))}px`;
  setTimeout(() => document.addEventListener('mousedown', outside, true), 0);
}
function outside(e: MouseEvent) { if (!(e.target as HTMLElement).closest('#menu')) closeMenu(); }
export function closeMenu() { $('#menu')?.remove(); document.removeEventListener('mousedown', outside, true); }

/** A menu under a button. */
export function menuBelow(button: HTMLElement, items: MenuItem[]) {
  const r = button.getBoundingClientRect();
  menu(r.left, r.bottom + 4, items);
}

export function authorsShort(a: string[]): string {
  if (!a.length) return '';
  const last = (n: string) => (n.includes(',') ? n.split(',')[0] : n.split(' ').slice(-1)[0]).trim();
  if (a.length === 1) return last(a[0]);
  if (a.length === 2) return `${last(a[0])} & ${last(a[1])}`;
  return `${last(a[0])} et al.`;
}
