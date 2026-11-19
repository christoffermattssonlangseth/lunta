//! Journal citation styles: the ~10,000 CSL styles of the Citation Style Language project
//! (as served by zotero.org/styles), formatted with the hayagriva CSL engine.
//!
//! Most journal styles are *dependent*: a few lines naming a base style ("Nature Neuroscience"
//! → "Nature"). Styles and the style list are fetched once and kept in a cache folder the app
//! chooses (`cache_dir`), so a chosen style also works offline afterwards.

use crate::export::split_name;
use crate::metadata::get_text;
use crate::model::{CoreError, Paper};
use hayagriva::citationberg::{json::Item, IndependentStyle, Locale, LocaleCode};
use hayagriva::{BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

type Result<T> = std::result::Result<T, CoreError>;

const INDEX_URL: &str = "https://www.zotero.org/styles-files/styles.json";
const STYLE_URL: &str = "https://www.zotero.org/styles/";
/// The style list changes slowly: refreshed when older than this.
const INDEX_MAX_AGE: Duration = Duration::from_secs(30 * 86400);

/// A journal style in the CSL catalogue.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CslStyleHit {
    /// The style's id ("nature-neuroscience"), as `cite_csl` takes it.
    pub id: String,
    /// "Nature Neuroscience".
    pub title: String,
    /// "numeric", "author-date", "author", "note" or "label".
    pub format: String,
}

/// One citation in a CSL style: its text, and whether the style raises it (superscript).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CslCitation {
    pub text: String,
    pub superscript: bool,
}

/// A reference list entry: the paper's id and the entry (Markdown *italic* and **bold**).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CslEntry {
    pub id: String,
    pub text: String,
}

/// Citations (in the order asked for) and the reference list (in the style's order).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CslRendered {
    pub citations: Vec<CslCitation>,
    pub bibliography: Vec<CslEntry>,
    /// The style's title, for display.
    pub style_title: String,
}

fn csl_dir(cache_dir: &str) -> Result<PathBuf> {
    let d = Path::new(cache_dir).join("csl");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

fn fresh(path: &Path, max_age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() < max_age)
        .unwrap_or(false)
}

/// A style id from user input: "nature", "Nature Neuroscience" won't do, but URLs are trimmed.
fn clean_id(id: &str) -> Result<String> {
    let id = id.trim().trim_end_matches('/').rsplit('/').next().unwrap_or("").to_lowercase();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(CoreError::Invalid(format!("not a style id: {id}")));
    }
    Ok(id)
}

/// Searches the catalogue by journal or style name: every word must appear in the title.
/// Exact and prefix matches first, then shorter titles.
pub fn search(cache_dir: &str, query: &str, limit: usize) -> Result<Vec<CslStyleHit>> {
    let path = csl_dir(cache_dir)?.join("styles.json");
    if !fresh(&path, INDEX_MAX_AGE) {
        match get_text(INDEX_URL) {
            Ok(t) => std::fs::write(&path, t)?,
            Err(e) if !path.exists() => return Err(e),
            Err(_) => {} // offline: the old list will do
        }
    }
    let list: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&path)?)
        .map_err(|e| CoreError::Invalid(format!("style list: {e}")))?;
    let q = query.trim().to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let mut hits: Vec<(u8, CslStyleHit)> = list
        .iter()
        .filter_map(|s| {
            let title = s.get("title")?.as_str()?.to_string();
            let id = s.get("name")?.as_str()?.to_string();
            let t = title.to_lowercase();
            if !words.iter().all(|w| t.contains(w) || id.contains(w)) {
                return None;
            }
            let rank = if t == q { 0 } else if t.starts_with(&q) { 1 } else { 2 };
            let format = s.pointer("/categories/format").and_then(|f| f.as_str()).unwrap_or("").to_string();
            Some((rank, CslStyleHit { id, title, format }))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.title.len().cmp(&b.1.title.len())).then(a.1.title.cmp(&b.1.title)));
    Ok(hits.into_iter().take(limit).map(|(_, h)| h).collect())
}

/// The style's own XML, fetched once and then read from the cache.
fn style_xml(cache_dir: &str, id: &str) -> Result<String> {
    let path = csl_dir(cache_dir)?.join(format!("{id}.csl"));
    if let Ok(x) = std::fs::read_to_string(&path) {
        return Ok(x);
    }
    let xml = get_text(&format!("{STYLE_URL}{id}"))?;
    if !xml.contains("<style") {
        return Err(CoreError::NotFound(format!("citation style {id}")));
    }
    std::fs::write(&path, &xml)?;
    Ok(xml)
}

