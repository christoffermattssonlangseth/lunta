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
use hayagriva::citationberg::taxonomy::Locator;
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, CitePurpose, ElemChild, ElemChildren, ElemMeta,
    Formatted, LocatorPayload, SpecificLocator,
};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
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
    /// In a raised citation, the start of `text` that is not raised: the writer's prefix ("see "
    /// before a superscript "4"). Empty otherwise.
    pub before: String,
    /// The end of `text` that is not raised: the writer's suffix (" for a review"). Empty otherwise.
    pub after: String,
}

/// A reference list entry: the paper's id and the entry (Markdown *italic* and **bold**).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CslEntry {
    pub id: String,
    pub text: String,
}

/// One paper in a citation, with what the writer added to it (as in Zotero's citation dialog).
/// Only `id` is needed; the rest empty is a plain citation.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Default)]
pub struct CslCiteItem {
    pub id: String,
    /// Where in the work: "12", "3–5" (the style adds the label: "p. 12").
    #[uniffi(default = "")]
    pub locator: String,
    /// The locator's kind, a CSL locator type: "page" (also when empty), "chapter", "section",
    /// "figure", "table", "paragraph", "line", "note", "volume", "equation", "supplement", "appendix"…
    #[uniffi(default = "")]
    pub label: String,
    /// Before the paper's part of the citation ("see ").
    #[uniffi(default = "")]
    pub prefix: String,
    /// After it (", for a review").
    #[uniffi(default = "")]
    pub suffix: String,
    /// "" as the style has it; "suppress": without the author ("(2024)"); "narrative": the author
    /// in the text, the rest in brackets ("Kukanja et al. (2024)").
    #[uniffi(default = "")]
    pub mode: String,
}

