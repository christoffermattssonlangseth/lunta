// The citation card in the PDF view (as on the Mac: apple/Shared/PDFCitations.swift): clicking a
// citation shows its reference in a card next to it instead of jumping to the reference list,
// with its DOI (printed in it, or found by Crossref), Add to Lunta / Open in Lunta, Open Paper,
// Copy and Go to Reference. A citation is a link into the reference list (the entry at the
// link's destination), or, without a link, "[12]", a superscript number or "(Smith et al., 2020)"
// under the click. The app finds the entry in the PDF's lines (lunta_core::refcite, pdfcite.rs);
// a link that goes elsewhere (a section, a figure) jumps as before.
import { openUrl } from '@tauri-apps/plugin-opener';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { api } from './api';
import { h } from './ui';

let card: HTMLElement | null = null;

export function closeCitation() {
  card?.remove();
  card = null;
}

/** The card for `text`, next to `anchor` (a rectangle in the window). */
function show(text: string, anchor: DOMRect, opts: { goTo?: () => void; open: (id: string) => void }) {
  closeCitation();
  const status = h('div.pdf-cite-status', { text: 'Looking up the DOI…' });
  const actions = h('div.pdf-cite-actions');
  const c = h('div.pdf-cite', { role: 'dialog' }, h('div.pdf-cite-ref', { text }), status, actions);
  card = c;
  document.body.append(c);
  const button = (label: string, onclick: () => void, cls = '') => {
    const b = h('button' + cls, { text: label, onclick: (e: Event) => { e.stopPropagation(); onclick(); } }) as HTMLButtonElement;
    actions.append(b);
    return b;
  };
  const place = () => {
    const w = c.offsetWidth, hh = c.offsetHeight, vw = window.innerWidth, vh = window.innerHeight;
    const left = Math.max(8, Math.min(vw - w - 8, anchor.left + anchor.width / 2 - w / 2));
    const below = anchor.bottom + 8 + hh < vh - 8 || anchor.top < vh / 2;
    c.style.left = `${left}px`;
    c.style.top = `${Math.max(8, Math.min(vh - hh - 8, below ? anchor.bottom + 8 : anchor.top - hh - 8))}px`;
  };
  const copy = () => { writeText(text).then(() => { copyB.textContent = 'Copied'; }, () => {}); };
  const fill = (found: { doi: string; arxiv: string; in_library: string | null } | null) => {
    if (card !== c) return;
    actions.replaceChildren();
    if (found?.in_library) {
      const id = found.in_library;
      button('Open in Lunta', () => { closeCitation(); opts.open(id); }, '.primary');
    } else if (found) {
      const add = button('Add to Lunta', async () => {
        add.disabled = true;
        add.textContent = 'Adding…';
        try {
          const r = await api.citePaper(found.doi, found.arxiv);
          if (r.id) {
            const id = r.id;
            add.replaceWith(h('button.primary', { text: 'Open in Lunta', onclick: (e: Event) => { e.stopPropagation(); closeCitation(); opts.open(id); } }));
            status.textContent = r.label;
          } else { add.textContent = r.label; }
        } catch (e) { add.textContent = 'Couldn’t add it'; status.textContent = String(e); }
      }, '.primary');
    }
    if (found?.doi) button('Open Paper', () => openUrl(`https://doi.org/${found.doi}`));
    else if (found?.arxiv) button('Open Paper', () => openUrl(`https://arxiv.org/abs/${found.arxiv}`));
    else button('Search', () => openUrl('https://scholar.google.com/scholar?q=' + encodeURIComponent(text.slice(0, 200))));
    actions.append(copyB);
    if (opts.goTo) { const go = opts.goTo; button('Go to Reference', () => { closeCitation(); go(); }); }
    status.textContent = found?.doi ? `DOI ${found.doi}` : found?.arxiv ? `arXiv ${found.arxiv}` : 'No DOI found for this reference.';
    place();
  };
  const copyB = h('button', { text: 'Copy', onclick: (e: Event) => { e.stopPropagation(); copy(); } }) as HTMLButtonElement;
  actions.append(copyB);
  place();
  api.lookupReference(text).then(fill, () => fill(null));
}

// closed by a click elsewhere, Escape or scrolling the PDF
document.addEventListener('mousedown', (e) => { if (card && !card.contains(e.target as Node)) closeCitation(); }, true);
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && card) { closeCitation(); e.stopPropagation(); } }, true);

