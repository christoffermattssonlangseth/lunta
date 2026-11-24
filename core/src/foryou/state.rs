//! What New for You keeps, in the library folder so it syncs: `watches.json` (the journals,
//! authors, papers and topics you follow) and `foryou.json` (your paragraph, the feed and your
//! feedback). The same files and the same JSON as the Apple apps (apple/Shared/Watches.swift:
//! `Watch`, `ForYouState`, `FeedRecord`, `FeedbackSnip`, `OrcidLink`, …), so every device
//! reads and writes one New for You. Swift's synthesized Codable needs every non-optional key,
//! so all of them are always written; an optional that is nil is left out, as Swift does; keys
//! this version doesn't know (written by a newer app) are kept.
//!
//! Like the Apple apps, a change re-reads the file, changes it and writes it back whole (another
//! device may have written since); the file itself is last-writer-wins (iCloud Drive, a synced
//! folder, or the Drive syncer's rule for shared root files).
//!
//! What only this device needs (when you last opened the feed, the last notification) is in
//! `local.json` in the app's own folder, as the Apple apps keep it in UserDefaults.

use super::time::{self, iso};
use crate::model::{CoreError, Result};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const STATE_FILE: &str = "foryou.json";
pub const WATCHES_FILE: &str = "watches.json";
pub const LOCAL_FILE: &str = "foryou-local.json";

/// What a watch follows (Watch.Kind).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum WatchType {
    Topic,
    Author,
    Citing,
    Journal,
}

impl WatchType {
    pub fn core(self) -> crate::metadata::WatchKind {
        use crate::metadata::WatchKind as K;
        match self {
            WatchType::Topic => K::Topic,
            WatchType::Author => K::Author,
            WatchType::Citing => K::Citing,
            WatchType::Journal => K::Journal,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            WatchType::Topic => "topic",
            WatchType::Author => "author",
            WatchType::Citing => "citing",
            WatchType::Journal => "journal",
        }
    }

    pub fn from_name(s: &str) -> Option<WatchType> {
        match s.to_ascii_lowercase().as_str() {
            "topic" => Some(WatchType::Topic),
            "author" => Some(WatchType::Author),
            "citing" => Some(WatchType::Citing),
            "journal" => Some(WatchType::Journal),
            _ => None,
        }
    }
}

/// A journal, author, paper or topic you follow (Swift's `Watch`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Watch {
    #[serde(default = "new_id")]
    pub id: String,
    pub kind: WatchType,
    /// words, an author's name (or ORCID iD / OpenAlex id), a DOI, or a journal's name
    pub value: String,
    /// shown in the app
    #[serde(default)]
    pub label: String,
    #[serde(with = "iso", default)]
    pub created: i64,
    #[serde(with = "iso::opt", default, skip_serializing_if = "Option::is_none")]
    pub last_checked: Option<i64>,
    /// (older versions) keys already shown
    #[serde(default)]
    pub seen: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}

impl Watch {
    pub fn new(kind: WatchType, value: &str, label: &str, now: i64) -> Watch {
        Watch {
            id: new_id(),
            kind,
            value: value.to_string(),
            label: label.to_string(),
            created: now / 1000 * 1000,
            last_checked: None,
            seen: vec![],
            extra: Map::new(),
        }
    }

    /// "Papers by Jane Doe" (Watch.describe).
    pub fn describe(&self) -> String {
        match self.kind {
            WatchType::Topic => format!("Papers on “{}”", self.label),
            WatchType::Author => format!("Papers by {}", self.label),
            WatchType::Citing => format!("Papers citing “{}”", self.label),
            WatchType::Journal => format!("New in {}", self.label),
        }
    }
}

