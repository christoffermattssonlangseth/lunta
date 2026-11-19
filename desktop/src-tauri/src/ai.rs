//! On-device AI (the core's `llm`): the open model Qwen3 4B, downloaded on demand into the app's
//! local data folder, writing the reader's Summary card and answering Ask, as Apple's on-device
//! model does in the Mac app (apple/Shared/Summary.swift, Assistant.swift).
//!
//! Events to the frontend (src/ai.ts):
//! - `ai-download` {done, total, finished, error}
//! - `ai-summary` {paper, state: loading | writing | done | none, text, note}
//! - `ai-ask` {request, text, finished, error}

use lunta_core::llm::{self, ask::AskSource, models, prompt::ChatTurn, summary::PaperSummary, AiEngine, AiError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppState, R};

#[derive(Default)]
pub struct Ai {
    engine: Mutex<Option<Arc<AiEngine>>>,
    /// loading the model (one at a time)
    loading: Mutex<()>,
    download: Mutex<Option<Arc<AtomicBool>>>,
    /// the summary being written: (paper, stop flag)
    summary: Mutex<Option<(String, Arc<AtomicBool>)>>,
    /// the Ask answer being written: (request, stop flag)
    asking: Mutex<Option<(u64, Arc<AtomicBool>)>>,
}

/// This device's choices (ai.json in the app's config folder).
#[derive(Serialize, Deserialize, Clone)]
struct Prefs {
    /// Settings › "Show summaries in the reader"
    summaries: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { summaries: true }
    }
}

fn prefs_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("ai.json"))
}

fn prefs(app: &AppHandle) -> Prefs {
    prefs_file(app).and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_prefs(app: &AppHandle, p: &Prefs) -> R<()> {
    let f = prefs_file(app).ok_or("no config folder")?;
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(f, serde_json::to_vec_pretty(p).unwrap_or_default()).map_err(|e| e.to_string())
}

/// Where the model goes: the local (not roaming) app data folder.
fn model_dir(app: &AppHandle) -> R<PathBuf> {
    app.path().app_local_data_dir().map(|d| d.join("models")).map_err(|e| e.to_string())
}

fn model() -> models::AiModel {
    models::desktop_model()
}

#[derive(Serialize)]
pub struct Status {
    name: String,
    size: u64,
    size_label: String,
    installed: bool,
    /// bytes of an unfinished download
    partial: u64,
    downloading: bool,
    summaries: bool,
    /// why this computer can't run it
    unsupported: Option<String>,
    folder: String,
}

#[tauri::command]
pub fn ai_status(app: AppHandle, ai: State<Ai>) -> R<Status> {
    let m = model();
    let dir = model_dir(&app)?;
    Ok(Status {
        name: m.name.clone(),
        size: m.size,
        size_label: models::size_label(m.size),
        installed: llm::download::installed(&m, &dir).is_some(),
        partial: llm::download::partial_bytes(&m, &dir),
        downloading: ai.download.lock().unwrap().is_some(),
        summaries: prefs(&app).summaries,
        unsupported: llm::engine::unsupported_reason(),
        folder: dir.to_string_lossy().to_string(),
    })
}

#[derive(Serialize, Clone)]
struct DownloadEvent {
    done: u64,
    total: u64,
    finished: bool,
    error: Option<String>,
}

/// Starts (or resumes) the download; progress comes as `ai-download` events.
#[tauri::command]
pub fn ai_download(app: AppHandle, ai: State<Ai>) -> R<()> {
    let dir = model_dir(&app)?;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut d = ai.download.lock().unwrap();
        if d.is_some() {
            return Ok(());
        }
        *d = Some(stop.clone());
    }
    std::thread::spawn(move || {
        let m = model();
        let mut last = Instant::now() - Duration::from_secs(1);
        let r = llm::download::download(&m, &dir, &llm::download::HttpFetch, &mut |done, total| {
            if last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                let _ = app.emit("ai-download", DownloadEvent { done, total, finished: false, error: None });
            }
            !stop.load(Ordering::SeqCst)
        });
        *app.state::<Ai>().download.lock().unwrap() = None;
        let error = match r {
            Ok(_) => None,
            Err(AiError::Cancelled) => Some(String::new()),
            Err(e) => Some(e.to_string()),
        };
        let done = llm::download::partial_bytes(&m, &dir).max(if error.is_none() { m.size } else { 0 });
        let _ = app.emit("ai-download", DownloadEvent { done, total: m.size, finished: true, error });
    });
    Ok(())
}

