//! Journal highlights: "From the journals you follow" in New for You. The latest research papers
//! in a few journals you choose (Nature, Science, Cell, …), whatever their topic, once a week
//! (Monday at the morning time) or every day, with a few sentences on the notable ones written by
//! the on-device model, every sentence citing the papers it is about.
//!
//! - **Where from:** Crossref, by the journal's ISSN, the works registered since the period began
//!   (`from-created-date`: Elsevier gives Cell's papers only a month as their date), then Europe
//!   PMC for the abstracts Crossref doesn't have (Elsevier deposits none; Springer Nature some).
//!   Neither charges against a daily allowance, unlike OpenAlex.
//! - **Research papers only:** journal articles, without news and comment (Nature's `d41586`
//!   DOIs), corrections, retractions, editorials, "In this issue", letters and other one- or
//!   two-page pieces, and pieces with only a teaser for an abstract or few authors and none
//!   ([`is_research`]).
//! - **Order:** closest to your paragraph, library and feedback first (the feed's ranking), at
//!   most 30 a journal and 90 in all. The digest takes the first twelve, at most four a journal.
//! - **The digest:** the model gets the titles and abstracts, numbered, and writes 3–6 sentences
//!   ending in the numbers they are about. Each sentence is checked as Summarize Highlights checks
//!   its sentences (`outline::summary::validate`): one without a citation, citing a number not
//!   given, or saying more than the cited titles and abstracts (their words, numbers and
//!   negations) is left out. Without a model, just the list.
//! - **State:** `<library>/journal-highlights.json` (synced like the other root files; its own
//!   file, so an older app writing foryou.json doesn't drop it). The morning time is New for
//!   You's (foryou.json).
//!
//! The Apple apps use it through UniFFI ([`JournalHighlights`]) and write the digest with Apple's
//! model (`digest_request` → the model → `set_digest`); the desktop and Android apps with the
//! core's model (`write_digest`).

use super::rank;
use super::run::{Finder, NetFinder, Writer};
use super::state::{write_atomic, FeedbackSnip, Files, ForYouState, Watch, WatchType};
use super::text;
use super::time::{self, iso};
use crate::metadata::{self, enc};
use crate::model::{CoreError, PaperFilter, PaperMeta, Result, SortKey};
use crate::outline::summary::{self, SummarySource};
use crate::Library;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const FILE: &str = "journal-highlights.json";

/// Journals to suggest, with their ISSNs (print, electronic) as Crossref has them.
pub const SUGGESTED: &[(&str, &[&str])] = &[
    ("Nature", &["0028-0836", "1476-4687"]),
    ("Science", &["0036-8075", "1095-9203"]),
    ("Cell", &["0092-8674"]),
    ("Nature Neuroscience", &["1097-6256", "1546-1726"]),
    ("Neuron", &["0896-6273"]),
    ("Nature Methods", &["1548-7091", "1548-7105"]),
    ("New England Journal of Medicine", &["0028-4793", "1533-4406"]),
    ("PNAS", &["0027-8424", "1091-6490"]),
    ("eLife", &["2050-084X"]),
    ("Nature Biotechnology", &["1087-0156", "1546-1696"]),
    ("Nature Genetics", &["1061-4036", "1546-1718"]),
    ("Nature Medicine", &["1078-8956", "1546-170X"]),
    ("Nature Immunology", &["1529-2908", "1529-2916"]),
    ("Immunity", &["1074-7613"]),
    ("Molecular Cell", &["1097-2765"]),
    ("Cell Stem Cell", &["1934-5909"]),
    ("The Lancet", &["0140-6736", "1474-547X"]),
];

/// The three offered when the section is first turned on.
pub const DEFAULTS: &[&str] = &["Nature", "Science", "Cell"];

/// Other names of the suggested journals (a followed journal as OpenAlex names it).
const ALIASES: &[(&str, &str)] = &[
    ("Proceedings of the National Academy of Sciences", "PNAS"),
    ("Proceedings of the National Academy of Sciences of the United States of America", "PNAS"),
    ("The New England Journal of Medicine", "New England Journal of Medicine"),
    ("NEJM", "New England Journal of Medicine"),
    ("Lancet", "The Lancet"),
];

const PER_JOURNAL: usize = 30;
const MAX_PAPERS: usize = 90;
const DIGEST_PAPERS: usize = 12;
const DIGEST_PER_JOURNAL: usize = 4;

// ---- the file ----

