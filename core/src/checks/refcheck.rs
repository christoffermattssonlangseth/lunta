//! Check a Reference List: for a manuscript's references (or one being reviewed), whether each
//! exists and whether its details match the published record. AI tools sometimes invent
//! references; people mistype DOIs, years and page numbers.
//!
//! Input: a .bib, a .ris, a pasted list (numbered or author–year), a paper's reference list (its
//! reading view's, or the PDF's as the apps read it) or a Lunta folder. Each reference is looked up:
//!
//! 1. its DOI at Crossref, then DataCite, then doi.org (registered with another agency);
//! 2. its arXiv id at arXiv;
//! 3. else (or when the DOI is wrong) Crossref's bibliographic search, then OpenAlex, then PubMed
//!    (through Europe PMC), a match taken only when the title is the reference's and the years
//!    (±1) and first author agree.
//!
//! The record found is compared field by field (title, first author, number of authors, year,
//! venue, volume, first page, DOI). Results: verified; minor differences (which fields); could not
//! be found in Crossref, OpenAlex or PubMed (which says where Lunta looked, not that the reference
//! is invented: books, reports and very new papers can be missing); the DOI points to a different
//! work (or nowhere). Retractions and other notices on the work found are listed.

use super::net::{Cached, LiveNet, Net};
use super::*;
use crate::model::*;
use crate::{export, metadata, refcite, ProgressListener};
use std::sync::Arc;
use std::time::Duration;

/// What became of a reference.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefVerdict {
    /// Found, and its details match.
    Verified,
    /// Found, with details that differ (see `differences`).
    MinorDifferences,
    /// Could not be found in Crossref, OpenAlex or PubMed.
    NotFound,
    /// Its DOI points to a different work, or doesn't resolve.
    WrongDoi,
    /// Not a scholarly work (software, a web page): not looked up.
    NotChecked,
    /// The services couldn't be asked (offline, or busy).
    Failed,
}

