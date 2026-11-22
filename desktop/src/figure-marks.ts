// Regions marked on figures (core/src/figmark.rs, apple/Shared/FigureMarks.swift): the desktop
// viewer's geometry and the marks' place in this reading view. Plain TypeScript without imports, so
// scripts/reader-tests/figure-marks.test.mjs runs it as it is.
//
// A mark is an annotation with `location` "figure" and its region as JSON in `position` (the
// figure's label, its image's file name and content hash, x/y/w/h from 0 to 1 of the image from its
// top left, "rect" or "ellipse", where the figure is in the text). The core keeps it a mark when it
// is saved (whatever kind the editor gives it).

/** A region of the image, 0–1 from its top left. */
export type Rect = { x: number; y: number; w: number; h: number };
/** A point on screen (client coordinates). */
export type Point = { x: number; y: number };
/** Where the image is drawn on screen (zoomed and moved: its bounding box). */
export type Box = { left: number; top: number; width: number; height: number };
/** A mark as the viewer draws it. */
export type Mark = Rect & { id: string; color?: string; ellipse?: boolean; note?: string; image?: number };

/** The region stored with a mark (core FigureRegion, as JSON). */
export type FigureRegion = {
  figure: string; index: number; image: string; hash: string; image_index: number;
  x: number; y: number; w: number; h: number; shape: string; start: number;
};

const clamp = (v: number) => Math.max(0, Math.min(1, v));

