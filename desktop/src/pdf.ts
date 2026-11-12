// The original PDF, shown with PDF.js (bundled from pdfjs-dist, no CDN): pages with a text
// layer, so text can be selected and copied. Highlighting in the PDF is not here yet (the Mac and
// iPhone apps have it; their PDF highlights are listed in the reader's Highlights & Notes).
import 'pdfjs-dist/legacy/web/pdf_viewer.css';
import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url';
import { api } from './api';

let viewer: any = null;
let doc: any = null;

async function lib() {
  const pdfjs: any = await import('pdfjs-dist/legacy/build/pdf.mjs');
  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
  (globalThis as any).pdfjsLib = pdfjs;      // pdf_viewer.mjs reads it from here
  const v: any = await import('pdfjs-dist/legacy/web/pdf_viewer.mjs');
  return { pdfjs, v };
}

export async function showPdf(host: HTMLElement, id: string) {
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
  viewer = new v.PDFViewer({ container, viewer: inner, eventBus, linkService, textLayerMode: 1, removePageBorders: false });
  linkService.setViewer(viewer);
  eventBus.on('pagesinit', () => { viewer.currentScaleValue = 'page-width'; });
  doc = await pdfjs.getDocument({ data, isEvalSupported: false }).promise;
  viewer.setDocument(doc);
  linkService.setDocument(doc, null);
  if (import.meta.env.DEV) (window as any).__pdf = { viewer, doc };
}

export function pdfZoom(f: number) {
  if (viewer) viewer.currentScale = Math.min(5, Math.max(0.3, viewer.currentScale * f));
}

export function closePdf() {
  try { viewer?.setDocument(null); } catch { /* not loaded */ }
  doc?.destroy?.();
  viewer = null;
  doc = null;
}
