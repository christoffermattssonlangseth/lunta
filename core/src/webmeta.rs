//! Web pages and blog posts as citable items: their details read off the page itself.
//!
//! A link with no scholarly identifier in it (no DOI, PubMed ID, PMCID or arXiv id) is fetched
//! once, as the person asked, with the core's HTTP agent and User-Agent (`metadata::get_page`).
//! A page that declares a DOI (`citation_doi`, Dublin Core, PRISM) is a paper after all: the
//! caller adds it by that DOI. Otherwise its details come from the page, each field from the
//! first of these that has it:
//!
//! 1. Highwire Press `citation_*` meta tags (journals, and blogs that cite themselves),
//! 2. Dublin Core (`dc.*`, `dcterms.*`),
//! 3. schema.org JSON-LD (BlogPosting / Article / NewsArticle / WebPage …: headline, author,
//!    datePublished, publisher, isPartOf),
//! 4. Open Graph and Twitter cards (og:title, article:author, article:published_time,
//!    og:site_name),
//! 5. the page itself: `<title>`, `<meta name="author">`, a byline (`rel="author"`,
//!    `itemprop="author"`, `class="author"/"byline"`), `<time datetime>`, the site's name in its
//!    header or feed link, and as a last resort a date in the address (`/2019/04/25/`).
//!
//! Authors are never made up: only names the page gives. One inference is made, and only on a
//! personal site: a site called "Andrej Karpathy blog" on karpathy.github.io (a name whose
//! family name is in the host) is that person's blog, so the post is theirs.
//!
//! The address and the date it was read (`accessed`, for "Retrieved …" / "Accessed …") are kept.
//! The type is CSL's `post-weblog` for blog posts (a JSON-LD BlogPosting, "blog" in the host or
//! path, a blogging platform: github.io, Medium, Substack, WordPress, Blogspot, Ghost, …) and
//! `webpage` for anything else.

use crate::convert::html;
use crate::convert::xml::{Doc, Node};
use crate::model::{CoreError, PaperMeta, Result};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

macro_rules! re {
    ($p:expr) => {{
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new($p).unwrap())
    }};
}

/// A plain web address: http(s), a host with a dot, and no DOI, PubMed ID, PMCID or arXiv id in
/// it (those are added as papers).
pub fn is_web_link(input: &str) -> bool {
    let s = input.trim();
    let lower = s.to_lowercase();
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if !host.contains('.') || s.contains(char::is_whitespace) {
        return false;
    }
    !crate::metadata::has_identifier(s)
}

/// Today's date (UTC) as "YYYY-MM-DD": when a page was read.
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 as (year, month, day) (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn month_of(w: &str) -> Option<u32> {
    let w = w.trim_end_matches('.').to_lowercase();
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| w.starts_with(m) && (w.len() == 3 || "january february march april may june july august september october november december sept".contains(&w))).map(|i| i as u32 + 1)
}