/// A paper in the feed (Swift's `FeedRecord`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeedRecord {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub journal: String,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub r#abstract: String,
    #[serde(default)]
    pub open_access: bool,
    #[serde(default)]
    pub cited_by: i64,
    /// why it was picked (one sentence)
    #[serde(default)]
    pub reason: String,
    /// where it was found ("search “…”", "New paper by …, whom you follow.", "bioRxiv", …)
    #[serde(default)]
    pub source: String,
    #[serde(default, deserialize_with = "finite")]
    pub score: f64,
    #[serde(with = "iso", default)]
    pub found: i64,
    /// the library's id once added
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_library: Option<String>,
    /// thumbs up: more like this
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boosted: Option<bool>,
    /// one sentence: what the paper does and finds
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn finite<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<f64, D::Error> {
    Ok(Value::deserialize(d)?.as_f64().filter(|x| x.is_finite()).unwrap_or(0.0))
}

/// A short piece of a paper you added or turned down (Swift's `FeedbackSnip`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FeedbackSnip {
    pub key: String,
    pub text: String,
}

/// One of your own papers, from your ORCID iD (Swift's `OwnPaper`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct OwnPaper {
    pub title: String,
    pub authors: Vec<String>,
    pub date: String,
    pub journal: String,
    pub doi: String,
    pub r#abstract: String,
    pub cited_by: i64,
}

impl OwnPaper {
    pub fn key(&self) -> String {
        if self.doi.is_empty() {
            super::text::title_key(&self.title)
        } else {
            super::text::doi_key(&self.doi)
        }
    }
}

/// Someone you publish with (Swift's `CoauthorRecord`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct CoauthorRecord {
    pub name: String,
    /// OpenAlex author id ("A…")
    pub author: String,
    /// "" when OpenAlex has none
    pub orcid: String,
    pub shared: i64,
}

impl CoauthorRecord {
    /// What an author watch follows: the ORCID iD, else the OpenAlex id (exact, unlike a name).
    pub fn watch_value(&self) -> String {
        if self.orcid.is_empty() {
            self.author.clone()
        } else {
            self.orcid.clone()
        }
    }
}

/// You, by your ORCID iD, found on OpenAlex (Swift's `OrcidLink`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OrcidLink {
    pub orcid: String,
    #[serde(default)]
    pub name: String,
    /// OpenAlex author id
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub works: i64,
    /// the latest, then the most cited
    #[serde(default)]
    pub papers: Vec<OwnPaper>,
    #[serde(default)]
    pub coauthors: Vec<CoauthorRecord>,
    #[serde(with = "iso", default)]
    pub updated: i64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// How many of the day's papers may be preprints (ForYouState.Preprints).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Preprints {
    #[default]
    Any,
    Few,
    None,
}

impl Preprints {
    pub fn cap(self, per_day: i64) -> i64 {
        match self {
            Preprints::Any => per_day,
            Preprints::Few => (per_day / 4).max(1),
            Preprints::None => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Preprints::Any => "Journals and preprints",
            Preprints::Few => "Mostly journals",
            Preprints::None => "Journals only",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Preprints::Any => "any",
            Preprints::Few => "few",
            Preprints::None => "none",
        }
    }

    pub fn from_name(s: &str) -> Preprints {
        match s {
            "few" => Preprints::Few,
            "none" => Preprints::None,
            _ => Preprints::Any,
        }
    }
}