/// A detail that differs: "year", "2016" in the reference, "2015" in Crossref.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FieldDifference {
    pub field: String,
    pub given: String,
    pub found: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct RefCheckItem {
    /// 1, 2, 3… in the order given.
    pub number: u32,
    /// The citation key (.bib, a folder's papers) or the reference's number in the list.
    pub key: String,
    /// The reference as given, on one line.
    pub reference: String,
    pub verdict: RefVerdict,
    /// The record found (for a wrong DOI: what the DOI points to).
    pub found: Option<PaperMeta>,
    /// Where it was found: "Crossref", "DataCite", "doi.org", "arXiv", "OpenAlex", "PubMed".
    pub source: String,
    /// A link to the record: https://doi.org/…, else OpenAlex, PubMed or arXiv.
    pub link: String,
    pub differences: Vec<FieldDifference>,
    /// Retractions, corrections and expressions of concern on the work found.
    pub notices: Vec<EditorialNotice>,
    /// A sentence for the report ("Could not be found in Crossref, OpenAlex or PubMed.").
    pub note: String,
    /// For a wrong DOI: the reference's own DOI, when it was found by its details.
    pub suggested_doi: String,
    /// The paper in the library it is, when the check ran on the library.
    pub in_library: Option<String>,
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct RefCheckReport {
    pub items: Vec<RefCheckItem>,
    pub verified: u32,
    pub minor: u32,
    pub not_found: u32,
    pub wrong_doi: u32,
    pub not_checked: u32,
    pub failed: u32,
    /// References whose work was retracted, withdrawn or removed.
    pub retracted: u32,
    /// References with a correction or an expression of concern.
    pub corrected: u32,
    /// "42 references: 35 verified, 4 with minor differences, …"
    pub summary: String,
    /// The report as Markdown (with links) and as CSV, to save or share.
    pub markdown: String,
    pub csv: String,
}

/// A reference to check: its key or number, its text, and its fields when it came as BibTeX or
/// RIS or from the library.
#[derive(Clone, Debug, Default)]
pub struct RefInput {
    pub key: String,
    pub text: String,
    pub meta: Option<PaperMeta>,
    /// The author list ends "and others" / "et al.": the record may have more.
    pub et_al: bool,
    /// Software, a web page: not looked up.
    pub not_a_paper: bool,
}

const NOT_FOUND: &str = "Could not be found in Crossref, OpenAlex or PubMed.";

// ---------------------------------------------------------------- reading the input

macro_rules! re {
    ($p:expr) => {{
        static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        R.get_or_init(|| regex::Regex::new($p).unwrap())
    }};
}

/// The references in a .bib, a .ris or a pasted list (the format told from the text).
pub fn parse_input(text: &str) -> Vec<RefInput> {
    let t = text.trim_start_matches('\u{feff}').trim();
    if re!(r"(?m)^\s*@[A-Za-z]+\s*[{(]").is_match(t) {
        return crate::import::parse_bibtex_raw(t)
            .into_iter()
            .map(|e| {
                let entry = crate::import::entry_from_bib(&e.kind, &e.fields);
                let meta = entry.meta;
                let et_al = e.get("author").to_lowercase().split_whitespace().collect::<Vec<_>>().windows(2).any(|w| w == ["and", "others"]);
                let not_a_paper = crate::bibimport::is_extra(&e, &meta);
                RefInput { key: e.key.clone(), text: one_line(&meta), meta: Some(meta), et_al, not_a_paper }
            })
            .collect();
    }
    if t.starts_with("TY  -") || t.contains("\nTY  -") {
        return crate::import::parse_ris(t)
            .into_iter()
            .enumerate()
            .map(|(i, e)| RefInput { key: (i + 1).to_string(), text: one_line(&e.meta), meta: Some(e.meta), ..Default::default() })
            .collect();
    }
    split_list(t)
        .into_iter()
        .enumerate()
        .map(|(i, (label, text))| RefInput { key: if label.is_empty() { (i + 1).to_string() } else { label }, text, ..Default::default() })
        .collect()
}

/// A reference given by its details, as one line for the report.
pub fn one_line(m: &PaperMeta) -> String {
    let authors = match m.authors.len() {
        0 => String::new(),
        1..=3 => m.authors.join(", "),
        _ => format!("{} et al.", m.authors[0]),
    };
    let mut parts = vec![];
    if !authors.is_empty() {
        parts.push(if m.year().is_empty() { authors } else { format!("{authors} ({})", m.year()) });
    } else if !m.year().is_empty() {
        parts.push(format!("({})", m.year()));
    }
    if !m.title.is_empty() {
        parts.push(m.title.trim_end_matches('.').to_string());
    }
    let mut venue = m.journal.clone();
    if !m.volume.is_empty() {
        venue = format!("{venue} {}", m.volume).trim().to_string();
    }
    if !m.pages.is_empty() {
        venue = format!("{venue}, {}", m.pages).trim_start_matches(", ").to_string();
    }
    if !venue.is_empty() {
        parts.push(venue);
    }
    let mut s = parts.join(". ");
    if !m.doi.is_empty() {
        s = format!("{s}. doi:{}", m.doi);
    }
    s
}

/// A pasted reference list as entries: numbered ("1.", "[1]", "(1)", "1)") entries run until the
/// next number; otherwise paragraphs (blank lines between them) or lines, a line joined to the one
/// before when that one isn't finished (no year yet, or ends in a comma or hyphen) or it starts in
/// lower case.
pub fn split_list(text: &str) -> Vec<(String, String)> {
    let label = re!(r"^\s*(?:\[(\d{1,4})\]|\((\d{1,4})\)|(\d{1,4})[.)])\s+");
    let indented: Vec<bool> = text.lines().map(|l| l.starts_with([' ', '\t'])).collect();
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    let labelled = lines.iter().filter(|l| label.is_match(l)).count();
    let mut out: Vec<(String, Vec<String>)> = vec![];
    let push_line = |cur: &mut Vec<String>, l: &str| cur.push(l.trim_start_matches(['•', '·', '–', '*']).trim().to_string());
    if labelled >= 2 || (labelled == 1 && lines.iter().filter(|l| !l.is_empty()).count() == 1) {
        for l in &lines {
            if l.is_empty() {
                continue;
            }
            if let Some(c) = label.captures(l) {
                let n = (1..=3).find_map(|i| c.get(i)).map(|m| m.as_str().to_string()).unwrap_or_default();
                out.push((n, vec![]));
                let rest = &l[c.get(0).unwrap().end()..];
                push_line(&mut out.last_mut().unwrap().1, rest);
            } else if let Some(last) = out.last_mut() {
                push_line(&mut last.1, l);
            } else {
                out.push((String::new(), vec![]));
                push_line(&mut out.last_mut().unwrap().1, l);
            }
        }
    } else if text.contains("\n\n") || text.contains("\r\n\r\n") {
        let mut cur: Vec<String> = vec![];
        for l in lines.iter().chain(std::iter::once(&"")) {
            if l.is_empty() {
                if !cur.is_empty() {
                    out.push((String::new(), std::mem::take(&mut cur)));
                }
            } else {
                push_line(&mut cur, l);
            }
        }
    } else {
        for (l, ind) in lines.iter().zip(&indented).filter(|(l, _)| !l.is_empty()) {
            let join = out.last().is_some_and(|(_, prev)| {
                let p = prev.join(" ");
                let p = p.trim_end();
                !re!(r"\b(1[89]|20)\d{2}[a-z]?\b").is_match(p)
                    || p.ends_with([',', ';', ':', '-', '–', '&'])
                    || l.chars().next().is_some_and(|c| c.is_lowercase())
                    || re!(r"^(10\.\d{4,9}/|https?://|doi:)").is_match(l)
                    || *ind
                    || !author_start(l)
            });
            if join {
                push_line(&mut out.last_mut().unwrap().1, l);
            } else {
                out.push((String::new(), vec![]));
                push_line(&mut out.last_mut().unwrap().1, l);
            }
        }
    }
    out.into_iter()
        .map(|(n, parts)| {
            let mut s = String::new();
            for p in parts {
                if s.ends_with('-') && p.chars().next().is_some_and(|c| c.is_lowercase()) && re!(r"\p{L}-$").is_match(&s) {
                    s.pop();
                    s.push_str(&p);
                } else if s.ends_with(['/', '.']) && re!(r"(10\.\d{4,9}/\S*|https?://\S*)$").is_match(&s) && !p.contains(' ') {
                    s.push_str(&p);
                } else {
                    if !s.is_empty() {
                        s.push(' ');
                    }
                    s.push_str(&p);
                }
            }
            (n, s)
        })
        .filter(|(_, s)| s.chars().filter(|c| c.is_alphabetic()).count() >= 8)
        .collect()
}

/// A line that starts with an author: "Smith JA,", "Smith, J. A.,", "J. A. Smith,", "van der Berg, K."
fn author_start(t: &str) -> bool {
    re!(concat!(
        r"^\s*(?:(?:van|von|de|der|den|di|da|du|del|la|le|dos|das|ten|ter|el|al)\s+)*\p{Lu}[\p{L}'’\-]+(?:[\s\-]\p{Lu}[\p{L}'’\-]+)?",
        r"(?:,\s*(?:\p{Lu}\.\s?-?){1,3}|,?\s+\p{Lu}{1,3}(?:[,.:]|\s+(?:and|&)\s)|,?\s+(?:\p{Lu}\.\s?-?){1,3}[,\s])",
        r"|^\s*(?:\p{Lu}\.\s?-?){1,3}\s*\p{Lu}[\p{L}'’\-]+(?:\s\p{Lu}[\p{L}'’\-]+)?,"
    ))
    .is_match(t)
}

/// The title in a reference given as text, at a guess: a quoted title, else the part after the
/// year in brackets ("Smith J (2020) The title. Journal"), else the sentence that reads most like
/// a title (lower-case words, not names and initials, not volume and pages).
pub fn guess_title(raw: &str) -> String {
    let t = re!(r"(?i)\b(doi:\s*|https?://\S+|10\.\d{4,9}/\S+)").replace_all(raw, " ");
    if let Some(c) = re!(r#"[“"]([^”"]{12,})[”"]"#).captures(&t) {
        return c[1].trim().trim_end_matches([',', '.']).to_string();
    }
    if let Some(c) = re!(r"\((?:1[89]|20)\d{2}[a-z]?\)\.?\s+([^.?!]{12,}[.?!])").captures(&t) {
        return c[1].trim().trim_end_matches('.').to_string();
    }
    let mut best = (0i32, String::new());
    for seg in re!(r"[.?!]\s+").split(&t) {
        let seg = seg.trim();
        let words: Vec<&str> = seg.split_whitespace().collect();
        if words.len() < 3 {
            continue;
        }
        let lower = words.iter().filter(|w| w.chars().next().is_some_and(|c| c.is_lowercase())).count() as i32;
        let initials = re!(r"\b\p{Lu}\.?(\s|,|$)").find_iter(seg).count() as i32;
        let numbers = re!(r"\d").find_iter(seg).count() as i32;
        let score = 2 * lower + words.len() as i32 - 3 * initials - numbers - if seg.contains(',') && initials > 0 { 5 } else { 0 };
        if score > best.0 {
            best = (score, seg.to_string());
        }
    }
    best.1
}

// ---------------------------------------------------------------- looking up

/// A record found for a reference.
#[derive(Clone, Debug, Default)]
pub(crate) struct Found {
    pub meta: PaperMeta,
    pub source: &'static str,
    pub link: String,
    pub notices: Vec<EditorialNotice>,
    /// Other names of the venue (Crossref's short container title).
    pub venues: Vec<String>,
    /// OpenAlex or PubMed say it is retracted (no notice details).
    pub retracted: bool,
}

fn crossref_found(m: &Value, source: &'static str) -> Found {
    let meta = metadata::crossref_item(m);
    let mut venues: Vec<String> = m["short-container-title"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    venues.extend(m["container-title"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)));
    Found { link: doi_link(&meta.doi), notices: crossref_notices(m), meta, source, venues, retracted: false }
}

fn json(body: &str) -> Result<Value> {
    serde_json::from_str(body).map_err(|e| CoreError::Network(format!("bad JSON: {e}")))
}

/// A DOI's Crossref record (the work, its notices).
pub(crate) fn crossref_doi(net: &dyn Net, doi: &str) -> Result<Option<Found>> {
    let Some(body) = net.get(&format!("https://api.crossref.org/works/{}", metadata::enc(doi)))? else { return Ok(None) };
    let j = json(&body)?;
    if j["message"].is_null() {
        return Ok(None);
    }
    Ok(Some(crossref_found(&j["message"], "Crossref")))
}

/// A DOI's DataCite record (arXiv, Zenodo, figshare, many repositories).
fn datacite_doi(net: &dyn Net, doi: &str) -> Result<Option<Found>> {
    let Some(body) = net.get(&format!("https://api.datacite.org/dois/{}", metadata::enc(doi)))? else { return Ok(None) };
    let j = json(&body)?;
    let a = &j["data"]["attributes"];
    if a.is_null() {
        return Ok(None);
    }
    let authors = a["creators"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let (g, f) = (s(&c["givenName"]), s(&c["familyName"]));
            if f.is_empty() { s(&c["name"]) } else { format!("{g} {f}").trim().to_string() }
        })
        .filter(|n| !n.is_empty())
        .collect();
    let kind = s(&a["types"]["resourceTypeGeneral"]).to_lowercase();
    let venue = if !s(&a["container"]["title"]).is_empty() { s(&a["container"]["title"]) } else { s(&a["publisher"]) };
    let meta = PaperMeta {
        title: a["titles"].as_array().and_then(|t| t.iter().find(|x| x["titleType"].is_null())).map(|t| s(&t["title"])).unwrap_or_default(),
        authors,
        date: s(&a["publicationYear"]),
        journal: venue,
        doi: s(&a["doi"]),
        work_type: if kind == "preprint" { "posted-content".into() } else { String::new() },
        ..Default::default()
    };
    Ok(Some(Found { link: doi_link(&meta.doi), meta, source: "DataCite", ..Default::default() }))
}