/// A date as written on pages and in exports, as ISO ("2019-04-25", "2019-04", "2019"); "" when
/// there is none. Takes "2019-04-25T10:00:00Z", "2019/04/25", "Apr 25, 2019", "25 April 2019",
/// "Thursday, April 25, 2019", "April 2019", "2019".
pub fn iso_date(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let ok = |y: u32, m: u32, d: u32| -> String {
        if !(1000..=2999).contains(&y) {
            return String::new();
        }
        match (m, d) {
            (1..=12, 1..=31) => format!("{y:04}-{m:02}-{d:02}"),
            (1..=12, _) => format!("{y:04}-{m:02}"),
            _ => format!("{y:04}"),
        }
    };
    if let Some(c) = re!(r"^(\d{4})(?:[-/.](\d{1,2})(?:[-/.](\d{1,2}))?)?(?:$|[T\s/])").captures(s) {
        let y = c[1].parse().unwrap_or(0);
        let m = c.get(2).and_then(|x| x.as_str().parse().ok()).unwrap_or(0);
        let d = c.get(3).and_then(|x| x.as_str().parse().ok()).unwrap_or(0);
        return ok(y, m, d);
    }
    // "Apr 25, 2019", "April 25 2019"
    if let Some(c) = re!(r"(?i)\b([a-z]{3,9})\.?\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(\d{4})\b").captures(s) {
        if let Some(m) = month_of(&c[1]) {
            return ok(c[3].parse().unwrap_or(0), m, c[2].parse().unwrap_or(0));
        }
    }
    // "25 April 2019", "25. April 2019"
    if let Some(c) = re!(r"(?i)\b(\d{1,2})(?:st|nd|rd|th)?\.?\s+([a-z]{3,9})\.?,?\s+(\d{4})\b").captures(s) {
        if let Some(m) = month_of(&c[2]) {
            return ok(c[3].parse().unwrap_or(0), m, c[1].parse().unwrap_or(0));
        }
    }
    // "April 2019"
    if let Some(c) = re!(r"(?i)\b([a-z]{3,9})\.?\s+(\d{4})\b").captures(s) {
        if let Some(m) = month_of(&c[1]) {
            return ok(c[2].parse().unwrap_or(0), m, 0);
        }
    }
    // "04/25/2019" is ambiguous with "25/04/2019": only the year
    if let Some(c) = re!(r"^\d{1,2}[/.]\d{1,2}[/.](\d{4})$").captures(s) {
        return ok(c[1].parse().unwrap_or(0), 0, 0);
    }
    String::new()
}

/// A date in a web address: "/2019/04/25/", "/2019/04/" (blogs' permalinks), "" when none.
pub fn date_in_url(url: &str) -> String {
    let path = url.split("://").nth(1).and_then(|r| r.find('/').map(|i| &r[i..])).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    if let Some(c) = re!(r"/((?:19|20)\d{2})/(0[1-9]|1[0-2])(?:/(0[1-9]|[12]\d|3[01]))?(?:/|$)").captures(path) {
        return match c.get(3) {
            Some(d) => format!("{}-{}-{}", &c[1], &c[2], d.as_str()),
            None => format!("{}-{}", &c[1], &c[2]),
        };
    }
    String::new()
}

fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_lowercase();
    let host = host.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_string();
    host.strip_prefix("www.").map(str::to_string).unwrap_or(host)
}

/// A blog's address: "blog" in the host or path, or a blogging platform (Medium, Substack,
/// WordPress.com, Blogger, Ghost, Tumblr, Hashnode, dev.to, write.as, …); a GitHub Pages site
/// (github.io) when the post's address is dated ("/2019/04/25/") or under /posts/.
pub fn blog_address(url: &str) -> bool {
    let host = host_of(url);
    if host.is_empty() {
        return false;
    }
    let path = url.split("://").nth(1).and_then(|r| r.find('/').map(|i| r[i..].to_lowercase())).unwrap_or_default();
    let path = path.split(['?', '#']).next().unwrap_or("").to_string();
    let under = |p: &str| host == p || host.ends_with(&format!(".{p}"));
    let platforms = ["medium.com", "substack.com", "wordpress.com", "ghost.io", "tumblr.com", "hashnode.dev", "dev.to", "write.as", "typepad.com", "livejournal.com", "svbtle.com"];
    let blog_path = re!(r"(^|/)(blog|blogs|weblog|posts?)(/|$)").is_match(&path);
    host.contains("blog")
        || platforms.iter().any(|p| under(p))
        || blog_path
        || (under("github.io") && !date_in_url(url).is_empty())
}

