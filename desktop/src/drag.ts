// Dragging inside the window (papers and folders onto the sidebar), done with plain mouse events.
// HTML5 drag and drop is unreliable in a Tauri window (the webview's own file-drop handling takes
// over the drag events on Windows), and this behaves the same on every platform.
import { h } from './ui';

export type Payload = { kind: 'papers'; ids: string[] } | { kind: 'folder'; id: string };

interface Options {
  /** What a mouse-down on this element would drag (null = nothing). Called when the drag starts. */
  begin(el: HTMLElement): { payload: Payload; label: string } | null;
  /** Whether the drop target (an element with data-drop) takes this payload. */
  accepts(payload: Payload, target: HTMLElement): boolean;
  drop(payload: Payload, target: HTMLElement): void;
}

const THRESHOLD = 5;

export function setupDrag(opts: Options) {
  document.addEventListener('mousedown', (down) => {
    if (down.button !== 0) return;
    const src = (down.target as HTMLElement).closest?.('[data-drag]') as HTMLElement | null;
    if (!src) return;
    const x0 = down.clientX, y0 = down.clientY;
    let started: { payload: Payload; ghost: HTMLElement } | null = null;
    let over: HTMLElement | null = null;

    const setOver = (el: HTMLElement | null) => {
      if (el === over) return;
      over?.classList.remove('drop-over');
      over = el;
      over?.classList.add('drop-over');
    };
    const targetAt = (x: number, y: number) => {
      if (!started) return null;
      const t = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest('[data-drop]') as HTMLElement | null;
      return t && opts.accepts(started.payload, t) ? t : null;
    };
    const move = (e: MouseEvent) => {
      if (!started) {
        if (Math.hypot(e.clientX - x0, e.clientY - y0) < THRESHOLD) return;
        const b = opts.begin(src);
        if (!b) { stop(); return; }
        const ghost = h('div.drag-ghost', { text: b.label });
        document.body.append(ghost);
        document.body.classList.add('dragging');
        started = { payload: b.payload, ghost };
      }
      e.preventDefault();
      started.ghost.style.left = `${e.clientX + 12}px`;
      started.ghost.style.top = `${e.clientY + 12}px`;
      setOver(targetAt(e.clientX, e.clientY));
    };
    const up = (e: MouseEvent) => {
      const s = started, t = s && targetAt(e.clientX, e.clientY);
      stop();
      if (s) {
        // the click that follows a drag must not select anything
        const swallow = (c: Event) => { c.stopPropagation(); c.preventDefault(); };
        window.addEventListener('click', swallow, { capture: true, once: true });
        setTimeout(() => window.removeEventListener('click', swallow, true), 0);
        if (t) opts.drop(s.payload, t);
      }
    };
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') { stop(); } };
    function stop() {
      setOver(null);
      started?.ghost.remove();
      started = null;
      document.body.classList.remove('dragging');
      document.removeEventListener('mousemove', move, true);
      document.removeEventListener('mouseup', up, true);
      document.removeEventListener('keydown', key, true);
    }
    document.addEventListener('mousemove', move, true);
    document.addEventListener('mouseup', up, true);
    document.addEventListener('keydown', key, true);
  });
}
