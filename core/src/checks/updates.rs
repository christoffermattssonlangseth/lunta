//! Retractions, corrections and published versions of the papers in the library.
//!
//! **Notices.** Crossref keeps update relations: a retraction, correction or expression of concern
//! deposited by the publisher, and, since Crossref acquired it (2023), every record of the
//! Retraction Watch database, as `updated-by` on the work (`source`: "publisher" or
//! "retraction-watch"; the notice has the reverse, `update-to`; `filter=update-type:retraction`
//! lists notices). The library's DOIs are asked 20 at a time (`filter=doi:…,doi:…` with
//! `select=DOI,type,updated-by,relation`).
//!
//! **Published versions** of preprints (`posted-content`, arXiv, bioRxiv, medRxiv… DOIs), in this
//! order: Crossref's `is-preprint-of` on the preprint, or `has-preprint` on the published article
//! (asked in reverse, `filter=relation.object:…`); bioRxiv's and medRxiv's own API
//! (`api.biorxiv.org/pubs/…`: the published DOI); arXiv's `doi` and `journal_ref`; OpenAlex's
//! locations (a published version in a journal); last, Crossref's search for the title, taken only
//! when the title is the same, the first author is the same and the year isn't earlier.
//! A published paper is never offered its preprint, and a version found must not be a preprint.
//!
//! What is found is kept with the paper (`OpKind::SetUpdates`) only when it changed. The apps
//! check on demand and once a week (`check_paper_updates_if_due`); answers are kept six days in
//! the device's cache, so devices sharing a library ask about what's new only.

use super::net::{Cached, LiveNet, Net};
use super::refcheck::{arxiv_records, crossref_doi, crossref_search, say};
use super::*;
use crate::model::*;
use crate::store::OpKind;
use crate::{metadata, Library, ProgressListener};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// What a check of the library found.
#[derive(uniffi::Record, serde::Serialize, Clone, Debug, Default, PartialEq)]
pub struct UpdatesSummary {
    /// Papers asked about.
    pub checked: u32,
    /// Papers that are retracted, withdrawn or removed (all of them, not only new ones).
    pub retracted: Vec<String>,
    /// With an expression of concern.
    pub concern: Vec<String>,
    /// Corrected.
    pub corrected: Vec<String>,
    /// Preprints with a published version to suggest.
    pub published: Vec<String>,
    /// Papers whose notices or published version are new since the last check.
    pub changed: Vec<String>,
    /// Services that couldn't be asked.
    pub errors: Vec<String>,
    /// "Checked 240 papers: 1 retracted, 2 corrected; 3 preprints have been published."
    pub text: String,
}

/// A paper as the check sees it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Subject {
    pub id: String,
    pub meta: PaperMeta,
    pub current: PaperUpdates,
}

const BATCH: usize = 20;

fn ndoi(d: &str) -> String {
    metadata::normalize_doi(d)
}

/// A preprint DOI without its version (".v2" at figshare-based servers).
fn unversioned(d: &str) -> String {
    let d = ndoi(d);
    match d.rfind(".v") {
        Some(i) if d[i + 2..].chars().all(|c| c.is_ascii_digit()) && i + 2 < d.len() => d[..i].to_string(),
        _ => d,
    }
}

/// Whether a published version may be offered for this paper: it's a preprint, not switched yet,
/// and has a DOI or arXiv id to look it up by.
fn wants_published(s: &Subject) -> bool {
    is_preprint(&s.meta) && s.current.preprint.is_none() && s.current.linked.is_none() && (!s.meta.doi.is_empty() || !s.meta.arxiv.is_empty())
}

fn json(body: &str) -> Result<Value> {
    serde_json::from_str(body).map_err(|e| CoreError::Network(format!("bad JSON: {e}")))
}

/// A version found is a published one: not a preprint server's DOI, not posted content.
fn acceptable(doi: &str, work_type: &str) -> bool {
    !doi.is_empty() && !metadata::is_preprint_doi(doi) && work_type != "posted-content" && work_type != "peer-review"
}

