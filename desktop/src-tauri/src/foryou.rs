//! New for You (the core's `foryou`): the daily feed of new papers for your research, as in the
//! Mac app (apple/Shared/Watches.swift, DiscoverViews.swift). Its state is the library's
//! foryou.json and watches.json, shared with the Mac, iPhone and Android; what only this computer
//! keeps (when the feed was last opened) is in the app's local data folder, one per library.
//!
//! The frontend (src/foryou.ts) asks every ten minutes (`foryou_tick`); the day's papers are
//! picked at the morning time (or as soon as Lunta runs after it), in the background. With the
//! on-device model downloaded (Settings ▾ › On-Device AI…) it writes the searches, reads the best
//! abstracts and summarizes, as Apple's model does on the Mac; without it the ranking is trusted
//! and rows show the abstract's first sentences.
//!
//! Events: `foryou` {running, stage, finished, found, error}.

use lunta_core::foryou::run::{NetFinder, Writer};
use lunta_core::foryou::view::{FeedView, SettingsView};
use lunta_core::foryou::ForYou;
use lunta_core::model::PaperMeta;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppState, R};

#[derive(Default)]
pub struct Feed {
    /// the library it is for, and the handle
    open: Mutex<Option<(PathBuf, Arc<ForYou>)>>,
    /// the digest key summaries were last tried for (once per feed and session: a paper the
    /// model can't summarize isn't asked again every ten minutes)
    summarized: Mutex<Option<String>>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The handle for the open library (made when the library changes).
fn handle(app: &AppHandle) -> R<Arc<ForYou>> {
    let state = app.state::<AppState>();
    let (lib, root) = (state.lib()?, state.root()?);
    let feed = app.state::<Feed>();
    let mut open = feed.open.lock().unwrap();
    if let Some((r, f)) = open.as_ref() {
        if *r == root {
            return Ok(f.clone());
        }
    }
    let local = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("foryou").join(folder_name(&root));
    let f = Arc::new(ForYou::new(lib, root.clone(), local));
    *open = Some((root, f.clone()));
    Ok(f)
}

/// A stable folder name for a library path (FNV-1a, as the search index's cache folder).
fn folder_name(root: &Path) -> String {
    let mut h: u64 = 1469598103934665603;
    for b in root.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

#[derive(Serialize, Clone)]
struct Event {
    running: bool,
    stage: Option<String>,
    finished: bool,
    found: u32,
    error: Option<String>,
}

#[derive(Serialize)]
pub struct Feedish {
    #[serde(flatten)]
    feed: FeedView,
    running: bool,
    stage: Option<String>,
    /// the on-device model is downloaded (summaries, screening, drafting)
    ai: bool,
}

#[tauri::command]
pub async fn foryou_feed(app: AppHandle, tz: i32) -> R<Feedish> {
    let f = handle(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (running, stage) = f.running();
        Feedish { feed: f.feed(now(), tz), running, stage, ai: crate::ai::model_installed(&app) }
    })
    .await
    .map_err(|e| e.to_string())
}

/// The badge alone (cheap: two small files).
#[tauri::command]
pub async fn foryou_count(app: AppHandle) -> R<u32> {
    Ok(handle(&app)?.new_count())
}

#[derive(Serialize)]
pub struct Settingsish {
    #[serde(flatten)]
    settings: SettingsView,
    ai: bool,
}

#[tauri::command]
pub async fn foryou_settings(app: AppHandle) -> R<Settingsish> {
    let f = handle(&app)?;
    Ok(Settingsish { settings: f.settings(), ai: crate::ai::model_installed(&app) })
}

/// Saves the settings; a changed paragraph starts a run.
#[tauri::command]
pub async fn foryou_save_settings(app: AppHandle, interests: String, per_day: u32, morning: u32, preprints: String, tz: i32) -> R<()> {
    let f = handle(&app)?;
    if f.set_settings(&interests, per_day, morning, &preprints).map_err(|e| e.to_string())? {
        start_run(&app, tz);
    }
    Ok(())
}

/// Follows a journal (its name as OpenAlex has it), author, paper (a DOI) or topic.
#[tauri::command]
pub async fn foryou_add_watch(app: AppHandle, kind: String, value: String, tz: i32) -> R<()> {
    let f = handle(&app)?;
    let v = value.trim().to_string();
    let added = tauri::async_runtime::spawn_blocking(move || -> R<bool> {
        let label = if kind == "journal" {
            match lunta_core::journal_name(v.clone()).map_err(|e| e.to_string())? {
                Some(name) => name,
                None => return Err(format!("No journal called “{v}” found.")),
            }
        } else if kind == "citing" {
            // the paper's title when it is in the library or can be looked up
            let doi = lunta_core::extract_doi(v.clone());
            let doi = if doi.is_empty() { v.clone() } else { doi };
            lunta_core::resolve_metadata(doi.clone()).map(|m| m.title.chars().take(70).collect()).unwrap_or(doi)
        } else {
            v.clone()
        };
        let value = if kind == "journal" { label.clone() } else { v.clone() };
        f.add_watch(&kind, &value, &label, now()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    if added {
        start_run(&app, tz);
    }
    Ok(())
}

#[tauri::command]
pub async fn foryou_remove_watch(app: AppHandle, id: String) -> R<()> {
    handle(&app)?.remove_watch(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn foryou_follow(app: AppHandle, author: String, tz: i32) -> R<()> {
    if handle(&app)?.follow(&author, now()).map_err(|e| e.to_string())? {
        start_run(&app, tz);
    }
    Ok(())
}

/// Looks you up on OpenAlex by your ORCID iD; the problem to show, if any.
#[tauri::command]
pub async fn foryou_connect_orcid(app: AppHandle, orcid: String) -> R<()> {
    let f = handle(&app)?;
    tauri::async_runtime::spawn_blocking(move || f.connect_orcid(&NetFinder, &orcid, now())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn foryou_disconnect_orcid(app: AppHandle) -> R<()> {
    handle(&app)?.disconnect_orcid().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn foryou_dismiss(app: AppHandle, key: String) -> R<()> {
    handle(&app)?.dismiss(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn foryou_boost(app: AppHandle, key: String) -> R<bool> {
    handle(&app)?.boost(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn foryou_mark_opened(app: AppHandle) -> R<()> {
    handle(&app)?.mark_opened(now()).map_err(|e| e.to_string())
}

/// Adds a paper from the feed: by its DOI (metadata and reading view as Add Paper does), else
/// with the details the feed has; then it counts as a paper you wanted. The library's id.
#[tauri::command]
pub async fn foryou_add(app: AppHandle, state: State<'_, AppState>, key: String, must_read: bool) -> R<String> {
    let f = handle(&app)?;
    let lib = state.lib()?;
    let log = crate::logger(&app);
    tauri::async_runtime::spawn_blocking(move || -> R<String> {
        let meta: PaperMeta = f.find_meta(&key).ok_or("That paper is no longer in New for You.")?;
        let existing = (!meta.doi.is_empty()).then(|| lib.find_by_doi(meta.doi.clone())).flatten();
        let id = match existing {
            Some(p) => p.id,
            None => {
                let by_doi = if meta.doi.is_empty() { None } else { crate::add::add(&lib, &meta.doi, &log).ok().map(|a| a.paper.id) };
                match by_doi {
                    Some(id) => id,
                    None => lib.add_paper(meta, None, None, None).map_err(|e| e.to_string())?.id,
                }
            }
        };
        if must_read {
            lib.set_must_read(id.clone(), true).map_err(|e| e.to_string())?;
        }
        f.added(&key, &id).map_err(|e| e.to_string())?;
        Ok(id)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Drafts the paragraph from your papers and library with the on-device model (seconds).
#[tauri::command]
pub async fn foryou_draft(app: AppHandle) -> R<Option<String>> {
    let f = handle(&app)?;
    tauri::async_runtime::spawn_blocking(move || -> R<Option<String>> {
        let Some(e) = crate::ai::engine(&app)? else { return Err("Download the on-device model first (Settings ▾ › On-Device AI…).".into()) };
        Ok(f.draft_interests(e.as_ref()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Looks for new papers now.
#[tauri::command]
pub async fn foryou_run(app: AppHandle, tz: i32) -> R<()> {
    handle(&app)?;
    start_run(&app, tz);
    Ok(())
}

#[tauri::command]
pub async fn foryou_stop(app: AppHandle) -> R<()> {
    handle(&app)?.stop();
    Ok(())
}

/// Every ten minutes and at start: the day's run when it is due; otherwise summaries for a feed
/// another device made without them (once per feed). The notice for a new feed, once (the
/// frontend shows it in the status bar: there is no system notification here).
#[tauri::command]
pub async fn foryou_tick(app: AppHandle, tz: i32) -> R<Option<(String, String)>> {
    let f = handle(&app)?;
    if f.running().0 {
        return Ok(None);
    }
    if f.is_due(now(), tz, false) {
        start_run(&app, tz);
        return Ok(None);
    }
    let notice = f.morning_notice(now(), tz);
    if notice.is_some() {
        f.mark_notified().map_err(|e| e.to_string())?;
    }
    if crate::ai::model_installed(&app) && f.needs_summaries(tz) {
        let key = f.state().digest_key(tz);
        let feed = app.state::<Feed>();
        let mut last = feed.summarized.lock().unwrap();
        if last.as_deref() != Some(key.as_str()) {
            *last = Some(key);
            let app2 = app.clone();
            std::thread::spawn(move || {
                if let Ok(Some(e)) = crate::ai::engine(&app2) {
                    if f.summarize(e.as_ref(), tz) > 0 {
                        let _ = app2.emit("foryou", Event { running: false, stage: None, finished: true, found: 0, error: None });
                    }
                }
            });
        }
    }
    Ok(notice)
}

/// Runs in the background (one at a time), telling the frontend what it is doing.
fn start_run(app: &AppHandle, tz: i32) {
    let Ok(f) = handle(app) else { return };
    if f.running().0 {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let say = |stage: &str| {
            let _ = app.emit("foryou", Event { running: true, stage: Some(stage.to_string()), finished: false, found: 0, error: None });
        };
        say("Looking for new papers…");
        // the model, if it is downloaded (loading it takes a few seconds)
        let engine = if crate::ai::model_installed(&app) { crate::ai::engine(&app).ok().flatten() } else { None };
        let writer: Option<&dyn Writer> = engine.as_deref().map(|e| e as &dyn Writer);
        let r = f.run(&NetFinder, writer, now(), tz, &say);
        if writer.is_some() {
            *app.state::<Feed>().summarized.lock().unwrap() = Some(f.state().digest_key(tz));
        }
        let (found, error) = match r {
            Ok(Some(r)) => (r.found, None),
            Ok(None) => return, // another run is going on
            Err(e) => (0, Some(e.to_string())),
        };
        let _ = app.emit("foryou", Event { running: false, stage: None, finished: true, found, error });
    });
}
