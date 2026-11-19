// Importing a library from Zotero, Mendeley, ReadCube, EndNote… and a folder of PDFs, like the Mac's
// File › Import (apple/Mac/LuntaApp.swift) and the iPhone's + › Import Library (ImportLibrary.swift).
// The work is in Rust (src-tauri/src/importer.rs → core/src/import_source.rs).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open as openDialog, message } from '@tauri-apps/plugin-dialog';
import { h, status } from './ui';

interface Source { export: string; root: string | null; temporary: string | null; suggested_folder: string; }
interface Summary { added: number; merged: number; pdfs: number; paper_ids: string[]; text: string; }
interface Progress { message: string; done: number; total: number; }
interface ViewsEvent { message: string; done: number; total: number; finished: boolean; }

let importing = false;
let onChange: () => void = () => {};

/** Called after papers came in (the library list reloads). */
export function setImportReload(f: () => void) { onChange = f; }

listen<Progress>('import-progress', (e) => { if (importing) status(e.payload.message, 'busy'); });
let viewsReload = 0;
listen<ViewsEvent>('reading-views', (e) => {
  const v = e.payload;
  if (v.finished) { if (v.message) status(v.message); onChange(); return; }
  // in the status bar unless something else is showing its progress there
  if (!importing && !document.querySelector('#status.busy')) status(v.message, 'info', true);
  // the list shows the new reading views now and then
  if (++viewsReload % 10 === 0) onChange();
});

/** A modal with buttons; resolves to the chosen button's key, or null when dismissed. */
function choose(title: string, body: (HTMLElement | string)[], buttons: [string, string, boolean?][]): Promise<string | null> {
  return new Promise((resolve) => {
    const done = (v: string | null) => { back.remove(); resolve(v); };
    const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) done(null); } },
      h('div.modal.wide', {}, h('h3', { text: title }), ...body,
        h('div.buttons', {}, buttons.map(([key, label, primary]) =>
          h(primary ? 'button.primary' : 'button', { type: 'button', text: label, onclick: () => done(key) })))));
    back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') done(null); });
    document.body.append(back);
    (back.querySelector('button.primary') as HTMLElement | null)?.focus();
  });
}

/** Import Library…: what to pick (the export's folder, or a .bib / .ris / ZIP file), then the import. */
export async function importLibrary() {
  if (importing) { status('An import is already running.', 'error'); return; }
  const how = await choose('Import a library', [
    h('p', { text: 'From Zotero, Mendeley, ReadCube Papers, EndNote or JabRef: export the library (or one collection) as BibTeX or RIS.' }),
    h('p.hint', { text: 'In Zotero: File › Export Library…, format BibTeX, tick “Export Files”. Then choose the folder it makes, so the PDFs come too. A ZIP of that folder works as well; the .bib or .ris file alone brings the papers, and the PDFs its paths point to on this computer.' }),
  ], [['cancel', 'Cancel'], ['file', 'Choose a .bib, .ris or ZIP File…'], ['folder', 'Choose the Export Folder…', true]]);
  if (how !== 'file' && how !== 'folder') return;
  const picked = how === 'folder'
    ? await openDialog({ directory: true, title: 'The exported library’s folder' })
    : await openDialog({ title: 'The exported library', filters: [{ name: 'BibTeX, RIS or ZIP', extensions: ['bib', 'bibtex', 'ris', 'txt', 'zip'] }] });
  if (typeof picked !== 'string') return;
  importing = true;
  status(`Opening ${picked.split(/[\\/]/).pop()}…`, 'busy');
  let source: Source;
  try { source = await invoke<Source>('import_open', { path: picked }); }
  catch (e) { importing = false; status(''); await message(String(e), { title: 'Import Library', kind: 'error' }); return; }
  status('');
  const folder = await askFolder(source.suggested_folder);
  if (folder === null) { importing = false; invoke('import_discard', { source }).catch(() => {}); return; }
  status(`Importing ${source.export.split(/[\\/]/).pop()}…`, 'busy');
  let summary: Summary;
  try { summary = await invoke<Summary>('import_library', { source, folder: folder || null }); }
  catch (e) { importing = false; status(`Import failed: ${e}`, 'error'); onChange(); return; }
  importing = false;
  status(summary.text, 'info', true);
  onChange();
  const views = summary.paper_ids.length > 0;
  await message(summary.text + (views ? '\n\nReading views are made now in the background; the status bar shows how far it has got.' : ''), { title: 'Import Done' });
  if (views) invoke('make_reading_views', { ids: summary.paper_ids }).catch((e) => status(String(e), 'error'));
}

/** "Put the imported papers in a folder?": the name (empty: none), or null when cancelled. */
function askFolder(suggested: string): Promise<string | null> {
  return new Promise((resolve) => {
    const input = h('input', { type: 'text', value: suggested, placeholder: 'Folder name' }) as HTMLInputElement;
    const done = (v: string | null) => { back.remove(); resolve(v); };
    const back = h('div.modal-back', { onmousedown: (e: MouseEvent) => { if (e.target === back) done(null); } },
      h('form.modal', { onsubmit: (e: Event) => { e.preventDefault(); done(input.value.trim()); } },
        h('h3', { text: 'Put the imported papers in a folder?' }),
        h('p.hint', { text: 'Exports don’t include your folders. To move a collection from Zotero or Mendeley, export it on its own and import it into a folder of the same name.' }),
        input,
        h('div.buttons', {},
          h('button', { type: 'button', text: 'Cancel', onclick: () => done(null) }),
          h('button', { type: 'button', text: 'Import Without Folder', onclick: () => done('') }),
          h('button.primary', { type: 'submit', text: 'Import into Folder' }))));
    back.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') done(null); });
    document.body.append(back);
    input.focus(); input.select();
  });
}

/** Import Folder of PDFs…: each PDF goes to the paper it belongs to (by its DOI, else its title). */
export async function importPdfFolder() {
  if (importing) { status('An import is already running.', 'error'); return; }
  const dir = await openDialog({ directory: true, title: 'A folder of PDFs to match to your papers' });
  if (typeof dir !== 'string') return;
  importing = true;
  status('Matching PDFs…', 'busy');
  try {
    const text = await invoke<string>('import_pdf_folder', { folder: dir });
    importing = false;
    status(text, 'info', true);
    onChange();
    invoke('make_reading_views', { ids: null }).catch(() => {});
  } catch (e) { importing = false; status(`Couldn't match the PDFs: ${e}`, 'error'); }
}

/** Make Missing Reading Views: every paper without one, in the background. */
export async function makeMissingReadingViews() {
  const started = await invoke<boolean>('make_reading_views', { ids: null }).catch((e) => { status(String(e), 'error'); return true; });
  if (!started) status('Reading views are already being made.');
}
