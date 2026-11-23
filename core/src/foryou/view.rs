//! What the apps show: the feed and the settings, ready to draw. Plain records (serde for the
//! desktop app's frontend; UniFFI records for Android when the `llm` feature, which the Android
//! build has, is on — kept out of the Apple build, whose app has its own New for You).

use super::state::*;
use super::text;
use super::time;
use serde::Serialize;

/// A paper in the feed.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "llm", derive(uniffi::Record))]
pub struct FindView {
    pub key: String,
    pub title: String,
    pub authors: Vec<String>,
    /// "Jane Doe et al. · Nature"
    pub byline: String,
    pub journal: String,
    pub date: String,
    /// "Published 6 Nov (7 days ago)"
    pub published: Option<String>,
    pub doi: String,
    /// doi.org link, else the paper's own
    pub link: String,
    pub abstract_text: String,
    pub open_access: bool,
    pub cited_by: u32,
    /// why it was picked
    pub reason: String,
    pub source: String,
    /// the model's one sentence, if one was written
    pub summary: Option<String>,
    /// what the row shows: the summary, else the abstract's first sentences
    pub preview: String,
    pub boosted: bool,
    /// the library's id once added
    pub in_library: Option<String>,
    pub found: i64,
    /// found after the feed was last opened on this device
    pub is_new: bool,
}

/// A piece of the digest: text, or a link to a paper in the feed.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "llm", derive(uniffi::Record))]
pub struct DigestPart {
    pub text: String,
    /// the paper's key when it is a link
    pub key: Option<String>,
}

/// The feed.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "llm", derive(uniffi::Record))]
pub struct FeedView {
    /// there is a paragraph (else the intro: "Describe Your Research…")
    pub has_interests: bool,
    /// something to look for: a paragraph, a watch or an ORCID iD
    pub has_setup: bool,
    pub interests: String,
    /// the latest run's papers
    pub latest: Vec<FindView>,
    /// earlier papers ("Earlier (12)")
    pub earlier: Vec<FindView>,
    /// "In short": the day's papers in a few sentences, with links to them (empty if none yet)
    pub digest: Vec<DigestPart>,
    /// the digest and its papers as text to send someone
    pub digest_share: String,
    pub last_run: Option<i64>,
    pub next_morning: i64,
    /// papers not added and found after the feed was last opened here: the sidebar's badge
    pub new_count: u32,
    /// what went wrong in this device's latest run ("OpenAlex's free daily allowance … is used up"),
    /// shown instead of "Nothing new today" with Retry
    pub problem: Option<String>,
    /// nothing could be fetched: "Couldn't reach OpenAlex" (else some sources failed)
    pub fetch_failed: bool,
}

/// A watch, as the settings list it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "llm", derive(uniffi::Record))]
pub struct WatchView {
    pub id: String,
    /// "topic", "author", "citing", "journal"
    pub kind: String,
    pub value: String,
    pub label: String,
    /// "Papers by Jane Doe"
    pub describe: String,
}

/// Someone you publish with.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "llm", derive(uniffi::Record))]
pub struct CoauthorView {
    pub name: String,
    /// OpenAlex author id: what Follow takes
    pub author: String,
    pub orcid: String,
    pub shared: u32,
    pub following: bool,
}

/// Your ORCID iD, connected.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "llm", derive(uniffi::Record))]
pub struct OrcidView {
    pub orcid: String,
    pub name: String,
    pub works: u32,
    pub coauthors: Vec<CoauthorView>,
}

/// The settings sheet.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "llm", derive(uniffi::Record))]
pub struct SettingsView {
    pub interests: String,
    pub per_day: u32,
    /// minutes after midnight
    pub morning: u32,
    /// "any", "few", "none"
    pub preprints: String,
    pub orcid: Option<OrcidView>,
    pub watches: Vec<WatchView>,
    /// your OpenAlex API key, "" for none
    pub openalex_key: String,
}

