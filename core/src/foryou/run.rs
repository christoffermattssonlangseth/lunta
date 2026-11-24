//! Making the day's feed (WatchCenter.run in apple/Shared/Watches.swift), step by step:
//!
//! 1. searches written from your paragraph (by the language model, else its own phrases);
//! 2. a wide net: those searches on OpenAlex, your watches, papers citing your own papers (with
//!    your ORCID iD) and the newest papers in your library, and the last days of bioRxiv and medRxiv;
//! 3. only papers new to you: not in the library, never offered before, not your own, papers with
//!    an abstract (no datasets, theses or book series);
//! 4. ranking (rank.rs), closeness weighted a little by impact (impact.rs: the journal's tier,
//!    citations so far), with bonuses for what you follow, journals you follow and new papers;
//! 5. the language model reads the best abstracts and keeps those that fit, saying why (without a
//!    model the ranking is trusted);
//! 6. the new picks go on top of the feed, best first.
//!
//! The network and the model come in through traits ([`Finder`], [`Writer`]), so the tests run
//! on recorded answers.

use super::impact;
use super::rank;
use super::state::*;
use super::text::{self, hit_key};
use super::time;
use crate::metadata::{self, DiscoverHit, OrcidProfile, WatchKind};
use crate::model::{PaperFilter, Result, SortKey};
use crate::Library;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Where new papers come from: OpenAlex and bioRxiv's API ([`NetFinder`]), or recorded answers.
pub trait Finder: Sync {
    fn watch_new(&self, kind: WatchKind, value: &str, since: &str, limit: u32) -> Result<Vec<DiscoverHit>>;
    fn citing_new(&self, doi: &str, exclude_author: &str, since: &str, limit: u32) -> Result<Vec<DiscoverHit>>;
    fn orcid_profile(&self, orcid: &str) -> Result<OrcidProfile>;
    /// A JSON document (bioRxiv's and medRxiv's API).
    fn get_json(&self, url: &str) -> Result<Value>;
}

/// The real thing: the core's OpenAlex calls (metadata.rs).
pub struct NetFinder;

impl Finder for NetFinder {
    fn watch_new(&self, kind: WatchKind, value: &str, since: &str, limit: u32) -> Result<Vec<DiscoverHit>> {
        metadata::watch_new(kind, value, since, limit)
    }
    fn citing_new(&self, doi: &str, exclude_author: &str, since: &str, limit: u32) -> Result<Vec<DiscoverHit>> {
        metadata::citing_new(doi, exclude_author, since, limit)
    }
    fn orcid_profile(&self, orcid: &str) -> Result<OrcidProfile> {
        metadata::orcid_profile(orcid)
    }
    fn get_json(&self, url: &str) -> Result<Value> {
        metadata::get_json(url)
    }
}

/// A language model that answers a prompt (the core's AiEngine with the `llm` feature).
pub trait Writer: Sync {
    /// The answer, or None when the model couldn't give one.
    fn answer(&self, instructions: &str, prompt: &str, max_tokens: u32) -> Option<String>;
}

/// What a run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunReport {
    /// papers added to the feed
    pub found: u32,
    /// new papers ranked
    pub candidates: u32,
    /// what each source brought and what was screened (for testing by hand)
    pub stats: String,
    /// what went wrong fetching ("OpenAlex's free daily allowance … is used up"), if anything
    pub problem: Option<String>,
    /// nothing could be fetched from OpenAlex: no feed was made, the run is still due
    pub failed: bool,
}

/// Everything a run needs.
pub struct Run<'a> {
    pub lib: &'a Library,
    pub files: &'a Files,
    pub finder: &'a dyn Finder,
    pub writer: Option<&'a dyn Writer>,
    pub now: i64,
    pub tz_min: i32,
    pub progress: &'a (dyn Fn(&str) + Sync),
    /// set to stop (between steps and papers)
    pub stop: Option<&'a AtomicBool>,
}

impl Run<'_> {
    fn stopped(&self) -> bool {
        self.stop.is_some_and(|s| s.load(Ordering::SeqCst))
    }
}

// ---- your ORCID iD ----

impl OwnPaper {
    pub fn from_hit(h: &DiscoverHit) -> OwnPaper {
        OwnPaper {
            title: h.meta.title.clone(),
            authors: h.meta.authors.iter().take(12).cloned().collect(),
            date: h.meta.date.clone(),
            journal: h.meta.journal.clone(),
            doi: h.meta.doi.clone(),
            r#abstract: h.meta.abstract_text.chars().take(700).collect(),
            cited_by: h.cited_by as i64,
        }
    }
}