/// The independent style for an id (following a journal's alias to its base style), its title
/// (the journal's), and the locale the journal asks for, if any.
fn load_style(cache_dir: &str, id: &str) -> Result<(IndependentStyle, String, Option<String>)> {
    let id = clean_id(id)?;
    let xml = style_xml(cache_dir, &id)?;
    let title = tag_text(&xml, "title").unwrap_or_else(|| id.clone());
    let locale = attr(&xml, "default-locale");
    let parent = xml
        .split("<link")
        .skip(1)
        .find(|l| l.contains("independent-parent"))
        .and_then(|l| attr(l, "href"));
    let independent = match parent {
        Some(href) => style_xml(cache_dir, &clean_id(&href)?)?,
        None => xml,
    };
    // hayagriva puts the cite-group delimiter between *different* authors when a style collapses
    // by year ("(Kukanja et al., 2024, Lee, 2021)"); without collapsing, each work is cited in full
    let re = regex::Regex::new(r#"\s(collapse="year[a-z-]*"|cite-group-delimiter="[^"]*")"#).unwrap();
    let independent = re.replace_all(&independent, "").into_owned();
    let style = IndependentStyle::from_xml(&independent).map_err(|e| CoreError::Invalid(format!("citation style {id}: {e}")))?;
    Ok((style, title, locale))
}

fn tag_text(xml: &str, tag: &str) -> Option<String> {
    let start = xml.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(unescape(&xml[start..end]))
}

fn attr(s: &str, name: &str) -> Option<String> {
    let start = s.find(&format!("{name}=\""))? + name.len() + 2;
    let end = s[start..].find('"')? + start;
    Some(s[start..end].to_string())
}

fn locales() -> &'static [Locale] {
    static L: OnceLock<Vec<Locale>> = OnceLock::new();
    L.get_or_init(hayagriva::archive::locales)
}

// The engine writes field text into its HTML as is, so a title with "<" or "&" would read as
// markup ("Signals <10 Hz" lost everything after "Signals"). Fields go in with these characters
// swapped for private-use stand-ins (case changes leave them alone) and come back out after the
// markup is read.
const STAND_INS: [(char, char); 3] = [('<', '\u{E000}'), ('>', '\u{E001}'), ('&', '\u{E002}')];

fn guard(s: &str) -> String {
    s.chars().map(|c| STAND_INS.iter().find(|(a, _)| *a == c).map(|(_, b)| *b).unwrap_or(c)).collect()
}

fn unguard(s: &str) -> String {
    s.chars().map(|c| STAND_INS.iter().find(|(_, b)| *b == c).map(|(a, _)| *a).unwrap_or(c)).collect()
}

/// A paper as CSL-JSON.
/// The CSL item type for a paper's kind.
pub fn csl_type(m: &crate::model::PaperMeta) -> &'static str {
    use crate::worktype::Kind::*;
    match crate::worktype::kind(m) {
        Article => "article-journal",
        Conference => "paper-conference",
        Chapter => "chapter",
        Book => "book",
        Preprint => "article",
        Report => "report",
        Standard => "standard",
        Thesis => "thesis",
        Dataset => "dataset",
        Other => if m.journal.is_empty() { "document" } else { "article-journal" },
    }
}

fn item(p: &Paper) -> Item {
    // titles and names in capitals in proper case (the library keeps them as they are)
    let m = &crate::worktype::for_export(&p.meta);
    let kind = csl_type(m);
    let mut j = serde_json::json!({ "id": p.id, "type": kind, "title": guard(&m.title) });
    let authors: Vec<serde_json::Value> = m
        .authors
        .iter()
        .map(|a| {
            if crate::worktype::is_organization(a) {
                return serde_json::json!({ "literal": guard(a) });
            }
            let a = guard(a);
            let (family, given) = split_name(&a);
            if given.is_empty() { serde_json::json!({ "family": family }) } else { serde_json::json!({ "family": family, "given": given }) }
        })
        .collect();
    if !authors.is_empty() {
        j["author"] = authors.into();
    }
    let parts: Vec<i64> = m.date.split('-').filter_map(|x| x.parse().ok()).collect();
    if !parts.is_empty() {
        j["issued"] = serde_json::json!({ "date-parts": [parts] });
    }
    for (k, v) in [
        ("container-title", &m.journal),
        ("volume", &m.volume),
        ("issue", &m.issue),
        ("page", &m.pages),
        ("DOI", &m.doi),
        ("PMID", &m.pmid),
        ("PMCID", &m.pmcid),
        ("URL", &m.url),
        ("publisher", &m.publisher),
        ("collection-title", &m.series),
    ] {
        if !v.trim().is_empty() {
            j[k] = guard(v.trim()).into();
        }
    }
    // a preprint: the server as publisher and the arXiv id as its number ("arXiv:2308.13418"),
    // as citation styles write preprints
    if kind == "article" {
        if !m.journal.is_empty() && m.publisher.is_empty() {
            j["publisher"] = guard(&m.journal).into();
        }
        if !m.arxiv.is_empty() {
            j["number"] = format!("arXiv:{}", m.arxiv).into();
        }
        j["genre"] = "Preprint".into();
    }
    if kind == "report" && !m.issue.is_empty() {
        j["number"] = guard(&m.issue).into();
    }
    serde_json::from_value(j).expect("CSL-JSON item")
}

