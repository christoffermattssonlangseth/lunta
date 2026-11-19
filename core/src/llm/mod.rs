//! On-device AI for the apps beyond Apple's (Windows, Linux, Android): an open-weights model
//! (Qwen3, GGUF) run locally with llama.cpp. Feature "llm"; not in the Apple build, whose apps use
//! Apple's on-device model or MLX.
//!
//! - `models`: which model each platform downloads (pinned revision, size, SHA-256)
//! - `download`: resumable, checksum-verified download into the app's data folder
//! - `prompt`: the Summary card's prompt and cleaning (the Apple apps' rules), Qwen3's chat template
//! - `ask`: questions about a paper (its best-matching passages) and about the library (a search)
//! - `summary`: summary.json in the paper's folder, the same file the Apple apps write
//! - `engine`: llama.cpp (llama-cpp-2): load once, stream a generation
//!
//! The apps use it through UniFFI (Android) or directly (the Tauri desktop app): the functions
//! and `AiEngine` below.

pub mod ask;
pub mod download;
pub mod engine;
pub mod models;
pub mod prompt;
pub mod summary;
#[cfg(test)]
mod tests;

use crate::model::Result as CoreResult;
use crate::Library;
use ask::{AskPrompt, LibraryFacts};
use models::AiModel;
use prompt::ChatTurn;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use summary::PaperSummary;

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum AiError {
    #[error("network error: {0}")]
    Network(String),
    #[error("{0}")]
    Checksum(String),
    #[error("cancelled")]
    Cancelled,
    #[error("file error: {0}")]
    Io(String),
    #[error("{0}")]
    Model(String),
    #[error("{0}")]
    TooLong(String),
    #[error("{0}")]
    Unsupported(String),
}

/// Download progress; return false to stop (the part downloaded is kept for next time).
#[uniffi::export(with_foreign)]
pub trait AiDownloadListener: Send + Sync {
    fn on_progress(&self, done: u64, total: u64) -> bool;
}

/// The text written so far, after every token; return false to stop the model there.
#[uniffi::export(with_foreign)]
pub trait AiTextListener: Send + Sync {
    fn on_text(&self, text: String) -> bool;
}

/// Qwen3 4B, for Windows and Linux.
#[uniffi::export]
pub fn ai_desktop_model() -> AiModel {
    models::desktop_model()
}

/// Qwen3 1.7B, for Android.
#[uniffi::export]
pub fn ai_phone_model() -> AiModel {
    models::phone_model()
}

/// The model's file in `dir`, if it has been downloaded (and checked).
#[uniffi::export]
pub fn ai_model_file(model: AiModel, dir: String) -> Option<String> {
    download::installed(&model, Path::new(&dir)).map(|p| p.to_string_lossy().to_string())
}

/// Bytes of an unfinished download of the model (to say "Resume download").
#[uniffi::export]
pub fn ai_partial_bytes(model: AiModel, dir: String) -> u64 {
    download::partial_bytes(&model, Path::new(&dir))
}

/// Downloads the model into `dir` (resuming an unfinished download), checks its SHA-256 and
/// returns its path. Blocks: call it off the main thread.
#[uniffi::export]
pub fn ai_download_model(model: AiModel, dir: String, listener: Arc<dyn AiDownloadListener>) -> Result<String, AiError> {
    download::download(&model, Path::new(&dir), &download::HttpFetch, &mut |d, t| listener.on_progress(d, t))
        .map(|p| p.to_string_lossy().to_string())
}

/// Deletes the model and any unfinished download of it.
#[uniffi::export]
pub fn ai_delete_model(model: AiModel, dir: String) -> Result<(), AiError> {
    download::remove(&model, Path::new(&dir))
}

/// Why this device can't run the model (None if it can).
#[uniffi::export]
pub fn ai_unsupported_reason() -> Option<String> {
    engine::unsupported_reason()
}

/// The key a summary is kept under: luntaSummarySource's text, hashed as the Apple apps do.
#[uniffi::export]
pub fn ai_source_hash(source: String) -> String {
    prompt::source_hash(&source)
}

/// Under a summary: who wrote it (summaries can be wrong); as SummaryModel.note in the Apple apps.
#[uniffi::export]
pub fn ai_summary_note(model_id: String) -> String {
    let who = match model_id.as_str() {
        "apple-on-device" => "Apple Intelligence".to_string(),
        "qwen3-4b" => "the open model (Qwen3 4B)".to_string(),
        "qwen3-1.7b" => "the open model (Qwen3 1.7B)".to_string(),
        other => format!("an open model ({other})"),
    };
    format!("Made on device by {who}. It can be wrong.")
}