/** A drag from `a` to `b` on screen as a region of the image drawn in `box`. */
export function figureRegion(a: Point, b: Point, box: Box): Rect {
  if (box.width <= 0 || box.height <= 0) return { x: 0, y: 0, w: 0, h: 0 };
  const x0 = clamp((Math.min(a.x, b.x) - box.left) / box.width), x1 = clamp((Math.max(a.x, b.x) - box.left) / box.width);
  const y0 = clamp((Math.min(a.y, b.y) - box.top) / box.height), y1 = clamp((Math.max(a.y, b.y) - box.top) / box.height);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** A region of the image drawn in `box`, on screen. */
export function regionBox(r: Rect, box: Box): Box {
  return { left: box.left + r.x * box.width, top: box.top + r.y * box.height, width: r.w * box.width, height: r.h * box.height };
}

/** The smallest mark under a point on screen (a few pixels of slack around thin ones). */
export function markAt(p: Point, box: Box, marks: Mark[], slack = 4): string | null {
  let best: Mark | null = null;
  for (const m of marks) {
    const b = regionBox(m, box);
    if (p.x < b.left - slack || p.x > b.left + b.width + slack || p.y < b.top - slack || p.y > b.top + b.height + slack) continue;
    if (!best || m.w * m.h < best.w * best.h) best = m;
  }
  return best ? best.id : null;
}

/** The region of a mark's annotation (null for a highlight). */
export function regionOf(a: { location: string; position: string }): FigureRegion | null {
  if (a.location !== 'figure') return null;
  try {
    const r = JSON.parse(a.position);
    return r && typeof r === 'object' ? { figure: '', index: 0, image: '', hash: '', image_index: 0, x: 0, y: 0, w: 0, h: 0, shape: 'rect', start: 0, ...r } : null;
  } catch { return null; }
}

/** "Figure 3", "Fig. 3" and "FIGURE 3." are one figure (core figmark::label_key). */
export function labelKey(label: string): string {
  const k = label.toLowerCase().replace(/[^\p{L}\p{N}]/gu, '');
  return k.replace(/figures/g, 'fig').replace(/figure/g, 'fig').replace(/figs/g, 'fig');
}

/** The file name in a URL or path, without a query and decoded. */
export function fileName(src: string): string {
  const last = src.split(/[?#]/)[0].split(/[\\/]/).pop() || '';
  try { return decodeURIComponent(last); } catch { return last; }
}

/** An image's content hash as the core makes it (FNV-1a, 64 bits, hex). */
export function hashBytes(b: Uint8Array): string {
  let hi = 0xcbf29ce4, lo = 0x84222325;
  for (let i = 0; i < b.length; i++) {
    lo = (lo ^ b[i]) >>> 0;
    // (hi:lo) × 0x100000001b3 mod 2^64 = (hi:lo) × 0x1b3 + (lo << 40)
    const l = lo * 0x1b3;
    hi = (Math.imul(hi, 0x1b3) + Math.floor(l / 4294967296) + (lo << 8)) >>> 0;
    lo = l >>> 0;
  }
  return hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0');
}

/** A figure as placing marks needs it: its label and its images. */
export type FigureImages = { label: string; srcs: string[] };

/** Where each mark is among the page's figures (core place_figure_marks): the figure with its label
 *  and image name; else the image with its content hash (`hash` gives an image's, async); else the
 *  figure with its label (nearest its old place), the image at its place; else, for a mark on a
 *  figure without a label, the image of its name. Marks whose figure isn't there are left out. */
export async function placeMarks<A extends { id: string; location: string; position: string }>(
  marks: A[], figures: FigureImages[], hash?: (src: string) => Promise<string>,
): Promise<Map<string, { figure: number; image: number }>> {
  const keys = figures.map((f) => labelKey(f.label));
  const names = figures.map((f) => f.srcs.map(fileName));
  const hashes = new Map<string, string>();
  const out = new Map<string, { figure: number; image: number }>();
  for (const a of marks) {
    const r = regionOf(a);
    if (!r) continue;
    const key = labelKey(r.figure), labelled = key !== '' && key !== 'fig';
    const same = labelled ? keys.map((k, i) => (k === key ? i : -1)).filter((i) => i >= 0) : [];
    let found: { figure: number; image: number } | null = null;
    if (r.image) for (const i of same) { const j = names[i].indexOf(r.image); if (j >= 0) { found = { figure: i, image: j }; break; } }
    if (!found && r.hash && hash) {
      outer: for (let i = 0; i < figures.length; i++) {
        for (let j = 0; j < figures[i].srcs.length; j++) {
          const src = figures[i].srcs[j];
          if (!hashes.has(src)) hashes.set(src, await hash(src).catch(() => ''));
          if (hashes.get(src) === r.hash) { found = { figure: i, image: j }; break outer; }
        }
      }
    }
    if (!found && same.length) {
      const i = same.reduce((b, x) => (Math.abs(x - r.index) < Math.abs(b - r.index) ? x : b), same[0]);
      found = { figure: i, image: Math.min(r.image_index, Math.max(0, figures[i].srcs.length - 1)) };
    }
    if (!found && !labelled && r.image) {
      for (let i = 0; i < figures.length && !found; i++) { const j = names[i].indexOf(r.image); if (j >= 0) found = { figure: i, image: j }; }
    }
    if (found) out.set(a.id, found);
  }
  return out;
}

/** "Figure 3 · region": a mark's line in lists. */
export function markTitle(r: FigureRegion | null): string {
  return `${r?.figure || 'Figure'} · region`;
}

/** A CSS crop of the region from its image: background size and position for a box showing just it. */
export function cropStyle(src: string, r: Rect): string {
  const pad = 0.04;
  const x = Math.max(0, r.x - pad), y = Math.max(0, r.y - pad);
  const w = Math.min(1 - x, r.w + 2 * pad), h = Math.min(1 - y, r.h + 2 * pad);
  const pos = (at: number, size: number) => (size >= 1 ? 0 : (at / (1 - size)) * 100);
  return `background-image: url(${JSON.stringify(src)}); background-size: ${100 / Math.max(w, 0.01)}% ${100 / Math.max(h, 0.01)}%;`
    + ` background-position: ${pos(x, w)}% ${pos(y, h)}%; background-repeat: no-repeat;`;
}
