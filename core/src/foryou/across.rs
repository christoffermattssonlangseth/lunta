//! Across science: in New for You, each morning, the standout new research paper (or two) in each
//! broad field of science, outside your own topics (as PaperBoat's "Paper of the Day" per field).
//!
//! - **Where from:** one OpenAlex list request: the works of the last days in the journals of the
//!   impact table (impact.rs: Nature, Science, Cell, the large broad journals and each field's
//!   flagships, 71 journals, one ISSN each in an OR filter), research articles only, newest first,
//!   200 a page with OpenAlex's own field for each (`primary_topic`), its citations and FWCI. A
//!   quiet day is one page; at most [`MAX_PAGES`] (a list request is 1 of the 1,000 credits of the
//!   free daily allowance; the feed's run is about 140). The first run looks back 7 days, later
//!   ones 4 (OpenAlex adds journals' papers a day or two late). The answer is kept on this device
//!   for the day (`across-science-pool.json` in the app's own folder): changing the fields or 1 or
//!   2 a field picks again from it, without asking again. No group_by, no search, no request per
//!   field.
//! - **Research papers only:** as "From the journals you follow" (journals.rs: no news, comment,
//!   corrections, editorials, one- or two-page pieces, teasers for abstracts).
//! - **Fields:** OpenAlex's 26 fields (and two subfields for genetics) folded into [`FIELDS`]
//!   (13); a paper without a topic takes its journal's field from the table.
//! - **Outside your own topics:** a paper about as close to your paragraph, library and feedback
//!   as the feed's papers (the feed's ranking, rank.rs: closeness ≥ [`YOURS`]) is left out (it is
//!   the feed's), as are papers in your library, in the feed or offered there, in "From the journals
//!   you follow", and your own.
//! - **Standout:** impact is what picks (impact.rs: the journal's tier, citations, FWCI), then how
//!   sure OpenAlex is of the field (`primary_topic.score`, up to +0.05), an abstract to read (+0.03),
//!   then the newest (up to +0.04). Per field the best,
//!   and with 2 a field the best from another journal if there is one. Never offered twice.
//! - **Not for Me** hides the paper and, here only, lowers papers like it a little (−0.1 × the
//!   closest turned down); the feed's own feedback (foryou.json) is not touched.
//! - **The digest:** one sentence per field (its first paper) by the on-device model, numbered as
//!   in the list, checked as the journals' digest (outline::summary::validate). Without a model,
//!   just the list.
//! - **State:** `<library>/across-science.json` (settings, the day's papers, offered keys, the
//!   digest); when: New for You's morning time (foryou.json). A fetch that fails is said
//!   ("Couldn't get today's papers across science", Retry) and the day stays due.

use super::impact;
use super::journals::{self, and_list, key_sentence, DigestSentence, HighlightCite};
use super::rank;
use super::run::{Finder, NetFinder, Writer};
use super::state::{write_atomic, FeedbackSnip, Files, ForYouState};
use super::text;
use super::time::{self, iso};
use crate::metadata;
use crate::model::{CoreError, PaperFilter, PaperMeta, Result, SortKey};
use crate::outline::summary::{self, SummarySource};
use crate::Library;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const FILE: &str = "across-science.json";
/// The day's OpenAlex answer, on this device only.
pub const POOL_FILE: &str = "across-science-pool.json";

/// At most this many pages of 200 a day (1 credit each).
pub const MAX_PAGES: usize = 5;
pub const PER_PAGE: usize = 200;
/// A paper this close to you (the feed's closeness, rank.rs: TF-IDF cosines with your paragraph,
/// library and feedback) is the feed's, not one from across science. On the recorded day, for a
/// paragraph on spatial transcriptomics and microglia: the MERFISH and microglia papers 0.025–0.09,
/// every other field's under 0.02.
pub const YOURS: f64 = 0.025;
/// The digest's abstracts are cut to this (the batch is one paper a field, up to 13).
const DIGEST_ABSTRACT: usize = 320;

/// A broad field: OpenAlex fields (and subfields) folded into one.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub id: &'static str,
    pub label: &'static str,
    /// OpenAlex field ids (https://openalex.org/fields/<id>)
    pub openalex: &'static [u32],
    /// OpenAlex subfield ids that belong here whatever their field
    pub subfields: &'static [u32],
}

const fn f(id: &'static str, label: &'static str, openalex: &'static [u32], subfields: &'static [u32]) -> Field {
    Field { id, label, openalex, subfields }
}