/// The address without what only tracks the visit (utm_*, fbclid, gclid, …) and the fragment.
pub fn clean_url(url: &str) -> String {
    let url = url.trim();
    let url = url.split('#').next().unwrap_or(url);
    let Some((base, query)) = url.split_once('?') else { return url.to_string() };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| {
            let k = p.split('=').next().unwrap_or("").to_lowercase();
            !(k.is_empty() || k.starts_with("utm_") || ["fbclid", "gclid", "mc_cid", "mc_eid", "ref_src", "igshid", "si"].contains(&k.as_str()))
        })
        .collect();
    if kept.is_empty() { base.to_string() } else { format!("{base}?{}", kept.join("&")) }
}

/// An address as it is compared: without the scheme, "www.", tracking parameters, the fragment
/// and a trailing slash, the host in lower case ("" for no address).
pub fn url_key(url: &str) -> String {
    let u = clean_url(url);
    let rest = u.split_once("://").map(|x| x.1).unwrap_or(&u);
    let (host, path) = match rest.find(['/', '?']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    format!("{host}{}", path.trim_end_matches('/'))
}

// ---- reading the page ----

/// `<meta>` values by lower-cased name (`name`, `property` or `itemprop`), in page order.
struct Metas(Vec<(String, String)>);

impl Metas {
    fn of(doc: Node) -> Metas {
        let mut out = vec![];
        for m in doc.all(|n| n.name() == "meta") {
            let Some(c) = m.attr("content").map(|c| clean_text(c)).filter(|c| !c.is_empty()) else { continue };
            for k in ["name", "property", "itemprop"] {
                if let Some(n) = m.attr(k) {
                    out.push((n.trim().to_lowercase(), c.clone()));
                }
            }
        }
        Metas(out)
    }
    fn first(&self, names: &[&str]) -> String {
        names.iter().find_map(|n| self.0.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone())).unwrap_or_default()
    }
    fn all(&self, names: &[&str]) -> Vec<String> {
        for n in names {
            let v: Vec<String> = self.0.iter().filter(|(k, _)| k == n).map(|(_, v)| v.clone()).collect();
            if !v.is_empty() {
                return v;
            }
        }
        vec![]
    }
}

