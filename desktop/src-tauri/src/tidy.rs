//! Tidy the Library and Outline from Highlights (lunta_core::tidy, lunta_core::outline), for the
//! desktop app. Lunta proposes, the researcher decides: the report lists duplicates, missing
//! details, topics and tags to approve (src/tidy.ts); what is approved is applied through the core
//! (the change log, so it syncs) and comes back as a TidyChange the window can undo. Merging is
//! not undoable (the window asks first). The open model (ai.rs) may propose the topics and group
//! an outline's highlights into themes; every point of an outline stays a highlight or note.
//!
//! Events: `tidy-progress` (a line for the status bar while DOIs are looked up).

use crate::{ai, err, AppState, R};
use lunta_core::outline::Outline;
use lunta_core::tidy::{DoiReport, DoiSuggestion, GroupProposal, TagSuggestion, TidyChange, TidyReport};
use lunta_core::ProgressListener;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

struct Emit(AppHandle);

impl ProgressListener for Emit {
    fn on_progress(&self, message: String) {
        let _ = self.0.emit("tidy-progress", message);
    }
}

/// How many paper titles (topics) and highlights (outline) fit the model's context with room
/// for its answer: about 110 and 50 with the desktop model's 4096 tokens.
fn budget() -> (u32, u32) {
    let ctx = lunta_core::llm::models::desktop_model().context.max(2048);
    (((ctx - 700) / 30).clamp(40, 150), ((ctx - 900) / 60).clamp(30, 100))
}

/// Everything Tidy Library suggests without going online (fast).
#[tauri::command]
pub async fn tidy_report(state: State<'_, AppState>) -> R<TidyReport> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.tidy_report()).await.map_err(err)
}

/// Don't suggest this again on this computer ("Not Duplicates", "doi:<id>", "tag:<id>:<tag>", "topic:<name>"…).
#[tauri::command]
pub async fn tidy_dismiss(state: State<'_, AppState>, key: String) -> R<()> {
    state.lib()?.tidy_dismiss(key).map_err(err)
}

#[tauri::command]
pub async fn tidy_reset_dismissed(state: State<'_, AppState>) -> R<()> {
    state.lib()?.tidy_reset_dismissed().map_err(err)
}

/// "Merge, Keep Selected" (confirmed in the window; not undoable).
#[tauri::command]
pub async fn tidy_merge(state: State<'_, AppState>, keep: String, others: Vec<String>) -> R<()> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.merge_papers(keep, others).map(|_| ())).await.map_err(err)?.map_err(err)
}

/// Looks up DOIs for papers without one (empty: all listed as missing one). Changes nothing.
#[tauri::command]
pub async fn tidy_find_dois(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>) -> R<DoiReport> {
    let lib = state.lib()?;
    let progress: Arc<dyn ProgressListener> = Arc::new(Emit(app));
    tauri::async_runtime::spawn_blocking(move || lib.find_missing_dois(ids, Some(progress))).await.map_err(err)
}

#[tauri::command]
pub async fn tidy_apply_dois(state: State<'_, AppState>, suggestions: Vec<DoiSuggestion>) -> R<TidyChange> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.apply_doi_suggestions(suggestions)).await.map_err(err)?.map_err(err)
}

/// Whether the on-device model is downloaded (and can run here).
#[tauri::command]
pub fn tidy_model_ready(app: AppHandle) -> bool {
    ai::model_installed(&app)
}

/// Topics proposed by the open model (seconds to a minute). Empty when it gave nothing usable:
/// the window keeps the topics found without it.
#[tauri::command]
pub async fn tidy_topics_ai(app: AppHandle, state: State<'_, AppState>) -> R<Vec<GroupProposal>> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || {
        let engine = ai::engine(&app)?.ok_or("Download the model first (Settings ▾ › On-Device AI…).")?;
        Ok(lib.tidy_topics_with_model(engine, budget().0))
    })
    .await
    .map_err(err)?
}

/// Makes the ticked topics into tags or folders.
#[tauri::command]
pub async fn tidy_create_groups(state: State<'_, AppState>, proposals: Vec<GroupProposal>, as_folders: bool) -> R<TidyChange> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.create_groups(proposals, as_folders)).await.map_err(err)?.map_err(err)
}

#[tauri::command]
pub async fn tidy_apply_tags(state: State<'_, AppState>, suggestions: Vec<TagSuggestion>) -> R<TidyChange> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.apply_tag_suggestions(suggestions)).await.map_err(err)?.map_err(err)
}

/// Undoes what applying tags, folders or DOIs did.
#[tauri::command]
pub async fn tidy_undo(state: State<'_, AppState>, change: TidyChange) -> R<()> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.undo_tidy_change(change)).await.map_err(err)?.map_err(err)
}

/// The outline of these papers' highlights and notes, about `topic` (may be empty); with
/// `use_model`, grouped into themes by the open model (by paper and section if it isn't there
/// or gives nothing usable).
#[tauri::command]
pub async fn outline_make(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>, topic: String, use_model: bool) -> R<Outline> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || {
        let engine = if use_model { ai::engine(&app)? } else { None };
        Ok(match engine {
            Some(e) => lib.outline_with_model(ids, topic, e, budget().1),
            None => lib.outline_from_highlights(ids, topic, None, None),
        })
    })
    .await
    .map_err(err)?
}