/** A PDF.js destination → page (from 0) and the point (PDF user space) it goes to. */
async function resolveDest(doc: any, dest: any): Promise<{ page: number; x: number | null; y: number } | null> {
  const explicit = typeof dest === 'string' ? await doc.getDestination(dest) : dest;
  if (!Array.isArray(explicit) || !explicit.length) return null;
  const ref = explicit[0];
  const page = typeof ref === 'number' ? ref : await doc.getPageIndex(ref).catch(() => null);
  if (page === null || page === undefined) return null;
  const kind = explicit[1]?.name;
  const num = (v: any) => (typeof v === 'number' ? v : null);
  if (kind === 'XYZ') { const y = num(explicit[3]); return y === null ? null : { page, x: num(explicit[2]), y }; }
  if (kind === 'FitH' || kind === 'FitBH') { const y = num(explicit[2]); return y === null ? null : { page, x: null, y }; }
  if (kind === 'FitR') { const y = num(explicit[5]); return y === null ? null : { page, x: num(explicit[2]), y }; }
  return null;
}

/** Hooks the card into a PDF.js viewer: link clicks into the reference list, and citations in the text. */
export function attachCitations(o: { container: HTMLElement; linkService: any; doc: () => any; id: string; open: (id: string) => void }) {
  api.pdfPrepareCitations(o.id).catch(() => {});
  o.container.addEventListener('scroll', closeCitation, { passive: true });
  let lastLink: { el: Element; t: number } | null = null;
  // which link was clicked (PDF.js's own click handler asks the link service to go)
  o.container.addEventListener('click', (e) => {
    const a = (e.target as Element).closest?.('.annotationLayer a, .annotationLayer section');
    lastLink = a ? { el: a, t: performance.now() } : null;
  }, true);
  const go = o.linkService.goToDestination.bind(o.linkService);
  o.linkService.goToDestination = async (dest: any) => {
    const l = lastLink;
    lastLink = null;
    if (l && performance.now() - l.t < 1000) {
      try {
        const d = await resolveDest(o.doc(), dest);
        const text = d && await api.pdfReferenceAt(o.id, d.page, d.x, d.y);
        if (text) { show(text, l.el.getBoundingClientRect(), { goTo: () => go(dest), open: o.open }); return; }
      } catch { /* jump as usual */ }
    }
    return go(dest);
  };
  // a citation without a link: "[12]", "cells¹²", "(Smith et al., 2020)" under the click
  o.container.addEventListener('click', async (e) => {
    const span = (e.target as Element).closest?.('.textLayer span') as HTMLElement | null;
    if (!span || (e.target as Element).closest('.annotationLayer') || !window.getSelection()?.isCollapsed) return;
    const line = lineAt(span, e.clientX, e.clientY);
    if (!line) return;
    const text = await api.pdfReferenceForCitation(o.id, line.text, line.offset).catch(() => null);
    if (text) show(text, span.getBoundingClientRect(), { open: o.open });
  });
}

/** The text of the line under a click in PDF.js's text layer, and where in it the click was
 *  (UTF-16): the clicked span with its neighbours on the same row, up to a column gap. */
function lineAt(span: HTMLElement, x: number, y: number): { text: string; offset: number } | null {
  const layer = span.closest('.textLayer');
  if (!layer) return null;
  const r0 = span.getBoundingClientRect();
  const hh = Math.max(4, r0.height);
  const row = [...layer.querySelectorAll<HTMLElement>('span')]
    .filter((s) => !s.querySelector('span'))
    .map((s) => ({ s, r: s.getBoundingClientRect() }))
    .filter(({ r }) => r.width > 0 && Math.abs((r.top + r.bottom) / 2 - (r0.top + r0.bottom) / 2) < hh * 0.6)
    .sort((a, b) => a.r.left - b.r.left);
  let i = row.findIndex((x) => x.s === span);
  if (i < 0) return null;
  let a = i, b = i;
  while (a > 0 && row[a].r.left - row[a - 1].r.right < 2 * hh) a--;
  while (b < row.length - 1 && row[b + 1].r.left - row[b].r.right < 2 * hh) b++;
  // where in the span: the caret at the click
  let inSpan = 0;
  const doc = document as any;
  const caret = doc.caretPositionFromPoint?.(x, y) ?? null;
  const range = caret ? null : doc.caretRangeFromPoint?.(x, y) ?? null;
  const node = caret ? caret.offsetNode : range?.startContainer;
  const off = caret ? caret.offset : range?.startOffset;
  if (node && span.contains(node) && typeof off === 'number') inSpan = off;
  else inSpan = Math.round(((x - r0.left) / Math.max(1, r0.width)) * (span.textContent || '').length);
  let text = '', offset = 0;
  for (let k = a; k <= b; k++) {
    const t = row[k].s.textContent || '';
    if (k > a && text && !/\s$/.test(text) && !/^\s/.test(t) && row[k].r.left - row[k - 1].r.right > hh * 0.2) text += ' ';
    if (k === i) offset = text.length + Math.min(inSpan, Math.max(0, t.length - 1));
    text += t;
  }
  return { text, offset };
}