/// foryou.json (Swift's `ForYouState`). Read as leniently as Swift's own decoder: a missing or
/// unreadable value is its default.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ForYouState {
    pub interests: String,
    pub queries: Vec<String>,
    pub keywords: Vec<String>,
    /// the paragraph the queries were written for
    pub queries_for: String,
    #[serde(deserialize_with = "lenient_int_8")]
    pub per_day: i64,
    #[serde(deserialize_with = "lenient_items")]
    pub items: Vec<FeedRecord>,
    /// keys already offered (never offered twice)
    pub shown: Vec<String>,
    pub liked: Vec<FeedbackSnip>,
    pub disliked: Vec<FeedbackSnip>,
    /// thumbs up: counts more than an add
    pub boosted: Vec<FeedbackSnip>,
    #[serde(with = "iso::opt", skip_serializing_if = "Option::is_none")]
    pub last_run: Option<i64>,
    /// a few sentences on the latest papers
    pub digest: String,
    /// the feed (its paper keys) the digest describes
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_opt_string")]
    pub digest_for: Option<String>,
    /// minutes after midnight (local time): when the day's papers are picked
    #[serde(deserialize_with = "lenient_int_morning")]
    pub morning: i64,
    #[serde(deserialize_with = "lenient_preprints")]
    pub preprints: Preprints,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_orcid")]
    pub orcid: Option<OrcidLink>,
    /// your OpenAlex API key (free), so Lunta has its own daily allowance on OpenAlex
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_opt_string")]
    pub openalex_key: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ForYouState {
    fn default() -> Self {
        ForYouState {
            interests: String::new(),
            queries: vec![],
            keywords: vec![],
            queries_for: String::new(),
            per_day: 8,
            items: vec![],
            shown: vec![],
            liked: vec![],
            disliked: vec![],
            boosted: vec![],
            last_run: None,
            digest: String::new(),
            digest_for: None,
            morning: 7 * 60,
            preprints: Preprints::Any,
            orcid: None,
            openalex_key: None,
            extra: Map::new(),
        }
    }
}

fn lenient_int_8<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<i64, D::Error> {
    Ok(Value::deserialize(d)?.as_i64().unwrap_or(8))
}
fn lenient_int_morning<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<i64, D::Error> {
    Ok(Value::deserialize(d)?.as_i64().unwrap_or(7 * 60))
}
fn lenient_preprints<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Preprints, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).unwrap_or_default())
}
fn lenient_opt_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
    Ok(Value::deserialize(d)?.as_str().map(str::to_string))
}
fn lenient_orcid<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<OrcidLink>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).ok())
}
/// A paper that can't be read is left out (Swift would drop the whole file).
fn lenient_items<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<FeedRecord>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(v.as_array().into_iter().flatten().filter_map(|x| serde_json::from_value(x.clone()).ok()).collect())
}

impl ForYouState {
    /// The latest time the day's papers were due: this morning, or yesterday's before then.
    pub fn last_morning(&self, now: i64, tz_min: i32) -> i64 {
        let today = time::start_of_day(now, tz_min) + self.morning.clamp(0, 24 * 60 - 1) * 60_000;
        if today <= now {
            today
        } else {
            today - time::DAY_MS
        }
    }

    pub fn next_morning(&self, now: i64, tz_min: i32) -> i64 {
        self.last_morning(now, tz_min) + time::DAY_MS
    }

    /// Something to look for: a paragraph, a watch or your ORCID iD.
    pub fn has_setup(&self, watches: &[Watch]) -> bool {
        !self.interests.is_empty() || !watches.is_empty() || self.orcid.is_some()
    }

    /// The start of the day of the latest run: papers found that day are the feed, earlier ones
    /// go under "Earlier" (WatchCenter.feedDay).
    pub fn feed_day(&self, tz_min: i32) -> Option<i64> {
        self.last_run.map(|r| time::start_of_day(r, tz_min))
    }

    pub fn latest(&self, tz_min: i32) -> Vec<&FeedRecord> {
        match self.feed_day(tz_min) {
            Some(d) => self.items.iter().filter(|r| r.found >= d).collect(),
            None => self.items.iter().collect(),
        }
    }

    pub fn earlier(&self, tz_min: i32) -> Vec<&FeedRecord> {
        match self.feed_day(tz_min) {
            Some(d) => self.items.iter().filter(|r| r.found < d).collect(),
            None => vec![],
        }
    }

    /// The papers the digest covers: the day's papers (best first, at most 15).
    pub fn digest_batch(&self, tz_min: i32) -> Vec<FeedRecord> {
        self.latest(tz_min).into_iter().take(15).cloned().collect()
    }