/// Looks for notices on every paper with a DOI and for the published version of every preprint.
/// Returns each paper's new updates (only those that changed) and the errors met.
pub(crate) fn find_updates(net: &dyn Net, subjects: &[Subject], progress: &dyn Fn(String)) -> (Vec<(String, PaperUpdates)>, Vec<String>) {
    let mut errors: Vec<String> = vec![];
    let mut err = |e: CoreError| {
        let m = e.to_string();
        if !errors.contains(&m) {
            errors.push(m);
        }
    };
    // DOI -> (notices, is-preprint-of DOIs); a DOI Crossref doesn't have isn't in it
    let mut crossref: HashMap<String, (Vec<EditorialNotice>, Vec<String>)> = HashMap::new();
    let dois: Vec<String> = {
        let mut v: Vec<String> = subjects.iter().map(|s| ndoi(&s.meta.doi)).filter(|d| !d.is_empty() && !d.contains(',') && !d.starts_with("10.48550/")).collect();
        v.sort();
        v.dedup();
        v
    };
    let batches = dois.chunks(BATCH).count();
    for (n, chunk) in dois.chunks(BATCH).enumerate() {
        progress(format!("Checking for retractions and corrections ({} of {batches})…", n + 1));
        let filter = chunk.iter().map(|d| format!("doi:{}", metadata::enc(d))).collect::<Vec<_>>().join(",");
        let url = format!("https://api.crossref.org/works?filter={filter}&rows={}&select=DOI,type,updated-by,relation", chunk.len());
        match net.get(&url).and_then(|b| b.map(|b| json(&b)).transpose()) {
            Ok(Some(j)) => {
                for m in j["message"]["items"].as_array().into_iter().flatten() {
                    let preprint_of: Vec<String> = m["relation"]["is-preprint-of"].as_array().into_iter().flatten().filter(|r| s(&r["id-type"]) == "doi").map(|r| ndoi(&s(&r["id"]))).collect();
                    crossref.insert(ndoi(&s(&m["DOI"])), (crossref_notices(m), preprint_of));
                }
            }
            Ok(None) => {}
            Err(e) => err(e),
        }
    }

    // published versions of preprints: (paper id) -> version
    // (in a fixed order, so the same library asks the same questions: answers are cached by URL)
    let mut preprints: Vec<&Subject> = subjects.iter().filter(|s| wants_published(s)).collect();
    preprints.sort_by_key(|s| (ndoi(&s.meta.doi), s.meta.arxiv.clone(), s.id.clone()));
    let mut found: HashMap<String, PublishedVersion> = HashMap::new();
    let declined = |s: &Subject, doi: &str| !s.current.declined_doi.is_empty() && ndoi(&s.current.declined_doi) == ndoi(doi);
    let take = |found: &mut HashMap<String, PublishedVersion>, s: &Subject, v: PublishedVersion| {
        if !found.contains_key(&s.id) && acceptable(&v.doi, "") && !declined(s, &v.doi) && ndoi(&v.doi) != ndoi(&s.meta.doi) {
            found.insert(s.id.clone(), v);
        }
    };
    // 1. is-preprint-of on the preprint
    for s in &preprints {
        if let Some((_, of)) = crossref.get(&ndoi(&s.meta.doi)) {
            if let Some(d) = of.first() {
                take(&mut found, s, PublishedVersion { doi: d.clone(), found_by: "Crossref (is-preprint-of)".into(), ..Default::default() });
            }
        }
    }
    // 2. has-preprint on the published article, asked in reverse
    let ask: Vec<&Subject> = preprints.iter().copied().filter(|s| !found.contains_key(&s.id) && !s.meta.doi.is_empty()).collect();
    for (n, chunk) in ask.chunks(BATCH).enumerate() {
        progress(format!("Looking for published versions of preprints ({} of {})…", n + 1, ask.chunks(BATCH).count()));
        let filter = chunk.iter().map(|s| format!("relation.object:{}", metadata::enc(&ndoi(&s.meta.doi)))).collect::<Vec<_>>().join(",");
        let url = format!("https://api.crossref.org/works?filter={filter}&rows=40&select=DOI,type,title,container-title,issued,relation");
        match net.get(&url).and_then(|b| b.map(|b| json(&b)).transpose()) {
            Ok(Some(j)) => {
                for m in j["message"]["items"].as_array().into_iter().flatten() {
                    let ty = s(&m["type"]);
                    let doi = s(&m["DOI"]);
                    if !acceptable(&doi, &ty) {
                        continue;
                    }
                    let of: Vec<String> = m["relation"]["has-preprint"].as_array().into_iter().flatten().map(|r| unversioned(&s(&r["id"]))).collect();
                    for sub in chunk {
                        if of.contains(&unversioned(&sub.meta.doi)) {
                            let v = PublishedVersion {
                                doi: doi.clone(),
                                title: metadata::strip_tags(&s(&m["title"])),
                                journal: s(&m["container-title"]),
                                date: date_parts(&m["issued"]),
                                found_by: "Crossref (has-preprint)".into(),
                            };
                            take(&mut found, sub, v);
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => err(e),
        }
    }
    // 3. bioRxiv and medRxiv
    let rxiv: Vec<&Subject> = preprints.iter().copied().filter(|p| !found.contains_key(&p.id)).collect();
    for sub in rxiv {
        let d = ndoi(&sub.meta.doi);
        if !(d.starts_with("10.1101/") || d.starts_with("10.64898/")) {
            continue;
        }
        let server = if sub.meta.journal.to_lowercase().contains("medrxiv") { "medrxiv" } else { "biorxiv" };
        progress(format!("Asking {} about “{}”…", if server == "medrxiv" { "medRxiv" } else { "bioRxiv" }, sub.meta.title.chars().take(60).collect::<String>()));
        match net.get(&format!("https://api.biorxiv.org/pubs/{server}/{d}")).and_then(|b| b.map(|b| json(&b)).transpose()) {
            Ok(Some(j)) => {
                let c = &j["collection"][0];
                let pd = s(&c["published_doi"]);
                if !pd.is_empty() && pd != "NA" {
                    let v = PublishedVersion { doi: pd, journal: s(&c["published_journal"]), date: s(&c["published_date"]), found_by: if server == "medrxiv" { "medRxiv".into() } else { "bioRxiv".into() }, ..Default::default() };
                    take(&mut found, sub, v);
                }
            }
            Ok(None) => {}
            Err(e) => err(e),
        }
    }
    // 4. arXiv: the DOI or journal reference the authors gave
    let arx: Vec<&Subject> = preprints
        .iter()
        .copied()
        .filter(|s| !found.contains_key(&s.id))
        .filter(|s| !arxiv_id(&s.meta).is_empty())
        .collect();
    let mut journal_refs: HashMap<String, String> = HashMap::new();
    for chunk in arx.chunks(BATCH) {
        progress("Asking arXiv about preprints…".into());
        let ids: Vec<String> = chunk.iter().map(|s| arxiv_id(&s.meta)).collect();
        match arxiv_records(net, &ids) {
            Ok(list) => {
                for (id, meta, jref) in list {
                    for s in chunk.iter().filter(|s| arxiv_id(&s.meta) == id) {
                        if !meta.doi.is_empty() && !metadata::is_preprint_doi(&meta.doi) {
                            take(&mut found, s, PublishedVersion { doi: meta.doi.clone(), journal: jref.clone(), found_by: "arXiv".into(), ..Default::default() });
                        } else if !jref.is_empty() {
                            journal_refs.insert(s.id.clone(), jref.clone());
                        }
                    }
                }
            }
            Err(e) => err(e),
        }
    }
    // 5. OpenAlex: a published version among the work's locations
    let oa: Vec<&Subject> = preprints.iter().copied().filter(|s| !found.contains_key(&s.id) && !s.meta.doi.is_empty() && !s.meta.doi.contains('|')).collect();
    for chunk in oa.chunks(BATCH) {
        progress("Asking OpenAlex about preprints…".into());
        let filter = chunk.iter().map(|s| metadata::enc(&ndoi(&s.meta.doi))).collect::<Vec<_>>().join("|");
        let url = format!("https://api.openalex.org/works?filter=doi:{filter}&per_page={}&select=doi,title,locations", chunk.len());
        match net.get(&url).and_then(|b| b.map(|b| json(&b)).transpose()) {
            Ok(Some(j)) => {
                for w in j["results"].as_array().into_iter().flatten() {
                    let wd = ndoi(&s(&w["doi"]));
                    let Some(sub) = chunk.iter().find(|s| ndoi(&s.meta.doi) == wd) else { continue };
                    for l in w["locations"].as_array().into_iter().flatten() {
                        let kind = s(&l["source"]["type"]);
                        let landing = s(&l["landing_page_url"]);
                        let d = metadata::doi_in(&landing).unwrap_or_default();
                        if s(&l["version"]) == "publishedVersion" && (kind == "journal" || kind == "conference") && acceptable(&d, "") && ndoi(&d) != wd {
                            take(&mut found, sub, PublishedVersion { doi: d, journal: s(&l["source"]["display_name"]), found_by: "OpenAlex".into(), ..Default::default() });
                            break;
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => err(e),
        }
    }
    // 6. Crossref's search by title and first author (strict)
    let left: Vec<&Subject> = preprints.iter().copied().filter(|p| !found.contains_key(&p.id)).collect();
    for s in left {
        if title_words(&s.meta.title).len() < 4 || s.meta.authors.is_empty() {
            continue;
        }
        progress(format!("Searching Crossref for “{}”…", s.meta.title.chars().take(60).collect::<String>()));
        let first = crate::export::split_name(&s.meta.authors[0]).0;
        let q = format!("{} {first} {}", s.meta.title, journal_refs.get(&s.id).cloned().unwrap_or_default());
        match crossref_search(net, &q) {
            Ok(list) => {
                let y = s.meta.year().parse::<i32>().ok();
                for f in list {
                    let m = &f.meta;
                    let later = match (y, m.year().parse::<i32>().ok()) {
                        (Some(a), Some(b)) => b >= a,
                        _ => true,
                    };
                    let same = (same_title(&s.meta.title, &m.title) || title_similarity(&s.meta.title, &m.title) >= 0.9)
                        && m.authors.first().is_some_and(|a| family_key(a) == family_key(&s.meta.authors[0]));
                    if same && later && acceptable(&m.doi, &m.work_type) && matches!(crate::worktype::kind(m), crate::worktype::Kind::Article | crate::worktype::Kind::Conference | crate::worktype::Kind::Chapter) {
                        take(&mut found, s, PublishedVersion { doi: m.doi.clone(), title: m.title.clone(), journal: m.journal.clone(), date: m.date.clone(), found_by: "Crossref search (title and first author)".into() });
                        break;
                    }
                }
            }
            Err(e) => err(e),
        }
    }
    // details of versions found by DOI alone
    for (id, v) in found.iter_mut() {
        let known = subjects.iter().find(|s| &s.id == id).and_then(|s| s.current.published.clone()).filter(|p| ndoi(&p.doi) == ndoi(&v.doi));
        if let Some(p) = known.filter(|p| !p.title.is_empty()) {
            v.title = p.title;
            v.journal = if v.journal.is_empty() { p.journal } else { v.journal.clone() };
            v.date = if v.date.is_empty() { p.date } else { v.date.clone() };
            continue;
        }
        if v.title.is_empty() || v.date.is_empty() {
            match crossref_doi(net, &v.doi) {
                Ok(Some(f)) => {
                    if !acceptable(&f.meta.doi, &f.meta.work_type) {
                        v.doi.clear();
                        continue;
                    }
                    v.title = f.meta.title;
                    if !f.meta.journal.is_empty() {
                        v.journal = f.meta.journal;
                    }
                    if !f.meta.date.is_empty() {
                        v.date = f.meta.date;
                    }
                }
                Ok(None) => {}
                Err(e) => err(e),
            }
        }
    }
    found.retain(|_, v| !v.doi.is_empty());

    let mut out = vec![];
    for s in subjects {
        let mut u = s.current.clone();
        if let Some((notices, _)) = crossref.get(&ndoi(&s.meta.doi)) {
            u.notices = notices.clone();
        }
        if wants_published(s) {
            if let Some(v) = found.get(&s.id) {
                u.published = Some(v.clone());
            }
        }
        if u != s.current {
            out.push((s.id.clone(), u));
        }
    }
    (out, errors)
}

fn arxiv_id(m: &PaperMeta) -> String {
    if !m.arxiv.is_empty() {
        return m.arxiv.clone();
    }
    let d = ndoi(&m.doi);
    d.strip_prefix("10.48550/arxiv.").map(String::from).unwrap_or_default()
}

fn ids_with(all: &HashMap<String, PaperUpdates>, f: impl Fn(&PaperUpdates) -> bool) -> Vec<String> {
    let mut v: Vec<String> = all.iter().filter(|(_, u)| f(u)).map(|(id, _)| id.clone()).collect();
    v.sort();
    v
}

fn summary_text(r: &UpdatesSummary) -> String {
    let mut parts = vec![];
    if !r.retracted.is_empty() {
        parts.push(format!("{} retracted", r.retracted.len()));
    }
    if !r.concern.is_empty() {
        parts.push(format!("{} with an expression of concern", r.concern.len()));
    }
    if !r.corrected.is_empty() {
        parts.push(format!("{} corrected", r.corrected.len()));
    }
    let mut s = format!("Checked {} paper{}", r.checked, if r.checked == 1 { "" } else { "s" });
    s += &if parts.is_empty() { ": no retractions or corrections".to_string() } else { format!(": {}", parts.join(", ")) };
    if !r.published.is_empty() {
        s += &format!("; {} preprint{} been published", r.published.len(), if r.published.len() == 1 { " has" } else { "s have" });
    }
    s += ".";
    if !r.errors.is_empty() {
        s += &format!(" (Not everything could be asked: {}.)", r.errors.join("; "));
    }
    s
}

const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);

fn schedule_file(cache: &std::path::Path) -> PathBuf {
    cache.join("checks").join("schedule.json")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Library {
    fn subjects(&self, ids: &[String]) -> Vec<Subject> {
        let g = self.inner.lock().unwrap();
        g.state
            .papers
            .iter()
            .filter(|(id, _)| ids.is_empty() || ids.contains(id))
            .map(|(id, p)| Subject { id: id.clone(), meta: p.meta.clone(), current: g.state.updates.get(id).cloned().unwrap_or_default() })
            .collect()
    }

    /// The check with `net` (the services, or recorded answers in tests); writes what changed.
    pub(crate) fn check_updates_with(&self, net: &dyn Net, ids: &[String], progress: &dyn Fn(String)) -> Result<UpdatesSummary> {
        let subjects = self.subjects(ids);
        let (changed, errors) = find_updates(net, &subjects, progress);
        if !changed.is_empty() {
            let mut g = self.inner.lock().unwrap();
            for (id, u) in &changed {
                g.store.append(OpKind::SetUpdates { paper: id.clone(), updates: u.clone() })?;
            }
            g.state = g.store.replay()?;
        }
        let all = self.all_paper_updates();
        let only = |v: Vec<String>| -> Vec<String> { v.into_iter().filter(|id| ids.is_empty() || ids.contains(id)).collect() };
        let mut r = UpdatesSummary {
            checked: subjects.len() as u32,
            retracted: only(ids_with(&all, |u| level(&u.notices) == NoticeLevel::Retracted)),
            concern: only(ids_with(&all, |u| level(&u.notices) == NoticeLevel::Concern)),
            corrected: only(ids_with(&all, |u| level(&u.notices) == NoticeLevel::Corrected)),
            published: only(ids_with(&all, |u| u.published.is_some())),
            changed: changed.into_iter().map(|(id, _)| id).collect(),
            errors,
            text: String::new(),
        };
        r.text = summary_text(&r);
        Ok(r)
    }

    /// Switches a preprint to its published version (details already fetched): the published
    /// details, with the preprint's abstract, keywords and arXiv id where the published record has
    /// none. Highlights, notes, tags and the citation key stay with the paper. The preprint's
    /// details are kept for Undo.
    pub(crate) fn switch_to(&self, id: &str, published: PaperMeta) -> Result<Paper> {
        let p = self.paper(id.to_string()).ok_or_else(|| CoreError::NotFound(id.to_string()))?;
        if !is_preprint(&p.meta) {
            return Err(CoreError::Invalid("Only a preprint is switched to its published version.".into()));
        }
        if published.title.is_empty() || is_preprint(&published) || metadata::is_preprint_doi(&published.doi) {
            return Err(CoreError::Invalid("That version is a preprint too: nothing was changed.".into()));
        }
        let mut m = published;
        for (mine, theirs) in [(&mut m.abstract_text, &p.meta.abstract_text), (&mut m.arxiv, &p.meta.arxiv)] {
            if mine.is_empty() {
                *mine = theirs.clone();
            }
        }
        if m.keywords.is_empty() {
            m.keywords = p.meta.keywords.clone();
        }
        let mut u = self.paper_updates(id.to_string()).unwrap_or_default();
        u.preprint = Some(p.meta.clone());
        u.switched_at = now_ms();
        u.published = None;
        u.linked = None;
        u.notices.clear();
        {
            let mut g = self.inner.lock().unwrap();
            g.write(OpKind::SetMeta { paper: id.to_string(), meta: m })?;
            g.write(OpKind::SetUpdates { paper: id.to_string(), updates: u })?;
        }
        self.reindex()?;
        self.paper(id.to_string()).ok_or_else(|| CoreError::NotFound(id.to_string()))
    }

    fn set_updates(&self, id: &str, f: impl FnOnce(&mut PaperUpdates)) -> Result<()> {
        let mut u = self.paper_updates(id.to_string()).unwrap_or_default();
        f(&mut u);
        self.op(OpKind::SetUpdates { paper: id.to_string(), updates: u })
    }
}

#[uniffi::export]
impl Library {
    /// What the checks found for a paper (retractions, corrections, a published version), if anything.
    pub fn paper_updates(&self, id: String) -> Option<PaperUpdates> {
        self.inner.lock().unwrap().state.updates.get(&id).cloned()
    }

    /// Every paper with something found (for badges in the list and the smart folders).
    pub fn all_paper_updates(&self) -> HashMap<String, PaperUpdates> {
        let g = self.inner.lock().unwrap();
        g.state.updates.iter().filter(|(id, _)| g.state.papers.contains_key(*id)).map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Checks papers (`ids`; empty: the whole library) for retractions, corrections and
    /// expressions of concern, and preprints for a published version. Writes what changed (as a
    /// status of the paper; the paper's details aren't touched). Blocking: call off the main thread.
    pub fn check_paper_updates(&self, ids: Vec<String>, progress: Option<Arc<dyn ProgressListener>>) -> Result<UpdatesSummary> {
        let file = self.inner.lock().unwrap().cache.join("checks").join("updates.json");
        let live = LiveNet;
        // asked on demand: answers from the last hour only
        let net = Cached::new(&live, Duration::from_secs(3600), Some(file));
        let r = self.check_updates_with(&net, &ids, &say(&progress))?;
        if ids.is_empty() {
            let f = schedule_file(&self.inner.lock().unwrap().cache);
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            let _ = std::fs::write(f, format!("{{\"last_run\":{}}}", now_secs()));
        }
        Ok(r)
    }

    /// The weekly check: the whole library, if a week has passed since the last (on this
    /// device). Answers are kept six days, so after another device's check little is asked again.
    /// None when it isn't due.
    pub fn check_paper_updates_if_due(&self, progress: Option<Arc<dyn ProgressListener>>) -> Result<Option<UpdatesSummary>> {
        let cache = self.inner.lock().unwrap().cache.clone();
        let f = schedule_file(&cache);
        let last = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|j| j["last_run"].as_u64()).unwrap_or(0);
        if now_secs().saturating_sub(last) < WEEK.as_secs() || self.inner.lock().unwrap().state.papers.is_empty() {
            return Ok(None);
        }
        let live = LiveNet;
        let net = Cached::new(&live, Duration::from_secs(6 * 24 * 3600), Some(cache.join("checks").join("updates.json")));
        let r = self.check_updates_with(&net, &[], &say(&progress))?;
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(&f, format!("{{\"last_run\":{}}}", now_secs()));
        Ok(Some(r))
    }

    /// "Switch to the Published Version": the preprint's details become the published version's
    /// (looked up by its DOI), keeping highlights, notes, tags and the citation key. Only for a
    /// preprint with a published version found (or linked). Undo with `undo_published_version`.
    /// Blocking (a lookup).
    pub fn use_published_version(&self, id: String) -> Result<Paper> {
        let u = self.paper_updates(id.clone()).unwrap_or_default();
        let v = u.published.or(u.linked).ok_or_else(|| CoreError::NotFound("no published version found for this paper".into()))?;
        let meta = metadata::resolve(&v.doi)?;
        self.switch_to(&id, meta)
    }

    /// Undoes "Switch to the Published Version": the preprint's details again, and the published
    /// version suggested again.
    pub fn undo_published_version(&self, id: String) -> Result<Paper> {
        let u = self.paper_updates(id.clone()).unwrap_or_default();
        let pre = u.preprint.clone().ok_or_else(|| CoreError::NotFound("this paper wasn't switched to a published version".into()))?;
        let p = self.paper(id.clone()).ok_or_else(|| CoreError::NotFound(id.clone()))?;
        let v = PublishedVersion { doi: p.meta.doi.clone(), title: p.meta.title.clone(), journal: p.meta.journal.clone(), date: p.meta.date.clone(), found_by: "your switch (undone)".into() };
        {
            let mut g = self.inner.lock().unwrap();
            g.write(OpKind::SetMeta { paper: id.clone(), meta: pre })?;
            let mut nu = u;
            nu.preprint = None;
            nu.switched_at = 0;
            nu.published = Some(v);
            nu.notices.clear();
            g.write(OpKind::SetUpdates { paper: id.clone(), updates: nu })?;
        }
        self.reindex()?;
        self.paper(id.clone()).ok_or(CoreError::NotFound(id))
    }

    /// "Link Both": the paper stays the preprint, and shows its published version with it.
    pub fn link_published_version(&self, id: String) -> Result<()> {
        self.set_updates(&id, |u| {
            if let Some(v) = u.published.take() {
                u.linked = Some(v);
            }
        })
    }

    /// "Keep the Preprint": this published version isn't suggested again (nor linked).
    pub fn keep_preprint(&self, id: String) -> Result<()> {
        self.set_updates(&id, |u| {
            if let Some(v) = u.published.take().or(u.linked.take()) {
                u.declined_doi = v.doi;
            }
        })
    }

    /// Checks the references of a folder's papers (`None`: the whole library) against the
    /// published record: their details as the library has them. Blocking.
    pub fn check_folder_references(&self, collection: Option<String>, progress: Option<Arc<dyn ProgressListener>>) -> refcheck::RefCheckReport {
        let papers = self.papers(PaperFilter { collection, ..Default::default() }, SortKey::FirstAuthor);
        let refs: Vec<refcheck::RefInput> = papers
            .iter()
            .map(|p| refcheck::RefInput { key: p.cite_key.clone(), text: refcheck::one_line(&p.meta), meta: Some(p.meta.clone()), ..Default::default() })
            .collect();
        let cache = self.inner.lock().unwrap().cache.to_string_lossy().to_string();
        let mut r = refcheck::with_live_net(&cache, Duration::from_secs(24 * 3600), |net| refcheck::check_all(net, &refs, &say(&progress)));
        for (i, p) in r.items.iter_mut().zip(&papers) {
            i.in_library = Some(p.id.clone());
        }
        r
    }

    /// A paper's own reference list, from its reading view (empty when it has none, or the
    /// reading view isn't on this device yet: then the app reads the PDF's).
    pub fn paper_reference_list(&self, id: String) -> Vec<String> {
        let path = self.inner.lock().unwrap().store.file_path(&id, FileKind::Epub);
        refcheck::epub_references(&path)
    }

    /// Checks a paper's reference list (from its reading view). Entries that are in the library
    /// are marked. Blocking.
    pub fn check_paper_references(&self, id: String, progress: Option<Arc<dyn ProgressListener>>) -> Result<refcheck::RefCheckReport> {
        let list = self.paper_reference_list(id.clone());
        if list.is_empty() {
            return Err(CoreError::NotFound("no reference list in this paper's reading view".into()));
        }
        let cache = self.inner.lock().unwrap().cache.to_string_lossy().to_string();
        let mut r = refcheck::check_reference_entries(list, cache, progress);
        self.mark_in_library(&mut r);
        Ok(r)
    }

    /// Marks the references of a report that are papers in the library.
    pub fn mark_references_in_library(&self, report: refcheck::RefCheckReport) -> refcheck::RefCheckReport {
        let mut r = report;
        self.mark_in_library(&mut r);
        r
    }
}

impl Library {
    fn mark_in_library(&self, r: &mut refcheck::RefCheckReport) {
        for i in r.items.iter_mut() {
            if i.in_library.is_some() {
                continue;
            }
            if let Some(f) = &i.found {
                i.in_library = self.find_cited(f.doi.clone(), f.title.clone()).map(|p| p.id);
            }
        }
    }
}