/// The fields, in the order the section lists them.
pub const FIELDS: &[Field] = &[
    f("cellbio", "Cell & molecular biology", &[13], &[]),
    f("genetics", "Genetics & genomics", &[], &[1311, 2716]),
    f("neuro", "Neuroscience", &[28], &[]),
    f("immuno", "Immunology & microbiology", &[24], &[]),
    f("medicine", "Medicine", &[27, 29, 30, 34, 35, 36], &[]),
    f("ecology", "Ecology, evolution & plants", &[11], &[]),
    f("chemistry", "Chemistry", &[16, 15], &[]),
    f("materials", "Materials, energy & engineering", &[25, 21, 22], &[]),
    f("physics", "Physics & astronomy", &[31], &[]),
    f("earth", "Earth & environment", &[19, 23], &[]),
    f("computing", "Computer science & AI", &[17], &[]),
    f("math", "Mathematics", &[26, 18], &[]),
    f("society", "Psychology & social sciences", &[32, 33, 20, 14, 12], &[]),
];

pub fn field(id: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.id == id)
}

/// The broad field of an OpenAlex field and subfield (ids or their URLs).
pub fn field_of(openalex_field: u32, subfield: u32) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.subfields.contains(&subfield)).or_else(|| FIELDS.iter().find(|f| f.openalex.contains(&openalex_field)))
}

fn id_number(v: &Value) -> u32 {
    v.as_str().and_then(|s| s.rsplit('/').next()).and_then(|n| n.parse().ok()).unwrap_or(0)
}

// ---- the file ----

/// A paper in the day's section.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AcrossRecord {
    pub key: String,
    /// the field's id (FIELDS)
    pub field: String,
    pub title: String,
    pub authors: Vec<String>,
    pub date: String,
    pub journal: String,
    pub doi: String,
    pub url: String,
    pub r#abstract: String,
    /// the impact prior it was picked by (0–1)
    pub impact: f64,
    pub cited_by: i64,
    #[serde(with = "iso")]
    pub found: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_library: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// across-science.json.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AcrossState {
    pub on: bool,
    /// fields switched off (new fields are on)
    pub off_fields: Vec<String>,
    /// 1 or 2 papers a field
    pub per_field: u32,
    #[serde(with = "iso::opt", skip_serializing_if = "Option::is_none")]
    pub last_run: Option<i64>,
    /// the first day the papers were looked for from ("2026-11-20")
    pub since: String,
    pub items: Vec<AcrossRecord>,
    /// keys already offered (never offered twice)
    pub shown: Vec<String>,
    /// turned down here (Not for Me): papers like them come a little lower here
    pub dismissed: Vec<FeedbackSnip>,
    pub digest: Vec<DigestSentence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest_for: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for AcrossState {
    fn default() -> Self {
        AcrossState {
            on: false,
            off_fields: vec![],
            per_field: 1,
            last_run: None,
            since: String::new(),
            items: vec![],
            shown: vec![],
            dismissed: vec![],
            digest: vec![],
            digest_for: None,
            extra: Map::new(),
        }
    }
}

impl AcrossState {
    pub fn per_field(&self) -> usize {
        self.per_field.clamp(1, 2) as usize
    }

    pub fn field_on(&self, id: &str) -> bool {
        !self.off_fields.iter().any(|x| x == id)
    }

    /// The papers the digest is about: each field's first.
    pub fn digest_batch(&self) -> Vec<&AcrossRecord> {
        let mut seen = HashSet::new();
        self.items.iter().filter(|r| seen.insert(r.field.as_str())).collect()
    }

    pub fn digest_key(&self) -> String {
        format!("v1|{}", self.digest_batch().iter().map(|r| r.key.as_str()).collect::<Vec<_>>().join("|"))
    }
}

fn path(files: &Files) -> PathBuf {
    files.library.join(FILE)
}

