//! Checks against the published record (lunta_core::checks), for the desktop app: Check a
//! Reference List (pasted, a .bib / .ris / text file, a folder's papers, a paper's own reference
//! list), and retractions, corrections and published versions of preprints, checked on demand and
//! weekly in the background ("updates-checked" event when something changed). Nothing in the
//! library changes until the researcher chooses (switching to a published version, linking it,
//! keeping the preprint), as on the Mac (apple/Shared/Checks.swift).

use crate::{err, AppState, R};
use lunta_core::checks::refcheck::{self, RefCheckReport};
use lunta_core::checks::updates::UpdatesSummary;
use lunta_core::checks::{self as core_checks, PaperUpdates};
use lunta_core::model::FileKind;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

fn cache(state: &AppState) -> String {
    state.cache_root.join("checks").to_string_lossy().to_string()
}

/// Checks a reference list given as text (pasted, or a file's contents).
#[tauri::command]
pub async fn check_references(state: State<'_, AppState>, text: String) -> R<RefCheckReport> {
    let lib = state.lib()?;
    let dir = cache(&state);
    tauri::async_runtime::spawn_blocking(move || lib.mark_references_in_library(refcheck::check_references(text, dir, None))).await.map_err(err)
}

/// Checks the reference list in a file (.bib, .ris, a text file with one reference per line).
#[tauri::command]
pub async fn check_references_file(state: State<'_, AppState>, path: String) -> R<RefCheckReport> {
    let text = std::fs::read_to_string(&path).map_err(err)?;
    check_references(state, text).await
}

/// Checks a folder's papers (None: the whole library).
#[tauri::command]
pub async fn check_folder_references(state: State<'_, AppState>, folder: Option<String>) -> R<RefCheckReport> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.check_folder_references(folder, None)).await.map_err(err)
}

/// Checks a paper's own reference list: its reading view's, else its PDF's (PDFium's lines).
#[tauri::command]
pub async fn check_paper_references(state: State<'_, AppState>, id: String) -> R<RefCheckReport> {
    let lib = state.lib()?;
    let dir = cache(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let mut entries = lib.paper_reference_list(id.clone());
        if entries.is_empty() {
            if let Some(path) = lib.file_path(id.clone(), FileKind::Pdf) {
                let data = std::fs::read(path).map_err(err)?;
                let (lines, _) = lunta_core::convert::pdf::text_lines(&data).map_err(err)?;
                entries = lunta_core::refcite::reference_list(&lines);
            }
        }
        if entries.is_empty() {
            return Err("No reference list was found in this paper (its reading view or PDF).".to_string());
        }
        Ok(lib.mark_references_in_library(refcheck::check_reference_entries(entries, dir, None)))
    })
    .await
    .map_err(err)?
}

/// What the checks found, per paper (for the badges, the detail banner and the smart folders).
#[tauri::command]
pub async fn paper_updates_all(state: State<'_, AppState>) -> R<HashMap<String, PaperUpdates>> {
    Ok(state.lib()?.all_paper_updates())
}

/// The banner's sentence for a paper ("This paper was retracted on 6 February 2010."), "" if none.
#[tauri::command]
pub async fn updates_headline(updates: PaperUpdates) -> R<String> {
    Ok(core_checks::updates_headline(updates))
}

/// Checks papers (empty: the library) for retractions, corrections and published versions now.
#[tauri::command]
pub async fn check_updates(state: State<'_, AppState>, ids: Vec<String>) -> R<UpdatesSummary> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.check_paper_updates(ids, None)).await.map_err(err)?.map_err(err)
}

/// "Switch to the Published Version" (confirmed in the window).
#[tauri::command]
pub async fn use_published_version(state: State<'_, AppState>, id: String) -> R<()> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.use_published_version(id).map(|_| ())).await.map_err(err)?.map_err(err)
}

#[tauri::command]
pub async fn undo_published_version(state: State<'_, AppState>, id: String) -> R<()> {
    state.lib()?.undo_published_version(id).map(|_| ()).map_err(err)
}

#[tauri::command]
pub async fn link_published_version(state: State<'_, AppState>, id: String) -> R<()> {
    state.lib()?.link_published_version(id).map_err(err)
}

#[tauri::command]
pub async fn keep_preprint(state: State<'_, AppState>, id: String) -> R<()> {
    state.lib()?.keep_preprint(id).map_err(err)
}

/// Writes a report (Markdown or CSV) where the save dialog said.
#[tauri::command]
pub async fn save_report(path: String, text: String) -> R<()> {
    std::fs::write(path, text).map_err(err)
}

/// The weekly check in the background: a minute after start, then every six hours (it runs
/// only when a week has passed on this computer).
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            let lib = app.state::<AppState>().lib().ok();
            if let Some(lib) = lib {
                if let Ok(Some(s)) = lib.check_paper_updates_if_due(None) {
                    if !s.changed.is_empty() {
                        let _ = app.emit("updates-checked", s);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}
