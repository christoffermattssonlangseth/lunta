// Typed calls into the Rust backend (src-tauri/src/lib.rs), which calls the core directly.
import { invoke } from '@tauri-apps/api/core';

export interface PaperMeta {
  title: string; authors: string[]; date: string; journal: string; volume: string; issue: string;
  pages: string; publisher: string; doi: string; pmid: string; pmcid: string; arxiv: string; url: string;
  abstract_text: string; keywords: string[]; license: string;
}
export interface Paper {
  id: string; meta: PaperMeta; year: string; added: number; has_pdf: boolean; has_epub: boolean;
  read: boolean; starred: boolean; collections: string[]; tags: string[]; annotation_count: number;
}
export interface Row { paper: Paper; snippet: string; }
export interface Folder { id: string; name: string; parent: string | null; paper_count: number; }
export interface Sidebar { all: number; starred: number; unread: number; folders: Folder[]; tags: string[]; smart: [string, string][]; }
export interface Info { library: string | null; opened: boolean; error: string | null; device_id: string; device_label: string; }
export interface Source { kind: 'all' | 'starred' | 'unread' | 'folder' | 'tag' | 'smart'; id?: string; }
export interface TocItem { title: string; level: number; anchor: string; }
export interface Book { path: string; toc: TocItem[]; }
export interface Annotation {
  id: string; paper_id: string; kind: 'Highlight' | 'Underline' | 'Note'; color: string; location: string;
  position: string; quote: string; prefix: string; suffix: string; note: string; created: number; modified: number;
}
export interface PageSelection { quote: string; prefix: string; suffix: string; start: number; }
export type CiteStyle = 'apa' | 'vancouver' | 'nature' | 'bibtex' | 'ris';

export const api = {
  info: () => invoke<Info>('app_info'),
  chooseLibrary: (path: string) => invoke<void>('choose_library', { path }),
  refresh: () => invoke<boolean>('refresh'),
  sidebar: () => invoke<Sidebar>('sidebar'),
  list: (source: Source, query: string, sort: string) => invoke<Row[]>('list', { source, query, sort }),
  paper: (id: string) => invoke<Paper | null>('paper', { id }),
  setRead: (id: string, read: boolean) => invoke<void>('set_read', { id, read }),
  setStarred: (id: string, starred: boolean) => invoke<void>('set_starred', { id, starred }),
  setTag: (id: string, tag: string, present: boolean) => invoke<void>('set_tag', { id, tag, present }),
  setMembership: (folder: string, id: string, present: boolean) => invoke<void>('set_membership', { folder, id, present }),
  createFolder: (name: string, parent: string | null) => invoke<string>('create_folder', { name, parent }),
  renameFolder: (id: string, name: string) => invoke<void>('rename_folder', { id, name }),
  moveFolder: (id: string, parent: string | null) => invoke<void>('move_folder', { id, parent }),
  deleteFolder: (id: string) => invoke<void>('delete_folder', { id }),
  deletePaper: (id: string) => invoke<void>('delete_paper', { id }),
  cite: (ids: string[], style: CiteStyle) => invoke<string>('cite', { ids, style }),
  addPaper: (input: string) => invoke<{ paper: Paper; note: string | null }>('add_paper', { input }),
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