/// Whether a DOI is registered at all (with any agency: mEDRA, JaLC, CNKI…), at doi.org.
fn doi_registered(net: &dyn Net, doi: &str) -> Result<bool> {
    let Some(body) = net.get(&format!("https://doi.org/api/handles/{}", metadata::enc(doi)))? else { return Ok(false) };
    Ok(json(&body).map(|j| j["responseCode"].as_i64() == Some(1)).unwrap_or(false))
}

/// A DOI's record: Crossref, else DataCite, else (registered elsewhere) a record with only the DOI.
pub(crate) fn lookup_doi(net: &dyn Net, doi: &str) -> Result<Option<Found>> {
    if let Some(f) = crossref_doi(net, doi)? {
        return Ok(Some(f));
    }
    if let Some(f) = datacite_doi(net, doi)? {
        return Ok(Some(f));
    }
    if doi_registered(net, doi)? {
        return Ok(Some(Found { meta: PaperMeta { doi: doi.to_string(), ..Default::default() }, source: "doi.org", link: doi_link(doi), ..Default::default() }));
    }
    Ok(None)
}

fn xml_field(block: &str, tag: &str) -> Vec<String> {
    regex::Regex::new(&format!(r"(?s)<{tag}[^>]*>(.*?)</{tag}>"))
        .unwrap()
        .captures_iter(block)
        .map(|c| c[1].split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

/// arXiv entries by id (several at once): (id without version, details, journal reference).
pub(crate) fn arxiv_records(net: &dyn Net, ids: &[String]) -> Result<Vec<(String, PaperMeta, String)>> {
    let url = format!("https://export.arxiv.org/api/query?id_list={}&max_results={}", ids.join(","), ids.len().max(1));
    let Some(xml) = net.get(&url)? else { return Ok(vec![]) };
    let mut out = vec![];
    for entry in xml.split("<entry>").skip(1) {
        let id_url = xml_field(entry, "id").into_iter().next().unwrap_or_default();
        let id = re!(r"abs/([^v\s]+?)(v\d+)?$").captures(&id_url).map(|c| c[1].to_string()).unwrap_or_default();
        let title = xml_field(entry, "title").into_iter().next().unwrap_or_default();
        if id.is_empty() || title.is_empty() || title == "Error" {
            continue;
        }
        let authors = re!(r"(?s)<author>\s*<name>(.*?)</name>").captures_iter(entry).map(|c| c[1].trim().to_string()).collect();
        let doi = xml_field(entry, "arxiv:doi").into_iter().next().unwrap_or_default();
        let meta = PaperMeta {
            title,
            authors,
            date: xml_field(entry, "published").into_iter().next().unwrap_or_default().chars().take(10).collect(),
            journal: "arXiv".into(),
            work_type: "posted-content".into(),
            doi,
            arxiv: id.clone(),
            url: format!("https://arxiv.org/abs/{id}"),
            ..Default::default()
        };
        let journal_ref = xml_field(entry, "arxiv:journal_ref").into_iter().next().unwrap_or_default();
        out.push((id, meta, journal_ref));
    }
    Ok(out)
}

const CROSSREF_SELECT: &str = "DOI,title,subtitle,author,issued,container-title,short-container-title,volume,issue,page,type,updated-by,publisher";

/// Crossref's bibliographic search: its best few records.
pub(crate) fn crossref_search(net: &dyn Net, query: &str) -> Result<Vec<Found>> {
    let q: String = query.chars().take(400).collect();
    let url = format!("https://api.crossref.org/works?query.bibliographic={}&rows=5&select={CROSSREF_SELECT}", metadata::enc(&q));
    let Some(body) = net.get(&url)? else { return Ok(vec![]) };
    let j = json(&body)?;
    Ok(j["message"]["items"].as_array().into_iter().flatten().map(|m| crossref_found(m, "Crossref")).collect())
}

/// OpenAlex's search for a title (in the years around `year`, when known).
fn openalex_search(net: &dyn Net, title: &str, year: Option<i32>) -> Result<Vec<Found>> {
    let q: String = title_words(title).join(" ");
    if q.is_empty() {
        return Ok(vec![]);
    }
    let filter = year.map(|y| format!("&filter=publication_year:{}-{}", y - 1, y + 1)).unwrap_or_default();
    let url = format!(
        "https://api.openalex.org/works?search={}{filter}&per_page=5&select=id,title,authorships,publication_date,primary_location,doi,ids,biblio,type,type_crossref,is_retracted",
        metadata::enc(&q)
    );
    let Some(body) = net.get(&url)? else { return Ok(vec![]) };
    let j = json(&body)?;
    let mut out = vec![];
    for w in j["results"].as_array().into_iter().flatten() {
        let one = serde_json::json!({ "results": [w] });
        let Some(hit) = metadata::parse_works(&one).into_iter().next() else { continue };
        let link = if hit.meta.doi.is_empty() { s(&w["id"]) } else { doi_link(&hit.meta.doi) };
        out.push(Found { meta: hit.meta, source: "OpenAlex", link, retracted: w["is_retracted"].as_bool().unwrap_or(false), ..Default::default() });
    }
    Ok(out)
}

/// PubMed (through Europe PMC's search of MEDLINE) for a title.
fn pubmed_search(net: &dyn Net, title: &str) -> Result<Vec<Found>> {
    let q = title_words(title).join(" ");
    if q.split_whitespace().count() < 3 {
        return Ok(vec![]);
    }
    let url = format!(
        "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={}&resultType=lite&format=json&pageSize=5",
        metadata::enc(&format!("TITLE:\"{q}\" AND SRC:MED")).replace('/', "%2F")
    );
    let Some(body) = net.get(&url)? else { return Ok(vec![]) };
    let j = json(&body)?;
    let mut out = vec![];
    for r in j["resultList"]["result"].as_array().into_iter().flatten() {
        // "Smith J, Jones K, Lee M." → "J Smith", …
        let authors = s(&r["authorString"])
            .trim_end_matches('.')
            .split(", ")
            .filter(|a| !a.trim().is_empty() && !a.contains("et al"))
            .map(|a| {
                let mut p: Vec<&str> = a.split_whitespace().collect();
                if p.len() > 1 && p.last().is_some_and(|x| x.chars().all(|c| c.is_uppercase())) {
                    let ini = p.pop().unwrap();
                    format!("{ini} {}", p.join(" "))
                } else {
                    a.to_string()
                }
            })
            .collect();
        let pmid = s(&r["pmid"]);
        let meta = PaperMeta {
            title: metadata::strip_tags(&s(&r["title"])).trim_end_matches('.').to_string(),
            authors,
            date: s(&r["pubYear"]),
            journal: s(&r["journalTitle"]),
            volume: s(&r["journalVolume"]),
            issue: s(&r["issue"]),
            pages: s(&r["pageInfo"]),
            doi: s(&r["doi"]),
            pmid: pmid.clone(),
            pmcid: s(&r["pmcid"]),
            ..Default::default()
        };
        let link = if !meta.doi.is_empty() { doi_link(&meta.doi) } else if !pmid.is_empty() { format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/") } else { String::new() };
        let retracted = s(&r["pubType"]).to_lowercase().contains("retracted publication");
        out.push(Found { meta, source: "PubMed", link, retracted, ..Default::default() });
    }
    Ok(out)
}

// ---------------------------------------------------------------- matching and comparing

fn years_in(t: &str) -> Vec<i32> {
    re!(r"\b((?:1[89]|20)\d{2})[a-z]?\b").captures_iter(t).filter_map(|c| c[1].parse().ok()).collect()
}

fn year_of(m: &PaperMeta) -> Option<i32> {
    m.year().parse().ok()
}

/// Letters and digits only, folded: for finding a name in a reference's text.
fn squash(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Whether the reference's text names this author (family name).
fn names(raw: &str, author: &str) -> bool {
    let k = family_key(author);
    !k.is_empty() && squash(raw).contains(&k)
}

/// The reference's own title: its field, or a guess from its text.
fn ref_title(r: &RefInput) -> String {
    match &r.meta {
        Some(m) if !m.title.is_empty() => m.title.clone(),
        _ => guess_title(&r.text),
    }
}

fn ref_year(r: &RefInput) -> Option<i32> {
    match &r.meta {
        Some(m) => year_of(m),
        None => years_in(&r.text).into_iter().next(),
    }
}

/// Whether a record found by searching is the reference: the title is the reference's (nearly all
/// its words), the years are at most one apart, and the first author agrees (or the title alone is
/// long and specific enough).
fn is_match(r: &RefInput, f: &Found) -> bool {
    let fm = &f.meta;
    if fm.title.is_empty() {
        return false;
    }
    let years_ok = match (year_of(fm), &r.meta) {
        (Some(y), Some(m)) => year_of(m).is_none_or(|g| (g - y).abs() <= 1),
        (Some(y), None) => {
            let ys = years_in(&r.text);
            ys.is_empty() || ys.iter().any(|g| (g - y).abs() <= 1)
        }
        (None, _) => true,
    };
    if !years_ok {
        return false;
    }
    match &r.meta {
        Some(m) => {
            let sim = title_similarity(&m.title, &fm.title);
            let title_ok = same_title(&m.title, &fm.title) || sim >= 0.85;
            let author_ok = m.authors.is_empty() || fm.authors.is_empty() || family_key(&m.authors[0]) == family_key(&fm.authors[0])
                || m.authors.iter().take(3).any(|a| fm.authors.iter().take(3).any(|b| family_key(a) == family_key(b)));
            title_ok && (author_ok || sim >= 0.95)
        }
        None => {
            if !refcite::title_in(&r.text, &fm.title) {
                return false;
            }
            let long = title_words(&fm.title).iter().filter(|w| w.len() >= 3).count() >= 6;
            let author_ok = fm.authors.is_empty() || fm.authors.iter().take(3).any(|a| names(&r.text, a));
            author_ok || long
        }
    }
}

/// Whether the record a DOI points to is the work the reference describes (leniently: details
/// that differ are reported as differences, not as a different work).
fn doi_is_this(r: &RefInput, f: &Found) -> bool {
    let fm = &f.meta;
    if fm.title.is_empty() {
        return true;
    }
    match &r.meta {
        Some(m) => m.title.is_empty() || same_title(&m.title, &fm.title) || title_similarity(&m.title, &fm.title) >= 0.5,
        None => {
            if refcite::title_in(&r.text, &fm.title) {
                return true;
            }
            // a reference without its title ("Smith J. Nature 2015;521:436"): author and year then
            let guess = title_words(&guess_title(&r.text)).len();
            let author = fm.authors.first().is_some_and(|a| names(&r.text, a));
            let year = year_of(fm).is_some_and(|y| years_in(&r.text).iter().any(|g| (g - y).abs() <= 1));
            guess < 4 && author && year
        }
    }
}

fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// The volume in a reference's text: "521(7553):436", "2015;521:436", "521, 436–444".
fn volume_in(raw: &str) -> Option<String> {
    for p in [re!(r";\s*(\d{1,4})\s*[(:]"), re!(r"\b(\d{1,4})\s*\(\s*[\w\-–]{1,8}\s*\)\s*[:,]"), re!(r"\b(\d{1,4})\s*:\s*[A-Za-z]?\d{1,6}\s*[-–]"), re!(r",\s*(\d{1,4}),\s*[A-Za-z]?\d{1,6}\s*[-–]\s*\d")] {
        if let Some(c) = p.captures(raw) {
            return Some(c[1].to_string());
        }
    }
    None
}

/// The details that differ between the reference and the record found.
pub(crate) fn differences(r: &RefInput, f: &Found) -> Vec<FieldDifference> {
    let fm = &f.meta;
    let mut out = vec![];
    let mut diff = |field: &str, given: String, found: String| out.push(FieldDifference { field: field.into(), given, found });
    match &r.meta {
        Some(m) => {
            if !m.title.is_empty() && !fm.title.is_empty() && !same_title(&m.title, &fm.title) {
                diff("title", m.title.clone(), fm.title.clone());
            }
            let (b, n) = (m.authors.len(), fm.authors.len());
            // a group as the author ("Qwen Team", "{Apple}"): the record names the people
            let group = b == 1 && (crate::worktype::is_organization(&m.authors[0]) || !m.authors[0].trim().contains(' '));
            if b > 0 && n > 0 && !group {
                if family_key(&m.authors[0]) != family_key(&fm.authors[0]) {
                    diff("first author", m.authors[0].clone(), fm.authors[0].clone());
                }
                if (!r.et_al && b != n) || (r.et_al && n < b) {
                    diff("number of authors", format!("{b}{}", if r.et_al { " and others" } else { "" }), n.to_string());
                }
            }
            if let (Some(g), Some(y)) = (year_of(m), year_of(fm)) {
                if g != y {
                    diff("year", g.to_string(), y.to_string());
                }
            }
            if !m.journal.is_empty() && !fm.journal.is_empty() && !std::iter::once(&fm.journal).chain(f.venues.iter()).any(|v| same_venue(&m.journal, v)) {
                diff("venue", m.journal.clone(), fm.journal.clone());
            }
            if !digits(&m.volume).is_empty() && !digits(&fm.volume).is_empty() && digits(&m.volume) != digits(&fm.volume) {
                diff("volume", m.volume.clone(), fm.volume.clone());
            }
            if !first_page(&m.pages).is_empty() && !first_page(&fm.pages).is_empty() && first_page(&m.pages) != first_page(&fm.pages) {
                diff("pages", m.pages.clone(), fm.pages.clone());
            }
            if !m.doi.is_empty() && !fm.doi.is_empty() && metadata::normalize_doi(&m.doi) != metadata::normalize_doi(&fm.doi) {
                diff("DOI", m.doi.clone(), fm.doi.clone());
            }
        }
        None => {
            let raw = &r.text;
            if let Some(a) = fm.authors.first() {
                if !names(raw, a) {
                    diff("first author", String::new(), a.clone());
                }
            }
            if let Some(y) = year_of(fm) {
                let ys = years_in(raw);
                if !ys.is_empty() && !ys.contains(&y) {
                    diff("year", ys[0].to_string(), y.to_string());
                }
            }
            if let Some(v) = volume_in(raw) {
                if !digits(&fm.volume).is_empty() && v != digits(&fm.volume) {
                    diff("volume", v, fm.volume.clone());
                }
            }
            let fp = first_page(&fm.pages);
            if !fp.is_empty() && re!(r"\b[A-Za-z]?\d{1,6}\s*[-–—]\s*[A-Za-z]?\d{1,6}\b").is_match(raw) {
                let tokens: Vec<String> = re!(r"[A-Za-z]?\d+").find_iter(raw).map(|m| m.as_str().to_lowercase()).collect();
                if !tokens.contains(&fp) {
                    let given = re!(r"\b([A-Za-z]?\d{1,6}\s*[-–—]\s*[A-Za-z]?\d{1,6})\b").captures_iter(raw).last().map(|c| c[1].to_string()).unwrap_or_default();
                    diff("pages", given, fm.pages.clone());
                }
            }
            if let Some(d) = metadata::doi_in(raw) {
                if !fm.doi.is_empty() && metadata::normalize_doi(&d) != metadata::normalize_doi(&fm.doi) {
                    diff("DOI", d, fm.doi.clone());
                }
            }
        }
    }
    out
}

/// Notices for a work found elsewhere than Crossref, from its Crossref record (by DOI).
fn add_crossref_notices(net: &dyn Net, f: &mut Found) {
    if f.source == "Crossref" || f.meta.doi.is_empty() || metadata::is_preprint_doi(&f.meta.doi) {
        return;
    }
    if let Ok(Some(c)) = crossref_doi(net, &f.meta.doi) {
        f.notices = c.notices;
    }
}

/// Searches Crossref, OpenAlex and PubMed for the reference: the first record that is plainly it.
fn search(net: &dyn Net, r: &RefInput, errors: &mut Vec<String>) -> Option<Found> {
    let title = ref_title(r);
    let query = match &r.meta {
        Some(m) => {
            let authors: Vec<String> = m.authors.iter().take(4).map(|a| export::split_name(a).0).collect();
            [authors.join(" "), m.title.clone(), m.journal.clone(), m.year()].iter().filter(|x| !x.trim().is_empty()).cloned().collect::<Vec<_>>().join(" ")
        }
        None => re!(r"^\s*(\[\d+\]|\(\d+\)|\d+[.)])\s*").replace(&r.text, "").to_string(),
    };
    if title_words(&query).len() < 4 {
        return None;
    }
    let mut try_source = |res: Result<Vec<Found>>| -> Option<Found> {
        match res {
            Ok(list) => list.into_iter().find(|f| is_match(r, f)),
            Err(e) => {
                errors.push(e.to_string());
                None
            }
        }
    };
    if let Some(f) = try_source(crossref_search(net, &query)) {
        return Some(f);
    }
    if title_words(&title).len() < 3 {
        return None;
    }
    if let Some(mut f) = try_source(openalex_search(net, &title, ref_year(r))) {
        add_crossref_notices(net, &mut f);
        return Some(f);
    }
    if let Some(mut f) = try_source(pubmed_search(net, &title)) {
        add_crossref_notices(net, &mut f);
        return Some(f);
    }
    None
}

fn arxiv_of(r: &RefInput) -> String {
    if let Some(m) = &r.meta {
        if !m.arxiv.is_empty() {
            return m.arxiv.clone();
        }
    }
    re!(r"(?i)arxiv(?:\.org/(?:abs|pdf)/|:\s*|\s+)(\d{4}\.\d{4,5})").captures(&r.text).map(|c| c[1].to_string()).unwrap_or_default()
}

fn item(r: &RefInput, number: u32) -> RefCheckItem {
    RefCheckItem {
        number,
        key: r.key.clone(),
        reference: r.text.clone(),
        verdict: RefVerdict::NotFound,
        found: None,
        source: String::new(),
        link: String::new(),
        differences: vec![],
        notices: vec![],
        note: String::new(),
        suggested_doi: String::new(),
        in_library: None,
    }
}

fn found_item(r: &RefInput, number: u32, f: Found) -> RefCheckItem {
    let mut it = item(r, number);
    it.differences = differences(r, &f);
    it.verdict = if it.differences.is_empty() { RefVerdict::Verified } else { RefVerdict::MinorDifferences };
    if f.source == "doi.org" {
        it.note = "The DOI is registered (with an agency other than Crossref and DataCite): its details weren't compared.".into();
    }
    it.notices = f.notices.clone();
    if f.retracted && level(&it.notices) < NoticeLevel::Retracted {
        it.notices.push(EditorialNotice { kind: "retraction".into(), label: "Retracted".into(), source: f.source.into(), ..Default::default() });
    }
    it.source = f.source.into();
    it.link = f.link.clone();
    it.found = Some(f.meta);
    it
}

/// Checks one reference.
pub(crate) fn check_one(net: &dyn Net, r: &RefInput, number: u32) -> RefCheckItem {
    if r.not_a_paper {
        let mut it = item(r, number);
        it.verdict = RefVerdict::NotChecked;
        it.note = "Not a scholarly work (software, a web page): not looked up.".into();
        return it;
    }
    let mut errors: Vec<String> = vec![];
    let doi = match &r.meta {
        Some(m) if !m.doi.is_empty() => metadata::doi_in(&m.doi).unwrap_or_else(|| m.doi.clone()),
        Some(_) => String::new(),
        None => metadata::doi_in(&r.text).unwrap_or_default(),
    };
    let mut wrong: Option<Found> = None;
    let mut unresolved = false;
    if !doi.is_empty() {
        match lookup_doi(net, &doi) {
            Ok(Some(f)) if doi_is_this(r, &f) => return found_item(r, number, f),
            Ok(Some(f)) => wrong = Some(f),
            Ok(None) => unresolved = true,
            Err(e) => errors.push(e.to_string()),
        }
    }
    let arxiv = arxiv_of(r);
    if wrong.is_none() && !arxiv.is_empty() {
        match arxiv_records(net, std::slice::from_ref(&arxiv)) {
            Ok(list) => {
                if let Some((_, meta, _)) = list.into_iter().next() {
                    let f = Found { link: format!("https://arxiv.org/abs/{arxiv}"), meta, source: "arXiv", ..Default::default() };
                    if doi_is_this(r, &f) {
                        let mut it = found_item(r, number, f);
                        if unresolved {
                            it.verdict = RefVerdict::WrongDoi;
                            it.note = format!("The DOI {doi} doesn't resolve; the reference is arXiv:{arxiv}.");
                        }
                        return it;
                    }
                }
            }
            Err(e) => errors.push(e.to_string()),
        }
    }
    let found = search(net, r, &mut errors);
    if let Some(w) = wrong {
        let mut it = item(r, number);
        it.verdict = RefVerdict::WrongDoi;
        let who = w.meta.authors.first().map(|a| format!("{} ", export::split_name(a).0)).unwrap_or_default();
        let year = w.meta.year();
        it.note = format!("The DOI {doi} points to a different work: “{}” ({who}{year}, {}).", w.meta.title, w.meta.journal.trim())
            .replace(" ()", "")
            .replace(", )", ")");
        if let Some(f) = &found {
            if !f.meta.doi.is_empty() && metadata::normalize_doi(&f.meta.doi) != metadata::normalize_doi(&doi) {
                it.suggested_doi = f.meta.doi.clone();
                it.note += &format!(" The reference itself is {} ({}).", f.meta.doi, f.source);
            } else {
                it.note += &format!(" The reference itself was found in {}.", f.source);
            }
        }
        it.source = w.source.into();
        it.link = w.link.clone();
        it.notices = w.notices.clone();
        it.found = Some(w.meta);
        return it;
    }
    match (unresolved, found) {
        (true, Some(f)) => {
            let suggested = f.meta.doi.clone();
            let mut it = found_item(r, number, f);
            it.verdict = RefVerdict::WrongDoi;
            it.note = if suggested.is_empty() {
                format!("The DOI {doi} doesn't resolve (not registered at Crossref, DataCite or doi.org); the reference was found in {}.", it.source)
            } else {
                format!("The DOI {doi} doesn't resolve (not registered at Crossref, DataCite or doi.org); the reference is {suggested} ({}).", it.source)
            };
            it.suggested_doi = suggested;
            it
        }
        (true, None) => {
            let mut it = item(r, number);
            it.note = format!("{} Its DOI {doi} doesn't resolve either.", NOT_FOUND);
            if !errors.is_empty() {
                it.note += &format!(" (Some services couldn't be asked: {}.)", errors.join("; "));
            }
            it
        }
        (false, Some(f)) => found_item(r, number, f),
        (false, None) => {
            let mut it = item(r, number);
            if errors.is_empty() {
                it.note = NOT_FOUND.into();
            } else {
                it.verdict = RefVerdict::Failed;
                it.note = format!("Couldn't be checked: {}.", errors.join("; "));
            }
            it
        }
    }
}

/// Checks every reference (one after another, politely), then totals and writes the report.
pub(crate) fn check_all(net: &dyn Net, refs: &[RefInput], progress: &dyn Fn(String)) -> RefCheckReport {
    let n = refs.len();
    let items: Vec<RefCheckItem> = refs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let what: String = r.text.chars().take(80).collect();
            progress(format!("Checking {} of {n}: {what}", i + 1));
            check_one(net, r, i as u32 + 1)
        })
        .collect();
    report(items)
}

/// Totals, summary, Markdown and CSV for checked references.
pub fn report(items: Vec<RefCheckItem>) -> RefCheckReport {
    let count = |v: RefVerdict| items.iter().filter(|i| i.verdict == v).count() as u32;
    let mut r = RefCheckReport {
        verified: count(RefVerdict::Verified),
        minor: count(RefVerdict::MinorDifferences),
        not_found: count(RefVerdict::NotFound),
        wrong_doi: count(RefVerdict::WrongDoi),
        not_checked: count(RefVerdict::NotChecked),
        failed: count(RefVerdict::Failed),
        retracted: items.iter().filter(|i| level(&i.notices) == NoticeLevel::Retracted).count() as u32,
        corrected: items.iter().filter(|i| matches!(level(&i.notices), NoticeLevel::Corrected | NoticeLevel::Concern)).count() as u32,
        ..Default::default()
    };
    r.items = items;
    r.summary = summary(&r);
    r.markdown = markdown(&r);
    r.csv = csv(&r);
    r
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn summary(r: &RefCheckReport) -> String {
    let mut parts = vec![format!("{} verified", r.verified)];
    if r.minor > 0 {
        parts.push(format!("{} with minor differences", r.minor));
    }
    if r.wrong_doi > 0 {
        parts.push(format!("{} whose DOI points to a different work", r.wrong_doi));
    }
    if r.not_found > 0 {
        parts.push(format!("{} that could not be found in Crossref, OpenAlex or PubMed", r.not_found));
    }
    if r.not_checked > 0 {
        parts.push(format!("{} not papers (not looked up)", r.not_checked));
    }
    if r.failed > 0 {
        parts.push(format!("{} that couldn't be checked (offline?)", r.failed));
    }
    let mut s = format!("{}: {}.", plural(r.items.len() as u32, "reference", "references"), parts.join(", "));
    if r.retracted > 0 {
        s += &format!(" {} retracted.", plural(r.retracted, "work cited was", "works cited were"));
    }
    if r.corrected > 0 {
        s += &format!(" {} a correction or an expression of concern.", plural(r.corrected, "has", "have"));
    }
    s
}

fn label_of(i: &RefCheckItem) -> String {
    if i.key.is_empty() || i.key == i.number.to_string() { format!("[{}]", i.number) } else { format!("[{}] {}", i.number, i.key) }
}

fn found_line(i: &RefCheckItem) -> String {
    let Some(f) = &i.found else { return String::new() };
    let mut s = one_line(&PaperMeta { doi: String::new(), ..f.clone() });
    if !i.link.is_empty() {
        s = format!("{s} — <{}>", i.link);
    }
    format!("{s} ({})", i.source)
}

fn md_escape(s: &str) -> String {
    s.replace('*', "\\*").replace('_', "\\_").replace('[', "\\[").replace(']', "\\]")
}

fn markdown(r: &RefCheckReport) -> String {
    let mut s = format!("# Reference check\n\n{}\n\n", r.summary);
    s += "Lunta looked each reference up by its DOI (Crossref, DataCite, doi.org) and by its details (Crossref, OpenAlex, PubMed through Europe PMC), and compared title, first author, number of authors, year, venue, volume, pages and DOI with the record found. A reference that could not be found may still exist (books, reports, theses and very new papers are often missing from these services): check it by hand.\n";
    let section = |s: &mut String, title: &str, items: Vec<&RefCheckItem>, detail: bool| {
        if items.is_empty() {
            return;
        }
        *s += &format!("\n## {title} ({})\n\n", items.len());
        for i in items {
            *s += &format!("- **{}** {}\n", md_escape(&label_of(i)), md_escape(&i.reference));
            if detail {
                if !i.note.is_empty() {
                    *s += &format!("  - {}\n", i.note);
                }
                let sentence = notice_sentence(&i.notices, "The work");
                if !sentence.is_empty() {
                    let links: Vec<String> = i.notices.iter().filter(|n| !n.doi.is_empty()).map(|n| format!("[{}](https://doi.org/{})", n.label, n.doi)).collect();
                    *s += &format!("  - **{sentence}**{}\n", if links.is_empty() { String::new() } else { format!(" Notice: {}", links.join(", ")) });
                }
                for d in &i.differences {
                    let given = if d.given.is_empty() { "not in the reference".to_string() } else { format!("“{}” in the reference", d.given) };
                    *s += &format!("  - {}: {given}, “{}” in {}\n", d.field, d.found, i.source);
                }
                if i.found.is_some() {
                    *s += &format!("  - Found: {}\n", found_line(i));
                }
            } else if !i.link.is_empty() {
                *s += &format!("  - <{}> ({})\n", i.link, i.source);
            }
        }
    };
    let flagged: Vec<&RefCheckItem> = r.items.iter().filter(|i| level(&i.notices) > NoticeLevel::None).collect();
    section(&mut s, "Retracted, withdrawn, corrected or with an expression of concern", flagged, true);
    section(&mut s, "DOI points to a different work", r.items.iter().filter(|i| i.verdict == RefVerdict::WrongDoi).collect(), true);
    section(&mut s, "Could not be found in Crossref, OpenAlex or PubMed", r.items.iter().filter(|i| i.verdict == RefVerdict::NotFound).collect(), true);
    section(&mut s, "Minor differences", r.items.iter().filter(|i| i.verdict == RefVerdict::MinorDifferences).collect(), true);
    section(&mut s, "Couldn't be checked", r.items.iter().filter(|i| i.verdict == RefVerdict::Failed).collect(), true);
    section(&mut s, "Not looked up (not papers)", r.items.iter().filter(|i| i.verdict == RefVerdict::NotChecked).collect(), false);
    section(&mut s, "Verified", r.items.iter().filter(|i| i.verdict == RefVerdict::Verified).collect(), false);
    s
}

pub fn verdict_name(v: RefVerdict) -> &'static str {
    match v {
        RefVerdict::Verified => "verified",
        RefVerdict::MinorDifferences => "minor differences",
        RefVerdict::NotFound => "not found",
        RefVerdict::WrongDoi => "DOI points to a different work",
        RefVerdict::NotChecked => "not looked up",
        RefVerdict::Failed => "couldn't be checked",
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn csv(r: &RefCheckReport) -> String {
    let mut s = String::from("number,key,result,reference,found_title,found_first_author,found_year,found_venue,found_doi,link,source,differences,notices,note\n");
    for i in &r.items {
        let f = i.found.clone().unwrap_or_default();
        let diffs: Vec<String> = i.differences.iter().map(|d| format!("{}: {} / {}", d.field, d.given, d.found)).collect();
        let notices: Vec<String> = i.notices.iter().map(|n| format!("{} {} {}", n.label, n.date, n.doi).trim().to_string()).collect();
        let row = [
            i.number.to_string(),
            i.key.clone(),
            verdict_name(i.verdict).to_string(),
            i.reference.clone(),
            f.title.clone(),
            f.authors.first().cloned().unwrap_or_default(),
            f.year(),
            f.journal.clone(),
            f.doi.clone(),
            i.link.clone(),
            i.source.clone(),
            diffs.join("; "),
            notices.join("; "),
            i.note.clone(),
        ];
        s += &row.iter().map(|x| csv_field(x)).collect::<Vec<_>>().join(",");
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------- for the apps

/// The services, with answers kept a day in `cache_dir` (none when empty).
pub(crate) fn with_live_net<T>(cache_dir: &str, ttl: Duration, f: impl FnOnce(&dyn Net) -> T) -> T {
    let live = LiveNet;
    let file = (!cache_dir.is_empty()).then(|| std::path::PathBuf::from(cache_dir).join("checks").join("answers.json"));
    let cached = Cached::new(&live, ttl, file);
    f(&cached)
}

pub(crate) fn say(progress: &Option<Arc<dyn ProgressListener>>) -> impl Fn(String) + '_ {
    move |m: String| {
        if let Some(p) = progress {
            p.on_progress(m);
        }
    }
}

const DAY: Duration = Duration::from_secs(24 * 3600);

/// Checks a reference list given as text: a .bib, a .ris, or a pasted list (numbered or
/// author–year). `cache_dir`: where answers are kept for a day ("" for nowhere). Blocking (about
/// a second per reference): call off the main thread.
#[uniffi::export]
pub fn check_references(text: String, cache_dir: String, progress: Option<Arc<dyn ProgressListener>>) -> RefCheckReport {
    let refs = parse_input(&text);
    with_live_net(&cache_dir, DAY, |net| check_all(net, &refs, &say(&progress)))
}

/// Checks references given one per entry (a PDF's reference list as the app read it).
#[uniffi::export]
pub fn check_reference_entries(entries: Vec<String>, cache_dir: String, progress: Option<Arc<dyn ProgressListener>>) -> RefCheckReport {
    let refs: Vec<RefInput> = entries
        .into_iter()
        .filter(|e| !e.trim().is_empty())
        .enumerate()
        .map(|(i, e)| {
            let label = re!(r"^\s*(?:\[(\d{1,4})\]|\((\d{1,4})\)|(\d{1,4})[.)])\s+").captures(&e).and_then(|c| (1..=3).find_map(|k| c.get(k)).map(|m| m.as_str().to_string()));
            RefInput { key: label.unwrap_or_else(|| (i + 1).to_string()), text: e.split_whitespace().collect::<Vec<_>>().join(" "), ..Default::default() }
        })
        .collect();
    with_live_net(&cache_dir, DAY, |net| check_all(net, &refs, &say(&progress)))
}

/// How many references a text holds, as the check would read it (for "Check 42 References").
#[uniffi::export]
pub fn count_references(text: String) -> u32 {
    parse_input(&text).len() as u32
}

/// A paper's reference list from its reading view (the converters' `ol.refs` and the other lists
/// the reader's References panel reads), one entry each.
pub fn epub_references(epub: &std::path::Path) -> Vec<String> {
    let Ok(f) = std::fs::File::open(epub) else { return vec![] };
    let Ok(mut zip) = zip::ZipArchive::new(f) else { return vec![] };
    let mut out = vec![];
    for i in 0..zip.len() {
        let Ok(mut file) = zip.by_index(i) else { continue };
        let name = file.name().map(|n| n.to_lowercase()).unwrap_or_default();
        if !(name.ends_with(".xhtml") || name.ends_with(".html") || name.ends_with(".htm")) {
            continue;
        }
        let mut html = String::new();
        if std::io::Read::read_to_string(&mut file, &mut html).is_err() {
            continue;
        }
        out.extend(html_references(&html));
    }
    out
}

/// The entries of the reference lists in a page: `<ol class="refs">` (and `ul.refs`,
/// `.references`, `.ltx_biblist` lists), one per `<li>`.
pub fn html_references(html: &str) -> Vec<String> {
    let mut out = vec![];
    for list in re!(r#"(?is)<(ol|ul)\b[^>]*class\s*=\s*"[^"]*\b(?:refs|references|ltx_biblist)\b[^"]*"[^>]*>(.*?)</(?:ol|ul)>"#).captures_iter(html) {
        for li in re!(r"(?is)<li\b[^>]*>(.*?)</li>").captures_iter(&list[2]) {
            let t = html_text(&li[1]);
            if t.chars().filter(|c| c.is_alphabetic()).count() >= 8 {
                out.push(t);
            }
        }
    }
    out
}

fn html_text(s: &str) -> String {
    let t = re!(r"(?s)<[^>]+>").replace_all(s, " ");
    let t = re!(r"&#(x[0-9a-fA-F]+|\d+);").replace_all(&t, |c: &regex::Captures| {
        let v = &c[1];
        let n = if let Some(h) = v.strip_prefix('x') { u32::from_str_radix(h, 16).ok() } else { v.parse().ok() };
        n.and_then(char::from_u32).map(|c| c.to_string()).unwrap_or_default()
    });
    let t = t.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}
