//! Importing a library from Zotero, Mendeley, ReadCube, EndNote… (a BibTeX or RIS export, the folder
//! Zotero's "Export Files" makes, or a ZIP of it), and a folder of PDFs matched to the papers. The
//! work is the core's (core/src/import_source.rs, shared with Android); this file runs it off the
//! main thread, reports progress ("import-progress" events) and makes the reading views afterwards
//! in the background ("reading-views" events), like the Mac's File › Import and Make Missing
//! Reading Views.

use crate::{add, err, AppState, R};
use lunta_core::import_source::{self, ImportProgress, ImportSource};
use lunta_core::model::{PaperFilter, SortKey};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// What was picked, as the frontend keeps it between the pick and the folder question.
#[derive(Serialize, Deserialize, Clone)]
pub struct Source {
    export: String,
    root: Option<String>,
    temporary: Option<String>,
    suggested_folder: String,
}

impl From<ImportSource> for Source {
    fn from(s: ImportSource) -> Self {
        Source { export: s.export, root: s.root, temporary: s.temporary, suggested_folder: s.suggested_folder }
    }
}

impl From<Source> for ImportSource {
    fn from(s: Source) -> Self {
        ImportSource { export: s.export, root: s.root, temporary: s.temporary, suggested_folder: s.suggested_folder }
    }
}

#[derive(Serialize)]
pub struct Summary {
    added: u32,
    merged: u32,
    pdfs: u32,
    paper_ids: Vec<String>,
    text: String,
}

#[derive(Serialize, Clone)]
struct Progress {
    message: String,
    done: u32,
    total: u32,
}

struct Emit(AppHandle);

impl ImportProgress for Emit {
    fn on_import_progress(&self, message: String, done: u32, total: u32) {
        let _ = self.0.emit("import-progress", Progress { message, done, total });
    }
}

fn temp_dir(state: &AppState) -> String {
    let d = state.cache_root.join("import");
    let _ = std::fs::create_dir_all(&d);
    d.to_string_lossy().to_string()
}

/// Opens what was picked: finds the export in a folder, unpacks a ZIP into the app's cache.
#[tauri::command]
pub async fn import_open(state: State<'_, AppState>, path: String) -> R<Source> {
    let tmp = temp_dir(&state);
    tauri::async_runtime::spawn_blocking(move || import_source::open_import_source(path, tmp).map(Source::from).map_err(err))
        .await
        .map_err(err)?
}

/// The import was cancelled after the pick: an unpacked ZIP goes.
#[tauri::command]
pub fn import_discard(source: Source) {
    import_source::discard_import_source(source.into());
}

/// Imports what was picked, into `folder` when one is given.
#[tauri::command]
pub async fn import_library(app: AppHandle, state: State<'_, AppState>, source: Source, folder: Option<String>) -> R<Summary> {
    let lib = state.lib()?;
    let progress: Arc<dyn ImportProgress> = Arc::new(Emit(app));
    tauri::async_runtime::spawn_blocking(move || {
        lib.import_library(source.into(), folder, Some(progress)).map_err(err).map(|s| Summary {
            added: s.added,
            merged: s.merged,
            pdfs: s.pdfs + s.matched,
            paper_ids: s.paper_ids,
            text: s.text,
        })
    })
    .await
    .map_err(err)?
}

/// Matches a folder of PDFs to papers in the library (by DOI, else title). Returns the summary.
#[tauri::command]
pub async fn import_pdf_folder(app: AppHandle, state: State<'_, AppState>, folder: String) -> R<String> {
    let lib = state.lib()?;
    let progress: Arc<dyn ImportProgress> = Arc::new(Emit(app));
    tauri::async_runtime::spawn_blocking(move || lib.match_pdf_folder(folder, vec![], Some(progress)).map(|r| r.text).map_err(err))
        .await
        .map_err(err)?
}

static MAKING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone)]
struct ViewsEvent {
    message: String,
    done: u32,
    total: u32,
    finished: bool,
}

/// Makes the missing reading views in the background, one paper after another: of `ids`, or of
/// every paper when none are given. Returns false when it is already running.
#[tauri::command]
pub fn make_reading_views(app: AppHandle, state: State<'_, AppState>, ids: Option<Vec<String>>) -> R<bool> {
    let lib = state.lib()?;
    if MAKING.swap(true, Ordering::SeqCst) {
        return Ok(false);
    }
    std::thread::spawn(move || {
        let todo: Vec<String> = match ids {
            Some(ids) => ids.into_iter().filter(|id| lib.paper(id.clone()).is_some_and(|p| !p.has_epub)).collect(),
            None => lib.papers(PaperFilter::default(), SortKey::Added).into_iter().filter(|p| !p.has_epub).map(|p| p.id).collect(),
        };
        let total = todo.len() as u32;
        let send = |message: String, done: u32, finished: bool| {
            let _ = app.emit("reading-views", ViewsEvent { message, done, total, finished });
        };
        let quiet = add::Logger(Arc::new(|_: &str| {}));
        let mut made = 0;
        for (i, id) in todo.iter().enumerate() {
            send(format!("Making reading views: {} of {total}", i + 1), i as u32, false);
            if add::make_reading_view(&lib, id, &quiet).is_ok() {
                made += 1;
                // a reading view the reader had cached is stale now
                app.state::<AppState>().books.lock().unwrap().remove(id);
            }
        }
        let message = if total == 0 {
            String::new()
        } else {
            format!("Made {made} of {total} reading views{}.", if made < total { " (the others have no open full text or readable PDF)" } else { "" })
        };
        send(message, total, true);
        MAKING.store(false, Ordering::SeqCst);
    });
    Ok(true)
}
