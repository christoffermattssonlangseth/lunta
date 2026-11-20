//! A DOI for a paper that has none. An arXiv preprint's is its arXiv DOI (10.48550/arXiv.…, no
//! lookup; not for a published paper that also has an arXiv id). Otherwise Crossref's bibliographic search for the title, first author and year, and
//! a record is taken only when it is plainly the same work: its title is the paper's
//! (`refcite::title_in` both ways, or the same title with a subtitle left out), the years are at
//! most one apart, the first author is among its authors, and a published paper isn't given a
//! preprint's DOI. The record's details come along, to fill details the paper lacks.

use super::DoiSuggestion;
use crate::checks::net::Net;
use crate::checks::refcheck::crossref_search;
use crate::checks::{self};
use crate::model::{PaperMeta, Result};
use crate::{metadata, refcite};

fn year(m: &PaperMeta) -> Option<i32> {
    m.year().parse().ok()
}

/// The two titles are one: each is (nearly) all in the other, or one is the other without its subtitle.
pub(crate) fn same_work_title(a: &str, b: &str) -> bool {
    checks::same_title(a, b) || (refcite::title_in(a, b) && refcite::title_in(b, a))
}

/// Whether a record found is this paper.
pub(crate) fn is_this(m: &PaperMeta, f: &PaperMeta) -> bool {
    if f.doi.is_empty() || !same_work_title(&m.title, &f.title) {
        return false;
    }
    if let (Some(a), Some(b)) = (year(m), year(f)) {
        if (a - b).abs() > 1 {
            return false;
        }
    }
    if let Some(first) = m.authors.first() {
        let me = checks::family_key(first);
        if !me.is_empty() && !f.authors.is_empty() && !f.authors.iter().any(|a| checks::family_key(a) == me) {
            return false;
        }
    }
    // a published paper never gets its preprint's DOI; reviews and parts of a work aren't the work
    let found_preprint = metadata::is_preprint_doi(&f.doi) || f.work_type == "posted-content";
    if found_preprint && !checks::is_preprint(m) && !m.journal.is_empty() {
        return false;
    }
    !matches!(f.work_type.as_str(), "peer-review" | "component" | "grant")
}

/// The search for a paper: its title, its first author's family name and its year.
pub(crate) fn query(m: &PaperMeta) -> String {
    let mut q = crate::metadata::strip_tags(&m.title);
    if let Some(a) = m.authors.first() {
        q.push(' ');
        q.push_str(&crate::export::split_name(a).0);
    }
    if !m.year().is_empty() {
        q.push(' ');
        q.push_str(&m.year());
    }
    q
}

/// A DOI for this paper, with the record it comes from; None when nothing is plainly it.
/// Err only when Crossref couldn't be asked.
pub(crate) fn find(net: &dyn Net, id: &str, m: &PaperMeta) -> Result<Option<DoiSuggestion>> {
    // the arXiv DOI only for the preprint itself: a published paper with an arXiv id is searched for
    if !m.arxiv.trim().is_empty() && (checks::is_preprint(m) || m.journal.trim().is_empty()) {
        let id_ = m.arxiv.trim().trim_start_matches("arXiv:");
        let bare = match id_.rfind('v') {
            Some(i) if i + 1 < id_.len() && id_[i + 1..].chars().all(|c| c.is_ascii_digit()) => &id_[..i],
            _ => id_,
        };
        let doi = format!("10.48550/arXiv.{bare}");
        return Ok(Some(DoiSuggestion { paper_id: id.to_string(), doi, found: PaperMeta::default(), source: "arXiv".into(), duplicate_of: None }));
    }
    if checks::title_words(&m.title).len() < 3 {
        return Ok(None);
    }
    for f in crossref_search(net, &query(m))? {
        if is_this(m, &f.meta) {
            return Ok(Some(DoiSuggestion { paper_id: id.to_string(), doi: f.meta.doi.clone(), found: f.meta, source: "Crossref".into(), duplicate_of: None }));
        }
    }
    Ok(None)
}

/// The paper's details with the DOI and whatever it lacks from the record (nothing it has is
/// changed).
pub(crate) fn with_doi(m: &PaperMeta, s: &DoiSuggestion) -> PaperMeta {
    let mut out = m.clone();
    out.doi = s.doi.clone();
    crate::merge_meta(&mut out, &s.found);
    out
}