pub fn read(files: &Files) -> AcrossState {
    std::fs::read_to_string(path(files)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn change<T>(files: &Files, f: impl FnOnce(&mut AcrossState) -> T) -> Result<T> {
    let mut s = read(files);
    let out = f(&mut s);
    let data = serde_json::to_vec_pretty(&s).map_err(|e| CoreError::Invalid(e.to_string()))?;
    write_atomic(&path(files), &data)?;
    Ok(out)
}

// ---- the request ----

const SELECT: &str = "id,doi,title,authorships,publication_date,primary_location,ids,cited_by_count,fwci,open_access,abstract_inverted_index,biblio,type,type_crossref,primary_topic";

/// The one OpenAlex request (a page of it, from 1): the table's journals' articles since `since`.
/// (Plain pages: OpenAlex refuses a cursor's later pages without an API key.)
pub fn request_url(since: &str, page: usize) -> String {
    let issns: Vec<&str> = impact::JOURNALS.iter().map(|j| j.issn[0]).collect();
    format!(
        "https://api.openalex.org/works?filter=primary_location.source.issn:{},from_publication_date:{since},type:article&sort=publication_date:desc&per_page={PER_PAGE}&page={page}&select={SELECT}",
        issns.join("|")
    )
}

/// The day's OpenAlex answer, kept on this device.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Pool {
    fetched: i64,
    since: String,
    works: Vec<Value>,
}

fn pool_path(files: &Files) -> PathBuf {
    files.local.join(POOL_FILE)
}

/// The works since `since`, page by page (at most MAX_PAGES). How many requests it took; an
/// error only when the first page couldn't be had (a later page's is a problem to say).
fn fetch(finder: &dyn Finder, since: &str) -> std::result::Result<(Vec<Value>, usize, Option<String>), String> {
    let mut works: Vec<Value> = vec![];
    let mut requests = 0;
    for page in 0..MAX_PAGES {
        requests += 1;
        let j = match finder.get_json(&request_url(since, page + 1)) {
            Ok(j) => j,
            Err(e) if page == 0 => return Err(said(e)),
            Err(e) => return Ok((works, requests, Some(format!("OpenAlex (page {}): {}", page + 1, said(e))))),
        };
        let results = j["results"].as_array().cloned().unwrap_or_default();
        let n = results.len();
        works.extend(results);
        let total = j["meta"]["count"].as_u64().unwrap_or(0) as usize;
        if n < PER_PAGE || works.len() >= total {
            break;
        }
    }
    Ok((works, requests, None))
}

fn said(e: CoreError) -> String {
    match e {
        CoreError::Network(m) => m,
        e => e.to_string(),
    }
}

/// A work as the section needs it.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub meta: PaperMeta,
    pub field: &'static Field,
    pub cited_by: u32,
    pub fwci: f64,
    pub tier: f64,
    /// how sure OpenAlex is of the topic (0–1)
    pub topic_score: f64,
}

/// An OpenAlex work as a candidate (None: no field, or no title).
pub fn candidate(w: &Value) -> Option<Candidate> {
    let hit = metadata::parse_works(&json!({ "results": [w] })).into_iter().next()?;
    let src = &w["primary_location"]["source"];
    let issns: Vec<String> = src["issn"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).chain(src["issn_l"].as_str().map(str::to_string)).collect();
    let journal = issns.iter().find_map(|i| impact::by_issn(i)).or_else(|| impact::by_name(&hit.meta.journal));
    let t = &w["primary_topic"];
    let field = field_of(id_number(&t["field"]["id"]), id_number(&t["subfield"]["id"])).or_else(|| journal.and_then(|j| field(j.field)))?;
    Some(Candidate {
        field,
        cited_by: hit.cited_by,
        fwci: w["fwci"].as_f64().unwrap_or(0.0),
        tier: journal.map_or(0.0, |j| j.tier),
        topic_score: t["score"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0),
        meta: hit.meta,
    })
}

impl Candidate {
    pub fn impact(&self) -> f64 {
        impact::impact_of(self.tier, self.cited_by, self.fwci)
    }
}

// ---- a run ----

/// What a run did.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AcrossReport {
    /// false when a run was already going on, or the section is off
    pub ran: bool,
    /// papers picked
    pub found: u32,
    /// fields with a paper today
    pub fields: u32,
    /// OpenAlex requests made (0: the day's answer was here already)
    pub requests: u32,
    /// research papers OpenAlex gave that are new to you
    pub candidates: u32,
    /// of those, left out as your own topics (the feed's)
    pub yours: u32,
    pub problem: Option<String>,
    /// nothing could be fetched: the papers are as they were and the run is still due
    pub failed: bool,
}