#[tauri::command]
pub fn ai_cancel_download(ai: State<Ai>) {
    if let Some(stop) = ai.download.lock().unwrap().as_ref() {
        stop.store(true, Ordering::SeqCst);
    }
}

/// Deletes the model (and an unfinished download); the memory it held is freed.
#[tauri::command]
pub fn ai_delete_model(app: AppHandle, ai: State<Ai>) -> R<()> {
    ai_cancel_download(ai.clone());
    stop_all(&ai);
    *ai.engine.lock().unwrap() = None;
    llm::download::remove(&model(), &model_dir(&app)?).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ai_set_summaries(app: AppHandle, on: bool) -> R<()> {
    let mut p = prefs(&app);
    p.summaries = on;
    save_prefs(&app, &p)
}

fn stop_all(ai: &Ai) {
    if let Some((_, s)) = ai.summary.lock().unwrap().as_ref() {
        s.store(true, Ordering::SeqCst);
    }
    if let Some((_, s)) = ai.asking.lock().unwrap().as_ref() {
        s.store(true, Ordering::SeqCst);
    }
}

/// Whether the model is downloaded (New for You uses it when it is).
pub(crate) fn model_installed(app: &AppHandle) -> bool {
    model_dir(app).is_ok_and(|d| llm::download::installed(&model(), &d).is_some()) && llm::engine::unsupported_reason().is_none()
}

/// The loaded model, loading it first if needed (seconds). None if it isn't downloaded.
pub(crate) fn engine(app: &AppHandle) -> Result<Option<Arc<AiEngine>>, String> {
    let ai = app.state::<Ai>();
    if let Some(e) = ai.engine.lock().unwrap().clone() {
        return Ok(Some(e));
    }
    let _one = ai.loading.lock().unwrap();
    if let Some(e) = ai.engine.lock().unwrap().clone() {
        return Ok(Some(e));
    }
    let m = model();
    let Some(path) = llm::download::installed(&m, &model_dir(app)?) else { return Ok(None) };
    let e = Arc::new(AiEngine::load_model(&path, &m.id, m.context, None).map_err(|e| e.to_string())?);
    *ai.engine.lock().unwrap() = Some(e.clone());
    Ok(Some(e))
}

#[derive(Serialize, Clone)]
struct SummaryEvent {
    paper: String,
    state: &'static str,
    text: String,
    note: String,
}

/// The Summary card for a paper: the one kept with it if it was made from this `source`
/// (luntaSummarySource's text), else a new one written now (unless `force`, the saved one is
/// used). Answers with `ai-summary` events; "none" means no card.
#[tauri::command]
pub async fn ai_summary(app: AppHandle, state: State<'_, AppState>, paper: String, title: String, source: String, force: bool) -> R<()> {
    let lib = state.lib()?;
    let ai = app.state::<Ai>();
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut cur = ai.summary.lock().unwrap();
        if let Some((p, s)) = cur.as_ref() {
            if *p == paper && !force {
                return Ok(()); // already at it
            }
            s.store(true, Ordering::SeqCst);
        }
        *cur = Some((paper.clone(), stop.clone()));
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let say = |state: &'static str, text: String, note: String| {
            let _ = app2.emit("ai-summary", SummaryEvent { paper: paper.clone(), state, text, note });
        };
        let hash = llm::prompt::source_hash(&source);
        let enough = source.split_whitespace().count() >= 80;
        let saved = lib.summary(paper.clone()).filter(|s| s.source == hash && !s.text.is_empty());
        let finish = || {
            let ai = app2.state::<Ai>();
            let mut cur = ai.summary.lock().unwrap();
            if cur.as_ref().is_some_and(|(_, s)| Arc::ptr_eq(s, &stop)) {
                *cur = None;
            }
        };
        if !enough {
            say("none", String::new(), String::new());
            return finish();
        }
        if let (false, Some(s)) = (force, &saved) {
            say("done", s.text.clone(), llm::ai_summary_note(s.model.clone()));
            return finish();
        }
        if !prefs(&app2).summaries || llm::download::installed(&model(), &model_dir(&app2).unwrap_or_default()).is_none() {
            // no model (a summary never starts a download): keep the old card on Regenerate
            if !force {
                say("none", String::new(), String::new());
            }
            return finish();
        }
        say("loading", "Summarizing…".into(), String::new());
        let e = match engine(&app2) {
            Ok(Some(e)) => e,
            Ok(None) | Err(_) => {
                say("none", String::new(), String::new());
                return finish();
            }
        };
        let started = Instant::now();
        let mut last = Instant::now() - Duration::from_secs(1);
        let r = e.write_summary(&title, &source, &mut |t| {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            // at most a few updates a second (each lays the page out again)
            if last.elapsed() >= Duration::from_millis(200) {
                last = Instant::now();
                say("writing", t.to_string(), String::new());
            }
            true
        });
        if stop.load(Ordering::SeqCst) {
            return finish();
        }
        match r {
            Ok(Some(text)) => {
                let s = PaperSummary { text: text.clone(), model: e.model_id(), created: lunta_core::model::now_ms(), source: hash };
                let _ = lib.save_summary(paper.clone(), s);
                say("done", text, llm::ai_summary_note(e.model_id()));
                eprintln!("Lunta: summary of {paper} in {:.1} s", started.elapsed().as_secs_f64());
            }
            Ok(None) | Err(_) => say("none", String::new(), String::new()),
        }
        finish();
    });
    Ok(())
}

