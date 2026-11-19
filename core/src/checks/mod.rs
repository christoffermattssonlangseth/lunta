//! Checks against the published record. Lunta proposes, the researcher decides: these report and
//! suggest; nothing in the library changes until the researcher confirms it, and then through the
//! change log like any other edit.
//!
//! - [`refcheck`]: **Check a Reference List**: does each reference exist (its DOI resolves at
//!   Crossref or DataCite, or Crossref, OpenAlex or PubMed find it, title and year checked), and do
//!   its details match the record? Verified, minor differences (which fields), could not be found,
//!   or its DOI points to a different work. Retracted works are flagged.
//! - [`updates`]: **retractions, corrections and expressions of concern** (Crossref's update
//!   relations, which include the Retraction Watch database) for the papers in the library, and
//!   **preprints that have been published** (Crossref relations, bioRxiv/medRxiv, arXiv,
//!   OpenAlex). Checked on demand and weekly, in batches, politely ([`net`]). What was found is
//!   kept per paper (`OpKind::SetUpdates`, which older versions skip) so every device shows it.

pub mod net;
pub mod refcheck;
pub mod updates;
#[cfg(test)]
mod tests;

use crate::model::PaperMeta;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

/// A retraction, correction, expression of concern… attached to a work by its publisher or by
/// Retraction Watch (Crossref's `updated-by`).
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct EditorialNotice {
    /// Crossref's update type: "retraction", "partial_retraction", "withdrawal", "removal",
    /// "expression_of_concern", "correction", "erratum", "corrigendum", "addendum", "clarification"…
    pub kind: String,
    /// As Crossref labels it ("Retraction", "Correction").
    pub label: String,
    /// The notice's DOI.
    pub doi: String,
    /// When the work was updated: YYYY-MM-DD (or shorter).
    pub date: String,
    /// "publisher", "Retraction Watch", or both.
    pub source: String,
}

/// How serious a paper's notices are, the most serious first.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoticeLevel {
    None,
    /// Corrected (correction, erratum, corrigendum, addendum, clarification).
    Corrected,
    /// An expression of concern.
    Concern,
    /// Retracted, withdrawn or removed (also partly retracted).
    Retracted,
}

/// The published version of a preprint.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PublishedVersion {
    pub doi: String,
    pub title: String,
    pub journal: String,
    /// YYYY-MM-DD (or shorter).
    pub date: String,
    /// How it was found: "Crossref (has-preprint)", "bioRxiv", "arXiv", "OpenAlex", "Crossref search".
    pub found_by: String,
}

/// What the checks found for a paper, kept in the change log (`OpKind::SetUpdates`; the latest
/// wins). Older versions skip the op, so they show nothing and lose nothing.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PaperUpdates {
    /// Retractions, corrections, expressions of concern.
    pub notices: Vec<EditorialNotice>,
    /// A preprint's published version, to suggest (None once switched, linked or kept).
    pub published: Option<PublishedVersion>,
    /// "Keep the Preprint": this published DOI isn't suggested again.
    pub declined_doi: String,
    /// "Link Both": the published version, shown with the preprint (which stays as it is).
    pub linked: Option<PublishedVersion>,
    /// After "Switch to the Published Version": the preprint's details before the switch (for
    /// undo, and to show where the paper came from).
    pub preprint: Option<PaperMeta>,
    /// When the paper was switched (ms), or 0.
    pub switched_at: i64,
}

impl PaperUpdates {
    pub fn is_empty(&self) -> bool {
        *self == PaperUpdates::default()
    }
}

/// How serious an update type is.
pub fn kind_level(kind: &str) -> NoticeLevel {
    match kind.to_lowercase().replace('-', "_").as_str() {
        "retraction" | "partial_retraction" | "withdrawal" | "removal" | "retracion" | "retration" => NoticeLevel::Retracted,
        "expression_of_concern" => NoticeLevel::Concern,
        "correction" | "erratum" | "err" | "corrigendum" | "addendum" | "clarification" | "corrected" | "corrected_article" | "publisher_note" => NoticeLevel::Corrected,
        _ => NoticeLevel::None,
    }
}

/// The most serious of a paper's notices.
#[uniffi::export]
pub fn updates_level(updates: PaperUpdates) -> NoticeLevel {
    level(&updates.notices)
}

