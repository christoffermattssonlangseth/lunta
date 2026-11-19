// Typed calls into the Rust backend (src-tauri/src/lib.rs), which calls the core directly.
import { invoke } from '@tauri-apps/api/core';

export interface PaperMeta {
  title: string; authors: string[]; date: string; journal: string; volume: string; issue: string;
  pages: string; publisher: string; doi: string; pmid: string; pmcid: string; arxiv: string; url: string;
  abstract_text: string; keywords: string[]; license: string;
  /** Crossref's work type ("journal-article", "proceedings-article", "book-chapter", "posted-content"…), "" if unknown. */
  work_type?: string;
  /** The book series of a chapter or proceedings paper ("Lecture Notes in Computer Science"). */
  series?: string;
}
export interface Paper {
  id: string; meta: PaperMeta; year: string; added: number; has_pdf: boolean; has_epub: boolean;
  read: boolean; starred: boolean; must_read: boolean; collections: string[]; tags: string[]; annotation_count: number;
  /** Citation key for LaTeX: fixed, unique, the same on every device. */
  cite_key: string;
  /** A natural-voice narration made on the Mac (narration.m4a + narration.json) is here. */
  has_audio?: boolean;
}
export interface Row { paper: Paper; snippet: string; }
export interface Folder { id: string; name: string; parent: string | null; paper_count: number; }
export interface Sidebar { all: number; starred: number; must_read: number; unread: number; folders: Folder[]; tags: string[]; smart: [string, string][]; }
export interface Info { library: string | null; opened: boolean; error: string | null; device_id: string; device_label: string; }
export interface Source { kind: 'all' | 'starred' | 'must_read' | 'unread' | 'folder' | 'tag' | 'smart' | 'foryou'; id?: string; }
export interface TocItem { title: string; level: number; anchor: string; }
export interface Book { path: string; toc: TocItem[]; }
export interface Annotation {
  id: string; paper_id: string; kind: 'Highlight' | 'Underline' | 'Note'; color: string; location: string;
  position: string; quote: string; prefix: string; suffix: string; note: string; created: number; modified: number;
}
export interface PageSelection { quote: string; prefix: string; suffix: string; start: number; }
export type CiteStyle = 'apa' | 'harvard' | 'chicago' | 'cell' | 'mla' | 'vancouver' | 'ieee' | 'ama' | 'nature' | 'science' | 'pnas' | 'acs' | 'bibtex' | 'latex' | 'ris';

export const api = {
  info: () => invoke<Info>('app_info'),
  chooseLibrary: (path: string) => invoke<void>('choose_library', { path }),
  refresh: () => invoke<boolean>('refresh'),
  sidebar: () => invoke<Sidebar>('sidebar'),
  list: (source: Source, query: string, sort: string) => invoke<Row[]>('list', { source, query, sort }),
  paper: (id: string) => invoke<Paper | null>('paper', { id }),
  setRead: (id: string, read: boolean) => invoke<void>('set_read', { id, read }),
  setStarred: (id: string, starred: boolean) => invoke<void>('set_starred', { id, starred }),
  setMustRead: (id: string, mustRead: boolean) => invoke<void>('set_must_read', { id, mustRead }),
  markOpened: (id: string) => invoke<void>('mark_opened', { id }),
  setTag: (id: string, tag: string, present: boolean) => invoke<void>('set_tag', { id, tag, present }),
  setMembership: (folder: string, id: string, present: boolean) => invoke<void>('set_membership', { folder, id, present }),
  createFolder: (name: string, parent: string | null) => invoke<string>('create_folder', { name, parent }),
  renameFolder: (id: string, name: string) => invoke<void>('rename_folder', { id, name }),
  moveFolder: (id: string, parent: string | null) => invoke<void>('move_folder', { id, parent }),
  deleteFolder: (id: string) => invoke<void>('delete_folder', { id }),
  deletePaper: (id: string) => invoke<void>('delete_paper', { id }),
  cite: (ids: string[], style: CiteStyle) => invoke<string>('cite', { ids, style }),
  setCiteKey: (id: string, key: string) => invoke<string>('set_cite_key', { id, key }),
  /** .bib files kept up to date on this device: '*' (all papers) or folder id -> path. */
  bibFiles: () => invoke<Record<string, string>>('bib_files'),
  setBibFile: (source: string, path: string | null) => invoke<void>('set_bib_file', { source, path }),
  updateBibFiles: () => invoke<Record<string, string>>('update_bib_files'),
  addPaper: (input: string) => invoke<{ paper: Paper; note: string | null }>('add_paper', { input }),
  citePaper: (doi: string, arxiv: string, reference?: string) => invoke<{ id: string | null; label: string }>('cite_paper', { doi, arxiv, reference: reference ?? null }),
  // the citation card in the PDF view (pdfcite.ts)
  pdfPrepareCitations: (id: string) => invoke<void>('pdf_prepare_citations', { id }),
  pdfReferenceAt: (id: string, page: number, x: number | null, y: number) => invoke<string | null>('pdf_reference_at', { id, page, x, y }),
  pdfReferenceForCitation: (id: string, text: string, offset: number) => invoke<string | null>('pdf_reference_for_citation', { id, text, offset }),
  lookupReference: (text: string) => invoke<{ doi: string; arxiv: string; in_library: string | null } | null>('lookup_reference', { text }),
  attachPdf: (id: string, path: string) => invoke<string | null>('attach_pdf', { id, path }),
  makeReadingView: (id: string) => invoke<void>('make_reading_view', { id }),
  findPdf: (id: string) => invoke<string | null>('find_pdf', { id }),
  pdfBytes: (id: string) => invoke<ArrayBuffer>('pdf_bytes', { id }),
  openBook: (id: string) => invoke<Book>('open_book', { id }),
  annotations: (id: string) => invoke<Annotation[]>('annotations', { id }),
  addHighlight: (id: string, selection: PageSelection, color: string, note: string) =>
    invoke<Annotation>('add_highlight', { id, selection, color, note }),
  saveAnnotation: (annotation: Annotation) => invoke<Annotation>('save_annotation', { annotation }),
  deleteAnnotation: (paper: string, id: string) => invoke<void>('delete_annotation', { paper, id }),
  positionLatest: (id: string) => invoke<{ offset: number; device: string; mine: boolean } | null>('position_latest', { id }),
  positionSave: (id: string, offset: number) => invoke<void>('position_save', { id, offset }),
};

export interface SmartFolder {
  id: string; name: string; text: string; tag: string; folder: string; unreadOnly: boolean; starredOnly: boolean;
  yearFrom: string; yearTo: string; addedDays: number; pdf: 'any' | 'has' | 'missing'; highlightsOnly: boolean;
  /** What Lunta's checks found: retracted or corrected, or a preprint published since it was added. */
  updates?: 'flagged' | 'published';
}
export const smartApi = {
  list: () => invoke<SmartFolder[]>('smart_folders'),
  save: (folder: SmartFolder) => invoke<string>('save_smart_folder', { folder }),
  remove: (id: string) => invoke<void>('delete_smart_folder', { id }),
  count: (folder: SmartFolder) => invoke<number>('smart_count', { folder }),
};
export const metaApi = {
  update: (id: string, meta: PaperMeta) => invoke<void>('update_meta', { id, meta }),
  lookup: (input: string) => invoke<PaperMeta>('lookup_meta', { input }),
};