/// The reader closed (or another paper opened): stop writing its summary.
#[tauri::command]
pub fn ai_stop_summary(ai: State<Ai>, paper: String) {
    let mut cur = ai.summary.lock().unwrap();
    if let Some((p, s)) = cur.as_ref() {
        if *p == paper {
            s.store(true, Ordering::SeqCst);
            *cur = None;
        }
    }
}

#[derive(Serialize, Clone)]
struct AskEvent {
    request: u64,
    text: String,
    finished: bool,
    error: Option<String>,
}

#[derive(Serialize)]
pub struct AskStarted {
    sources: Vec<SourceDto>,
}

#[derive(Serialize)]
pub struct SourceDto {
    n: u32,
    text: String,
    paper_id: String,
}

impl From<AskSource> for SourceDto {
    fn from(s: AskSource) -> Self {
        SourceDto { n: s.n, text: s.text, paper_id: s.paper_id }
    }
}

#[derive(Deserialize)]
pub struct Turn {
    role: String,
    text: String,
}

/// Asks about a paper (`paper`) or the library (none). Returns what was looked up (the sources the
/// answer cites); the answer comes as `ai-ask` events for `request`.
#[tauri::command]
pub async fn ai_ask(app: AppHandle, state: State<'_, AppState>, request: u64, paper: Option<String>, question: String, history: Vec<Turn>) -> R<AskStarted> {
    if llm::download::installed(&model(), &model_dir(&app)?).is_none() {
        return Err("Download the model first (Settings ▾ › On-Device AI…).".into());
    }
    let lib = state.lib()?;
    let history: Vec<ChatTurn> = history.into_iter().map(|t| ChatTurn { role: t.role, text: t.text }).collect();
    let prompt = match &paper {
        Some(id) => lib.ask_paper_prompt(id.clone(), question, history, None).map_err(|e| e.to_string())?,
        None => lib.ask_library_prompt(question, history),
    };
    let sources = prompt.sources.iter().cloned().map(SourceDto::from).collect();
    let ai = app.state::<Ai>();
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut cur = ai.asking.lock().unwrap();
        if let Some((_, s)) = cur.as_ref() {
            s.store(true, Ordering::SeqCst);
        }
        *cur = Some((request, stop.clone()));
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let say = |text: String, finished: bool, error: Option<String>| {
            let _ = app2.emit("ai-ask", AskEvent { request, text, finished, error });
        };
        let r = engine(&app2).and_then(|e| e.ok_or_else(|| "The model isn't downloaded.".to_string())).and_then(|e| {
            let mut last = Instant::now() - Duration::from_secs(1);
            e.write_answer(&prompt, &mut |t| {
                if stop.load(Ordering::SeqCst) {
                    return false;
                }
                if last.elapsed() >= Duration::from_millis(120) {
                    last = Instant::now();
                    say(t.to_string(), false, None);
                }
                true
            })
            .map_err(|e| e.to_string())
        });
        match r {
            Ok(text) => say(text, true, None),
            Err(e) => say(String::new(), true, Some(e)),
        }
        let ai = app2.state::<Ai>();
        let mut cur = ai.asking.lock().unwrap();
        if cur.as_ref().is_some_and(|(r, _)| *r == request) {
            *cur = None;
        }
    });
    Ok(AskStarted { sources })
}

#[tauri::command]
pub fn ai_stop_ask(ai: State<Ai>) {
    if let Some((_, s)) = ai.asking.lock().unwrap().as_ref() {
        s.store(true, Ordering::SeqCst);
    }
}