/// Text from a page: entities decoded (also when a page escaped them twice), whitespace collapsed.
fn clean_text(s: &str) -> String {
    let s = html::decode(s);
    let s = if s.contains('&') { html::decode(&s) } else { s };
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the JSON-LD on a page says about it.
#[derive(Default, Debug)]
struct LinkedData {
    types: Vec<String>,
    headline: String,
    authors: Vec<String>,
    date: String,
    publisher: String,
    site: String,
    description: String,
    keywords: Vec<String>,
}

const ARTICLE_TYPES: [&str; 14] = [
    "blogposting", "article", "newsarticle", "techarticle", "scholarlyarticle", "report", "socialmediaposting", "liveblogposting",
    "analysisnewsarticle", "opinionnewsarticle", "reportagenewsarticle", "reviewnewsarticle", "backgroundnewsarticle", "advertisercontentarticle",
];

fn json_types(v: &Value) -> Vec<String> {
    match &v["@type"] {
        Value::String(s) => vec![s.to_lowercase()],
        Value::Array(a) => a.iter().filter_map(|x| x.as_str()).map(|s| s.to_lowercase()).collect(),
        _ => vec![],
    }
}

fn json_str(v: &Value) -> String {
    match v {
        Value::String(s) => clean_text(s),
        Value::Array(a) => a.iter().map(json_str).find(|s| !s.is_empty()).unwrap_or_default(),
        Value::Object(_) => json_str(&v["name"]).or_else_empty(|| json_str(&v["@value"])),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

trait OrElseEmpty {
    fn or_else_empty(self, f: impl FnOnce() -> String) -> String;
}
impl OrElseEmpty for String {
    fn or_else_empty(self, f: impl FnOnce() -> String) -> String {
        if self.is_empty() { f() } else { self }
    }
}

fn json_names(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().flat_map(json_names).collect(),
        Value::Object(o) => {
            let n = json_str(o.get("name").unwrap_or(&Value::Null));
            if n.is_empty() { vec![] } else { vec![n] }
        }
        Value::String(s) => vec![clean_text(s)],
        _ => vec![],
    }
}

/// Every JSON object in the page's `application/ld+json` scripts (arrays and `@graph` unpacked).
fn json_ld_objects(doc: Node) -> Vec<Value> {
    let mut out = vec![];
    fn walk(v: Value, out: &mut Vec<Value>) {
        match v {
            Value::Array(a) => a.into_iter().for_each(|x| walk(x, out)),
            Value::Object(ref o) => {
                if let Some(g) = o.get("@graph").cloned() {
                    walk(g, out);
                }
                out.push(v);
            }
            _ => {}
        }
    }
    for s in doc.all(|n| n.name() == "script" && n.attr("type").is_some_and(|t| t.to_lowercase().contains("ld+json"))) {
        let text = s.all_text();
        let text = text.trim().trim_start_matches("<!--").trim_end_matches("-->").trim();
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            walk(v, &mut out);
        }
    }
    out
}

fn linked_data(doc: Node) -> LinkedData {
    let objects = json_ld_objects(doc);
    let mut ld = LinkedData::default();
    // the article (a post, a news story) beats the page around it
    let article = objects.iter().find(|o| json_types(o).iter().any(|t| ARTICLE_TYPES.contains(&t.as_str())));
    let page = objects.iter().find(|o| json_types(o).iter().any(|t| t.ends_with("webpage") || t == "collectionpage"));
    let site = objects.iter().find(|o| json_types(o).iter().any(|t| t == "website" || t == "blog"));
    for o in objects.iter() {
        ld.types.extend(json_types(o));
    }
    if let Some(a) = article.or(page) {
        ld.headline = json_str(&a["headline"]).or_else_empty(|| json_str(&a["name"]));
        ld.authors = json_names(&a["author"]);
        if ld.authors.is_empty() {
            ld.authors = json_names(&a["creator"]);
        }
        ld.date = iso_date(&json_str(&a["datePublished"]).or_else_empty(|| json_str(&a["dateCreated"])));
        ld.publisher = json_str(&a["publisher"]);
        ld.description = json_str(&a["description"]);
        ld.site = json_str(&a["isPartOf"]);
        ld.keywords = match &a["keywords"] {
            Value::Array(k) => k.iter().map(json_str).filter(|s| !s.is_empty()).collect(),
            Value::String(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
            _ => vec![],
        };
    }
    if ld.site.is_empty() {
        if let Some(s) = site {
            ld.site = json_str(&s["name"]);
        }
    }
    ld
}

/// Names from a byline or meta author: "By Jane Doe and John Smith" → two names. URLs, handles
/// ("@karpathy") and e-mail addresses are not names.
fn names_from(s: &str) -> Vec<String> {
    let s = clean_text(s);
    let s = re!(r"(?i)^(written\s+)?by\s+").replace(&s, "").to_string();
    let s = re!(r"(?i)\s*[|·•—–]\s*.*$").replace(&s, "").to_string();
    s.split(" and ")
        .flat_map(|p| p.split([';', '&']))
        .map(|p| p.trim().trim_matches(',').trim().to_string())
        .filter(|p| {
            !p.is_empty()
                && p.chars().count() <= 80
                && !p.contains("://")
                && !p.starts_with("www.")
                && !p.starts_with('@')
                && !p.contains('@')
                && p.chars().any(|c| c.is_alphabetic())
                && !["admin", "administrator", "staff", "editor", "guest", "unknown", "anonymous"].contains(&p.to_lowercase().as_str())
        })
        .collect()
}

fn add_names(into: &mut Vec<String>, names: Vec<String>) {
    for n in names {
        if !into.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
            into.push(n);
        }
    }
}

/// The name of the site, from what the page shows: its header's site title, the title of its feed.
fn site_from_page(doc: Node) -> String {
    let title = doc
        .first(|n| {
            n.name() == "a" && n.name_tokens().iter().any(|t| t == "site" || t == "brand" || t == "logo")
                && n.name_tokens().iter().any(|t| t == "title" || t == "name" || t == "brand" || t == "logo")
        })
        .map(|n| n.plain())
        .filter(|t| !t.is_empty() && t.chars().count() <= 80);
    if let Some(t) = title {
        return t;
    }
    doc.first(|n| {
        n.name() == "link"
            && n.attr("rel").is_some_and(|r| r.to_lowercase().contains("alternate"))
            && n.attr("type").is_some_and(|t| t.contains("rss") || t.contains("atom"))
    })
    .and_then(|n| n.attr("title"))
    .map(clean_text)
    .map(|t| re!(r"(?i)\s*(»|-|–|—|:)?\s*(rss|atom)?\s*(feed|posts|entries|articles)?$").replace(&t, "").trim().to_string())
    .unwrap_or_default()
}

/// A byline in the page: `rel="author"` links, `itemprop="author"`, an element whose class is
/// "author", "byline", "post-author" (its text, not its photo or bio).
fn byline(doc: Node) -> Vec<String> {
    let mut out = vec![];
    for n in doc.all(|n| n.name() == "a" && n.attr("rel").is_some_and(|r| r.split_whitespace().any(|x| x == "author"))) {
        add_names(&mut out, names_from(&n.plain()));
    }
    if out.is_empty() {
        for n in doc.all(|n| n.attr("itemprop") == Some("author")) {
            let name = n.first(|x| x.attr("itemprop") == Some("name")).map(|x| x.attr("content").map(clean_text).unwrap_or_else(|| x.plain())).unwrap_or_else(|| n.plain());
            add_names(&mut out, names_from(&name));
        }
    }
    if out.is_empty() {
        for n in doc.all(|n| {
            let t = n.name_tokens();
            (t.contains("byline") || t.contains("author") && (n.classes().iter().any(|c| { let c = c.to_lowercase(); c == "author" || c.ends_with("-author") || c.ends_with("__author") || c == "author-name" || c.ends_with("author-name") })))
                && !t.contains("bio") && !t.contains("avatar") && !t.contains("photo") && !t.contains("box")
                && !n.has_ancestor(|a| a.name() == "footer" || a.name_tokens().contains("comments") || a.name_tokens().contains("comment"))
        }) {
            let t = n.plain();
            if t.chars().count() <= 120 {
                add_names(&mut out, names_from(&t));
            }
            if out.len() >= 1 {
                break;
            }
        }
    }
    out
}

/// A personal blog's owner: a site called "Andrej Karpathy blog" / "Jane Doe's Blog" whose
/// address carries the family name (karpathy.github.io). Nothing for anything less certain.
fn blog_owner(site: &str, url: &str) -> Option<String> {
    let c = re!(r"^(\p{Lu}[\p{L}.'-]+(?:\s+\p{Lu}[\p{L}.'-]+){1,2})(?:['’]s)?\s+(?:blog|weblog|website|homepage)$").captures(site.trim())?;
    let name = c[1].to_string();
    let host = host_of(url);
    let family = name.split_whitespace().last()?.to_lowercase();
    let family: String = crate::export::ascii_fold(&family).chars().filter(|c| c.is_alphanumeric()).collect();
    (family.len() >= 3 && host.replace(['-', '.'], "").contains(&family)).then_some(name)
}

/// The page's own date: `<time datetime>` (the one marked as published, else the first).
fn time_element(doc: Node) -> String {
    let times = doc.all(|n| n.name() == "time" && !n.has_ancestor(|a| a.name_tokens().contains("comments") || a.name_tokens().contains("comment")));
    let published = times.iter().find(|t| {
        t.attr("pubdate").is_some() || t.attr("itemprop") == Some("datePublished") || t.name_tokens().iter().any(|x| x == "published" || x == "pubdate" || x == "entry")
    });
    for t in published.into_iter().chain(times.iter()) {
        let d = iso_date(t.attr("datetime").unwrap_or(""));
        if !d.is_empty() {
            return d;
        }
        let d = iso_date(&t.plain());
        if !d.is_empty() {
            return d;
        }
    }
    String::new()
}

/// " | Site", " - Site", " — Site" taken off a `<title>` (and "Site | " off its start).
#[cfg(test)]
fn strip_site(title: &str, site: &str, host: &str) -> String {
    title_and_site(title, site, host).0
}

/// A `<title>` without the site's name, and the name when the title is how the page gives it
/// ("Configuration · Example Tool Docs": the page names no site otherwise).
fn title_and_site(title: &str, site: &str, host: &str) -> (String, String) {
    let t = title.trim();
    let host_word = host.split('.').next().unwrap_or("").replace('-', "");
    for sep in [" | ", " · ", " – ", " — ", " - ", " :: ", " » "] {
        if let Some((a, b)) = t.rsplit_once(sep) {
            let (a, b) = (a.trim(), b.trim());
            let lb = b.to_lowercase();
            let squashed: String = lb.chars().filter(|c| c.is_alphanumeric()).collect();
            let named = !site.is_empty() && (lb == site.to_lowercase() || site.to_lowercase().contains(&lb));
            let is_host = lb == host || (!host_word.is_empty() && squashed.contains(&host_word));
            // with no site to compare with: a short tail after a separator titles don't use
            // inside themselves ("·", "|"), or one that is the host's name
            let tail = site.is_empty() && b.split_whitespace().count() <= 4 && (matches!(sep, " | " | " · " | " :: " | " » ") || is_host);
            // Sphinx and similar doc generators: "Page — Project 1.0 documentation", or a tail
            // that repeats the title ("Dive into Deep Learning — Dive into Deep Learning 1.0.3 …")
            let docs = lb.ends_with(" documentation") || (a.len() >= 8 && lb.starts_with(&a.to_lowercase()));
            if !a.is_empty() && (named || is_host || tail || docs) {
                return (a.to_string(), if named || !site.is_empty() { String::new() } else { b.to_string() });
            }
        }
        if let Some((a, b)) = t.split_once(sep) {
            if !site.is_empty() && a.trim().eq_ignore_ascii_case(site) && !b.trim().is_empty() {
                return (b.trim().to_string(), String::new());
            }
        }
    }
    (t.to_string(), String::new())
}

/// A page that is a scholarly article after all: the DOI (or "arXiv:…") its meta tags declare.
pub fn scholarly_id(html_text: &str) -> Option<String> {
    let doc = html::parse(&head_part(html_text));
    scholarly_id_in(doc.root())
}

fn scholarly_id_in(root: Node) -> Option<String> {
    let m = Metas::of(root);
    for k in ["citation_doi", "prism.doi", "bepress_citation_doi", "dc.identifier", "dcterms.identifier", "dc.identifier.doi"] {
        for v in m.all(&[k]) {
            if let Some(d) = crate::metadata::doi_in(&v) {
                return Some(d);
            }
        }
    }
    let a = m.first(&["citation_arxiv_id"]);
    if !a.is_empty() {
        return Some(format!("arXiv:{a}"));
    }
    None
}

/// The `<head>` and the start of the body (enough for the details; pages can be megabytes).
fn head_part(h: &str) -> String {
    h.chars().take(400_000).collect()
}

/// The details of a web page or blog post read from its HTML: `url` the address it was read at,
/// `accessed` the day ("" for today).
pub fn page_meta(html_text: &str, url: &str, accessed: &str) -> PaperMeta {
    let doc = html::parse(html_text);
    page_meta_doc(&doc, url, accessed)
}

pub fn page_meta_doc(doc: &Doc, url: &str, accessed: &str) -> PaperMeta {
    let root = doc.root();
    let tags = Metas::of(root);
    let ld = linked_data(root);
    let url = clean_url(url);
    let host = host_of(&url);

    let mut title = tags.first(&["citation_title", "dc.title", "dcterms.title"]);
    if title.is_empty() {
        title = ld.headline.clone();
    }
    if title.is_empty() {
        title = tags.first(&["og:title", "twitter:title"]);
    }
    let page_title = root.first(|n| n.name() == "title").map(|t| clean_text(&t.all_text())).unwrap_or_default();

    let mut authors: Vec<String> = vec![];
    for a in tags.all(&["citation_author"]) {
        // "Doe, Jane" as the citation tags write names
        let parts: Vec<&str> = a.split(',').map(str::trim).collect();
        add_names(&mut authors, vec![if parts.len() == 2 && !parts[1].is_empty() { format!("{} {}", parts[1], parts[0]) } else { a.clone() }]);
    }
    if authors.is_empty() {
        for a in tags.all(&["dc.creator", "dcterms.creator", "dc.contributor"]) {
            add_names(&mut authors, names_from(&a));
        }
    }
    if authors.is_empty() {
        add_names(&mut authors, ld.authors.iter().flat_map(|a| names_from(a)).collect());
    }
    if authors.is_empty() {
        for a in tags.all(&["article:author", "og:article:author", "author", "parsely-author", "sailthru.author", "byl"]) {
            add_names(&mut authors, names_from(&a));
        }
    }
    if authors.is_empty() {
        authors = byline(root);
    }

    let mut date = iso_date(&tags.first(&["citation_publication_date", "citation_date", "citation_online_date"]));
    if date.is_empty() {
        date = iso_date(&tags.first(&["dc.date", "dcterms.date", "dc.date.issued", "dcterms.issued", "dcterms.created", "dc.date.created"]));
    }
    if date.is_empty() {
        date = ld.date.clone();
    }
    if date.is_empty() {
        date = iso_date(&tags.first(&["article:published_time", "og:article:published_time", "datepublished", "date", "publish-date", "pubdate", "parsely-pub-date", "sailthru.date"]));
    }
    if date.is_empty() {
        date = time_element(root);
    }
    let in_url = date_in_url(&url);
    if date.is_empty() || (date.len() == 4 && in_url.starts_with(&date)) {
        date = if in_url.is_empty() { date } else { in_url };
    }

    let mut site = tags.first(&["citation_journal_title", "citation_publisher", "dc.publisher", "dcterms.publisher"]);
    if site.is_empty() {
        site = ld.site.clone();
    }
    if site.is_empty() {
        site = tags.first(&["og:site_name", "application-name", "apple-mobile-web-app-title"]);
    }
    if site.is_empty() {
        site = ld.publisher.clone();
    }
    if site.is_empty() {
        site = site_from_page(root);
    }
    let publisher = if ld.publisher.is_empty() || ld.publisher.eq_ignore_ascii_case(&site) { String::new() } else { ld.publisher.clone() };

    if title.is_empty() || title == page_title {
        let (t, from_title) = title_and_site(&page_title, &site, &host);
        title = t;
        if site.is_empty() {
            site = from_title;
        }
    }
    if title.is_empty() {
        title = root.first(|n| n.name() == "h1").map(|h| h.plain()).unwrap_or_default();
    }
    if authors.is_empty() {
        if let Some(owner) = blog_owner(&site, &url) {
            authors.push(owner);
        }
    }

    let blog = ld.types.iter().any(|t| t == "blogposting" || t == "blog")
        || tags.first(&["og:type"]).eq_ignore_ascii_case("blog")
        || blog_address(&url)
        || site.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w == "blog" || w == "weblog")
        || tags.first(&["generator"]).to_lowercase().contains("wordpress") && tags.first(&["og:type"]).eq_ignore_ascii_case("article");
    let description = {
        let d = tags.first(&["citation_abstract", "dc.description", "dcterms.abstract"]);
        let d = if d.is_empty() { ld.description.clone() } else { d };
        let d = if d.is_empty() { tags.first(&["og:description", "description", "twitter:description"]) } else { d };
        // a site's motto on every page is not this page's summary
        if !site.is_empty() && d.eq_ignore_ascii_case(&tags.first(&["og:site_name"])) || d.chars().count() < 40 { String::new() } else { d }
    };
    let mut keywords = ld.keywords.clone();
    if keywords.is_empty() {
        keywords = tags.all(&["article:tag", "citation_keywords"]).into_iter().flat_map(|k| k.split([';', ',']).map(|x| x.trim().to_string()).collect::<Vec<_>>()).filter(|k| !k.is_empty()).collect();
    }
    keywords.truncate(12);

    PaperMeta {
        title: title.trim().to_string(),
        authors,
        date,
        journal: site.trim().to_string(),
        publisher,
        url,
        abstract_text: description,
        keywords,
        work_type: if blog { "post-weblog" } else { "webpage" }.into(),
        accessed: if accessed.trim().is_empty() { today() } else { iso_date(accessed) },
        ..Default::default()
    }
}