pub fn level(notices: &[EditorialNotice]) -> NoticeLevel {
    notices.iter().map(|n| kind_level(&n.kind)).max().unwrap_or(NoticeLevel::None)
}

/// "6 February 2010" from "2010-02-06" ("February 2010", "2010").
pub fn long_date(d: &str) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let p: Vec<&str> = d.split('-').collect();
    let month = p.get(1).and_then(|m| m.parse::<usize>().ok()).filter(|m| (1..=12).contains(m)).map(|m| MONTHS[m - 1]);
    let day = p.get(2).and_then(|x| x.parse::<u32>().ok());
    match (day, month) {
        (Some(day), Some(m)) => format!("{day} {m} {}", p[0]),
        (None, Some(m)) => format!("{m} {}", p[0]),
        _ => p.first().unwrap_or(&"").to_string(),
    }
}

/// The notice that matters most (the most serious, then the latest).
pub fn main_notice(notices: &[EditorialNotice]) -> Option<&EditorialNotice> {
    notices.iter().filter(|n| kind_level(&n.kind) > NoticeLevel::None).max_by(|a, b| kind_level(&a.kind).cmp(&kind_level(&b.kind)).then(a.date.cmp(&b.date)))
}

/// The sentence the reader's banner and the reports show: "This paper was retracted on
/// 6 February 2010." Empty when there is nothing to say.
pub fn notice_sentence(notices: &[EditorialNotice], subject: &str) -> String {
    let Some(n) = main_notice(notices) else { return String::new() };
    let what = match n.kind.to_lowercase().replace('-', "_").as_str() {
        "retraction" | "retracion" | "retration" => "was retracted",
        "partial_retraction" => "was partly retracted",
        "withdrawal" => "was withdrawn",
        "removal" => "was removed",
        "expression_of_concern" => "has an expression of concern",
        _ => "was corrected",
    };
    let on = if n.date.is_empty() { String::new() } else if n.date.len() >= 10 { format!(" on {}", long_date(&n.date)) } else { format!(" in {}", long_date(&n.date)) };
    let more = notices.iter().filter(|x| kind_level(&x.kind) > NoticeLevel::None).count().saturating_sub(1);
    let more = match more {
        0 => String::new(),
        1 => " (and 1 more notice)".into(),
        k => format!(" (and {k} more notices)"),
    };
    format!("{subject} {what}{on}{more}.")
}

/// "This paper was retracted on 6 February 2010." for the reader's banner; "" when the paper
/// has no notices.
#[uniffi::export]
pub fn updates_headline(updates: PaperUpdates) -> String {
    notice_sentence(&updates.notices, "This paper")
}

/// "https://doi.org/…" for a DOI.
pub fn doi_link(doi: &str) -> String {
    if doi.is_empty() { String::new() } else { format!("https://doi.org/{doi}") }
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.first().map(s).unwrap_or_default(),
        _ => String::new(),
    }
}

fn date_parts(v: &Value) -> String {
    v["date-parts"][0]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_i64())
                .enumerate()
                .map(|(i, x)| if i == 0 { format!("{x:04}") } else { format!("{x:02}") })
                .collect::<Vec<_>>()
                .join("-")
        })
        .unwrap_or_default()
}

/// The notices in a Crossref work's `updated-by` (one per notice DOI and type; the publisher's
/// and Retraction Watch's records of the same notice are joined).
pub fn crossref_notices(m: &Value) -> Vec<EditorialNotice> {
    let mut out: Vec<EditorialNotice> = vec![];
    for u in m["updated-by"].as_array().into_iter().flatten() {
        let kind = s(&u["type"]).to_lowercase().replace('-', "_");
        if kind_level(&kind) == NoticeLevel::None {
            continue;
        }
        let source = match s(&u["source"]).as_str() {
            "retraction-watch" => "Retraction Watch".to_string(),
            "" => "publisher".to_string(),
            x => x.to_string(),
        };
        let doi = s(&u["DOI"]).to_lowercase();
        let date = date_parts(&u["updated"]);
        if let Some(n) = out.iter_mut().find(|n| n.doi == doi && n.kind == kind) {
            if !n.source.contains(&source) {
                n.source = format!("{}, {source}", n.source);
            }
            if n.date.is_empty() {
                n.date = date;
            }
            continue;
        }
        let label = if s(&u["label"]).is_empty() { kind.replace('_', " ") } else { s(&u["label"]) };
        out.push(EditorialNotice { kind, label, doi, date, source });
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(a.doi.cmp(&b.doi)));
    out
}