/// Formats citations (each a list of paper ids, in document order) and the reference list.
pub fn render(cache_dir: &str, style_id: &str, papers: &[Paper], citations: &[Vec<String>]) -> Result<CslRendered> {
    let (style, title, locale) = load_style(cache_dir, style_id)?;
    let locale = locale.map(LocaleCode);
    let items: Vec<Item> = papers.iter().map(item).collect();
    let find = |id: &str| papers.iter().position(|p| p.id == id).map(|i| &items[i]);
    let mut driver = BibliographyDriver::new();
    for ids in citations {
        let cited: Vec<CitationItem<Item>> = ids.iter().filter_map(|id| find(id)).map(CitationItem::with_entry).collect();
        driver.citation(CitationRequest::new(cited, &style, locale.clone(), locales(), None));
    }
    let out = driver.finish(BibliographyRequest::new(&style, locale, locales()));
    let citations = out
        .citations
        .iter()
        .map(|c| {
            let mut html = String::new();
            let _ = c.citation.write_buf(&mut html, BufWriteFormat::Html);
            let t = html.trim();
            CslCitation { text: unguard(&tidy(&strip_tags(t))), superscript: all_superscript(t) }
        })
        .collect();
    let bibliography = out
        .bibliography
        .map(|b| {
            b.items
                .iter()
                .map(|i| {
                    let mut html = String::new();
                    if let Some(first) = &i.first_field {
                        let _ = first.write_buf(&mut html, BufWriteFormat::Html);
                        html.push(' ');
                    }
                    let _ = i.content.write_buf(&mut html, BufWriteFormat::Html);
                    CslEntry { id: i.key.clone(), text: unguard(&tidy(&html_to_markdown(html.trim()))) }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(CslRendered { citations, bibliography, style_title: title })
}

/// Whether every piece of text in the citation is raised (`<sup>` or vertical-align: super).
fn all_superscript(html: &str) -> bool {
    let re = regex::Regex::new(r#"<sup>.*?</sup>|<span style="vertical-align:\s*super;?">.*?</span>"#).unwrap();
    re.is_match(html) && strip_tags(&re.replace_all(html, "")).trim().is_empty()
}

/// Punctuation the engine doesn't merge, as CSL asks: "2009.. DOI" → "2009. DOI".
fn tidy(s: &str) -> String {
    let re = regex::Regex::new(r"([^.])\.\.(\s|$)").unwrap();
    re.replace_all(s, "$1.$2").replace(" ,", ",").trim().to_string()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&amp;", "&")
}

/// Text without markup.
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    unescape(&out)
}

/// The engine's HTML as Markdown *italic* / **bold** (what the apps and add-ins format).
fn html_to_markdown(html: &str) -> String {
    let mut s = html.to_string();
    // a style's prefix can land inside the italics ("<i>. Cell</i>"): spaces and punctuation at
    // the edges go outside, so *…* hugs words (as the add-ins read it)
    let lead = regex::Regex::new(r#"(<(?:i|em|b|strong|span style="[^"]*")>)([\s.,;:]+)"#).unwrap();
    s = lead.replace_all(&s, "$2$1").into_owned();
    let trail = regex::Regex::new(r#"(\s+)(</(?:i|em|b|strong|span)>)"#).unwrap();
    s = trail.replace_all(&s, "$2$1").into_owned();
    for (tags, md) in [(["<i>", "</i>"], "*"), (["<em>", "</em>"], "*"), (["<b>", "</b>"], "**"), (["<strong>", "</strong>"], "**")] {
        s = s.replace(tags[0], md).replace(tags[1], md);
    }
    // italic or bold set with style="…"
    let re_i = regex::Regex::new(r#"<span style="font-style:\s*italic;?">(.*?)</span>"#).unwrap();
    s = re_i.replace_all(&s, "*$1*").into_owned();
    let re_b = regex::Regex::new(r#"<span style="font-weight:\s*bold;?">(.*?)</span>"#).unwrap();
    s = re_b.replace_all(&s, "**$1**").into_owned();
    strip_tags(&s)
}