impl CslCiteItem {
    pub fn plain(id: &str) -> Self {
        CslCiteItem { id: id.to_string(), ..Default::default() }
    }
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
/// Also whether the style prints locators (Cell, Nature and other superscript styles don't) and,
/// for those that don't, the style without collapsing numbers into ranges, for citations whose
/// locators Lunta adds ("1(p1),2,3", not "1(p1)–3").
/// Parsed once per style and kept while its files are unchanged (Update All and every journal-style
/// request asked for it again: reading both files, two regexes and parsing the XML twice).
fn load_style(cache_dir: &str, id: &str) -> Result<Arc<LoadedStyle>> {
    type Cache = Mutex<HashMap<PathBuf, (Vec<(PathBuf, Option<SystemTime>)>, Arc<LoadedStyle>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let stamp = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let key = csl_dir(cache_dir)?.join(format!("{}.csl", clean_id(id)?));
    let cache = CACHE.get_or_init(Default::default);
    if let Some((files, loaded)) = cache.lock().unwrap().get(&key) {
        if files.iter().all(|(p, t)| t.is_some() && stamp(p) == *t) {
            return Ok(loaded.clone());
        }
    }
    let (loaded, parent) = parse_style(cache_dir, id)?;
    let loaded = Arc::new(loaded);
    let mut files = vec![key.clone()];
    if let Some(parent) = parent {
        files.push(csl_dir(cache_dir)?.join(format!("{parent}.csl")));
    }
    let files = files.into_iter().map(|p| { let t = stamp(&p); (p, t) }).collect();
    cache.lock().unwrap().insert(key, (files, loaded.clone()));
    Ok(loaded)
}

/// A style's independent style, its title, locale and the style without collapsing (`load_style`).
type LoadedStyle = (IndependentStyle, String, Option<String>, Option<IndependentStyle>);

/// `load_style` without the cache, and the id of the independent parent it read, if any.
fn parse_style(cache_dir: &str, id: &str) -> Result<(LoadedStyle, Option<String>)> {
    let id = clean_id(id)?;
    let xml = style_xml(cache_dir, &id)?;
    let title = tag_text(&xml, "title").unwrap_or_else(|| id.clone());
    let locale = attr(&xml, "default-locale");
    let parent = xml
        .split("<link")
        .skip(1)
        .find(|l| l.contains("independent-parent"))
        .and_then(|l| attr(l, "href"));
    let parent = parent.map(|href| clean_id(&href)).transpose()?;
    let independent = match &parent {
        Some(p) => style_xml(cache_dir, p)?,
        None => xml,
    };
    // hayagriva puts the cite-group delimiter between *different* authors when a style collapses
    // by year ("(Kukanja et al., 2024, Lee, 2021)"); without collapsing, each work is cited in full
    let re = regex::Regex::new(r#"\s(collapse="year[a-z-]*"|cite-group-delimiter="[^"]*")"#).unwrap();
    let independent = re.replace_all(&independent, "").into_owned();
    let style = IndependentStyle::from_xml(&independent).map_err(|e| CoreError::Invalid(format!("citation style {id}: {e}")))?;
    let locators = regex::Regex::new(r#"variable="[^"]*\blocator\b"#).unwrap().is_match(&independent);
    let uncollapsed = if locators {
        None
    } else {
        let re = regex::Regex::new(r#"\scollapse="[^"]*""#).unwrap();
        IndependentStyle::from_xml(&re.replace_all(&independent, "")).ok()
    };
    Ok(((style, title, locale, uncollapsed), parent))
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
        WebPage => "webpage",
        BlogPost => "post-weblog",
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
    // a web page or blog post: the site as its container, the day it was read
    if kind == "webpage" || kind == "post-weblog" {
        let parts: Vec<i64> = m.accessed.split('-').filter_map(|x| x.parse().ok()).collect();
        if !parts.is_empty() {
            j["accessed"] = serde_json::json!({ "date-parts": [parts] });
        }
    }
    if kind == "report" && !m.issue.is_empty() {
        j["number"] = guard(&m.issue).into();
    }
    serde_json::from_value(j).expect("CSL-JSON item")
}

/// Formats citations (each a list of paper ids, in document order) and the reference list.
pub fn render(cache_dir: &str, style_id: &str, papers: &[Paper], citations: &[Vec<String>]) -> Result<CslRendered> {
    let items: Vec<Vec<CslCiteItem>> = citations.iter().map(|ids| ids.iter().map(|id| CslCiteItem::plain(id)).collect()).collect();
    render_items(cache_dir, style_id, papers, &items)
}

/// `render`, with each cited paper's locator, prefix, suffix and author mode (`CslCiteItem`).
/// The locator and the narrative form are the engine's (the style decides "p. 12" and its place);
/// the prefix and suffix go around the paper's part of the citation, and "suppress" takes the
/// author's names out of it. In a raised (superscript) citation the prefix of its first paper and
/// the suffix of its last are not raised (`CslCitation::before`, `after`: "see ⁴"), and a style
/// that prints no locators (Cell, Nature) gets each one after the paper's number, as the built-in
/// superscript styles write them ("⁴⁽ᵖ⁵⁾", "[4, p. 5]"), so the page the writer gave isn't lost.
pub fn render_items(cache_dir: &str, style_id: &str, papers: &[Paper], citations: &[Vec<CslCiteItem>]) -> Result<CslRendered> {
    let loaded = load_style(cache_dir, style_id)?;
    let (style, title, locale, uncollapsed) = (&loaded.0, loaded.1.clone(), loaded.2.clone(), &loaded.3);
    let prints_locators = uncollapsed.is_none();
    let locale = locale.map(LocaleCode);
    let items: Vec<Item> = papers.iter().map(item).collect();
    let find = |id: &str| papers.iter().position(|p| p.id == id).map(|i| &items[i]);
    let mut driver = BibliographyDriver::new();
    // per citation, the options of the papers in the order given to the engine
    let mut asked: Vec<Vec<&CslCiteItem>> = Vec::new();
    for list in citations {
        let mut cited: Vec<CitationItem<Item>> = Vec::new();
        let mut opts = Vec::new();
        for c in list {
            let Some(entry) = find(&c.id) else { continue };
            let locator = (!c.locator.trim().is_empty()).then(|| {
                SpecificLocator(locator_kind(&c.label), LocatorPayload::Str(c.locator.trim()))
            });
            let purpose = (c.mode == "narrative").then_some(CitePurpose::Prose);
            cited.push(CitationItem::new(entry, locator, None, false, purpose));
            opts.push(c);
        }
        let adds_locators = !prints_locators && opts.iter().any(|o: &&CslCiteItem| !o.locator.trim().is_empty());
        let st = if adds_locators { uncollapsed.as_ref().unwrap_or(style) } else { style };
        driver.citation(CitationRequest::new(cited, st, locale.clone(), locales(), None));
        asked.push(opts);
    }
    let out = driver.finish(BibliographyRequest::new(style, locale, locales()));
    let citations = out
        .citations
        .iter()
        .zip(asked.iter())
        .map(|(c, opts)| {
            let mut tree = c.citation.clone();
            let (mut before, mut after) = (String::new(), String::new());
            let add_locators = !prints_locators && opts.iter().any(|o| !o.locator.trim().is_empty());
            if add_locators || opts.iter().any(|o| !o.prefix.is_empty() || !o.suffix.is_empty() || o.mode == "suppress") {
                (tree, before, after) = with_options(tree, opts, add_locators);
            }
            let mut html = String::new();
            let _ = tree.write_buf(&mut html, BufWriteFormat::Html);
            let t = html.trim();
            let text = unguard(&tidy(&strip_tags(t)));
            let superscript = all_superscript(t);
            if !superscript || text.is_empty() {
                // nothing raised: the affixes are plain text with the rest
                let text = format!("{before}{text}{after}").trim().to_string();
                return CslCitation { text, superscript, before: String::new(), after: String::new() };
            }
            CslCitation { text: format!("{before}{text}{after}"), superscript, before, after }
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

/// A CSL locator type by name ("page" when empty or unknown).
fn locator_kind(label: &str) -> Locator {
    let l = label.trim().to_lowercase();
    if l.is_empty() {
        return Locator::Page;
    }
    l.parse().unwrap_or(Locator::Page)
}

/// The engine's citation with each paper's prefix and suffix around its part, and the author's
/// names taken out of a part whose paper is cited without them ("suppress"). The engine marks
/// each paper's part (`ElemMeta::Entry`, by its place in the request). `add_locators`: the style
/// prints none, so Lunta adds them. Returns the citation and, for a raised one, the words before
/// and after it that are not raised.
fn with_options(tree: ElemChildren, opts: &[&CslCiteItem], add_locators: bool) -> (ElemChildren, String, String) {
    fn first_format(c: &ElemChildren) -> Option<hayagriva::Formatting> {
        c.0.iter().find_map(|x| match x {
            ElemChild::Text(t) => Some(t.formatting),
            ElemChild::Elem(e) => first_format(&e.children),
            _ => None,
        })
    }
    /// Removes the first names element and the punctuation after it ("Kukanja et al., 2024" → "2024").
    fn drop_names(c: &mut ElemChildren) -> bool {
        for i in 0..c.0.len() {
            if let ElemChild::Elem(e) = &mut c.0[i] {
                if matches!(e.meta, Some(ElemMeta::Names(_))) {
                    c.0.remove(i);
                    // the delimiter that followed the names
                    while i < c.0.len() {
                        match &mut c.0[i] {
                            ElemChild::Text(t) if t.text.trim_start_matches(|ch: char| ch.is_whitespace() || ",;:".contains(ch)).is_empty() => {
                                c.0.remove(i);
                            }
                            ElemChild::Text(t) => {
                                t.text = t.text.trim_start_matches(|ch: char| ch.is_whitespace() || ",;:".contains(ch)).to_string();
                                break;
                            }
                            _ => break,
                        }
                    }
                    return true;
                }
                if drop_names(&mut e.children) {
                    // a group that started with the names: its own leading delimiter
                    if let Some(ElemChild::Text(t)) = e.children.0.first_mut() {
                        t.text = t.text.trim_start_matches(|ch: char| ch.is_whitespace() || ",;:".contains(ch)).to_string();
                    }
                    return true;
                }
            }
        }
        false
    }
    /// The papers' parts in the order the engine wrote them (by their place in the request).
    fn order(c: &ElemChildren, out: &mut Vec<usize>) {
        for child in &c.0 {
            if let ElemChild::Elem(e) = child {
                match e.meta {
                    Some(ElemMeta::Entry(i)) => out.push(i),
                    _ => order(&e.children, out),
                }
            }
        }
    }
    struct Walk<'a> {
        opts: &'a [&'a CslCiteItem],
        seen: Vec<bool>,
        /// the part whose prefix / suffix go outside a raised citation
        outer_prefix: Option<usize>,
        outer_suffix: Option<usize>,
        add_locators: bool,
        raised: bool,
    }
    fn walk(c: ElemChildren, w: &mut Walk) -> ElemChildren {
        let mut out = Vec::new();
        for child in c.0 {
            match child {
                ElemChild::Elem(mut e) => {
                    if let Some(ElemMeta::Entry(i)) = e.meta {
                        if let Some(o) = w.opts.get(i) {
                            w.seen[i] = true;
                            if o.mode == "suppress" {
                                drop_names(&mut e.children);
                            }
                            let f = first_format(&e.children).unwrap_or_default();
                            if !o.prefix.is_empty() && w.outer_prefix != Some(i) {
                                e.children.0.insert(0, ElemChild::Text(Formatted { text: guard(&affix(&o.prefix, true)), formatting: f }));
                            }
                            if w.add_locators && !o.locator.trim().is_empty() {
                                e.children.0.push(ElemChild::Text(Formatted { text: guard(&added_locator(o, w.raised)), formatting: f }));
                            }
                            if !o.suffix.is_empty() && w.outer_suffix != Some(i) {
                                e.children.0.push(ElemChild::Text(Formatted { text: guard(&affix(&o.suffix, false)), formatting: f }));
                            }
                        }
                        out.push(ElemChild::Elem(e));
                    } else {
                        e.children = walk(e.children, w);
                        out.push(ElemChild::Elem(e));
                    }
                }
                other => out.push(other),
            }
        }
        ElemChildren(out)
    }
    let f = first_format(&tree).unwrap_or_default();
    // a raised citation ("⁴"): the writer's words before and after it stay on the line ("see ⁴")
    let raised = f.vertical_align == hayagriva::citationberg::VerticalAlign::Sup;
    let mut parts = Vec::new();
    order(&tree, &mut parts);
    let mut w = Walk {
        opts,
        seen: vec![false; opts.len()],
        outer_prefix: if raised { parts.first().copied() } else { None },
        outer_suffix: if raised { parts.last().copied() } else { None },
        add_locators,
        raised,
    };
    let mut tree = walk(tree, &mut w);
    let (mut before, mut after) = (String::new(), String::new());
    if let Some(o) = w.outer_prefix.and_then(|i| opts.get(i)) {
        before = affix(&o.prefix, true);
    }
    if let Some(o) = w.outer_suffix.and_then(|i| opts.get(i)) {
        after = affix(&o.suffix, false);
    }
    // a part the engine merged into another (a collapsed range "1–3"): its prefix at the start and
    // its suffix (and a locator the style doesn't print) at the end of the citation, so nothing the
    // writer typed is lost
    for (i, o) in opts.iter().enumerate() {
        if w.seen[i] {
            continue;
        }
        if !o.prefix.is_empty() {
            if raised {
                before = format!("{}{before}", affix(&o.prefix, true));
            } else {
                tree.0.insert(0, ElemChild::Text(Formatted { text: guard(&affix(&o.prefix, true)), formatting: f }));
            }
        }
        if add_locators && !o.locator.trim().is_empty() {
            tree.0.push(ElemChild::Text(Formatted { text: guard(&added_locator(o, raised)), formatting: f }));
        }
        if !o.suffix.is_empty() {
            if raised {
                after.push_str(&affix(&o.suffix, false));
            } else {
                tree.0.push(ElemChild::Text(Formatted { text: guard(&affix(&o.suffix, false)), formatting: f }));
            }
        }
    }
    (tree, before, after)
}

/// A locator the style doesn't print, as the built-in styles write it: after a raised number
/// "(p5)" (AMA's form), else ", p. 5".
fn added_locator(o: &CslCiteItem, raised: bool) -> String {
    const SHORT: [(&str, &str, &str); 12] = [
        ("page", "p.", "pp."), ("chapter", "chap.", "chaps."), ("section", "sec.", "secs."), ("figure", "fig.", "figs."),
        ("table", "tbl.", "tbls."), ("paragraph", "para.", "paras."), ("line", "l.", "ll."), ("note", "n.", "nn."),
        ("volume", "vol.", "vols."), ("equation", "eq.", "eqs."), ("supplement", "suppl.", "suppls."), ("appendix", "app.", "apps."),
    ];
    let loc = regex::Regex::new(r"(\d)\s*-\s*(\d)").unwrap().replace_all(o.locator.trim(), "$1–$2").into_owned();
    let kind = o.label.trim().to_lowercase();
    let (_, one, many) = SHORT.iter().find(|(k, _, _)| *k == kind).copied().unwrap_or(SHORT[0]);
    let plural = loc.contains(['–', ',', '&']) || loc.contains(" and ");
    let label = if plural { many } else { one };
    if raised {
        format!("({}{loc})", label.replace('.', ""))
    } else {
        format!(", {label} {loc}")
    }
}

/// A prefix ends with a space and a suffix starts with one, unless the writer gave punctuation
/// ("see" → "see ", "for a review" → " for a review", ", emphasis added" stays).
fn affix(s: &str, prefix: bool) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    if prefix {
        format!("{t} ")
    } else if t.starts_with(|c: char| ",.;:)]".contains(c)) {
        t.to_string()
    } else {
        format!(" {t}")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PaperMeta;

    /// An author–date style that prints locators ("(Smith, 2020, p. 12)"), and a numbered one.
    const AUTHOR_DATE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
  <info><title>Test Locators</title><id>http://www.zotero.org/styles/test-locators</id><category citation-format="author-date"/><updated>2024-01-01T00:00:00+00:00</updated></info>
  <macro name="author"><names variable="author"><name form="short" and="symbol" et-al-min="3" et-al-use-first="1"/></names></macro>
  <macro name="year"><date variable="issued"><date-part name="year"/></date></macro>
  <citation>
    <sort><key macro="author"/><key macro="year"/></sort>
    <layout prefix="(" suffix=")" delimiter="; "><group delimiter=", "><text macro="author"/><text macro="year"/><group delimiter=" "><label variable="locator" form="short"/><text variable="locator"/></group></group></layout>
  </citation>
  <bibliography><layout><names variable="author"><name/></names><text prefix=" " variable="title"/></layout></bibliography>
</style>"#;
    const NUMERIC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
  <info><title>Test Numeric Locators</title><id>http://www.zotero.org/styles/test-numeric-locators</id><category citation-format="numeric"/><updated>2024-01-01T00:00:00+00:00</updated></info>
  <citation>
    <layout prefix="[" suffix="]" delimiter="; "><group delimiter=", "><text variable="citation-number"/><group delimiter=" "><label variable="locator" form="short"/><text variable="locator"/></group></group></layout>
  </citation>
  <bibliography><layout><text variable="citation-number" suffix=". "/><text variable="title"/></layout></bibliography>
</style>"#;

    /// Superscript numbers as Cell writes them: no locators, ranges collapsed.
    const SUPERSCRIPT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
  <info><title>Test Superscript</title><id>http://www.zotero.org/styles/test-superscript</id><category citation-format="numeric"/><updated>2024-01-01T00:00:00+00:00</updated></info>
  <citation collapse="citation-number"><sort><key variable="citation-number"/></sort>
    <layout vertical-align="sup" delimiter=","><text variable="citation-number"/></layout>
  </citation>
  <bibliography><layout><text variable="citation-number" suffix=". "/><text variable="title"/></layout></bibliography>
</style>"#;
    /// Superscript numbers with the locator in brackets, as AMA does: "4(p5)".
    const SUPERSCRIPT_LOCATORS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
  <info><title>Test Superscript Locators</title><id>http://www.zotero.org/styles/test-superscript-locators</id><category citation-format="numeric"/><updated>2024-01-01T00:00:00+00:00</updated></info>
  <citation><sort><key variable="citation-number"/></sort>
    <layout vertical-align="sup" delimiter=","><group><text variable="citation-number"/><group prefix="(" suffix=")"><label variable="locator" form="short" strip-periods="true"/><text variable="locator"/></group></group></layout>
  </citation>
  <bibliography><layout><text variable="citation-number" suffix=". "/><text variable="title"/></layout></bibliography>
</style>"#;

    fn cache() -> String {
        let dir = std::env::temp_dir().join(format!("lunta-csl-items-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(dir.join("csl")).unwrap();
        std::fs::write(dir.join("csl/test-locators.csl"), AUTHOR_DATE).unwrap();
        std::fs::write(dir.join("csl/test-numeric-locators.csl"), NUMERIC).unwrap();
        std::fs::write(dir.join("csl/test-superscript.csl"), SUPERSCRIPT).unwrap();
        std::fs::write(dir.join("csl/test-superscript-locators.csl"), SUPERSCRIPT_LOCATORS).unwrap();
        dir.to_string_lossy().into()
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    fn paper(id: &str, family: &str, year: &str, title: &str) -> Paper {
        crate::cited_paper(id, PaperMeta { title: title.into(), authors: vec![format!("Ann {family}")], date: year.into(), ..Default::default() })
    }

    fn it(id: &str, f: impl FnOnce(&mut CslCiteItem)) -> CslCiteItem {
        let mut c = CslCiteItem::plain(id);
        f(&mut c);
        c
    }

    #[test]
    fn locators_affixes_and_author_modes() {
        let dir = cache();
        let papers = [paper("a", "Smith", "2020", "Alpha"), paper("b", "Lee", "2021", "Beta <&>")];
        let cites = vec![
            vec![it("a", |c| c.locator = "12".into())],
            vec![it("a", |c| { c.locator = "3-5".into(); c.label = "chapter".into(); c.prefix = "see".into(); }), it("b", |c| c.suffix = "for a review".into())],
            vec![it("a", |c| c.mode = "suppress".into())],
            vec![it("b", |c| { c.mode = "narrative".into(); c.locator = "7".into(); })],
            vec![it("b", |c| { c.prefix = "e.g. <x>".into(); c.suffix = ", & more".into(); })],
            vec![it("a", |_| {})],
        ];
        let r = render_items(&dir, "test-locators", &papers, &cites).unwrap();
        let t: Vec<&str> = r.citations.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(t[0], "(Smith, 2020, p. 12)");
        assert_eq!(t[1], "(Lee, 2021 for a review; see Smith, 2020, chaps. 3–5)");
        assert_eq!(t[2], "(2020)");
        assert_eq!(t[3], "Lee (2021, p. 7)");
        assert_eq!(t[4], "(e.g. <x> Lee, 2021, & more)");
        assert_eq!(t[5], "(Smith, 2020)");

        let r = render_items(&dir, "test-numeric-locators", &papers, &[
            vec![it("a", |c| c.locator = "12".into())],
            vec![it("b", |c| { c.prefix = "see".into(); c.mode = "suppress".into(); })],
        ]).unwrap();
        let t: Vec<&str> = r.citations.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(t, ["[1, p. 12]", "[see 2]"]);
        assert!(r.citations.iter().all(|c| c.before.is_empty() && c.after.is_empty()));
    }

    #[test]
    fn superscript_citations_keep_the_writers_words_on_the_line_and_their_pages() {
        let dir = cache();
        let papers = [paper("a", "Smith", "2020", "Alpha"), paper("b", "Lee", "2021", "Beta"), paper("c", "Wu", "2022", "Gamma")];
        let cites = vec![
            vec![it("a", |c| c.locator = "1".into()), it("b", |_| {}), it("c", |_| {})],
            vec![it("c", |c| { c.prefix = "see".into(); c.locator = "5".into(); c.label = "page".into(); })],
            vec![it("b", |c| { c.prefix = "e.g.".into(); c.suffix = "for a review".into(); }), it("c", |c| c.prefix = "and".into())],
            vec![it("a", |_| {}), it("b", |_| {}), it("c", |c| c.locator = "3-5".into())],
            vec![it("c", |c| { c.prefix = "see".into(); c.suffix = "for a review".into(); })],
        ];
        // Cell: no locators in the style, so Lunta adds them; the prefix of the first and the
        // suffix of the last paper aren't raised
        let r = render_items(&dir, "test-superscript", &papers, &cites).unwrap();
        let c: Vec<(&str, bool, &str, &str)> = r.citations.iter().map(|c| (c.text.as_str(), c.superscript, c.before.as_str(), c.after.as_str())).collect();
        assert_eq!(c[0], ("1(p1),2,3", true, "", ""));
        assert_eq!(c[1], ("see 3(p5)", true, "see ", ""));
        // a paper's words inside the citation stay with it, raised
        assert_eq!(c[2], ("e.g. 2 for a review,and 3", true, "e.g. ", ""));
        assert_eq!(c[3], ("1,2,3(pp3–5)", true, "", ""));
        assert_eq!(c[4], ("see 3 for a review", true, "see ", " for a review"));

        // a style that prints locators: its own form, nothing added
        let r = render_items(&dir, "test-superscript-locators", &papers, &cites[..2]).unwrap();
        let c: Vec<(&str, bool, &str)> = r.citations.iter().map(|c| (c.text.as_str(), c.superscript, c.before.as_str())).collect();
        assert_eq!(c[0], ("1(p1),2,3", true, ""));
        assert_eq!(c[1], ("see 3(p5)", true, "see "));
    }

    /// A style that writes web pages as APA does (type, site, address, the day it was read).
    const WEB: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
  <info><title>Test Web</title><id>http://www.zotero.org/styles/test-web</id><category citation-format="author-date"/><updated>2024-01-01T00:00:00+00:00</updated></info>
  <citation><layout prefix="(" suffix=")"><names variable="author"><name form="short"/></names><date variable="issued" prefix=", "><date-part name="year"/></date></layout></citation>
  <bibliography><layout>
    <names variable="author" suffix=". "><name name-as-sort-order="all" initialize-with=". "/></names>
    <choose><if type="post-weblog"><text value="[blog] "/></if><else-if type="webpage"><text value="[web] "/></else-if></choose>
    <text variable="title" suffix=". "/><text variable="container-title" suffix=". "/>
    <date variable="accessed" prefix="Retrieved "><date-part name="month"/><date-part name="day" prefix=" "/><date-part name="year" prefix=", "/></date>
    <text variable="URL" prefix=" from "/>
  </layout></bibliography>
</style>"#;

    #[test]
    fn web_pages_with_their_site_and_access_date() {
        let dir = cache();
        std::fs::write(Path::new(&dir).join("csl/test-web.csl"), WEB).unwrap();
        let blog = crate::cited_paper("k", PaperMeta {
            title: "A Recipe for Training Neural Networks".into(),
            authors: vec!["Andrej Karpathy".into()],
            date: "2019-04-25".into(),
            journal: "Andrej Karpathy blog".into(),
            url: "https://karpathy.github.io/2019/04/25/recipe/".into(),
            work_type: "post-weblog".into(),
            accessed: "2026-11-21".into(),
            ..Default::default()
        });
        let r = render(&dir, "test-web", &[blog], &[vec!["k".into()]]).unwrap();
        assert_eq!(r.citations[0].text, "(Karpathy, 2019)");
        assert_eq!(
            r.bibliography[0].text,
            "Karpathy, A. [blog] A Recipe for Training Neural Networks. Andrej Karpathy blog. Retrieved November 21, 2026 from https://karpathy.github.io/2019/04/25/recipe/"
        );
    }

    #[test]
    fn a_style_parsed_once_and_again_when_its_file_changes() {
        let dir = cache();
        let papers = [paper("a", "Smith", "2020", "Alpha")];
        let one = load_style(&dir, "test-superscript").unwrap();
        assert!(Arc::ptr_eq(&one, &load_style(&dir, "test-superscript").unwrap()), "kept");
        let path = std::path::Path::new(&dir).join("csl/test-superscript.csl");
        std::fs::write(&path, SUPERSCRIPT.replace("<title>Test Superscript</title>", "<title>Renamed</title>")).unwrap();
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(5)).unwrap();
        let out = render(&dir, "test-superscript", &papers, &[vec!["a".into()]]).unwrap();
        assert_eq!(out.style_title, "Renamed");
        assert_eq!(out.citations[0].text, "1");
    }

    /// Cell Systems (an alias of Cell, CC BY-SA 3.0, in testdata/csl), as Word's citation box and
    /// Update All ask for it: `cargo test -p lunta-core --release csl_render_timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn csl_render_timing() {
        let dir = std::env::temp_dir().join(format!("lunta-csl-timing-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(dir.join("csl")).unwrap();
        for f in ["cell.csl", "cell-systems.csl"] {
            std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata/csl").join(f), dir.join("csl").join(f)).unwrap();
        }
        let dir = dir.to_string_lossy().to_string();
        let papers: Vec<Paper> = (0..30).map(|i| paper(&format!("p{i}"), &format!("Author{i}"), "2020", &format!("Title {i}"))).collect();
        let three = vec![vec!["p0".to_string(), "p1".to_string(), "p2".to_string()]];
        let all: Vec<Vec<String>> = (0..26).map(|i| vec![format!("p{i}"), format!("p{}", i + 4)]).collect();
        render(&dir, "cell-systems", &papers[..3], &three).unwrap();
        let t = std::time::Instant::now();
        for _ in 0..100 {
            render(&dir, "cell-systems", &papers[..3], &three).unwrap();
        }
        println!("3 papers, 1 citation: {:.2} ms per render", t.elapsed().as_secs_f64() * 10.0);
        let t = std::time::Instant::now();
        for _ in 0..20 {
            render(&dir, "cell-systems", &papers, &all).unwrap();
        }
        println!("30 papers, 26 citations: {:.2} ms per render", t.elapsed().as_secs_f64() * 50.0);
        let t = std::time::Instant::now();
        for _ in 0..100 {
            load_style(&dir, "cell-systems").unwrap();
        }
        println!("load_style: {:.2} ms", t.elapsed().as_secs_f64() * 10.0);
    }
}