impl OrcidLink {
    /// You as OpenAlex has you: your latest papers, then your most cited ones not among them.
    pub fn from_profile(p: &OrcidProfile, now: i64) -> OrcidLink {
        let mut papers: Vec<OwnPaper> = p.works.iter().map(OwnPaper::from_hit).collect();
        for h in &p.most_cited {
            let o = OwnPaper::from_hit(h);
            if !papers.iter().any(|x| x.key() == o.key()) {
                papers.push(o);
            }
        }
        OrcidLink {
            orcid: p.orcid.clone(),
            name: p.name.clone(),
            author: p.openalex_id.clone(),
            works: p.works_count as i64,
            papers,
            coauthors: p
                .coauthors
                .iter()
                .map(|c| CoauthorRecord { name: c.name.clone(), author: c.openalex_id.clone(), orcid: c.orcid.clone(), shared: c.shared as i64 })
                .collect(),
            updated: now / 1000 * 1000,
            extra: Default::default(),
        }
    }

    /// The papers to watch for citations: the most cited and the latest (with DOIs), up to `n`.
    pub fn citable(&self, n: usize) -> Vec<OwnPaper> {
        let with_doi: Vec<&OwnPaper> = self.papers.iter().filter(|p| !p.doi.is_empty()).collect();
        let mut by_cites = with_doi.clone();
        by_cites.sort_by(|a, b| b.cited_by.cmp(&a.cited_by));
        let mut out: Vec<OwnPaper> = by_cites.into_iter().take(n / 2).cloned().collect();
        for p in with_doi {
            if out.len() < n && !out.iter().any(|x| x.key() == p.key()) {
                out.push(p.clone());
            }
        }
        out
    }

    /// Whether you are one of these authors (by name, as OpenAlex writes it).
    pub fn is_mine(&self, authors: &[String]) -> bool {
        let me = plain(&self.name);
        !me.is_empty() && authors.iter().any(|a| plain(a) == me)
    }
}

/// A name without case, accents or anything but letters ("Gonçalo Castelo-Branco" →
/// "goncalocastelobranco").
pub fn plain(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).map(fold).filter(|c| c.is_alphabetic()).collect()
}

fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ğ' | 'ģ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => 'i',
        'ķ' => 'k',
        'ł' | 'ľ' | 'ļ' | 'ĺ' => 'l',
        'ñ' | 'ń' | 'ň' | 'ņ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' | 'ő' => 'o',
        'ř' | 'ŕ' => 'r',
        'ś' | 'š' | 'ş' | 'ș' => 's',
        'ť' | 'ţ' | 'ț' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    }
}

/// Looks you up on OpenAlex by your ORCID iD and remembers it (synced). The problem to show
/// when it can't.
pub fn connect_orcid(files: &Files, finder: &dyn Finder, input: &str, now: i64) -> std::result::Result<(), String> {
    let Some(orcid) = metadata::normalize_orcid(input) else {
        return Err("That isn’t an ORCID iD. It looks like 0000-0002-1825-0097.".into());
    };
    let p = finder.orcid_profile(&orcid).map_err(|e| e.to_string())?;
    files.change(|s| s.orcid = Some(OrcidLink::from_profile(&p, now))).map_err(|e| e.to_string())
}

/// Your papers as OpenAlex has them today (once a day; kept as they were on failure).
fn refresh_orcid(r: &Run) {
    let s = r.files.state();
    let Some(link) = s.orcid else { return };
    if link.updated >= r.now - 20 * time::HOUR_MS {
        return;
    }
    if let Ok(p) = r.finder.orcid_profile(&link.orcid) {
        let _ = r.files.change(|s| {
            if s.orcid.as_ref().is_some_and(|o| o.orcid == link.orcid) {
                s.orcid = Some(OrcidLink::from_profile(&p, r.now));
            }
        });
    }
}

// ---- the feed's records ----

impl FeedRecord {
    pub fn from_hit(h: &DiscoverHit, reason: &str, score: f64, source: &str, now: i64) -> FeedRecord {
        FeedRecord {
            key: hit_key(h),
            title: h.meta.title.clone(),
            authors: h.meta.authors.clone(),
            date: h.meta.date.clone(),
            journal: h.meta.journal.clone(),
            doi: h.meta.doi.clone(),
            url: h.meta.url.clone(),
            r#abstract: h.meta.abstract_text.chars().take(3000).collect(),
            open_access: h.open_access,
            cited_by: h.cited_by as i64,
            reason: reason.to_string(),
            source: source.to_string(),
            score: if score.is_finite() { score } else { 0.0 },
            found: now / 1000 * 1000,
            in_library: h.in_library.clone(),
            boosted: None,
            summary: None,
            extra: Default::default(),
        }
    }
}