// ---- comparing details ----

pub(crate) fn fold(s: &str) -> String {
    crate::export::ascii_fold(s).to_lowercase()
}

/// A title's words, folded to ASCII and lower case, without markup and punctuation.
pub fn title_words(s: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[a-z0-9]+").unwrap());
    let t = fold(&crate::metadata::strip_tags(s));
    re.find_iter(&t).map(|m| m.as_str().to_string()).collect()
}

/// How alike two titles are, 0–1: the share of words in common (Dice).
pub fn title_similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (title_words(unmarked_title(a)), title_words(unmarked_title(b)));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut rest = b.clone();
    let mut common = 0;
    for w in &a {
        if let Some(i) = rest.iter().position(|x| x == w) {
            rest.swap_remove(i);
            common += 1;
        }
    }
    2.0 * common as f64 / (a.len() + b.len()) as f64
}

/// A title without the mark publishers put before a retracted or withdrawn work's ("RETRACTED: …").
pub fn unmarked_title(t: &str) -> &str {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)^\s*(retracted|withdrawn|retraction|expression of concern)(\s+article)?\s*[:.\-–]\s*").unwrap());
    match re.find(t) {
        Some(m) => &t[m.end()..],
        None => t,
    }
}

/// Whether two titles are the same, allowing a subtitle that one leaves out, a "RETRACTED:" mark
/// and differences in punctuation, accents and case.
pub fn same_title(a: &str, b: &str) -> bool {
    let (wa, wb) = (title_words(unmarked_title(a)).concat(), title_words(unmarked_title(b)).concat());
    !wa.is_empty() && (wa == wb || (wa.len() >= 10 && wb.starts_with(&wa)) || (wb.len() >= 10 && wa.starts_with(&wb)))
}

/// A family name as compared: folded, letters only ("Müller-Lüdenscheidt" → "mullerludenscheidt").
pub fn family_key(name: &str) -> String {
    fold(&crate::export::split_name(name).0).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A journal's name and its abbreviation are the same journal: "Nat. Neurosci." and "Nature
/// Neuroscience", "J Biol Chem" and "Journal of Biological Chemistry", "PNAS" and "Proceedings of
/// the National Academy of Sciences" (initials).
pub fn same_venue(a: &str, b: &str) -> bool {
    const SKIP: [&str; 8] = ["of", "the", "and", "for", "in", "on", "a", "an"];
    // "(UIST)", "(ECDL 2009)": the conference's short name, which the record leaves out
    static PAREN: OnceLock<Regex> = OnceLock::new();
    let paren = PAREN.get_or_init(|| Regex::new(r"\([^)]*\)").unwrap());
    let words = |s: &str| -> Vec<String> {
        fold(&paren.replace_all(s, " ")).replace('&', " and ").split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty() && !SKIP.contains(w)).map(String::from).collect()
    };
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return true;
    }
    if wa == wb {
        return true;
    }
    // each word of the shorter is the start of the matching word of the longer
    let abbrev = |short: &[String], long: &[String]| short.len() == long.len() && short.iter().zip(long).all(|(s, l)| l.starts_with(s.as_str()));
    if abbrev(&wa, &wb) || abbrev(&wb, &wa) {
        return true;
    }
    // initials: "pnas", "jama"
    let initials = |short: &[String], long: &[String]| short.len() == 1 && short[0].len() >= 3 && long.iter().map(|w| w.chars().next().unwrap_or(' ')).collect::<String>() == short[0];
    initials(&wa, &wb) || initials(&wb, &wa)
}

/// The first page of a page range ("1234–45" → "1234", "e1002" → "e1002").
pub fn first_page(p: &str) -> String {
    p.split(['-', '–', '—', ',']).next().unwrap_or("").trim().trim_start_matches(['p', 'P', '.', ' ']).to_lowercase()
}

/// A preprint server's work that is never to be offered in place of a published paper.
pub fn is_preprint(m: &PaperMeta) -> bool {
    crate::worktype::kind(m) == crate::worktype::Kind::Preprint
}