    pub fn digest_key(&self, tz_min: i32) -> String {
        format!("v2|{}", self.digest_batch(tz_min).iter().map(|r| r.key.as_str()).collect::<Vec<_>>().join("|"))
    }
}

/// What only this device keeps (UserDefaults "forYouOpened" / "forYouNotified" on Apple).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct LocalState {
    /// when the feed was last looked at here (ms)
    pub opened: i64,
    /// the run the last notification was for (ms)
    pub notified: i64,
    /// what went wrong in this device's latest run, "" if nothing
    pub problem: String,
    /// nothing could be fetched (no feed was made)
    pub failed: bool,
    /// when (ms)
    pub problem_at: i64,
    /// what went wrong in this device's latest journal highlights run, "" if nothing
    pub highlights_problem: String,
    /// what went wrong in this device's latest Across science run, "" if nothing
    pub across_problem: String,
}

impl LocalState {
    /// The problem to show with this feed: this device's latest run's, unless a run since (here or
    /// on another device) made the feed.
    pub fn problem_for(&self, s: &ForYouState) -> Option<(&str, bool)> {
        (!self.problem.is_empty() && self.problem_at >= s.last_run.unwrap_or(0)).then_some((self.problem.as_str(), self.failed))
    }
}

// ---- files ----

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes whole, through a temporary file (not named *.json, so no syncer picks it up half-written).
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| CoreError::Io("no folder".into()))?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        CoreError::Io(e.to_string())
    })
}

/// The two shared files in a library folder, and this device's own file.
#[derive(Clone, Debug)]
pub struct Files {
    pub library: PathBuf,
    pub local: PathBuf,
}

impl Files {
    pub fn new(library: impl Into<PathBuf>, local_dir: impl Into<PathBuf>) -> Files {
        Files { library: library.into(), local: local_dir.into() }
    }

    pub fn state_path(&self) -> PathBuf {
        self.library.join(STATE_FILE)
    }

    pub fn watches_path(&self) -> PathBuf {
        self.library.join(WATCHES_FILE)
    }

    /// foryou.json as it is now (the defaults if there is none, or it can't be read).
    pub fn state(&self) -> ForYouState {
        read_json(&self.state_path()).unwrap_or_default()
    }

    pub fn save_state(&self, s: &ForYouState) -> Result<()> {
        let data = serde_json::to_vec_pretty(s).map_err(|e| CoreError::Invalid(e.to_string()))?;
        write_atomic(&self.state_path(), &data)
    }

    /// Re-reads the file, changes it and writes it back (another device may have written since).
    pub fn change<T>(&self, f: impl FnOnce(&mut ForYouState) -> T) -> Result<T> {
        let mut s = self.state();
        let out = f(&mut s);
        self.save_state(&s)?;
        Ok(out)
    }

    pub fn watches(&self) -> Vec<Watch> {
        let Ok(text) = std::fs::read_to_string(self.watches_path()) else { return vec![] };
        let Ok(Value::Array(list)) = serde_json::from_str::<Value>(&text) else { return vec![] };
        list.into_iter().filter_map(|w| serde_json::from_value(w).ok()).collect()
    }

    pub fn save_watches(&self, w: &[Watch]) -> Result<()> {
        let data = serde_json::to_vec_pretty(w).map_err(|e| CoreError::Invalid(e.to_string()))?;
        write_atomic(&self.watches_path(), &data)
    }

    pub fn local(&self) -> LocalState {
        read_json(&self.local.join(LOCAL_FILE)).unwrap_or_default()
    }

    pub fn save_local(&self, l: &LocalState) -> Result<()> {
        let data = serde_json::to_vec_pretty(l).map_err(|e| CoreError::Invalid(e.to_string()))?;
        write_atomic(&self.local.join(LOCAL_FILE), &data)
    }
}