// ---- feedback ----

/// "Not for me": removes it here and on the other devices, and ranks similar papers lower.
pub fn dismiss(s: &mut ForYouState, key: &str) {
    let Some(i) = s.items.iter().position(|r| r.key == key) else { return };
    let r = s.items.remove(i);
    s.disliked.push(FeedbackSnip { key: key.to_string(), text: text::snip(&r.title, &r.r#abstract) });
    keep_last(&mut s.disliked, 150);
}

/// Thumbs up ("more like this"): ranks similar papers higher and shows the model what you want.
/// Again takes it back. Returns whether it is on now.
pub fn boost(s: &mut ForYouState, key: &str) -> bool {
    let Some(i) = s.items.iter().position(|r| r.key == key) else { return false };
    let on = !s.items[i].boosted.unwrap_or(false);
    s.items[i].boosted = Some(on);
    s.boosted.retain(|b| b.key != key);
    if on {
        let r = &s.items[i];
        s.boosted.push(FeedbackSnip { key: key.to_string(), text: text::snip(&r.title, &r.r#abstract) });
        keep_last(&mut s.boosted, 100);
    }
    on
}

/// A paper from the feed was added to the library: remembered as one you wanted.
pub fn added(s: &mut ForYouState, key: &str, id: &str) {
    let Some(i) = s.items.iter().position(|r| r.key == key) else { return };
    s.items[i].in_library = Some(id.to_string());
    if !s.liked.iter().any(|l| l.key == key) {
        let r = &s.items[i];
        s.liked.push(FeedbackSnip { key: key.to_string(), text: text::snip(&r.title, &r.r#abstract) });
        keep_last(&mut s.liked, 150);
    }
}

fn keep_last<T>(v: &mut Vec<T>, n: usize) {
    if v.len() > n {
        v.drain(..v.len() - n);
    }
}

// ---- when ----

/// Whether the day's papers are due: there is something to look for and no run since this
/// morning's time. A phone leaves the first two hours to a computer, which usually makes the
/// day's feed (as the iPhone app does), unless it never ran.
pub fn is_due(s: &ForYouState, watches: &[Watch], now: i64, tz_min: i32, phone: bool) -> bool {
    if !s.has_setup(watches) {
        return false;
    }
    let morning = s.last_morning(now, tz_min);
    if s.last_run.unwrap_or(i64::MIN) >= morning {
        return false;
    }
    !(phone && s.last_run.is_some() && now < morning + 2 * time::HOUR_MS)
}

// ---- making the feed ----

/// Search queries and keywords from the paragraph (its own phrases without a model).
pub fn searches(interests: &str, writer: Option<&dyn Writer>) -> (Vec<String>, Vec<String>) {
    if let Some(w) = writer {
        if let Some((q, k)) = w.answer(text::SEARCHES_INSTRUCTIONS, &text::searches_prompt(interests), 400).as_deref().and_then(text::parse_searches) {
            return (q, k);
        }
    }
    text::fallback_searches(interests)
}

struct Job {
    kind: WatchKind,
    value: String,
    source: String,
    explicit: bool,
    limit: u32,
    /// citing your paper: leave out your own (OpenAlex author id)
    not_by: Option<String>,
}

#[derive(Clone)]
struct Found {
    hit: DiscoverHit,
    source: String,
    explicit: bool,
}

/// Runs `f` over `items` on up to `threads` threads, results in any order.
fn parallel<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|sc| {
        for _ in 0..threads.min(items.len()).max(1) {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(it) = items.get(i) else { break };
                let r = f(it);
                out.lock().unwrap().push(r);
            });
        }
    });
    out.into_inner().unwrap()
}

/// The last days of bioRxiv or medRxiv, keeping papers that use your keywords. The API serves a
/// page per request, so after the first page the rest are fetched six at a time. An error when
/// the first page couldn't be fetched.
pub fn preprints(finder: &dyn Finder, server: &str, from: &str, to: &str, keywords: &[String]) -> std::result::Result<Vec<DiscoverHit>, String> {
    let kws: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).filter(|k| k.chars().count() > 3).collect();
    if kws.is_empty() {
        return Ok(vec![]);
    }
    let fetch = |cursor: usize| -> std::result::Result<(Vec<Value>, usize), String> {
        let j = finder.get_json(&format!("https://api.biorxiv.org/details/{server}/{from}/{to}/{cursor}/json")).map_err(said)?;
        let msg = &j["messages"][0];
        let total = msg["total"].as_u64().map(|x| x as usize).or_else(|| msg["total"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
        Ok((j["collection"].as_array().cloned().unwrap_or_default(), total))
    };
    let page = |cursor: usize| fetch(cursor).unwrap_or_default();
    let (mut all, total) = fetch(0)?;
    let size = all.len().max(1);
    let cursors: Vec<usize> = (size..total.min(3000)).step_by(size).collect();
    for chunk in cursors.chunks(6) {
        for items in parallel(chunk, 6, |c| page(*c).0) {
            all.extend(items);
        }
    }
    let name = if server == "biorxiv" { "bioRxiv" } else { "medRxiv" };
    let mut order: Vec<String> = vec![];
    let mut out: HashMap<String, DiscoverHit> = HashMap::new();
    let st = |v: &Value| v.as_str().unwrap_or("").to_string();
    for it in all {
        let title = st(&it["title"]).trim().to_string();
        let abstract_text = st(&it["abstract"]);
        let doi = st(&it["doi"]);
        let (t, a) = (title.to_lowercase(), abstract_text.to_lowercase());
        let in_title = kws.iter().any(|k| t.contains(k.as_str()));
        let in_abstract = kws.iter().filter(|k| a.contains(k.as_str())).count();
        if !(in_title || in_abstract >= 2) || doi.is_empty() {
            continue;
        }
        let authors: Vec<String> = st(&it["authors"])
            .split(';')
            .map(|part| {
                // "Smith, J. A." → "J. A. Smith"
                let bits: Vec<&str> = part.split(',').map(str::trim).collect();
                if bits.len() == 2 {
                    format!("{} {}", bits[1], bits[0])
                } else {
                    part.trim().to_string()
                }
            })
            .filter(|a| !a.is_empty())
            .collect();
        let hit = DiscoverHit {
            meta: crate::model::PaperMeta {
                title,
                authors,
                date: st(&it["date"]),
                journal: name.to_string(),
                doi: doi.clone(),
                url: format!("https://www.{server}.org/content/{doi}"),
                abstract_text,
                ..Default::default()
            },
            cited_by: 0,
            open_access: true,
            in_library: None,
        };
        if !out.contains_key(&doi) {
            order.push(doi.clone());
        }
        out.insert(doi, hit);
    }
    Ok(order.into_iter().filter_map(|d| out.remove(&d)).collect())
}

/// Papers only: no data or code deposits, theses servers or book series.
fn not_a_paper(venue: &str) -> bool {
    let v = venue.to_lowercase();
    ["zenodo", "figshare", "dryad", "osf", "repository", "publikationsserver", "book series", "dissertation", "thesis"]
        .iter()
        .any(|x| v.contains(x))
}

/// Finds, ranks and screens new papers, and puts the best at the top of the feed.
pub fn run(r: &Run) -> Result<RunReport> {
    let mut report = RunReport::default();
    let watches = r.files.watches();
    if !r.files.state().has_setup(&watches) {
        return Ok(report);
    }
    refresh_orcid(r);
    let state = r.files.state();
    let me = state.orcid.clone();
    let interests = state.interests.clone();
    let first = state.last_run.is_none();
    let since = time::utc_date(r.now - if first { 30 } else { 9 } * time::DAY_MS);

    // 1. searches written from the paragraph
    if !interests.is_empty() && (state.queries_for != interests || state.queries.is_empty()) {
        (r.progress)("Reading your interests…");
        let (q, k) = searches(&interests, r.writer);
        r.files.change(|s| {
            s.queries = q;
            s.keywords = k;
            s.queries_for = interests.clone();
        })?;
    }
    let state = r.files.state();
    if r.stopped() {
        return Ok(report);
    }

    // 2. the wide net
    (r.progress)("Looking for new papers…");
    let mut jobs: Vec<Job> = vec![];
    let job = |kind, value: &str, source: String, explicit, limit| Job { kind, value: value.to_string(), source, explicit, limit, not_by: None };
    for q in &state.queries {
        jobs.push(job(WatchKind::Topic, q, format!("search “{q}”"), false, 40));
    }
    for w in &watches {
        jobs.push(match w.kind {
            WatchType::Topic => job(WatchKind::Topic, &w.value, format!("your topic “{}”", w.label), false, 25),
            WatchType::Author => job(WatchKind::Author, &w.value, format!("New paper by {}, whom you follow.", w.label), true, 15),
            WatchType::Citing => job(WatchKind::Citing, &w.value, format!("Cites “{}”, which you watch.", w.label), true, 15),
            WatchType::Journal => job(WatchKind::Journal, &w.value, text::journal_source(&w.label), false, 30),
        });
    }
    // papers citing your own (as if you followed them), without your own citing them
    if let Some(me) = &me {
        for p in me.citable(15) {
            jobs.push(Job { not_by: Some(me.author.clone()), ..job(WatchKind::Citing, &p.doi, text::cites_your_paper(&p.title), true, 10) });
        }
    }
    let own_keys: HashSet<String> = me.as_ref().map(|m| m.papers.iter().map(OwnPaper::key).collect()).unwrap_or_default();
    let every = r.lib.papers(PaperFilter::default(), SortKey::Added);
    for p in every.iter().filter(|p| !p.meta.doi.is_empty() && !own_keys.contains(&text::doi_key(&p.meta.doi))).take(8) {
        let title: String = p.meta.title.chars().take(80).collect();
        jobs.push(job(WatchKind::Citing, &p.meta.doi, format!("Cites “{title}” in your library."), false, 10));
    }
    let fetched: Vec<std::result::Result<Vec<Found>, String>> = parallel(&jobs, 4, |j| {
        let hits = match &j.not_by {
            Some(a) => r.finder.citing_new(&j.value, a, &since, j.limit),
            None => r.finder.watch_new(j.kind, &j.value, &since, j.limit),
        };
        hits.map(|h| h.into_iter().map(|hit| Found { hit, source: j.source.clone(), explicit: j.explicit }).collect::<Vec<_>>()).map_err(said)
    });
    // a source that can't be reached is said, not taken for a day without new papers
    let errors: Vec<String> = fetched.iter().filter_map(|f| f.as_ref().err().cloned()).collect();
    if !jobs.is_empty() && errors.len() == jobs.len() {
        let why = errors[0].clone();
        note_problem(r.files, &why, true, r.now)?;
        report.problem = Some(why);
        report.failed = true;
        return Ok(report);
    }
    let mut problems: Vec<String> = vec![];
    if let Some(e) = errors.first() {
        problems.push(format!("{} of {} OpenAlex requests failed: {e}", errors.len(), jobs.len()));
    }
    let mut pool: Vec<Found> = fetched.into_iter().filter_map(std::result::Result::ok).flatten().collect();
    if !state.keywords.is_empty() && state.preprints != Preprints::None {
        let from = time::utc_date(r.now - if first { 4 } else { 2 } * time::DAY_MS);
        let to = time::utc_date(r.now);
        for (server, label) in [("biorxiv", "bioRxiv"), ("medrxiv", "medRxiv")] {
            match preprints(r.finder, server, &from, &to, &state.keywords) {
                Ok(hits) => pool.extend(hits.into_iter().map(|hit| Found { hit, source: label.to_string(), explicit: false })),
                Err(e) => problems.push(format!("Couldn't reach {label}: {e}")),
            }
        }
    }
    let problem = problems.join(" ");
    note_problem(r.files, &problem, false, r.now)?;
    report.problem = (!problem.is_empty()).then_some(problem);
    // jobs finish in any order: keep the order they were asked in, as the Swift does
    let job_order: HashMap<&str, usize> = jobs.iter().enumerate().map(|(i, j)| (j.source.as_str(), i)).collect();
    pool.sort_by_key(|f| *job_order.get(f.source.as_str()).unwrap_or(&usize::MAX));
    // which are in the library already (by DOI or title)
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let titles: HashMap<String, String> = every.iter().map(|p| (norm(&p.meta.title), p.id.clone())).filter(|(t, _)| t.len() >= 12).collect();
    for f in pool.iter_mut() {
        if f.hit.in_library.is_none() {
            f.hit.in_library = (!f.hit.meta.doi.is_empty()).then(|| r.lib.find_by_doi(f.hit.meta.doi.clone()).map(|p| p.id)).flatten().or_else(|| titles.get(&norm(&f.hit.meta.title)).cloned());
        }
    }
    if r.stopped() {
        return Ok(report);
    }

    // 3. new to you: not in the library, never offered before, not your own
    let candidates = candidates(pool, &state, me.as_ref(), &own_keys);
    report.candidates = candidates.len() as u32;
    if candidates.is_empty() {
        r.files.change(|s| s.last_run = Some(r.now / 1000 * 1000))?;
        return Ok(report);
    }

    // 4. ranking
    (r.progress)(&format!("Ranking {} papers…", candidates.len()));
    let mut library: Vec<String> = every.iter().take(400).map(|p| text::snip(&p.meta.title, &p.meta.abstract_text)).collect();
    // your own papers count like papers in your library
    library.extend(me.iter().flat_map(|m| m.papers.iter().map(|p| text::snip(&p.title, &p.r#abstract))));
    let texts: Vec<String> = candidates.iter().map(|c| text::snip(&c.hit.meta.title, &c.hit.meta.abstract_text)).collect();
    let snips = |l: &[FeedbackSnip]| l.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
    let scores = rank::score(&texts, &interests, &library, &snips(&state.liked), &snips(&state.disliked), &snips(&state.boosted));
    let mut by_source: HashMap<String, usize> = HashMap::new();
    for c in &candidates {
        let k = if c.source.starts_with("search") {
            "searches".to_string()
        } else if text::cites_yours(&c.source) {
            "citing your papers".to_string()
        } else if c.source.starts_with("Cites") {
            "citing library".to_string()
        } else {
            c.source.clone()
        };
        *by_source.entry(k).or_default() += 1;
    }
    let mut parts: Vec<String> = by_source.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    parts.sort();
    report.stats = format!("new candidates {} ({})", candidates.len(), parts.join(", "));
    let mut ranked: Vec<(Found, f64)> = candidates
        .into_iter()
        .zip(scores)
        .map(|(c, s)| {
            // closeness, a little more for a paper in a top journal or already cited (impact.rs)
            let s = impact::weigh(s, impact::impact(&c.hit.meta.journal, c.hit.cited_by, 0.0));
            let s = s + if c.explicit { 0.15 } else { 0.0 } + if text::from_journal(&c.source) { 0.06 } else { 0.0 } + rank::recency(&c.hit.meta.date, r.now, r.tz_min);
            (c, s)
        })
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // 5. the language model reads the best abstracts
    let (picked, turned_down, screened) = pick(r, &state, &ranked);
    report.stats += &format!("\nscreened {screened}: picked {}, turned down {}", picked.len(), turned_down.len());

    // 6. the feed: the new picks on top, best first
    let mut fresh = picked;
    fresh.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    report.found = fresh.len() as u32;
    r.files.change(|s| {
        let keys: HashSet<&str> = fresh.iter().map(|f| f.key.as_str()).collect();
        let old: Vec<FeedRecord> = s.items.iter().filter(|o| !keys.contains(o.key.as_str())).cloned().collect();
        s.items = fresh.iter().cloned().chain(old).take(80).collect();
        s.shown.extend(fresh.iter().map(|f| f.key.clone()).chain(turned_down.iter().cloned()));
        keep_last(&mut s.shown, 3000);
        s.last_run = Some(r.now / 1000 * 1000);
    })?;
    if let Some(w) = r.writer {
        summarize(r.files, w, r.tz_min, r.stop);
    }
    Ok(report)
}

/// An error as the apps say it (a network error's message without "network error: ").
fn said(e: crate::model::CoreError) -> String {
    match e {
        crate::model::CoreError::Network(m) => m,
        e => e.to_string(),
    }
}

/// What went wrong in this device's latest run ("" when nothing did), kept on this device: another
/// device may well reach the sources.
fn note_problem(files: &Files, problem: &str, failed: bool, now: i64) -> Result<()> {
    let mut l = files.local();
    if l.problem.is_empty() && problem.is_empty() {
        return Ok(());
    }
    l.problem = problem.to_string();
    l.failed = failed && !problem.is_empty();
    l.problem_at = now;
    files.save_local(&l)
}

/// Step 3: the papers new to you, a paper found twice keeping its strongest reason (citing your
/// paper, then other follows).
fn candidates(pool: Vec<Found>, state: &ForYouState, me: Option<&OrcidLink>, own_keys: &HashSet<String>) -> Vec<Found> {
    let offered: HashSet<&str> = state.shown.iter().map(String::as_str).chain(state.items.iter().map(|i| i.key.as_str())).collect();
    let (yours, rest): (Vec<Found>, Vec<Found>) = pool.into_iter().partition(|c| text::cites_yours(&c.source));
    let (explicit, open): (Vec<Found>, Vec<Found>) = rest.into_iter().partition(|c| c.explicit);
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = vec![];
    for c in yours.into_iter().chain(explicit).chain(open) {
        let key = hit_key(&c.hit);
        if key.is_empty() || c.hit.in_library.is_some() || offered.contains(key.as_str()) || seen.contains(&key) || c.hit.meta.title.chars().count() <= 10 {
            continue;
        }
        if own_keys.contains(&key) || me.is_some_and(|m| m.is_mine(&c.hit.meta.authors)) {
            continue;
        }
        if not_a_paper(&c.hit.meta.journal) {
            continue;
        }
        if !c.explicit && c.hit.meta.abstract_text.chars().count() < 300 {
            continue;
        }
        seen.insert(key);
        out.push(c);
    }
    out
}

/// Step 5: what you follow goes in; the best of the rest are read by the model (or trusted).
/// Returns (picked, keys turned down, how many were screened).
fn pick(r: &Run, state: &ForYouState, ranked: &[(Found, f64)]) -> (Vec<FeedRecord>, Vec<String>, usize) {
    let mut picked: Vec<FeedRecord> = vec![];
    let mut turned_down: Vec<String> = vec![];
    let explicit: Vec<&(Found, f64)> = ranked.iter().filter(|(c, _)| c.explicit).collect();
    let ordered = explicit.iter().filter(|(c, _)| text::cites_yours(&c.source)).chain(explicit.iter().filter(|(c, _)| !text::cites_yours(&c.source)));
    for (c, s) in ordered.take(12) {
        picked.push(FeedRecord::from_hit(&c.hit, &c.source, *s, &c.source, r.now));
    }
    // the best papers from journals you follow come first (and pass at a lower bar), then the rest
    let journal: Vec<&(Found, f64)> = ranked.iter().filter(|(c, _)| !c.explicit && text::from_journal(&c.source)).take(12).collect();
    let others = ranked.iter().filter(|(c, _)| !c.explicit && !text::from_journal(&c.source)).take(40 - journal.len());
    let open: Vec<&(Found, f64)> = journal.iter().copied().chain(others).collect();
    let cap = state.preprints.cap(state.per_day);
    let mut preprints_kept = 0;
    let mut near_misses: Vec<(FeedRecord, f64)> = vec![]; // scored 6: used on a quiet day
    let judge = if state.interests.is_empty() { None } else { r.writer };
    let boosted: Vec<String> = state.items.iter().filter(|i| i.boosted == Some(true)).map(|i| i.title.clone()).collect();
    let examples: Vec<String> = boosted[boosted.len().saturating_sub(8)..].to_vec();
    let mut kept = 0;
    for (i, (c, s)) in open.iter().map(|x| (&x.0, x.1)).enumerate() {
        if kept >= state.per_day || r.stopped() {
            break;
        }
        let preprint = text::is_preprint(&c.hit, &c.source);
        if preprint && preprints_kept >= cap {
            continue;
        }
        if let Some(judge) = judge {
            (r.progress)(&format!("Reading abstracts ({} of {})…", i + 1, open.len()));
            let Some((score, why)) = judge.answer(text::SCREEN_INSTRUCTIONS, &text::screen_prompt(&c.hit, &state.interests, &examples), 80).as_deref().and_then(text::parse_screen) else {
                continue;
            };
            if score >= if text::from_journal(&c.source) { 5.0 } else { 7.0 } {
                picked.push(FeedRecord::from_hit(&c.hit, &why, s, &c.source, r.now));
                kept += 1;
                if preprint {
                    preprints_kept += 1;
                }
            } else {
                if score >= 6.0 {
                    near_misses.push((FeedRecord::from_hit(&c.hit, &why, s, &c.source, r.now), s));
                }
                turned_down.push(hit_key(&c.hit));
            }
        } else {
            // no language model here: trust the ranking
            let why = if c.source.starts_with("search") || c.source.starts_with("your topic") || c.source.ends_with("Rxiv") {
                format!("Close to your interests (from {}).", c.source)
            } else {
                c.source.clone()
            };
            picked.push(FeedRecord::from_hit(&c.hit, &why, s, &c.source, r.now));
            kept += 1;
            if preprint {
                preprints_kept += 1;
            }
        }
    }
    // a quiet day: the best near misses, up to three papers in all
    near_misses.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let room = 3usize.saturating_sub(kept as usize);
    let fill: Vec<FeedRecord> = near_misses
        .into_iter()
        .filter(|(rec, _)| !text::is_preprint(&text::record_hit(rec), &rec.source) || preprints_kept < cap)
        .take(room)
        .map(|x| x.0)
        .collect();
    for rec in fill {
        turned_down.retain(|k| *k != rec.key);
        picked.push(rec);
    }
    (picked, turned_down, open.len())
}

/// One sentence for each paper without one (the first 30), then a few sentences on the day's
/// papers. Made by whichever device gets there first; both sync with the feed. Returns how many
/// summaries were written.
pub fn summarize(files: &Files, w: &dyn Writer, tz_min: i32, stop: Option<&AtomicBool>) -> u32 {
    let stopped = || stop.is_some_and(|s| s.load(Ordering::SeqCst));
    let state = files.state();
    let mut n = 0;
    for r in state.items.iter().take(30).filter(|r| r.summary.is_none() && !r.r#abstract.is_empty()) {
        if stopped() {
            return n;
        }
        let Some(line) = w.answer(text::SUMMARY_INSTRUCTIONS, &text::summary_prompt(&r.title, &r.r#abstract), 90).as_deref().and_then(text::clean_summary) else {
            continue;
        };
        let _ = files.change(|s| {
            if let Some(i) = s.items.iter_mut().find(|i| i.key == r.key) {
                i.summary = Some(line.clone());
            }
        });
        n += 1;
    }
    // the day's papers in a few sentences
    let state = files.state();
    let (batch, key) = (state.digest_batch(tz_min), state.digest_key(tz_min));
    if state.digest_for.as_deref() == Some(key.as_str()) || batch.is_empty() || stopped() {
        return n;
    }
    if let Some(d) = w.answer(text::DIGEST_INSTRUCTIONS, &text::digest_prompt(&state.interests, &batch), 220).as_deref().and_then(|a| text::clean_digest(a, &batch)) {
        let _ = files.change(|s| {
            s.digest = d;
            s.digest_for = Some(key);
        });
    }
    n
}

/// Drafts the interests paragraph from your own papers (with your ORCID iD) and the library's
/// most recent papers.
pub fn draft_interests(lib: &Library, files: &Files, w: &dyn Writer) -> Option<String> {
    let papers = lib.papers(PaperFilter::default(), SortKey::Added);
    let state = files.state();
    let own: Vec<(String, String)> = state.orcid.iter().flat_map(|o| o.papers.iter().take(25).map(|p| (p.title.clone(), p.r#abstract.clone()))).collect();
    if papers.is_empty() && own.is_empty() {
        return None;
    }
    let saved: Vec<String> = papers.iter().take(if own.is_empty() { 60 } else { 30 }).map(|p| p.meta.title.clone()).collect();
    let a = w.answer(text::DRAFT_INSTRUCTIONS, &text::draft_prompt(&own, &saved), 300)?;
    let a = a.trim().to_string();
    (!a.is_empty()).then_some(a)
}

/// The morning notification for the latest feed: "New for You · 6 papers" and the digest's
/// first sentence (or the top titles), for a feed made since the last morning and not yet seen or
/// announced on this device.
pub fn morning_notice(s: &ForYouState, local: &LocalState, now: i64, tz_min: i32) -> Option<(String, String)> {
    let run = s.last_run?;
    if run < s.last_morning(now, tz_min) - 3 * time::HOUR_MS || run <= local.opened || run <= local.notified {
        return None;
    }
    let day = time::start_of_day(run, tz_min);
    let today: Vec<&FeedRecord> = s.items.iter().filter(|r| r.found >= day && r.in_library.is_none()).collect();
    if today.is_empty() {
        return None;
    }
    let mut body = String::new();
    if !s.digest.is_empty() {
        let plain = text::share_text(&s.digest, &[]).replacen("New for You (Lunta)\n\n", "", 1);
        let first = plain.split(". ").next().unwrap_or(&plain).to_string();
        body = if first.ends_with('.') { first } else { format!("{first}.") };
    }
    if body.chars().count() < 20 {
        body = today.iter().take(2).map(|r| r.title.as_str()).collect::<Vec<_>>().join(" · ");
    }
    let n = today.len();
    Some((format!("New for You · {n} paper{}", if n == 1 { "" } else { "s" }), body))
}
