//! Check Citations on Windows and Linux, as on the Mac (core/src/claims): for each citation, the
//! cited paper's passages that best match the citing sentence and, with the open model (Qwen3,
//! when downloaded: ai.rs), a suggested verdict whose quotes the core checks word for word. Lunta
//! proposes, the writer decides; everything stays on this computer.
//!   - the Word and Google Docs panels: /cite/claims and /cite/open (citeapi.rs), with the hooks here
//!   - a .tex file: `tex_claims` (its \cite commands, their sentences, the library's papers for the
//!     keys, from the library's own keys or the .bib's DOI or title) and `check_claim`, shown by
//!     src/claims.ts

use crate::{ai, err, AppState, R};
use lunta_core::claims::{self, ClaimJudge};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager, State};

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Gives the hooks the app (at start).
pub(crate) fn start(app: AppHandle) {
    let _ = APP.set(app);
}

/// The open model, when downloaded (loaded the first time: seconds). Blocking.
pub(crate) struct HookJudge;

impl ClaimJudge for HookJudge {
    fn answer(&self, instructions: String, prompt: String, max_tokens: u32) -> Option<String> {
        let app = APP.get()?;
        if !ai::model_installed(app) {
            return None;
        }
        let engine = ai::engine(app).ok()??;
        engine.plain_answer(&instructions, &prompt, max_tokens).ok()
    }
}

/// Shows the paper, found at the passage's words (a panel's evidence clicked): src/main.ts.
pub(crate) fn open_passage(id: &str, find: &str) {
    let Some(app) = APP.get() else { return };
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    let _ = app.emit("open-passage", json!({"id": id, "find": find}).to_string());
}

/// The citations of a .tex file: [{keys, ids, command, line, sentence, previous, claim}] and
/// whether a .bib was read (keys the library doesn't have are looked up there by DOI or title).
#[tauri::command]
pub async fn tex_claims(state: State<'_, AppState>, path: String) -> R<Value> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn’t read {path} ({e}). Lunta reads .tex files written in UTF-8."))?;
        let found = claims::tex_claims(&text);
        let dir = Path::new(&path).parent().map(Path::to_path_buf).unwrap_or_default();
        let bib: String = claims::tex_bib_files(&text).iter().filter_map(|f| std::fs::read_to_string(dir.join(f)).ok()).collect::<Vec<_>>().join("\n");
        let mut keys: Vec<String> = found.iter().flat_map(|c| c.keys.clone()).collect();
        keys.sort();
        keys.dedup();
        let ids = lib.papers_for_keys(&keys, &bib);
        let by_key: std::collections::HashMap<&String, &String> = keys.iter().zip(ids.iter()).collect();
        Ok(json!({
            "bib": !bib.is_empty(),
            "claims": found.iter().map(|c| json!({
                "keys": c.keys, "ids": c.keys.iter().map(|k| by_key.get(k).map(|s| s.as_str()).unwrap_or("")).collect::<Vec<_>>(),
                "command": c.command, "line": c.line, "sentence": c.sentence, "previous": c.previous, "claim": c.claim,
            })).collect::<Vec<_>>(),
        }))
    })
    .await
    .map_err(err)?
}

/// One citation's check, as the panels' /cite/claims answers it (the model when downloaded).
#[tauri::command]
pub async fn check_claim(state: State<'_, AppState>, claim: String, context: String, ids: Vec<String>) -> R<Value> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || {
        let body = json!({"claim": claim, "context": context, "ids": ids}).to_string();
        let (status, v) = lib.claims_answer(&body, Some(&HookJudge));
        if status == 200 { Ok(v) } else { Err(v.get("error").and_then(Value::as_str).unwrap_or("Couldn’t check it.").to_string()) }
    })
    .await
    .map_err(err)?
}

/// Whether the open model is downloaded here (else the passages alone).
#[tauri::command]
pub fn claims_model(app: AppHandle) -> bool {
    ai::model_installed(&app)
}