/// A loaded model, ready to write. Load it off the main thread (a few seconds); keep it while
/// the feature is in use (it takes about the model's size in memory).
#[derive(uniffi::Object)]
pub struct AiEngine {
    engine: engine::Engine,
    model_id: String,
    context: u32,
    /// cancel() bumps it; a generation stops when it changes under it
    generation: AtomicU64,
}

impl AiEngine {
    pub fn load_model(path: &Path, model_id: &str, context: u32, threads: Option<i32>) -> Result<AiEngine, AiError> {
        Ok(AiEngine { engine: engine::Engine::load(path, threads)?, model_id: model_id.to_string(), context: context.max(2048), generation: AtomicU64::new(0) })
    }

    fn run(&self, prompt: &str, max_tokens: u32, temperature: f32, on_text: &mut dyn FnMut(&str) -> bool) -> Result<String, AiError> {
        let mine = self.generation.load(Ordering::SeqCst);
        let opts = engine::GenOptions { max_tokens, temperature, context: self.context };
        let mut stopped = false;
        let out = self.engine.generate(prompt, &opts, &mut |t| {
            if self.generation.load(Ordering::SeqCst) != mine {
                stopped = true;
                return false;
            }
            on_text(t)
        })?;
        if stopped {
            return Err(AiError::Cancelled);
        }
        Ok(out)
    }

    /// Writes a summary of the paper from luntaSummarySource's text; `on_text` gets the summary
    /// so far, cleaned. None when the answer isn't a summary (a refusal, a fragment).
    pub fn write_summary(&self, title: &str, source: &str, on_text: &mut dyn FnMut(&str) -> bool) -> Result<Option<String>, AiError> {
        let user = prompt::summary_prompt(title, source);
        let mut p = prompt::chatml(prompt::SUMMARY_INSTRUCTIONS, &[ChatTurn { role: "user".into(), text: user.clone() }], false);
        // the excerpts are cut to fit when a paper's text takes more tokens than usual
        let budget = self.context.saturating_sub(prompt::SUMMARY_TOKENS + 16) as usize;
        let n = self.engine.count_tokens(&p);
        if n > budget {
            let keep = (source.split_whitespace().count() * budget / n * 92 / 100).max(250);
            let cut = source.split(' ').take(keep).collect::<Vec<_>>().join(" ");
            p = prompt::chatml(prompt::SUMMARY_INSTRUCTIONS, &[ChatTurn { role: "user".into(), text: prompt::summary_prompt(title, &cut) }], false);
        }
        let mut last = String::new();
        let raw = self.run(&p, prompt::SUMMARY_TOKENS, 0.2, &mut |t| {
            let c = prompt::clean_summary(t);
            if c != last && !c.is_empty() {
                last = c;
                return on_text(&last);
            }
            true
        })?;
        let s = prompt::clean_summary(&raw);
        Ok(prompt::summary_acceptable(&s).then_some(s))
    }

    /// One answer to a plain prompt with these instructions (New for You's searches, screening,
    /// summaries and digest: AskEngine.plainAnswer in the Apple apps), thinking off; the text
    /// without a thinking trace. Prompts longer than the context are cut from the middle.
    pub fn plain_answer(&self, instructions: &str, user: &str, max_tokens: u32) -> Result<String, AiError> {
        let mut text = user.to_string();
        let mut p = prompt::chatml(instructions, &[ChatTurn { role: "user".into(), text: text.clone() }], false);
        while self.engine.count_tokens(&p) as u32 + max_tokens + 16 > self.context && text.chars().count() > 400 {
            let n = text.chars().count();
            let keep = n * 4 / 5;
            // keep the start (the material) and the end (the request)
            let head: String = text.chars().take(keep - keep / 4).collect();
            let tail: String = text.chars().skip(n - keep / 4).collect();
            text = format!("{head}\n…\n{tail}");
            p = prompt::chatml(instructions, &[ChatTurn { role: "user".into(), text: text.clone() }], false);
        }
        let raw = self.run(&p, max_tokens, 0.2, &mut |_| true)?;
        Ok(prompt::without_thinking(&raw).trim().to_string())
    }