/// Fetches a page (one request, as the person asked for it) with the core's HTTP agent: its
/// final address after redirects and its HTML. Not a page (a PDF, an image): an error.
pub fn fetch(url: &str) -> Result<(String, String)> {
    let (final_url, content_type, body) = crate::metadata::get_page(url.trim())?;
    let head = String::from_utf8_lossy(&body[..body.len().min(1024)]).to_lowercase();
    if body.starts_with(b"%PDF") {
        return Err(CoreError::Invalid("That link is a PDF: add it with Add PDF instead.".into()));
    }
    if !(content_type.contains("html") || content_type.contains("xml") || content_type.is_empty() || head.contains("<html") || head.contains("<!doctype html")) {
        return Err(CoreError::Invalid(format!("That link isn't a web page ({content_type}).")));
    }
    Ok((final_url, decode_body(&body, &content_type)))
}

/// The page's text: UTF-8, or Latin-1 / Windows-1252 when the page says so.
fn decode_body(body: &[u8], content_type: &str) -> String {
    let head = String::from_utf8_lossy(&body[..body.len().min(2048)]).to_lowercase();
    let latin = content_type.to_lowercase().contains("iso-8859-1") || content_type.to_lowercase().contains("windows-1252")
        || re!(r#"<meta[^>]+charset=["']?(iso-8859-1|windows-1252|latin1)"#).is_match(&head);
    match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        Err(_) if latin => body.iter().map(|&b| b as char).collect(),
        Err(_) => String::from_utf8_lossy(body).into_owned(),
    }
}

/// A web page read for Lunta: its details, a DOI it declares (then it is a paper: add that), and
/// its main text for the reading view.
#[derive(uniffi::Record, Clone, Debug)]
pub struct WebPage {
    /// The address it was read at (after redirects, without tracking parameters).
    pub url: String,
    pub meta: PaperMeta,
    /// A DOI ("10.…") or "arXiv:…" the page's meta tags declare: a scholarly article, to be added
    /// as a paper by it. Empty for web pages and blog posts.
    pub scholarly_id: String,
    pub article: crate::webarticle::WebArticle,
}

/// Reads a web page's HTML (no network): details, declared DOI and main text.
pub fn read(html_text: &str, url: &str, accessed: &str) -> WebPage {
    let doc = html::parse(html_text);
    let meta = page_meta_doc(&doc, url, accessed);
    let scholarly = scholarly_id_in(doc.root()).unwrap_or_default();
    let article = crate::webarticle::extract_doc(&doc, &meta.url, &meta.title);
    WebPage { url: meta.url.clone(), meta, scholarly_id: scholarly, article }
}

#[cfg(test)]
mod tests;