/// Looks for today's papers (or picks again from today's answer) and puts each field's best in
/// place of the last ones.
pub fn run(lib: &Library, files: &Files, finder: &dyn Finder, now: i64, tz_min: i32) -> Result<AcrossReport> {
    let s = read(files);
    if !s.on {
        return Ok(AcrossReport::default());
    }
    let fy = files.state();
    let period_start = fy.last_morning(now, tz_min);
    let mut report = AcrossReport { ran: true, ..Default::default() };

    // the day's answer: kept from earlier today, else asked for
    let kept: Option<Pool> = std::fs::read_to_string(pool_path(files)).ok().and_then(|t| serde_json::from_str(&t).ok());
    let (works, since) = match kept {
        Some(p) if p.fetched >= period_start && p.fetched <= now => (p.works, p.since),
        _ => {
            let days = if s.last_run.is_none() { 7 } else { 4 };
            let since = time::utc_date(time::start_of_day(now, tz_min) - days * time::DAY_MS + (tz_min as i64).max(0) * 60_000);
            match fetch(finder, &since) {
                Ok((works, requests, problem)) => {
                    report.requests = requests as u32;
                    report.problem = problem;
                    let pool = Pool { fetched: now, since: since.clone(), works };
                    if let Ok(data) = serde_json::to_vec(&pool) {
                        let _ = write_atomic(&pool_path(files), &data);
                    }
                    (pool.works, since)
                }
                Err(e) => {
                    let why = format!("Couldn't reach OpenAlex. {e}");
                    note_problem(files, &why)?;
                    report.requests = 1;
                    report.problem = Some(why);
                    report.failed = true;
                    return Ok(report);
                }
            }
        }
    };

    // research papers, new to you, not yours
    let today: HashSet<&str> = s.items.iter().filter(|r| r.found >= period_start).map(|r| r.key.as_str()).collect();
    let dismissed: HashSet<&str> = s.dismissed.iter().map(|d| d.key.as_str()).collect();
    let offered: HashSet<&str> = s.shown.iter().map(String::as_str).filter(|k| !today.contains(k)).chain(dismissed.iter().copied()).collect();
    let feed: HashSet<&str> = fy.shown.iter().chain(fy.items.iter().map(|r| &r.key)).map(String::as_str).collect();
    let hl = journals::read(files);
    let in_highlights: HashSet<&str> = hl.items.iter().map(|r| r.key.as_str()).collect();
    let own: HashSet<String> = fy.orcid.as_ref().map(|o| o.papers.iter().map(|p| p.key()).collect()).unwrap_or_default();
    let mut seen = HashSet::new();
    let pool: Vec<Candidate> = works
        .iter()
        .filter_map(candidate)
        .filter(|c| !journals::not_research(&c.meta) && journals::is_research(&c.meta) && c.meta.title.chars().count() > 10)
        .filter(|c| {
            let key = text::doi_key(&c.meta.doi);
            !key.is_empty() && seen.insert(key.clone()) && !offered.contains(key.as_str()) && !feed.contains(key.as_str()) && !in_highlights.contains(key.as_str()) && !own.contains(&key)
        })
        .filter(|c| !fy.orcid.as_ref().is_some_and(|o| o.is_mine(&c.meta.authors)))
        .filter(|c| lib.find_by_doi(c.meta.doi.clone()).is_none())
        .collect();
    report.candidates = pool.len() as u32;

    // your own topics stay out (they are the feed's); the rest by impact
    let every = lib.papers(PaperFilter::default(), SortKey::Added);
    let mut library: Vec<String> = every.iter().take(400).map(|p| text::snip(&p.meta.title, &p.meta.abstract_text)).collect();
    if let Some(me) = &fy.orcid {
        library.extend(me.papers.iter().map(|p| text::snip(&p.title, &p.r#abstract)));
    }
    let texts: Vec<String> = pool.iter().map(|c| text::snip(&c.meta.title, &c.meta.abstract_text)).collect();
    let snips = |l: &[FeedbackSnip]| l.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
    let closeness = rank::score(&texts, &fy.interests, &library, &snips(&fy.liked), &snips(&fy.disliked), &snips(&fy.boosted));
    let turned_down = rank::score(&texts, "", &snips(&s.dismissed), &[], &[], &[]);
    let mut yours = 0;
    let mut scored: Vec<(Candidate, f64)> = pool
        .into_iter()
        .zip(closeness)
        .zip(turned_down)
        .filter(|((_, close), _)| *close < YOURS || {
            yours += 1;
            false
        })
        .filter(|((c, _), _)| s.field_on(c.field.id))
        .map(|((c, _), down)| {
            // (a paper dated ahead, an issue's date, isn't newer than today's)
            let recency = rank::recency(&c.meta.date, now, tz_min).min(0.04);
            let readable = if c.meta.abstract_text.chars().count() >= 300 { 0.03 } else { 0.0 };
            let standout = c.impact() + 0.05 * c.topic_score + recency + readable - 0.1 * down;
            (c, standout)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    report.yours = yours;

    // each field's best (and with 2 a field, the best from another journal if there is one)
    let mut records: Vec<AcrossRecord> = vec![];
    for fld in FIELDS.iter().filter(|f| s.field_on(f.id)) {
        let mine: Vec<&(Candidate, f64)> = scored.iter().filter(|(c, _)| c.field.id == fld.id).collect();
        let mut picks: Vec<&(Candidate, f64)> = mine.iter().take(1).copied().collect();
        if s.per_field() > 1 {
            let other = mine.iter().skip(1).find(|(c, _)| picks.iter().all(|(p, _)| p.meta.journal != c.meta.journal)).or_else(|| mine.get(1));
            picks.extend(other.copied());
        }
        records.extend(picks.into_iter().map(|(c, _)| AcrossRecord {
            key: text::doi_key(&c.meta.doi),
            field: fld.id.to_string(),
            in_library: None,
            title: c.meta.title.clone(),
            authors: c.meta.authors.clone(),
            date: c.meta.date.clone(),
            journal: c.meta.journal.clone(),
            url: if c.meta.url.is_empty() { format!("https://doi.org/{}", c.meta.doi) } else { c.meta.url.clone() },
            doi: c.meta.doi.clone(),
            r#abstract: c.meta.abstract_text.chars().take(3000).collect(),
            impact: (c.impact() * 1000.0).round() / 1000.0,
            cited_by: c.cited_by as i64,
            found: now / 1000 * 1000,
            extra: Map::new(),
        }));
    }
    report.found = records.len() as u32;
    report.fields = records.iter().map(|r| r.field.as_str()).collect::<HashSet<_>>().len() as u32;
    change(files, |a| {
        for r in &records {
            if !a.shown.contains(&r.key) {
                a.shown.push(r.key.clone());
            }
        }
        let excess = a.shown.len().saturating_sub(3000);
        a.shown.drain(..excess);
        a.items = records;
        a.since = since.clone();
        a.last_run = Some(now / 1000 * 1000);
    })?;
    note_problem(files, report.problem.as_deref().unwrap_or(""))?;
    Ok(report)
}

/// What went wrong in this device's latest run, kept on this device ("" for nothing).
fn note_problem(files: &Files, problem: &str) -> Result<()> {
    let mut l = files.local();
    if l.across_problem == problem {
        return Ok(());
    }
    l.across_problem = problem.to_string();
    files.save_local(&l)
}

/// Whether today's papers are due: on, and no run since the morning time (a phone leaves the
/// first two hours to a computer, unless it never ran).
pub fn is_due(files: &Files, now: i64, tz_min: i32, phone: bool) -> bool {
    let s = read(files);
    if !s.on {
        return false;
    }
    let Some(last) = s.last_run else { return true };
    let due = files.state().last_morning(now, tz_min);
    last < due && !(phone && now < due + 2 * time::HOUR_MS)
}

// ---- the digest ----

pub const DIGEST_INSTRUCTIONS: &str = "You write one-sentence digests of new research papers for a curious scientist from another field. You use only what the titles and abstracts say, and every sentence cites the paper it is about.";

/// What to ask the model for the digest.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AcrossDigestRequest {
    pub instructions: String,
    pub prompt: String,
    /// give it back with the answer
    pub key: String,
}

/// The prompt; None when it is written already, or there are fewer than two papers.
pub fn digest_request(s: &AcrossState) -> Option<AcrossDigestRequest> {
    let batch = s.digest_batch();
    if batch.len() < 2 || s.digest_for.as_deref() == Some(s.digest_key().as_str()) {
        return None;
    }
    let number: HashMap<&str, usize> = s.items.iter().enumerate().map(|(i, r)| (r.key.as_str(), i + 1)).collect();
    let list = batch
        .iter()
        .map(|r| {
            let abs = if r.r#abstract.is_empty() { "(no abstract)".to_string() } else { text::first_sentences(&r.r#abstract, DIGEST_ABSTRACT) };
            let label = field(&r.field).map_or("", |f| f.label);
            format!("[{}] {label}: {} ({})\n    {}", number[r.key.as_str()], r.title, r.journal, abs)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "New research papers today, one for each field of science:\n{list}\n\n\
         Write one sentence for each paper: what it did or found, in the words of its title and abstract, plain enough \
         for a scientist from another field, ending with its number in square brackets, e.g. [3]. \
         No numbers, names or claims that aren't in the title and abstract. No heading, no list, no preamble."
    );
    Some(AcrossDigestRequest { instructions: DIGEST_INSTRUCTIONS.into(), prompt, key: s.digest_key() })
}

/// The model's answer, checked: the sentences that cite one of the batch's papers and say only
/// what its title and abstract say, at most one a field. How many were kept.
pub fn set_digest(files: &Files, key: &str, answer: &str) -> Result<u32> {
    change(files, |s| {
        if s.digest_key() != key {
            return 0;
        }
        let sentences = check_digest(answer, s);
        let n = sentences.len() as u32;
        s.digest = sentences;
        s.digest_for = Some(key.to_string());
        n
    })
}

pub fn check_digest(answer: &str, s: &AcrossState) -> Vec<DigestSentence> {
    let batch: Vec<&AcrossRecord> = s.digest_batch();
    // numbered as in the list (what the model was given)
    let sources: Vec<SummarySource> = s
        .items
        .iter()
        .enumerate()
        .filter(|(_, r)| batch.iter().any(|b| b.key == r.key))
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
    // positional: [n] is the list's n-th paper (one not in the batch isn't a source: dropped)
    let ids: Vec<String> = s.items.iter().map(|r| r.key.clone()).collect();
    let by_number: HashMap<u32, String> = sources.iter().map(|x| (x.number, x.annotation_id.clone())).collect();
    let (kept, _drops) = summary::validate(answer, &ids, &sources, None, batch.len());
    let field_of_key: HashMap<&str, &str> = s.items.iter().map(|r| (r.key.as_str(), r.field.as_str())).collect();
    let mut fields_said = HashSet::new();
    let mut out: Vec<DigestSentence> = kept
        .into_iter()
        .map(|x| DigestSentence { text: x.text, keys: x.citations.iter().filter_map(|n| by_number.get(n).cloned()).collect() })
        .filter(|d| !d.keys.is_empty() && fields_said.insert(field_of_key.get(d.keys[0].as_str()).copied().unwrap_or("")))
        .collect();
    // A field whose sentence was left out (the model skipped it, or it said more than the title and
    // abstract: a paper without an abstract has only its title to go on) is still in the summary, with
    // the paper's title, so the numbers run on without a gap.
    for r in &batch {
        if fields_said.insert(field_of_key.get(r.key.as_str()).copied().unwrap_or("")) {
            out.push(DigestSentence { text: r.title.trim().trim_end_matches('.').to_string(), keys: vec![r.key.clone()] });
        }
    }
    let number: HashMap<&str, usize> = s.items.iter().enumerate().map(|(i, r)| (r.key.as_str(), i)).collect();
    out.sort_by_key(|d| number.get(d.keys[0].as_str()).copied().unwrap_or(usize::MAX));
    out
}

/// The digest by the core's model (desktop and Android). How many sentences were kept.
pub fn write_digest(files: &Files, w: &dyn Writer) -> u32 {
    let s = read(files);
    let Some(req) = digest_request(&s) else { return 0 };
    let Some(answer) = w.answer(&req.instructions, &req.prompt, 600) else { return 0 };
    set_digest(files, &req.key, &answer).unwrap_or(0)
}

// ---- feedback ----

/// Not for Me: off the list, never offered again, and papers like it a little lower here (not in
/// the feed).
pub fn dismiss(files: &Files, key: &str) -> Result<()> {
    change(files, |s| {
        let Some(i) = s.items.iter().position(|r| r.key == key) else { return };
        let r = s.items.remove(i);
        s.dismissed.retain(|d| d.key != key);
        s.dismissed.push(FeedbackSnip { key: key.to_string(), text: text::snip(&r.title, &r.r#abstract) });
        let excess = s.dismissed.len().saturating_sub(100);
        s.dismissed.drain(..excess);
    })
}

pub fn added(files: &Files, key: &str, id: &str) -> Result<()> {
    change(files, |s| {
        if let Some(r) = s.items.iter_mut().find(|r| r.key == key) {
            r.in_library = Some(id.to_string());
        }
    })
}

// ---- what the apps show ----

/// A paper in the section.
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct AcrossPaper {
    pub key: String,
    /// its number in the list (what the digest cites)
    pub number: u32,
    /// the field's id and name ("neuro", "Neuroscience")
    pub field: String,
    pub field_label: String,
    pub title: String,
    pub authors: Vec<String>,
    /// "Jane Doe et al. · Nature"
    pub byline: String,
    pub journal: String,
    pub date: String,
    pub published: Option<String>,
    pub doi: String,
    pub link: String,
    pub abstract_text: String,
    /// the abstract's "Here we show…" sentence, else its first
    pub line: String,
    pub in_library: Option<String>,
}

/// A sentence of the digest: its field and the paper(s) it cites.
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct AcrossSentence {
    pub field_label: String,
    pub text: String,
    pub cites: Vec<HighlightCite>,
}

/// A field to switch on or off in the settings.
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct FieldChoice {
    pub id: String,
    pub label: String,
    pub on: bool,
}

/// "Across science".
#[derive(Serialize, Clone, Debug, PartialEq, uniffi::Record)]
pub struct AcrossView {
    pub on: bool,
    /// 1 or 2
    pub per_field: u32,
    /// "13 fields · 24 Nov"
    pub subtitle: String,
    pub papers: Vec<AcrossPaper>,
    pub digest: Vec<AcrossSentence>,
    /// the digest and its papers as text to send someone
    pub digest_share: String,
    /// fields on without a paper today
    pub quiet_fields: Vec<String>,
    pub last_run: Option<i64>,
    pub next_run: i64,
    /// what went wrong in this device's latest run
    pub problem: Option<String>,
}

/// The link a digest citation makes.
pub fn across_link(key: &str) -> String {
    format!("foryou://across?key={}", text::feed_link(key).trim_start_matches("foryou://paper?key="))
}

/// The key of a foryou://across link.
pub fn across_key(url: &str) -> Option<String> {
    let rest = url.strip_prefix("foryou://across?key=")?;
    text::feed_key(&format!("foryou://paper?key={rest}"))
}

pub fn choices(s: &AcrossState) -> Vec<FieldChoice> {
    FIELDS.iter().map(|f| FieldChoice { id: f.id.into(), label: f.label.into(), on: s.field_on(f.id) }).collect()
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub fn view(s: &AcrossState, morning: i64, local_problem: Option<&str>, now: i64, tz_min: i32) -> AcrossView {
    let papers: Vec<AcrossPaper> = s
        .items
        .iter()
        .enumerate()
        .map(|(i, r)| AcrossPaper {
            key: r.key.clone(),
            number: i as u32 + 1,
            field: r.field.clone(),
            field_label: field(&r.field).map_or(String::new(), |f| f.label.to_string()),
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
    let by_key: HashMap<&str, &AcrossPaper> = papers.iter().map(|p| (p.key.as_str(), p)).collect();
    // a sentence about a paper turned down since goes
    let digest: Vec<AcrossSentence> = s
        .digest
        .iter()
        .filter(|d| !d.keys.is_empty() && d.keys.iter().all(|k| by_key.contains_key(k.as_str())))
        .map(|d| AcrossSentence {
            field_label: by_key[d.keys[0].as_str()].field_label.clone(),
            text: d.text.clone(),
            cites: d.keys.iter().map(|k| HighlightCite { number: by_key[k.as_str()].number, key: k.clone() }).collect(),
        })
        .collect();
    let on_fields: Vec<&Field> = FIELDS.iter().filter(|f| s.field_on(f.id)).collect();
    let quiet: Vec<String> = match s.last_run {
        Some(_) => on_fields.iter().filter(|f| !s.items.iter().any(|r| r.field == f.id)).map(|f| f.label.to_string()).collect(),
        None => vec![],
    };
    let day = s.last_run.map(|r| {
        let (_, m, d) = time::civil(time::local_day(r, tz_min));
        format!("{d} {}", MONTHS[m as usize - 1])
    });
    let fields = if on_fields.len() == FIELDS.len() { format!("{} fields", FIELDS.len()) } else { and_list(&on_fields.iter().map(|f| f.label.to_string()).collect::<Vec<_>>()) };
    let share = if digest.is_empty() {
        String::new()
    } else {
        let text = digest.iter().map(|d| format!("{}: {} [{}]", d.field_label, d.text.trim_end_matches(['.', ' ']), d.cites.iter().map(|c| c.number.to_string()).collect::<Vec<_>>().join(", ")) + ".").collect::<Vec<_>>().join("\n");
        let list = papers
            .iter()
            .filter(|p| digest.iter().any(|d| d.cites.iter().any(|c| c.key == p.key)))
            .map(|p| format!("[{}] {} ({})\n    {}", p.number, p.title, p.journal, p.link))
            .collect::<Vec<_>>()
            .join("\n");
        format!("Across science{}, Lunta\n\n{text}\n\n{list}", day.as_deref().map(|d| format!(" ({d})")).unwrap_or_default())
    };
    let fy = ForYouState { morning, ..Default::default() };
    AcrossView {
        on: s.on,
        per_field: s.per_field() as u32,
        subtitle: [Some(fields), day].into_iter().flatten().collect::<Vec<_>>().join(" · "),
        papers,
        digest,
        digest_share: share,
        quiet_fields: quiet,
        last_run: s.last_run,
        next_run: if s.last_run.is_none() { now } else { fy.next_morning(now, tz_min) },
        problem: local_problem.filter(|p| !p.is_empty()).map(str::to_string),
    }
}

// ---- the handle ----

/// Across science on one library (all apps: UniFFI for Apple and Android, directly from the
/// desktop app's Rust).
#[derive(uniffi::Object)]
pub struct AcrossScience {
    lib: Arc<Library>,
    files: Files,
    lock: Mutex<()>,
    running: AtomicBool,
}

impl AcrossScience {
    pub fn files(&self) -> &Files {
        &self.files
    }

    pub fn state(&self) -> AcrossState {
        read(&self.files)
    }

    /// Runs with this finder (the tests' recorded answers).
    pub fn run_with(&self, finder: &dyn Finder, now_ms: i64, tz_min: i32) -> Result<AcrossReport> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(AcrossReport::default());
        }
        crate::metadata::set_openalex_key(self.files.state().openalex_key.as_deref().unwrap_or(""));
        let r = run(&self.lib, &self.files, finder, now_ms, tz_min);
        self.running.store(false, Ordering::SeqCst);
        r
    }

    fn set(&self, f: impl FnOnce(&mut AcrossState)) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        change(&self.files, f)
    }
}

#[uniffi::export]
impl AcrossScience {
    /// `library_dir`: the library folder; `local_dir`: this app's own folder (as for NewForYou).
    #[uniffi::constructor]
    pub fn new(library: Arc<Library>, library_dir: String, local_dir: String) -> Arc<Self> {
        Arc::new(AcrossScience { lib: library, files: Files::new(library_dir, local_dir), lock: Mutex::new(()), running: AtomicBool::new(false) })
    }

    pub fn view(&self, now_ms: i64, tz_min: i32) -> AcrossView {
        let local = self.files.local();
        view(&self.state(), self.files.state().morning, Some(local.across_problem.as_str()), now_ms, tz_min)
    }

    pub fn fields(&self) -> Vec<FieldChoice> {
        choices(&self.state())
    }

    /// On (due at once) or off.
    pub fn set_on(&self, on: bool) -> Result<()> {
        self.set(|s| {
            if s.on != on {
                s.on = on;
                s.last_run = None;
            }
        })
    }

    /// A field on or off; the next run picks again (from the day's answer: no request).
    pub fn set_field(&self, id: String, on: bool) -> Result<()> {
        self.set(|s| {
            s.off_fields.retain(|x| *x != id);
            if !on && field(&id).is_some() {
                s.off_fields.push(id.clone());
            }
            s.last_run = None;
        })
    }

    /// 1 or 2 papers a field; the next run picks again (from the day's answer).
    pub fn set_per_field(&self, n: u32) -> Result<()> {
        self.set(|s| {
            let n = n.clamp(1, 2);
            if s.per_field != n {
                s.per_field = n;
                s.last_run = None;
            }
        })
    }

    /// `phone`: leave the first two hours after the morning time to a computer.
    pub fn is_due(&self, now_ms: i64, tz_min: i32, phone: bool) -> bool {
        is_due(&self.files, now_ms, tz_min, phone)
    }

    /// Looks for today's papers now (blocking: one OpenAlex request, a few seconds), or picks
    /// again from today's answer.
    pub fn run(&self, now_ms: i64, tz_min: i32) -> Result<AcrossReport> {
        self.run_with(&NetFinder, now_ms, tz_min)
    }

    pub fn digest_request(&self) -> Option<AcrossDigestRequest> {
        digest_request(&self.state())
    }

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

    /// The paper's details, to add it.
    pub fn find_meta(&self, key: String) -> Option<PaperMeta> {
        self.state().items.into_iter().find(|r| r.key == key).map(|r| PaperMeta {
            title: r.title,
            authors: r.authors,
            date: r.date,
            journal: r.journal,
            doi: r.doi,
            url: r.url,
            abstract_text: r.r#abstract,
            work_type: "journal-article".into(),
            ..Default::default()
        })
    }
}

#[cfg(feature = "llm")]
#[uniffi::export]
impl AcrossScience {
    /// The digest by the on-device model (blocking). How many sentences were kept.
    pub fn write_digest(&self, engine: Arc<crate::llm::AiEngine>) -> u32 {
        write_digest(&self.files, engine.as_ref())
    }
}
