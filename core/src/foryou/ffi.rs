//! New for You through UniFFI, for the Android app: `NewForYou` wraps [`ForYou`] with the real
//! network ([`NetFinder`]) and, when given one, the on-device model (`AiEngine`). Built with the
//! `llm` feature (the Android and desktop builds), so it stays out of the Apple apps' bindings:
//! they have their own New for You on the same files.
//!
//! Times are milliseconds since 1970; `tz_min` is the local offset from UTC in minutes
//! (TimeZone.getDefault().getOffset(now) / 60000 on Android).

use super::run::{NetFinder, Writer};
use super::view::{FeedView, SettingsView};
use super::ForYou;
use crate::llm::AiEngine;
use crate::model::{CoreError, PaperMeta, Result};
use crate::Library;
use std::sync::Arc;

/// What a run is doing ("Looking for new papers…", "Reading abstracts (3 of 40)…").
#[uniffi::export(with_foreign)]
pub trait ForYouListener: Send + Sync {
    fn on_stage(&self, stage: String);
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct ForYouRunReport {
    /// false when a run was already going on
    pub ran: bool,
    /// papers added to the feed
    pub found: u32,
    pub candidates: u32,
    pub stats: String,
}

/// The morning notification: "New for You · 6 papers" and a sentence.
#[derive(uniffi::Record, Clone, Debug)]
pub struct ForYouNotice {
    pub title: String,
    pub body: String,
}

#[derive(uniffi::Object)]
pub struct NewForYou {
    inner: ForYou,
}

#[uniffi::export]
impl NewForYou {
    /// `library_dir`: the library folder (foryou.json and watches.json are shared through it);
    /// `local_dir`: the app's own folder (what only this device keeps).
    #[uniffi::constructor]
    pub fn new(library: Arc<Library>, library_dir: String, local_dir: String) -> Arc<Self> {
        Arc::new(NewForYou { inner: ForYou::new(library, library_dir, local_dir) })
    }

    pub fn feed(&self, now_ms: i64, tz_min: i32) -> FeedView {
        self.inner.feed(now_ms, tz_min)
    }

    pub fn settings(&self) -> SettingsView {
        self.inner.settings()
    }

    pub fn new_count(&self) -> u32 {
        self.inner.new_count()
    }

    pub fn is_running(&self) -> bool {
        self.inner.running().0
    }

    pub fn stage(&self) -> Option<String> {
        self.inner.running().1
    }

    pub fn mark_opened(&self, now_ms: i64) -> Result<()> {
        self.inner.mark_opened(now_ms)
    }

    /// True when the paragraph changed: run now.
    pub fn set_settings(&self, interests: String, per_day: u32, morning: u32, preprints: String) -> Result<bool> {
        self.inner.set_settings(&interests, per_day, morning, &preprints)
    }

    /// `kind`: "journal", "author", "citing" (a DOI) or "topic". False if already followed.
    pub fn add_watch(&self, kind: String, value: String, label: String, now_ms: i64) -> Result<bool> {
        self.inner.add_watch(&kind, &value, &label, now_ms)
    }

    pub fn remove_watch(&self, id: String) -> Result<()> {
        self.inner.remove_watch(&id)
    }

    /// Follows a co-author (their OpenAlex id, CoauthorView.author).
    pub fn follow(&self, author: String, now_ms: i64) -> Result<bool> {
        self.inner.follow(&author, now_ms)
    }

    /// Looks you up on OpenAlex by your ORCID iD (blocking: network).
    pub fn connect_orcid(&self, input: String, now_ms: i64) -> Result<()> {
        self.inner.connect_orcid(&NetFinder, &input, now_ms).map_err(CoreError::Invalid)
    }

    pub fn disconnect_orcid(&self) -> Result<()> {
        self.inner.disconnect_orcid()
    }

    pub fn dismiss(&self, key: String) -> Result<()> {
        self.inner.dismiss(&key)
    }

    pub fn boost(&self, key: String) -> Result<bool> {
        self.inner.boost(&key)
    }

    pub fn added(&self, key: String, id: String) -> Result<()> {
        self.inner.added(&key, &id)
    }

    /// The details of a find, to add it (the DOI's lookup falls back on them).
    pub fn find_meta(&self, key: String) -> Option<PaperMeta> {
        self.inner.find_meta(&key)
    }

    /// `phone`: leave the first two hours after the morning time to a computer.
    pub fn is_due(&self, now_ms: i64, tz_min: i32, phone: bool) -> bool {
        self.inner.is_due(now_ms, tz_min, phone)
    }

    /// Makes the feed now (blocking: a minute or more; many with a model screening). With
    /// `engine` the model writes the searches, screens the abstracts and summarizes.
    pub fn run(&self, now_ms: i64, tz_min: i32, engine: Option<Arc<AiEngine>>, listener: Option<Arc<dyn ForYouListener>>) -> Result<ForYouRunReport> {
        let writer: Option<&dyn Writer> = engine.as_deref().map(|e| e as &dyn Writer);
        let say = |s: &str| {
            if let Some(l) = &listener {
                l.on_stage(s.to_string());
            }
        };
        Ok(match self.inner.run(&NetFinder, writer, now_ms, tz_min, &say)? {
            Some(r) => ForYouRunReport { ran: true, found: r.found, candidates: r.candidates, stats: r.stats },
            None => ForYouRunReport { ran: false, found: 0, candidates: 0, stats: String::new() },
        })
    }

    pub fn stop(&self) {
        self.inner.stop()
    }

    pub fn needs_summaries(&self, tz_min: i32) -> bool {
        self.inner.needs_summaries(tz_min)
    }

    /// Summaries for the papers without one, then the digest (blocking). How many were written.
    pub fn summarize(&self, engine: Arc<AiEngine>, tz_min: i32) -> u32 {
        self.inner.summarize(engine.as_ref(), tz_min)
    }

    /// A paragraph drafted from your papers and library (blocking).
    pub fn draft_interests(&self, engine: Arc<AiEngine>) -> Option<String> {
        self.inner.draft_interests(engine.as_ref())
    }

    pub fn morning_notice(&self, now_ms: i64, tz_min: i32) -> Option<ForYouNotice> {
        self.inner.morning_notice(now_ms, tz_min).map(|(title, body)| ForYouNotice { title, body })
    }

    pub fn mark_notified(&self) -> Result<()> {
        self.inner.mark_notified()
    }
}