/// A journal in the highlights.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HighlightJournal {
    pub name: String,
    pub issn: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A paper in the highlights.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HighlightRecord {
    pub key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub date: String,
    pub journal: String,
    pub doi: String,
    pub url: String,
    pub r#abstract: String,
    pub score: f64,
    #[serde(with = "iso")]
    pub found: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_library: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A sentence of the digest and the papers it cites (their keys).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct DigestSentence {
    pub text: String,
    pub keys: Vec<String>,
}

/// journal-highlights.json.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HighlightsState {
    pub journals: Vec<HighlightJournal>,
    /// "weekly" (Monday at the morning time) or "daily"
    pub cadence: String,
    #[serde(with = "iso::opt", skip_serializing_if = "Option::is_none")]
    pub last_run: Option<i64>,
    /// the first day the latest papers were looked for from ("2026-11-16")
    pub since: String,
    pub items: Vec<HighlightRecord>,
    /// keys already offered (never offered twice)
    pub shown: Vec<String>,
    pub digest: Vec<DigestSentence>,
    /// the papers (keys) the digest is about
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest_for: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for HighlightsState {
    fn default() -> Self {
        HighlightsState {
            journals: vec![],
            cadence: "weekly".into(),
            last_run: None,
            since: String::new(),
            items: vec![],
            shown: vec![],
            digest: vec![],
            digest_for: None,
            extra: Map::new(),
        }
    }
}

impl HighlightsState {
    pub fn daily(&self) -> bool {
        self.cadence == "daily"
    }

    /// The papers the digest is about: the closest twelve, at most four a journal.
    pub fn digest_batch(&self) -> Vec<&HighlightRecord> {
        let mut per: HashMap<&str, usize> = HashMap::new();
        let mut out = vec![];
        for r in &self.items {
            let n = per.entry(r.journal.as_str()).or_default();
            if *n >= DIGEST_PER_JOURNAL {
                continue;
            }
            *n += 1;
            out.push(r);
            if out.len() >= DIGEST_PAPERS {
                break;
            }
        }
        out
    }

    pub fn digest_key(&self) -> String {
        format!("v1|{}", self.digest_batch().iter().map(|r| r.key.as_str()).collect::<Vec<_>>().join("|"))
    }

    /// The latest time the papers were due: this morning (daily) or Monday morning (weekly).
    pub fn last_due(&self, morning: i64, now: i64, tz_min: i32) -> i64 {
        let fy = ForYouState { morning, ..Default::default() };
        let mut due = fy.last_morning(now, tz_min);
        if !self.daily() {
            // 1970-01-01 was a Thursday: Monday is day 0 of (day + 3) % 7
            let weekday = (time::local_day(due, tz_min) + 3).rem_euclid(7);
            due -= weekday * time::DAY_MS;
        }
        due
    }
}

/// The file in the library folder, read and written whole (re-read before each change).
fn path(files: &Files) -> PathBuf {
    files.library.join(FILE)
}

pub fn read(files: &Files) -> HighlightsState {
    std::fs::read_to_string(path(files)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn change<T>(files: &Files, f: impl FnOnce(&mut HighlightsState) -> T) -> Result<T> {
    let mut s = read(files);
    let out = f(&mut s);
    let data = serde_json::to_vec_pretty(&s).map_err(|e| CoreError::Invalid(e.to_string()))?;
    write_atomic(&path(files), &data)?;
    Ok(out)
}

// ---- journals ----

fn norm(name: &str) -> String {
    let n: String = name.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    n.split_whitespace().filter(|w| *w != "the").collect::<Vec<_>>().join(" ")
}

/// The suggested journal by this name (or another name for it).
pub fn suggested(name: &str) -> Option<(&'static str, &'static [&'static str])> {
    let n = norm(name);
    let alias = ALIASES.iter().find(|(a, _)| norm(a) == n).map(|(_, to)| norm(to));
    let n = alias.unwrap_or(n);
    SUGGESTED.iter().find(|(s, _)| norm(s) == n).copied()
}

/// A journal's ISSNs: the table's, else Crossref's journal by that name (network).
pub fn resolve(finder: &dyn Finder, name: &str) -> std::result::Result<HighlightJournal, String> {
    if let Some((n, issn)) = suggested(name) {
        return Ok(HighlightJournal { name: n.to_string(), issn: issn.iter().map(|s| s.to_string()).collect(), ..Default::default() });
    }
    let j = finder.get_json(&format!("https://api.crossref.org/journals?query={}&rows=10", enc(name.trim()))).map_err(said)?;
    let items = j["message"]["items"].as_array().cloned().unwrap_or_default();
    let title = |x: &Value| x["title"].as_str().unwrap_or("").to_string();
    let best = items.iter().find(|x| norm(&title(x)) == norm(name)).or(items.first());
    match best {
        Some(x) => {
            let issn: Vec<String> = x["ISSN"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
            if issn.is_empty() {
                return Err(format!("Crossref has no ISSN for “{}”.", title(x)));
            }
            Ok(HighlightJournal { name: title(x), issn, ..Default::default() })
        }
        None => Err(format!("No journal called “{}” found.", name.trim())),
    }
}

/// A journal to choose, as the settings list it.
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct JournalChoice {
    pub name: String,
    /// in the highlights
    pub on: bool,
    /// a journal you follow in New for You
    pub followed: bool,
}

/// The chosen journals, then the ones you follow, then the suggestions.
pub fn choices(s: &HighlightsState, watches: &[Watch]) -> Vec<JournalChoice> {
    let mut out: Vec<JournalChoice> = vec![];
    let followed: Vec<&str> = watches.iter().filter(|w| w.kind == WatchType::Journal).map(|w| w.label.as_str()).collect();
    let is_followed = |name: &str| followed.iter().any(|f| norm(f) == norm(name) || suggested(f).is_some_and(|(n, _)| n == name));
    let mut push = |name: &str, on: bool| {
        let canonical = suggested(name).map(|(n, _)| n.to_string()).unwrap_or_else(|| name.to_string());
        if !out.iter().any(|c| norm(&c.name) == norm(&canonical)) {
            out.push(JournalChoice { followed: is_followed(&canonical), name: canonical, on });
        }
    };
    for j in &s.journals {
        push(&j.name, true);
    }
    for f in &followed {
        push(f, false);
    }
    for (n, _) in SUGGESTED {
        push(n, false);
    }
    out
}

// ---- fetching ----

fn said(e: CoreError) -> String {
    match e {
        CoreError::Network(m) => m,
        e => e.to_string(),
    }
}

/// Titles that aren't research papers.
const NOT_RESEARCH: &[&str] = &[
    "author correction", "publisher correction", "correction", "corrigendum", "erratum", "errata", "retraction", "retracted",
    "expression of concern", "editorial", "editor's note", "editors' note", "editor’s note", "editor's choice", "editor’s choice",
    "in this issue", "this week", "in science journals", "in other journals", "news", "research highlight", "daily briefing",
    "books in brief", "obituary", "reply to", "response to", "correspondence", "snapshot", "podcast", "in brief", "highlights from",
    "the week in", "addendum", "clarification", "withdrawal", "announcement", "call for papers", "reviewer acknowledg",
    "briefing chat", "career column", "where i work", "image of the week", "images in clinical medicine", "matters arising",
];

/// DOI prefixes of news and comment.
const NOT_RESEARCH_DOI: &[&str] = &["10.1038/d41586-", "10.1126/science.z"];

/// A page range's length ("628-633" → 6, "5012-5030.e21" → 19); None when unknown.
fn page_span(pages: &str) -> Option<i64> {
    let num = |s: &str| s.trim().split(|c: char| !c.is_ascii_digit()).find(|x| !x.is_empty()).and_then(|x| x.parse::<i64>().ok());
    let (a, b) = pages.split_once(['-', '–'])?;
    let (a, b) = (num(a)?, num(b)?);
    (b >= a).then_some(b - a + 1)
}

/// Before the abstracts are filled in: not a correction, news, editorial, short piece, ….
fn not_research(m: &PaperMeta) -> bool {
    let t = m.title.trim().to_lowercase().replace(['‘', '’'], "'");
    if t.is_empty() || NOT_RESEARCH.iter().any(|x| t.starts_with(&x.replace('’', "'"))) {
        return true;
    }
    let doi = m.doi.to_lowercase();
    if NOT_RESEARCH_DOI.iter().any(|p| doi.starts_with(p)) {
        return true;
    }
    !m.work_type.is_empty() && m.work_type != "journal-article" || page_span(&m.pages).is_some_and(|n| n <= 2)
}

/// A research paper: past [`not_research`], with an abstract (not a teaser), or with none at all
/// but three or more authors (Cell's papers have no abstract on Crossref, and Europe PMC may not
/// have them yet).
pub fn is_research(m: &PaperMeta) -> bool {
    if not_research(m) {
        return false;
    }
    let abs = m.abstract_text.chars().count();
    abs >= 300 || (abs == 0 && m.authors.len() >= 3)
}

/// A journal's works registered since `since` (Crossref), by its first ISSN that answers.
fn journal_works(finder: &dyn Finder, j: &HighlightJournal, since: &str) -> std::result::Result<Vec<PaperMeta>, String> {
    let mut last = format!("No ISSN for {}.", j.name);
    for issn in &j.issn {
        let url = format!(
            "https://api.crossref.org/journals/{issn}/works?filter=from-created-date:{since},type:journal-article&sort=created&order=desc&rows=300&select=DOI,title,subtitle,author,issued,created,container-title,abstract,type,page,URL"
        );
        match finder.get_json(&url) {
            Ok(v) => {
                let items = v["message"]["items"].as_array().cloned().unwrap_or_default();
                return Ok(items
                    .iter()
                    .map(|m| {
                        let mut meta = metadata::crossref_item(m);
                        // the date it went online (Crossref's issued date can be the print issue's month)
                        if let Some(c) = m["created"]["date-time"].as_str() {
                            if meta.date.len() < 10 || c[..10] < *meta.date.as_str() {
                                meta.date = c[..10].to_string();
                            }
                        }
                        meta.journal = j.name.clone();
                        meta
                    })
                    .collect());
            }
            Err(e) => last = said(e),
        }
    }
    Err(last)
}

/// Abstracts from Europe PMC (PubMed and more) for papers without one, by DOI.
fn fill_abstracts(finder: &dyn Finder, papers: &mut [PaperMeta]) -> std::result::Result<(), String> {
    let want: Vec<String> = papers.iter().filter(|p| p.abstract_text.chars().count() < 300 && !p.doi.is_empty()).map(|p| p.doi.to_lowercase()).collect();
    let mut found: HashMap<String, String> = HashMap::new();
    let mut error = None;
    for chunk in want.chunks(25) {
        let q = chunk.iter().map(|d| format!("DOI:\"{d}\"")).collect::<Vec<_>>().join(" OR ");
        let url = format!("https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={}&resultType=core&format=json&pageSize=100", enc(&format!("({q})")).replace('/', "%2F"));
        match finder.get_json(&url) {
            Ok(j) => {
                for r in j["resultList"]["result"].as_array().into_iter().flatten() {
                    let (doi, abs) = (r["doi"].as_str().unwrap_or("").to_lowercase(), r["abstractText"].as_str().unwrap_or(""));
                    if !doi.is_empty() && !abs.is_empty() {
                        found.insert(doi, metadata::strip_tags(abs));
                    }
                }
            }
            Err(e) => error = Some(said(e)),
        }
    }
    for p in papers.iter_mut() {
        if let Some(a) = found.remove(&p.doi.to_lowercase()) {
            if a.chars().count() > p.abstract_text.chars().count() {
                p.abstract_text = a;
            }
        }
    }
    error.map_or(Ok(()), Err)
}

/// What a run did.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct HighlightsReport {
    /// false when a run was already going on, or no journal is chosen
    pub ran: bool,
    /// research papers found
    pub found: u32,
    /// what went wrong fetching, if anything
    pub problem: Option<String>,
    /// nothing could be fetched: the papers are as they were and the run is still due
    pub failed: bool,
}

/// Looks for the period's papers in the chosen journals and puts them, closest first, in place of
/// the last period's.
pub fn run(lib: &Library, files: &Files, finder: &dyn Finder, now: i64, tz_min: i32) -> Result<HighlightsReport> {
    let s = read(files);
    if s.journals.is_empty() {
        return Ok(HighlightsReport::default());
    }
    let days = if s.daily() { 2 } else { 7 };
    let since = time::utc_date(time::start_of_day(now, tz_min) - days * time::DAY_MS + (tz_min as i64).max(0) * 60_000);
    let mut problems = vec![];
    let mut papers: Vec<PaperMeta> = vec![];
    let mut failed = 0;
    for j in &s.journals {
        match journal_works(finder, j, &since) {
            Ok(w) => papers.extend(w.into_iter().filter(|m| !not_research(m))),
            Err(e) => {
                failed += 1;
                problems.push(format!("{}: {e}", j.name));
            }
        }
    }
    if failed == s.journals.len() {
        let why = format!("Couldn't reach Crossref. {}", problems.join(" "));
        note_problem(files, &why)?;
        return Ok(HighlightsReport { ran: true, found: 0, problem: Some(why), failed: true });
    }
    if let Err(e) = fill_abstracts(finder, &mut papers) {
        problems.push(format!("Europe PMC (abstracts): {e}"));
    }
    let shown: HashSet<&str> = s.shown.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    let papers: Vec<PaperMeta> = papers
        .into_iter()
        .filter(|m| is_research(m) && m.title.chars().count() > 10)
        .filter(|m| {
            let key = text::doi_key(&m.doi);
            !key.is_empty() && !shown.contains(key.as_str()) && seen.insert(key)
        })
        .collect();

    // closest to you first (the feed's ranking: your paragraph, library and feedback)
    let fy = files.state();
    let every = lib.papers(PaperFilter::default(), SortKey::Added);
    let mut library: Vec<String> = every.iter().take(400).map(|p| text::snip(&p.meta.title, &p.meta.abstract_text)).collect();
    if let Some(me) = &fy.orcid {
        library.extend(me.papers.iter().map(|p| text::snip(&p.title, &p.r#abstract)));
    }
    let texts: Vec<String> = papers.iter().map(|m| text::snip(&m.title, &m.abstract_text)).collect();
    let snips = |l: &[FeedbackSnip]| l.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
    let scores = rank::score(&texts, &fy.interests, &library, &snips(&fy.liked), &snips(&fy.disliked), &snips(&fy.boosted));
    let mut ranked: Vec<(PaperMeta, f64)> = papers.into_iter().zip(scores).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut per: HashMap<String, usize> = HashMap::new();
    let records: Vec<HighlightRecord> = ranked
        .into_iter()
        .filter(|(m, _)| {
            let n = per.entry(m.journal.clone()).or_default();
            *n += 1;
            *n <= PER_JOURNAL
        })
        .take(MAX_PAPERS)
        .map(|(m, score)| HighlightRecord {
            key: text::doi_key(&m.doi),
            in_library: lib.find_by_doi(m.doi.clone()).map(|p| p.id),
            title: m.title,
            authors: m.authors,
            date: m.date,
            journal: m.journal,
            url: if m.url.is_empty() { format!("https://doi.org/{}", m.doi) } else { m.url },
            doi: m.doi,
            r#abstract: m.abstract_text.chars().take(3000).collect(),
            score,
            found: now / 1000 * 1000,
            extra: Map::new(),
        })
        .collect();
    let found = records.len() as u32;
    change(files, |h| {
        h.shown.extend(records.iter().map(|r| r.key.clone()));
        let excess = h.shown.len().saturating_sub(3000);
        h.shown.drain(..excess);
        h.items = records;
        h.since = since.clone();
        h.last_run = Some(now / 1000 * 1000);
    })?;
    let problem = problems.join(" ");
    note_problem(files, &problem)?;
    Ok(HighlightsReport { ran: true, found, problem: (!problem.is_empty()).then_some(problem), failed: false })
}

/// What went wrong in this device's latest run, kept on this device ("" for nothing).
fn note_problem(files: &Files, problem: &str) -> Result<()> {
    let mut l = files.local();
    if l.highlights_problem == problem {
        return Ok(());
    }
    l.highlights_problem = problem.to_string();
    files.save_local(&l)
}

/// Whether the period's papers are due: the first time, and then at the morning time each Monday
/// (weekly) or each day. A phone leaves the first two hours to a computer.
pub fn is_due(files: &Files, now: i64, tz_min: i32, phone: bool) -> bool {
    let s = read(files);
    if s.journals.is_empty() {
        return false;
    }
    let Some(last) = s.last_run else { return true };
    let due = s.last_due(files.state().morning, now, tz_min);
    last < due && !(phone && now < due + 2 * time::HOUR_MS)
}

// ---- the digest ----

pub const DIGEST_INSTRUCTIONS: &str = "You write short digests of new research papers for a scientist. You use only what the titles and abstracts say, and every sentence cites the papers it is about.";

/// What to ask the model for the digest.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HighlightsDigestRequest {
    pub instructions: String,
    pub prompt: String,
    /// give it back with the answer (the digest is for these papers)
    pub key: String,
}

fn and_list(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        n => format!("{} and {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// The prompt for the digest; None when it is written already, or there are fewer than two papers.
pub fn digest_request(s: &HighlightsState, interests: &str) -> Option<HighlightsDigestRequest> {
    let batch = s.digest_batch();
    if batch.len() < 2 || s.digest_for.as_deref() == Some(s.digest_key().as_str()) {
        return None;
    }
    let journals: Vec<String> = s.journals.iter().map(|j| j.name.clone()).collect();
    let list = batch
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let abs = if r.r#abstract.is_empty() { "(no abstract)".to_string() } else { text::first_sentences(&r.r#abstract, 420) };
            format!("[{}] {} ({})\n    {}", i + 1, r.title, r.journal, abs)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let about = if interests.trim().is_empty() { String::new() } else { format!("The scientist's interests (mention what is closest to them first):\n{}\n\n", interests.trim()) };
    let prompt = format!(
        "{about}New research papers {} in {}:\n{list}\n\n\
         Write 3 to 6 sentences on the most notable of these papers. Each sentence is about one or two papers, \
         says what they did or found in the words of their titles and abstracts, and ends with their numbers in \
         square brackets, e.g. [2] or [3, 7]. No numbers, names or claims that aren't in the titles and abstracts. \
         No heading, no list, no preamble.",
        if s.daily() { "since yesterday" } else { "this week" },
        and_list(&journals)
    );
    Some(HighlightsDigestRequest { instructions: DIGEST_INSTRUCTIONS.into(), prompt, key: s.digest_key() })
}

/// Checks the model's digest (as Summarize Highlights checks its sentences) and keeps what holds
/// up. How many sentences were kept (0 also when the papers have changed since the request).
pub fn set_digest(files: &Files, key: &str, answer: &str) -> Result<u32> {
    change(files, |s| {
        if s.digest_key() != key {
            return 0;
        }
        let batch: Vec<HighlightRecord> = s.digest_batch().into_iter().cloned().collect();
        let sentences = check_digest(answer, &batch);
        let n = sentences.len() as u32;
        s.digest = sentences;
        s.digest_for = Some(key.to_string());
        n
    })
}

/// The answer's sentences that cite papers of the batch and say only what their titles and
/// abstracts say; at most six.
pub fn check_digest(answer: &str, batch: &[HighlightRecord]) -> Vec<DigestSentence> {
    let sources: Vec<SummarySource> = batch
        .iter()
        .enumerate()
        .map(|(i, r)| SummarySource {
            number: i as u32 + 1,
            annotation_id: r.key.clone(),
            paper_id: r.key.clone(),
            quote: format!("{}. {}", r.title, r.r#abstract),
            note: String::new(),
            section: String::new(),
            label: r.journal.clone(),
            cite_key: String::new(),
            reference: String::new(),
            link: String::new(),
            context: String::new(),
        })
        .collect();
    let ids: Vec<String> = batch.iter().map(|r| r.key.clone()).collect();
    let (kept, _drops) = summary::validate(answer, &ids, &sources, None, 6);
    kept.into_iter()
        .map(|s| DigestSentence { text: s.text, keys: s.citations.iter().filter_map(|n| batch.get(*n as usize - 1).map(|r| r.key.clone())).collect() })
        .filter(|s| !s.keys.is_empty())
        .collect()
}

/// The digest by the core's model (desktop and Android). How many sentences were kept.
pub fn write_digest(files: &Files, w: &dyn Writer) -> u32 {
    let s = read(files);
    let Some(req) = digest_request(&s, &files.state().interests) else { return 0 };
    let Some(answer) = w.answer(&req.instructions, &req.prompt, 500) else { return 0 };
    set_digest(files, &req.key, &answer).unwrap_or(0)
}

// ---- feedback ----

/// Not for Me: off the list (and never offered again).
pub fn dismiss(files: &Files, key: &str) -> Result<()> {
    change(files, |s| s.items.retain(|r| r.key != key))
}

/// Added to the library as `id`.
pub fn added(files: &Files, key: &str, id: &str) -> Result<()> {
    change(files, |s| {
        if let Some(r) = s.items.iter_mut().find(|r| r.key == key) {
            r.in_library = Some(id.to_string());
        }
    })
}

// ---- what the apps show ----

/// A paper in the list.
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct HighlightPaper {
    pub key: String,
    /// its number in the list (what the digest cites)
    pub number: u32,
    pub title: String,
    pub authors: Vec<String>,
    /// "Jane Doe et al. · Nature"
    pub byline: String,
    pub journal: String,
    pub date: String,
    /// "Published 18 Nov (5 days ago)"
    pub published: Option<String>,
    pub doi: String,
    /// doi.org link, else the paper's own
    pub link: String,
    pub abstract_text: String,
    /// one line: the abstract's sentence saying what was found ("Here we show …"), else its first
    pub line: String,
    pub in_library: Option<String>,
}

/// A paper a digest sentence cites.
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct HighlightCite {
    pub number: u32,
    pub key: String,
}

/// A sentence of the digest.
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct HighlightSentence {
    pub text: String,
    pub cites: Vec<HighlightCite>,
}

/// "From the journals you follow".
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct HighlightsView {
    /// journals are chosen
    pub on: bool,
    pub journals: Vec<String>,
    /// "weekly" or "daily"
    pub cadence: String,
    /// "Nature, Science and Cell · 16–23 Nov"
    pub subtitle: String,
    pub papers: Vec<HighlightPaper>,
    pub digest: Vec<HighlightSentence>,
    /// the digest as Markdown, each citation a link "[2](foryou://highlight?key=…)"
    pub digest_markdown: String,
    /// the digest and its papers as text to send someone
    pub digest_share: String,
    pub last_run: Option<i64>,
    /// when the next papers are due
    pub next_run: i64,
    /// what went wrong in this device's latest run
    pub problem: Option<String>,
}

/// The link a digest citation makes.
pub fn highlight_link(key: &str) -> String {
    format!("foryou://highlight?key={}", text::feed_link(key).trim_start_matches("foryou://paper?key="))
}

/// The key of a foryou://highlight link.
pub fn highlight_key(url: &str) -> Option<String> {
    let rest = url.strip_prefix("foryou://highlight?key=")?;
    text::feed_key(&format!("foryou://paper?key={rest}"))
}

/// The abstract's sentence that says what was found, else its first (cut to about 240 characters).
pub fn key_sentence(abstract_text: &str) -> String {
    let text = abstract_text.split_whitespace().collect::<Vec<_>>().join(" ");
    let sentences: Vec<&str> = text.split_inclusive(". ").map(str::trim).filter(|s| !s.is_empty()).collect();
    const CUES: &[&str] = &["here we", "here, we", "we show", "we find", "we found", "we report", "we demonstrate", "we identify", "we reveal", "we present", "we describe", "we develop", "we discover", "our results", "these results", "this study", "in this study"];
    let pick = sentences.iter().find(|s| CUES.iter().any(|c| s.to_lowercase().contains(c))).or(sentences.first()).copied().unwrap_or("");
    let pick = pick.trim_start_matches("Here, ").trim_start_matches("Here ");
    let mut out: String = pick.to_string();
    if let Some(c) = out.chars().next() {
        out = c.to_uppercase().chain(out.chars().skip(1)).collect();
    }
    if out.chars().count() > 260 {
        out = format!("{}…", out.chars().take(240).collect::<String>().trim_end());
    }
    out
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "16–23 Nov", "28 Nov – 5 Dec".
fn period(since: &str, until_ms: i64, tz_min: i32) -> String {
    let Some(from) = time::published(since, tz_min) else { return String::new() };
    let (_, m1, d1) = time::civil(time::local_day(from, tz_min));
    let (_, m2, d2) = time::civil(time::local_day(until_ms, tz_min));
    if m1 == m2 {
        format!("{d1}–{d2} {}", MONTHS[m2 as usize - 1])
    } else {
        format!("{d1} {} – {d2} {}", MONTHS[m1 as usize - 1], MONTHS[m2 as usize - 1])
    }
}

pub fn view(s: &HighlightsState, morning: i64, local_problem: Option<&str>, now: i64, tz_min: i32) -> HighlightsView {
    let papers: Vec<HighlightPaper> = s
        .items
        .iter()
        .enumerate()
        .map(|(i, r)| HighlightPaper {
            key: r.key.clone(),
            number: i as u32 + 1,
            title: r.title.clone(),
            authors: r.authors.clone(),
            byline: {
                let who = match r.authors.first() {
                    Some(a) if r.authors.len() > 1 => format!("{a} et al."),
                    Some(a) => a.clone(),
                    None => String::new(),
                };
                [who, r.journal.clone()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ")
            },
            journal: r.journal.clone(),
            date: r.date.clone(),
            published: time::published_label(&r.date, now, tz_min),
            doi: r.doi.clone(),
            link: if r.doi.is_empty() { r.url.clone() } else { format!("https://doi.org/{}", r.doi) },
            abstract_text: r.r#abstract.clone(),
            line: key_sentence(&r.r#abstract),
            in_library: r.in_library.clone(),
        })
        .collect();
    let number: HashMap<&str, u32> = papers.iter().map(|p| (p.key.as_str(), p.number)).collect();
    // a sentence citing a paper turned down since goes (a new period's papers leave none of the
    // last digest)
    let digest: Vec<HighlightSentence> = s
        .digest
        .iter()
        .filter(|d| !d.keys.is_empty() && d.keys.iter().all(|k| number.contains_key(k.as_str())))
        .map(|d| HighlightSentence { text: d.text.clone(), cites: d.keys.iter().map(|k| HighlightCite { number: number[k.as_str()], key: k.clone() }).collect() })
        .collect();
    let markdown = digest
        .iter()
        .map(|d| {
            let body = d.text.trim_end_matches(['.', ' ']);
            let cites = d.cites.iter().map(|c| format!("[{}]({})", c.number, highlight_link(&c.key))).collect::<Vec<_>>().join(", ");
            format!("{body} ({cites}).")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let journals: Vec<String> = s.journals.iter().map(|j| j.name.clone()).collect();
    let when = match s.last_run {
        Some(r) if !s.since.is_empty() => period(&s.since, r, tz_min),
        _ => String::new(),
    };
    let share = if digest.is_empty() {
        String::new()
    } else {
        let text = digest.iter().map(|d| format!("{} [{}]", d.text.trim_end_matches(['.', ' ']), d.cites.iter().map(|c| c.number.to_string()).collect::<Vec<_>>().join(", ")) + ".").collect::<Vec<_>>().join(" ");
        let list = papers
            .iter()
            .filter(|p| digest.iter().any(|d| d.cites.iter().any(|c| c.key == p.key)))
            .map(|p| format!("[{}] {} ({})\n    {}", p.number, p.title, p.journal, p.link))
            .collect::<Vec<_>>()
            .join("\n");
        format!("From {} ({when}), Lunta\n\n{text}\n\n{list}", and_list(&journals))
    };
    let next = match s.last_run {
        None => now,
        Some(_) => s.last_due(morning, now, tz_min) + if s.daily() { time::DAY_MS } else { 7 * time::DAY_MS },
    };
    HighlightsView {
        on: !s.journals.is_empty(),
        subtitle: [and_list(&journals), when].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · "),
        journals,
        cadence: if s.daily() { "daily".into() } else { "weekly".into() },
        papers,
        digest,
        digest_markdown: markdown,
        digest_share: share,
        last_run: s.last_run,
        next_run: next,
        problem: local_problem.filter(|p| !p.is_empty()).map(str::to_string),
    }
}

// ---- the handle ----

/// Journal highlights on one library (all apps: UniFFI for Apple and Android, directly from the
/// desktop app's Rust).
#[derive(uniffi::Object)]
pub struct JournalHighlights {
    lib: Arc<Library>,
    files: Files,
    lock: Mutex<()>,
    running: AtomicBool,
}

impl JournalHighlights {
    pub fn files(&self) -> &Files {
        &self.files
    }

    pub fn state(&self) -> HighlightsState {
        read(&self.files)
    }

    /// Runs with this finder (the tests' recorded answers).
    pub fn run_with(&self, finder: &dyn Finder, now_ms: i64, tz_min: i32) -> Result<HighlightsReport> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(HighlightsReport::default());
        }
        let r = run(&self.lib, &self.files, finder, now_ms, tz_min);
        self.running.store(false, Ordering::SeqCst);
        r
    }

    pub fn set_journal_with(&self, finder: &dyn Finder, name: &str, on: bool) -> Result<()> {
        let j = if on { Some(resolve(finder, name).map_err(CoreError::Invalid)?) } else { None };
        let _g = self.lock.lock().unwrap();
        change(&self.files, |s| {
            let same = |x: &HighlightJournal| norm(&x.name) == norm(name) || suggested(name).is_some_and(|(n, _)| n == x.name);
            s.journals.retain(|x| !same(x));
            if let Some(j) = j {
                s.journals.push(j);
            }
            // other journals: a new period's papers
            s.last_run = None;
        })
    }
}

#[uniffi::export]
impl JournalHighlights {
    /// `library_dir`: the library folder; `local_dir`: this app's own folder (as for NewForYou).
    #[uniffi::constructor]
    pub fn new(library: Arc<Library>, library_dir: String, local_dir: String) -> Arc<Self> {
        Arc::new(JournalHighlights { lib: library, files: Files::new(library_dir, local_dir), lock: Mutex::new(()), running: AtomicBool::new(false) })
    }

    pub fn view(&self, now_ms: i64, tz_min: i32) -> HighlightsView {
        let local = self.files.local();
        view(&self.state(), self.files.state().morning, Some(local.highlights_problem.as_str()), now_ms, tz_min)
    }

    /// The journals to choose from: the chosen ones, the ones you follow, the suggestions.
    pub fn choices(&self) -> Vec<JournalChoice> {
        choices(&self.state(), &self.files.watches())
    }

    /// Puts a journal in the highlights or takes it out (its ISSN from the table, else Crossref:
    /// network). The next check looks for the period's papers again.
    pub fn set_journal(&self, name: String, on: bool) -> Result<()> {
        self.set_journal_with(&NetFinder, &name, on)
    }

    /// Nature, Science and Cell.
    pub fn turn_on_defaults(&self) -> Result<()> {
        for n in DEFAULTS {
            self.set_journal_with(&NetFinder, n, true)?;
        }
        Ok(())
    }

    /// "weekly" (Monday at the morning time) or "daily".
    pub fn set_cadence(&self, cadence: String) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        change(&self.files, |s| {
            let c = if cadence == "daily" { "daily" } else { "weekly" };
            if s.cadence != c {
                s.cadence = c.into();
                s.last_run = None;
            }
        })
    }

    /// `phone`: leave the first two hours after the morning time to a computer.
    pub fn is_due(&self, now_ms: i64, tz_min: i32, phone: bool) -> bool {
        is_due(&self.files, now_ms, tz_min, phone)
    }

    /// Looks for the period's papers now (blocking: network, a few seconds).
    pub fn run(&self, now_ms: i64, tz_min: i32) -> Result<HighlightsReport> {
        self.run_with(&NetFinder, now_ms, tz_min)
    }

    /// What to ask the model for the digest; None when it is written, or there is nothing to write.
    pub fn digest_request(&self) -> Option<HighlightsDigestRequest> {
        digest_request(&self.state(), &self.files.state().interests)
    }

    /// The model's answer to `digest_request`: checked, and what holds up kept. How many
    /// sentences were kept.
    pub fn set_digest(&self, key: String, answer: String) -> Result<u32> {
        let _g = self.lock.lock().unwrap();
        set_digest(&self.files, &key, &answer)
    }

    pub fn dismiss(&self, key: String) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        dismiss(&self.files, &key)
    }

    pub fn added(&self, key: String, id: String) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        added(&self.files, &key, &id)
    }

    /// The paper's details, to add it (the DOI's lookup falls back on them).
    pub fn find_meta(&self, key: String) -> Option<PaperMeta> {
        self.state().items.into_iter().find(|r| r.key == key).map(|r| PaperMeta {
            title: r.title,
            authors: r.authors,
            date: r.date,
            journal: r.journal,
            doi: r.doi,
            url: r.url,
            abstract_text: r.r#abstract,
            ..Default::default()
        })
    }
}

#[cfg(feature = "llm")]
#[uniffi::export]
impl JournalHighlights {
    /// The digest by the on-device model (blocking). How many sentences were kept.
    pub fn write_digest(&self, engine: Arc<crate::llm::AiEngine>) -> u32 {
        write_digest(&self.files, engine.as_ref())
    }
}
