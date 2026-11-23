//! New for You for the desktop and Android apps: a daily, ranked feed of new papers for your
//! research, as in the Apple apps (apple/Shared/Watches.swift, whose logic this ports).
//!
//! You describe your interests in a paragraph (the on-device model can draft it from your
//! library), optionally connect your ORCID iD (looked up on OpenAlex, no sign-in) and follow
//! journals, authors, papers or topics. Once a day, at your morning time, the new papers on
//! OpenAlex, bioRxiv and medRxiv are gathered, ranked against your paragraph, your library and
//! your feedback, and (with the on-device model) screened, summarized and digested.
//!
//! The state is shared with the Apple apps through the library folder: `foryou.json` and
//! `watches.json`, same files, same JSON ([`state`]). Whichever device runs first after the
//! morning time makes the day's feed; the others show it.
//!
//! [`ForYou`] is the handle the apps use (directly from the desktop app's Rust; through UniFFI as
//! `NewForYou` on Android, see `ffi.rs`).

pub mod journals;
pub mod rank;
pub mod run;
pub mod state;
pub mod text;
pub mod time;
pub mod view;

#[cfg(feature = "llm")]
mod ffi;
#[cfg(feature = "llm")]
pub use ffi::{ForYouListener, ForYouNotice, ForYouRunReport, NewForYou};

#[cfg(test)]
mod tests;

use crate::model::{PaperMeta, Result};
use crate::Library;
use run::{Finder, RunReport, Writer};
use state::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// New for You on one library.
pub struct ForYou {
    lib: Arc<Library>,
    files: Files,
    /// one change of the files at a time (from this process)
    lock: Mutex<()>,
    running: AtomicBool,
    stage: Mutex<Option<String>>,
    stop: AtomicBool,
}

impl ForYou {
    /// `library_dir`: the library folder (where foryou.json and watches.json are);
    /// `local_dir`: this app's own folder (what only this device keeps).
    pub fn new(lib: Arc<Library>, library_dir: impl Into<PathBuf>, local_dir: impl Into<PathBuf>) -> ForYou {
        let f = ForYou { lib, files: Files::new(library_dir, local_dir), lock: Mutex::new(()), running: AtomicBool::new(false), stage: Mutex::new(None), stop: AtomicBool::new(false) };
        f.use_openalex_key();
        f
    }

    /// Every request to OpenAlex (here and in the rest of the app) carries your key, if you gave one.
    fn use_openalex_key(&self) {
        crate::metadata::set_openalex_key(self.state().openalex_key.as_deref().unwrap_or(""));
    }

    /// Your OpenAlex API key ("" for none), kept with New for You's settings (so every device has it).
    pub fn set_openalex_key(&self, key: &str) -> Result<()> {
        let k = key.trim().to_string();
        self.change(|s| s.openalex_key = (!k.is_empty()).then(|| k.clone()))?;
        self.use_openalex_key();
        Ok(())
    }

    pub fn files(&self) -> &Files {
        &self.files
    }

    pub fn state(&self) -> ForYouState {
        self.files.state()
    }

    pub fn watches(&self) -> Vec<Watch> {
        self.files.watches()
    }

    fn change<T>(&self, f: impl FnOnce(&mut ForYouState) -> T) -> Result<T> {
        let _g = self.lock.lock().unwrap();
        self.files.change(f)
    }

    // ---- what the apps show ----

    pub fn feed(&self, now: i64, tz_min: i32) -> view::FeedView {
        view::feed_view(&self.state(), &self.watches(), &self.files.local(), now, tz_min)
    }

    pub fn settings(&self) -> view::SettingsView {
        view::settings_view(&self.state(), &self.watches())
    }

    /// The badge: papers found since the feed was last opened here, not added.
    pub fn new_count(&self) -> u32 {
        view::new_count(&self.state(), &self.files.local())
    }

    /// A run is going on, and what it is doing ("Reading abstracts (3 of 40)…").
    pub fn running(&self) -> (bool, Option<String>) {
        (self.running.load(Ordering::SeqCst), self.stage.lock().unwrap().clone())
    }