pub fn find_view(r: &FeedRecord, opened: i64, now: i64, tz_min: i32) -> FindView {
    let preview = match &r.summary {
        Some(s) => s.clone(),
        None => text::first_sentences(&r.r#abstract, 280),
    };
    FindView {
        key: r.key.clone(),
        title: r.title.clone(),
        authors: r.authors.clone(),
        byline: text::byline(r),
        journal: r.journal.clone(),
        date: r.date.clone(),
        published: time::published_label(&r.date, now, tz_min),
        doi: r.doi.clone(),
        link: if r.doi.is_empty() { r.url.clone() } else { format!("https://doi.org/{}", r.doi) },
        abstract_text: r.r#abstract.clone(),
        open_access: r.open_access,
        cited_by: r.cited_by.max(0) as u32,
        reason: r.reason.clone(),
        source: r.source.clone(),
        summary: r.summary.clone(),
        preview,
        boosted: r.boosted == Some(true),
        in_library: r.in_library.clone(),
        found: r.found,
        is_new: r.in_library.is_none() && r.found > opened,
    }
}

pub fn new_count(s: &ForYouState, local: &LocalState) -> u32 {
    s.items.iter().filter(|r| r.in_library.is_none() && r.found > local.opened).count() as u32
}

pub fn feed_view(s: &ForYouState, watches: &[Watch], local: &LocalState, now: i64, tz_min: i32) -> FeedView {
    let batch = s.digest_batch(tz_min);
    let digest = if s.digest.is_empty() || s.interests.is_empty() || batch.is_empty() {
        vec![]
    } else {
        text::digest_pieces(&s.digest, &batch).into_iter().map(|p| DigestPart { text: p.text, key: p.key }).collect()
    };
    FeedView {
        has_interests: !s.interests.is_empty(),
        has_setup: s.has_setup(watches),
        interests: s.interests.clone(),
        latest: s.latest(tz_min).into_iter().map(|r| find_view(r, local.opened, now, tz_min)).collect(),
        earlier: s.earlier(tz_min).into_iter().map(|r| find_view(r, local.opened, now, tz_min)).collect(),
        digest_share: if digest.is_empty() { String::new() } else { text::share_text(&s.digest, &batch) },
        digest,
        last_run: s.last_run,
        next_morning: s.next_morning(now, tz_min),
        new_count: new_count(s, local),
        problem: local.problem_for(s).map(|p| p.0.to_string()),
        fetch_failed: local.problem_for(s).is_some_and(|p| p.1),
    }
}

/// Whether you already follow this person (by ORCID iD, OpenAlex id or name).
pub fn follows(watches: &[Watch], c: &CoauthorRecord) -> bool {
    watches.iter().any(|w| {
        w.kind == WatchType::Author && [&c.orcid, &c.author, &c.name].iter().any(|v| !v.is_empty() && w.value.eq_ignore_ascii_case(v))
    })
}

pub fn settings_view(s: &ForYouState, watches: &[Watch]) -> SettingsView {
    SettingsView {
        interests: s.interests.clone(),
        per_day: s.per_day.clamp(0, 1000) as u32,
        morning: s.morning.clamp(0, 24 * 60 - 1) as u32,
        preprints: s.preprints.name().to_string(),
        orcid: s.orcid.as_ref().map(|o| OrcidView {
            orcid: o.orcid.clone(),
            name: o.name.clone(),
            works: o.works.max(0) as u32,
            coauthors: o
                .coauthors
                .iter()
                .map(|c| CoauthorView {
                    name: c.name.clone(),
                    author: c.author.clone(),
                    orcid: c.orcid.clone(),
                    shared: c.shared.max(0) as u32,
                    following: follows(watches, c),
                })
                .collect(),
        }),
        watches: watches
            .iter()
            .map(|w| WatchView { id: w.id.clone(), kind: w.kind.name().into(), value: w.value.clone(), label: w.label.clone(), describe: w.describe() })
            .collect(),
        openalex_key: s.openalex_key.clone().unwrap_or_default(),
    }
}
