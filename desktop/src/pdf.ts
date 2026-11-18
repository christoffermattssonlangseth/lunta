// The original PDF, shown with PDF.js (bundled from pdfjs-dist, no CDN): pages with a text
// layer, so text can be selected and copied, and found (PDF.js's own find controller, driven by
// the reader's find bar). Highlighting in the PDF is not here yet (the Mac and
// iPhone apps have it; their PDF highlights are listed in the reader's Highlights & Notes).
import 'pdfjs-dist/legacy/web/pdf_viewer.css';
import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url';
import { api } from './api';
import { attachCitations, closeCitation } from './pdfcite';

let viewer: any = null;
let doc: any = null;
let bus: any = null;
let findState: any = null;
let findQuery = '';
/** Told the find's progress: matches so far and the current one (-1: none), or null while searching. */
let findListener: ((r: { count: number; index: number } | null) => void) | null = null;

async function lib() {
  const pdfjs: any = await import('pdfjs-dist/legacy/build/pdf.mjs');
  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
  (globalThis as any).pdfjsLib = pdfjs;      // pdf_viewer.mjs reads it from here
  const v: any = await import('pdfjs-dist/legacy/web/pdf_viewer.mjs');
  return { pdfjs, v };
}

/** Shows the paper's PDF in `host`; `open` opens another paper (a citation card's Open in Lunta). */
export async function showPdf(host: HTMLElement, id: string, open: (id: string) => void = () => {}) {
  const { pdfjs, v } = await lib();
  const data = new Uint8Array(await api.pdfBytes(id));
  const container = document.createElement('div');
  container.className = 'pdf-container';
  const inner = document.createElement('div');
  inner.className = 'pdfViewer';
  container.append(inner);
  host.replaceChildren(container);
  const eventBus = new v.EventBus();
  const linkService = new v.PDFLinkService({ eventBus });
  // the find bar waits for typing to pause already
  const findController = new v.PDFFindController({ eventBus, linkService, delay: 0 });
  viewer = new v.PDFViewer({ container, viewer: inner, eventBus, linkService, findController, textLayerMode: 1, removePageBorders: false });
  bus = eventBus;
  findState = v.FindState;
  const report = (m: { current: number; total: number } | undefined, pending = false) => {
    if (!findListener || !findQuery) return;
    findListener(pending ? null : { count: m?.total || 0, index: (m?.current || 0) - 1 });
  };
  // while the pages are being searched the count can be 0 for a moment: only NOT_FOUND means none
  eventBus.on('updatefindmatchescount', (e: any) => { if (e.matchesCount?.total) report(e.matchesCount); });
  eventBus.on('updatefindcontrolstate', (e: any) =>
    report(e.matchesCount, e.state === findState.PENDING || (e.state !== findState.NOT_FOUND && !e.matchesCount?.total)));
  linkService.setViewer(viewer);
  // a clicked citation: its reference in a card instead of the jump (pdfcite.ts)
  attachCitations({ container, linkService, doc: () => doc, id, open });
  eventBus.on('pagesinit', () => { viewer.currentScaleValue = 'page-width'; });
  doc = await pdfjs.getDocument({ data, isEvalSupported: false }).promise;
  viewer.setDocument(doc);
  linkService.setDocument(doc, null);
  if (import.meta.env.DEV) (window as any).__pdf = { viewer, doc };
}

export function pdfZoom(f: number) {
  if (viewer) viewer.currentScale = Math.min(5, Math.max(0.3, viewer.currentScale * f));
}

/** Finds `query` in the PDF (case and accents ignored), from the page shown; '' clears. */
export function pdfFind(query: string, onUpdate: (r: { count: number; index: number } | null) => void) {
  findListener = onUpdate;
  findQuery = query;
  if (!bus) return;
  if (!query) { bus.dispatch('findbarclose', { source: null }); return; }
  bus.dispatch('find', { source: null, type: '', query, caseSensitive: false, entireWord: false, highlightAll: true, findPrevious: false, matchDiacritics: false });
}

/** The next (or previous) match of the current search. */
export function pdfFindStep(forward: boolean) {
  if (!bus || !findQuery) return;
  bus.dispatch('find', { source: null, type: 'again', query: findQuery, caseSensitive: false, entireWord: false, highlightAll: true, findPrevious: !forward, matchDiacritics: false });
}

export function pdfFindClear() {
  findQuery = '';
  findListener = null;
  bus?.dispatch('findbarclose', { source: null });
}

export function closePdf() {
  closeCitation();
  try { viewer?.setDocument(null); } catch { /* not loaded */ }
  doc?.destroy?.();
  viewer = null;
  doc = null;
  bus = null;
  findQuery = '';
  findListener = null;
}