    /// The feed was looked at (the badge counts from here).
    pub fn mark_opened(&self, now: i64) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut l = self.files.local();
        l.opened = l.opened.max(now);
        self.files.save_local(&l)
    }

    /// The paper of a find, to add it to the library (the DOI path with these details to fall
    /// back on).
    pub fn find_meta(&self, key: &str) -> Option<PaperMeta> {
        self.state().items.iter().find(|r| r.key == key).map(|r| text::record_hit(r).meta)
    }

    // ---- settings ----

    /// Saves the settings sheet. True when the paragraph changed (and isn't empty): a run is
    /// due now.
    pub fn set_settings(&self, interests: &str, per_day: u32, morning: u32, preprints: &str) -> Result<bool> {
        let t = interests.trim().to_string();
        self.change(|s| {
            let changed = t != s.interests;
            s.interests = t.clone();
            s.per_day = per_day.clamp(1, 50) as i64;
            s.morning = morning.min(24 * 60 - 1) as i64;
            s.preprints = Preprints::from_name(preprints);
            changed && !t.is_empty()
        })
    }

    /// Follows a journal, author, paper (citing it) or topic. False if already followed.
    pub fn add_watch(&self, kind: &str, value: &str, label: &str, now: i64) -> Result<bool> {
        let kind = WatchType::from_name(kind).ok_or_else(|| crate::model::CoreError::Invalid(format!("no such watch: {kind}")))?;
        let value = value.trim();
        if value.is_empty() {
            return Ok(false);
        }
        let _g = self.lock.lock().unwrap();
        let mut list = self.files.watches();
        if list.iter().any(|w| w.kind == kind && w.value.to_lowercase() == value.to_lowercase()) {
            return Ok(false);
        }
        let label = if label.trim().is_empty() { value } else { label.trim() };
        list.push(Watch::new(kind, value, label, now));
        self.files.save_watches(&list)?;
        Ok(true)
    }

    pub fn remove_watch(&self, id: &str) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut list = self.files.watches();
        list.retain(|w| w.id != id);
        self.files.save_watches(&list)
    }

    /// Follows someone you publish with (by their OpenAlex id, from the settings).
    pub fn follow(&self, author: &str, now: i64) -> Result<bool> {
        let Some(c) = self.state().orcid.and_then(|o| o.coauthors.into_iter().find(|c| c.author == author)) else { return Ok(false) };
        if view::follows(&self.watches(), &c) {
            return Ok(false);
        }
        self.add_watch("author", &c.watch_value(), &c.name, now)
    }

    /// Looks you up on OpenAlex by your ORCID iD (blocking). The problem to show, if any.
    pub fn connect_orcid(&self, finder: &dyn Finder, input: &str, now: i64) -> std::result::Result<(), String> {
        let _g = self.lock.lock().unwrap();
        run::connect_orcid(&self.files, finder, input, now)
    }

    pub fn disconnect_orcid(&self) -> Result<()> {
        self.change(|s| s.orcid = None)
    }

    // ---- feedback ----

    pub fn dismiss(&self, key: &str) -> Result<()> {
        self.change(|s| run::dismiss(s, key))
    }

    /// More like this (again: undo). Whether it is on now.
    pub fn boost(&self, key: &str) -> Result<bool> {
        self.change(|s| run::boost(s, key))
    }

    /// A paper from the feed was added to the library as `id`.
    pub fn added(&self, key: &str, id: &str) -> Result<()> {
        self.change(|s| run::added(s, key, id))
    }

    // ---- the daily run ----

    pub fn is_due(&self, now: i64, tz_min: i32, phone: bool) -> bool {
        run::is_due(&self.state(), &self.watches(), now, tz_min, phone)
    }

    /// Makes the day's feed now (blocking: a minute or more, many minutes when the model screens).
    /// None if a run is already going on.
    pub fn run(&self, finder: &dyn Finder, writer: Option<&dyn Writer>, now: i64, tz_min: i32, progress: &(dyn Fn(&str) + Sync)) -> Result<Option<RunReport>> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(None);
        }
        self.stop.store(false, Ordering::SeqCst);
        let say = |s: &str| {
            *self.stage.lock().unwrap() = Some(s.to_string());
            progress(s);
        };
        self.use_openalex_key();
        let r = run::Run { lib: &self.lib, files: &self.files, finder, writer, now, tz_min, progress: &say, stop: Some(&self.stop) };
        let out = run::run(&r);
        *self.stage.lock().unwrap() = None;
        self.running.store(false, Ordering::SeqCst);
        out.map(Some)
    }

    /// Stops a run or summaries between papers.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// One sentence for each paper without one, then the digest (blocking).
    pub fn summarize(&self, writer: &dyn Writer, tz_min: i32) -> u32 {
        run::summarize(&self.files, writer, tz_min, Some(&self.stop))
    }

    /// Whether some papers in the feed have no summary yet (or the day's digest is missing).
    pub fn needs_summaries(&self, tz_min: i32) -> bool {
        let s = self.state();
        s.items.iter().take(30).any(|r| r.summary.is_none() && !r.r#abstract.is_empty())
            || (!s.items.is_empty() && !s.digest_batch(tz_min).is_empty() && s.digest_for.as_deref() != Some(s.digest_key(tz_min).as_str()))
    }

    pub fn draft_interests(&self, writer: &dyn Writer) -> Option<String> {
        run::draft_interests(&self.lib, &self.files, writer)
    }

    /// The notification to show for the latest feed (title, body), once per feed on this device.
    pub fn morning_notice(&self, now: i64, tz_min: i32) -> Option<(String, String)> {
        run::morning_notice(&self.state(), &self.files.local(), now, tz_min)
    }

    /// The notification for the latest feed was shown.
    pub fn mark_notified(&self) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut l = self.files.local();
        l.notified = self.state().last_run.unwrap_or(0).max(l.notified);
        self.files.save_local(&l)
    }
}

#[cfg(feature = "llm")]
impl Writer for crate::llm::AiEngine {
    fn answer(&self, instructions: &str, prompt: &str, max_tokens: u32) -> Option<String> {
        self.plain_answer(instructions, prompt, max_tokens).ok().filter(|a| !a.trim().is_empty())
    }
}