    /// Answers a question put together by Library::ask_paper_prompt / ask_library_prompt;
    /// `on_text` gets the answer so far.
    pub fn write_answer(&self, ask: &AskPrompt, on_text: &mut dyn FnMut(&str) -> bool) -> Result<String, AiError> {
        let mut turns = ask.turns.clone();
        let mut p = prompt::chatml(&ask.system, &turns, false);
        // too long: drop earlier turns, then shorten the passages
        while self.engine.count_tokens(&p) as u32 + ask::ANSWER_TOKENS + 16 > self.context {
            if turns.len() > 1 {
                turns.remove(0);
            } else {
                let t = &mut turns[0].text;
                let keep = t.chars().count() * 4 / 5;
                *t = t.chars().take(keep).collect();
                if keep < 400 {
                    break;
                }
            }
            p = prompt::chatml(&ask.system, &turns, false);
        }
        let n = ask.sources.len();
        let raw = self.run(&p, ask::ANSWER_TOKENS, 0.3, &mut |t| on_text(&ask::clean_answer(t, n)))?;
        Ok(ask::clean_answer(&raw, n))
    }
}

#[uniffi::export]
impl AiEngine {
    /// `model_id`: what summaries are marked with (AiModel.id); `threads`: 0 for the default.
    #[uniffi::constructor]
    pub fn load(path: String, model_id: String, context: u32, threads: i32) -> Result<Arc<Self>, AiError> {
        Ok(Arc::new(Self::load_model(Path::new(&path), &model_id, context, (threads > 0).then_some(threads))?))
    }

    pub fn model_id(&self) -> String {
        self.model_id.clone()
    }

    pub fn model_path(&self) -> String {
        self.engine.path().to_string_lossy().to_string()
    }

    /// Summarizes from luntaSummarySource's text. Blocks; `listener` gets the summary so far.
    pub fn summarize(&self, title: String, source: String, listener: Arc<dyn AiTextListener>) -> Result<Option<String>, AiError> {
        self.write_summary(&title, &source, &mut |t| listener.on_text(t.to_string()))
    }

    /// Answers an Ask question. Blocks; `listener` gets the answer so far.
    pub fn answer(&self, prompt: AskPrompt, listener: Arc<dyn AiTextListener>) -> Result<String, AiError> {
        self.write_answer(&prompt, &mut |t| listener.on_text(t.to_string()))
    }

    /// Stops what it is writing (the call returns AiError::Cancelled).
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

#[uniffi::export]
impl Library {
    /// The summary kept with the paper (summary.json), if any.
    pub fn summary(&self, id: String) -> Option<PaperSummary> {
        summary::read(&self.inner.lock().unwrap().store.paper_dir(&id))
    }

    pub fn save_summary(&self, id: String, summary: PaperSummary) -> CoreResult<()> {
        let dir = self.inner.lock().unwrap().store.paper_dir(&id);
        summary::write(&dir, &summary)
    }

    pub fn remove_summary(&self, id: String) -> CoreResult<()> {
        let dir = self.inner.lock().unwrap().store.paper_dir(&id);
        summary::remove(&dir)
    }

    /// What to give the model for a question about one paper: its passages that best match the
    /// question (from `text`, else its fulltext.txt, else just the abstract) and the user's highlights.
    pub fn ask_paper_prompt(&self, id: String, question: String, history: Vec<ChatTurn>, text: Option<String>) -> CoreResult<AskPrompt> {
        let paper = self.paper(id.clone()).ok_or_else(|| crate::model::CoreError::NotFound(id.clone()))?;
        let text = match text.filter(|t| !t.trim().is_empty()) {
            Some(t) => t,
            None => self
                .file_path(id.clone(), crate::model::FileKind::Fulltext)
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default(),
        };
        let marks: Vec<(String, String)> = self.annotations(id).into_iter().map(|a| (a.quote, a.note)).collect();
        Ok(ask::paper_prompt(&paper, &text, &marks, &question, &history))
    }

    /// What to give the model for a question about the library: the papers a search for it finds.
    pub fn ask_library_prompt(&self, question: String, history: Vec<ChatTurn>) -> AskPrompt {
        let papers = self.papers(crate::model::PaperFilter::default(), crate::model::SortKey::Added);
        let facts = LibraryFacts {
            all: papers.len(),
            unread: papers.iter().filter(|p| !p.read).count(),
            starred: papers.iter().filter(|p| p.starred).count(),
            tags: self.tags(),
            collections: self.collections().into_iter().map(|c| c.name).collect(),
        };
        let search = |q: &str, n: u32| -> Vec<(crate::model::Paper, String)> {
            self.search(q.to_string(), n).unwrap_or_default().into_iter().map(|h| (h.paper, h.snippet)).collect()
        };
        // "what did I read last" has no search words: the latest papers then
        let mut found = ask::search_for(&search, &question, 8);
        if found.is_empty() && ask::terms(&question).is_empty() {
            found = papers.into_iter().take(8).map(|p| (p, String::new())).collect();
        }
        ask::library_prompt(&facts, &found, &question, &history)
    }
}
