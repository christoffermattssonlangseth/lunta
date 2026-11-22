//! **Outline from Highlights**: the highlights and notes in a set of papers (a folder, a tag, a
//! search, a selection), optionally about a topic, as a Markdown outline: themed headings, under
//! each the highlights word for word (or the researcher's note, marked as theirs) with their
//! citation (author–year, a lunta:// link to the paper, its citation key), and the sources.
//!
//! Nothing is invented: every point is a highlight's quote or a note, copied. The on-device model,
//! when there is one, only groups the points (by their numbers) and names the themes; it may add
//! a one-line summary per theme, marked as the model's. Without a model the points are grouped by
//! paper, and within a paper by the section of the reading view they are in (or the PDF page).
//! `outline_check` verifies an outline: every point must be a highlight or note.

use crate::model::*;
use crate::tidy::ModelPrompt;
use crate::Library;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

pub mod summary;

/// A highlight or note as the outline uses it.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutlineItem {
    pub annotation_id: String,
    pub paper_id: String,
    /// The highlighted text, as highlighted (empty for a note on the paper).
    pub quote: String,
    /// The researcher's note (may be empty).
    pub note: String,
    /// The section of the reading view it is in ("Results"), or "Page 4", or "".
    pub section: String,
    /// Where in the paper (to keep the paper's order).
    pub order: u64,
    /// A region marked on a figure (quote "Figure 3 (region)"): the start of the figure's
    /// caption, all a summary may say about it besides the note. "" for highlights and notes.
    #[serde(default)]
    #[uniffi(default = "")]
    pub context: String,
}

/// A theme of the outline.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OutlineTheme {
    pub title: String,
    /// The model's one-line summary ("" without a model): marked as the model's in the outline.
    pub summary: String,
    pub annotation_ids: Vec<String>,
}

/// The outline.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Outline {
    pub title: String,
    pub markdown: String,
    /// The same as HTML (for copying as rich text).
    pub html: String,
    pub themes: Vec<OutlineTheme>,
    /// Highlights and notes in it.
    pub items: u32,
    pub papers: u32,
    /// Themes grouped by the model (else by paper and section).
    pub by_model: bool,
    /// "No highlight mentions “X”: all are listed." and the like ("" when there is nothing to say).
    pub note: String,
}

// ---------------------------------------------------------------- citations

/// "Kukanja et al. 2024", "Smith and Lee 2019", "Smith 2019"; a short title without authors;
/// "n.d." without a year.
pub fn author_year(m: &PaperMeta) -> String {
    let fam = |a: &String| crate::export::split_name(a).0;
    let who = match m.authors.len() {
        0 => {
            let t: Vec<&str> = m.title.split_whitespace().take(4).collect();
            if t.is_empty() { "Anonymous".to_string() } else { format!("“{}”", t.join(" ")) }
        }
        1 => fam(&m.authors[0]),
        2 => format!("{} and {}", fam(&m.authors[0]), fam(&m.authors[1])),
        _ => format!("{} et al.", fam(&m.authors[0])),
    };
    let y = m.year();
    format!("{who} {}", if y.trim().is_empty() { "n.d.".to_string() } else { y })
}

/// The paper's link in the apps.
pub fn paper_link(id: &str) -> String {
    format!("lunta://open?id={id}")
}

/// Labels for the papers, made unique with a, b… ("Smith 2019a", "Smith 2019b").
fn labels(papers: &[Paper]) -> HashMap<String, String> {
    let mut by: BTreeMap<String, Vec<&Paper>> = BTreeMap::new();
    for p in papers {
        by.entry(author_year(&p.meta)).or_default().push(p);
    }
    let mut out = HashMap::new();
    for (l, mut ps) in by {
        if ps.len() == 1 {
            out.insert(ps[0].id.clone(), l);
            continue;
        }
        ps.sort_by(|a, b| a.meta.title.to_lowercase().cmp(&b.meta.title.to_lowercase()));
        for (i, p) in ps.iter().enumerate() {
            out.insert(p.id.clone(), format!("{l}{}", (b'a' + (i as u8).min(25)) as char));
        }
    }
    out
}

// ---------------------------------------------------------------- Markdown

/// Text safe in Markdown: one line, the characters that would format it escaped.
pub fn md_escape(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(one.len());
    for c in one.chars() {
        if matches!(c, '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '`' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `md_escape` undone.
pub fn md_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut esc = false;
    for c in s.chars() {
        if esc || c != '\\' {
            out.push(c);
            esc = false;
        } else {
            esc = true;
        }
    }
    out
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const NOTE: &str = "Note:";
const SUMMARY: &str = "Summary (written by the on-device model; check it against the quotes):";

/// One point: the quote, the note marked as the researcher's, the citation.
fn bullet(it: &OutlineItem, p: &Paper, label: &str) -> String {
    let mut s = String::from("- ");
    let (q, n) = (one_line(&it.quote), one_line(&it.note));
    if !q.is_empty() {
        s += &format!("“{}”", md_escape(&q));
        if !n.is_empty() {
            s += &format!(" — *{NOTE}* {}", md_escape(&n));
        }
    } else {
        s += &format!("*{NOTE}* {}", md_escape(&n));
    }
    let key = if p.cite_key.is_empty() { String::new() } else { format!(", [@{}]", p.cite_key) };
    s += &format!(" ([{}]({}){key})", md_escape(label), paper_link(&p.id));
    s
}

fn sources(papers: &[Paper], labels: &HashMap<String, String>) -> String {
    let mut ps: Vec<&Paper> = papers.iter().collect();
    ps.sort_by_key(|p| labels.get(&p.id).cloned().unwrap_or_default().to_lowercase());
    let mut s = String::from("## Sources\n\n");
    for p in ps {
        let cite = crate::export::citation(p, CitationStyle::Apa);
        let key = if p.cite_key.is_empty() { String::new() } else { format!(" [@{}]", p.cite_key) };
        // the citation as formatted (its *journal* in italics)
        s += &format!("- **{}**{key}: {} [Open in Lunta]({})\n", md_escape(labels.get(&p.id).map(String::as_str).unwrap_or("")), one_line(&cite), paper_link(&p.id));
    }
    s
}

// ---------------------------------------------------------------- sections

/// The reading view's section headings: (UTF-16 offset in the body's text, title). Levels 1–2
/// when the view has level-2 headings (sections), else every level.
fn headings(xhtml: &str) -> Vec<(u64, String)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?is)<h([1-4])\b[^>]*>(.*?)</h[1-4]>").unwrap());
    let found: Vec<(usize, u32, String)> = re
        .captures_iter(xhtml)
        .filter_map(|c| {
            let t = one_line(&crate::highlight::dom_text(&format!("<body>{}</body>", &c[2])));
            let level: u32 = c[1].parse().unwrap_or(9);
            (!t.is_empty() && t.chars().count() <= 120).then(|| (c.get(0).unwrap().start(), level, t))
        })
        .collect();
    let top = if found.iter().any(|f| f.1 == 2) { 2 } else { 4 };
    found
        .into_iter()
        .filter(|f| f.1 <= top)
        .map(|(at, _, t)| (crate::highlight::dom_text(&xhtml[..at]).encode_utf16().count() as u64, t))
        .collect()
}

/// A highlight's place: its section (or page) and its order in the paper.
fn place(a: &Annotation, heads: Option<&Vec<(u64, String)>>) -> (String, u64) {
    // a figure mark: where its figure is in the text
    let fig = crate::figmark::region_of(a).map(|r| r.start);
    if fig.is_none() && a.location.starts_with("pdf") {
        let page: u64 = a.location.trim_start_matches("pdf").trim_start_matches(':').parse().unwrap_or(0);
        return (if page > 0 { format!("Page {page}") } else { String::new() }, page * 1_000_000);
    }
    let pos: u64 = fig.unwrap_or_else(|| a.position.trim().parse().unwrap_or(0));
    let sec = heads.and_then(|h| h.iter().rev().find(|(at, _)| *at <= pos)).map(|x| x.1.clone()).unwrap_or_default();
    (sec, pos)
}

// ---------------------------------------------------------------- topic

fn topic_words(topic: &str) -> Vec<String> {
    crate::tidy::text::phrase_key(topic).split(' ').filter(|w| w.chars().count() >= 3 && !crate::tidy::text::is_stop(w)).map(String::from).collect()
}

/// Whether a highlight (or its paper) is about the topic: one of the topic's words is in it.
fn about(it: &OutlineItem, words: &[String]) -> bool {
    let hay = crate::tidy::text::phrase_key(&format!("{} {} {}", it.quote, it.note, it.context));
    let hw: HashSet<&str> = hay.split(' ').collect();
    words.iter().any(|w| hw.contains(w.as_str()) || (w.len() >= 5 && hay.contains(w.as_str())))
}

// ---------------------------------------------------------------- the model

pub const OUTLINE_INSTRUCTIONS: &str = "You help a researcher outline what they highlighted in scientific papers. \
You get their highlights and notes, numbered. Group them into 2 to 7 themes. For each theme write exactly three lines:\n\
Theme: a short title (2 to 8 words)\n\
Summary: one sentence on what its highlights have in common, using only what they say\n\
Items: the numbers of its highlights, like 3, 7, 12\n\
Put each highlight in one theme. Use only the numbers given. Write nothing else.";

fn prompt_for(items: &[OutlineItem], labels: &HashMap<String, String>, topic: &str, max_items: usize) -> ModelPrompt {
    let take: Vec<&OutlineItem> = items.iter().take(max_items.max(1)).collect();
    let mut lines = String::new();
    for (i, it) in take.iter().enumerate() {
        let q: String = one_line(&it.quote).chars().take(300).collect();
        let n: String = one_line(&it.note).chars().take(160).collect();
        let who = labels.get(&it.paper_id).cloned().unwrap_or_default();
        let mut l = format!("{}. ({who})", i + 1);
        if !q.is_empty() {
            l += &format!(" \"{q}\"");
        }
        if !it.context.is_empty() {
            l += &format!(" Caption: {}", one_line(&it.context).chars().take(160).collect::<String>());
        }
        if !n.is_empty() {
            l += &format!(" Note: {n}");
        }
        lines += &l;
        lines.push('\n');
    }
    let topic = topic.trim();
    let about = if topic.is_empty() { String::new() } else { format!("The outline is about: {topic}. Group the highlights by what they say about it.\n\n") };
    ModelPrompt {
        instructions: OUTLINE_INSTRUCTIONS.into(),
        prompt: format!("{about}Highlights:\n{lines}\nGroup them into themes (Theme, Summary, Items)."),
        max_tokens: 600,
        ids: take.iter().map(|i| i.annotation_id.clone()).collect(),
    }
}

fn clean_title(s: &str) -> String {
    let s = one_line(s.trim().trim_matches(|c: char| matches!(c, '"' | '“' | '”' | '*' | '_' | '`' | '#' | ':')).trim());
    let s: String = s.chars().filter(|c| !matches!(c, '[' | ']' | '<' | '>' | '*' | '`')).collect();
    let s = s.trim().to_string();
    if s.chars().count() > 80 { s.chars().take(80).collect::<String>().trim_end().to_string() + "…" } else { s }
}

/// The themes in the model's answer (titles, summaries, highlight ids from the prompt). Each
/// highlight goes in the first theme that names it; themes without highlights are dropped.
pub fn parse_themes(prompt: &ModelPrompt, answer: &str) -> Vec<OutlineTheme> {
    static NUM: OnceLock<Regex> = OnceLock::new();
    static HEAD: OnceLock<Regex> = OnceLock::new();
    static INLINE: OnceLock<Regex> = OnceLock::new();
    let num = NUM.get_or_init(|| Regex::new(r"\d+").unwrap());
    let head = HEAD.get_or_init(|| Regex::new(r"(?i)^\s*(?:#+\s*|[-*•]\s*|\d+[.)]\s+)?\**\s*(theme|title)\s*\d*\s*\**\s*[:.\-–—]\s*(.*)$").unwrap());
    let inline = INLINE.get_or_init(|| Regex::new(r"^\s*(?:#+\s*|[-*•]\s*)?\**([^:\n]{3,80}?)\**\s*:\s*((?:\d+\s*,?\s*)+)\s*$").unwrap());
    let answer = crate::tidy::without_thinking(answer);
    let mut themes: Vec<OutlineTheme> = vec![];
    let mut used: HashSet<String> = HashSet::new();
    let ids_of = |s: &str, used: &mut HashSet<String>| -> Vec<String> {
        let mut v = vec![];
        for m in num.find_iter(s) {
            if let Some(id) = m.as_str().parse::<usize>().ok().filter(|n| *n >= 1).and_then(|n| prompt.ids.get(n - 1)) {
                if used.insert(id.clone()) {
                    v.push(id.clone());
                }
            }
        }
        v
    };
    for line in answer.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let low = l.trim_start_matches(|c: char| matches!(c, '*' | '-' | '#' | ' ')).to_lowercase();
        if let Some(c) = head.captures(l) {
            themes.push(OutlineTheme { title: clean_title(&c[2]), summary: String::new(), annotation_ids: vec![] });
        } else if low.starts_with("summary") {
            if let (Some(t), Some(i)) = (themes.last_mut(), l.find(':')) {
                let s: String = one_line(&l[i + 1..]).trim_matches(|c: char| c == '*' || c == '_').trim().chars().take(300).collect();
                t.summary = s;
            }
        } else if low.starts_with("items") || low.starts_with("highlights") || low.starts_with("numbers") {
            let rest = l.find(':').map(|i| &l[i + 1..]).unwrap_or("");
            let ids = ids_of(rest, &mut used);
            if let Some(t) = themes.last_mut() {
                t.annotation_ids.extend(ids);
            }
        } else if let Some(c) = inline.captures(l) {
            let ids = ids_of(&c[2], &mut used);
            themes.push(OutlineTheme { title: clean_title(&c[1]), summary: String::new(), annotation_ids: ids });
        } else if l.starts_with("## ") || l.starts_with("### ") {
            themes.push(OutlineTheme { title: clean_title(l), summary: String::new(), annotation_ids: vec![] });
        }
    }
    themes.retain(|t| !t.annotation_ids.is_empty() && !t.title.is_empty());
    themes
}

// ---------------------------------------------------------------- writing it

fn write(title: &str, intro: &str, themes: &[OutlineTheme], items: &[OutlineItem], papers: &[Paper], by_model: bool) -> String {
    let labels = labels(papers);
    let paper: HashMap<&str, &Paper> = papers.iter().map(|p| (p.id.as_str(), p)).collect();
    let item: HashMap<&str, &OutlineItem> = items.iter().map(|i| (i.annotation_id.as_str(), i)).collect();
    let mut md = format!("# {}\n\n*{}*\n\n", md_escape(title), md_escape(intro));
    for t in themes {
        md += &format!("## {}\n\n", md_escape(&t.title));
        if by_model && !t.summary.trim().is_empty() {
            md += &format!("*{SUMMARY}* {}\n\n", md_escape(&t.summary));
        }
        let mut section = String::new();
        for id in &t.annotation_ids {
            let Some(it) = item.get(id.as_str()) else { continue };
            let Some(p) = paper.get(it.paper_id.as_str()) else { continue };
            if !by_model && it.section != section {
                section = it.section.clone();
                if !section.is_empty() {
                    md += &format!("\n### {}\n\n", md_escape(&section));
                }
            }
            md += &bullet(it, p, labels.get(&p.id).map(String::as_str).unwrap_or(""));
            md.push('\n');
        }
        md.push('\n');
    }
    if !papers.is_empty() {
        md += &sources(papers, &labels);
    }
    md.replace("\n\n\n", "\n\n")
}

/// Problems with an outline (none for one written here): every point under a theme must be a
/// highlight's quote, word for word, or a note's text, and any other text a heading, a marked
/// model summary, the line saying where it is from, or a source.
pub fn check(markdown: &str, items: &[OutlineItem]) -> Vec<String> {
    let norm = |s: &str| one_line(s);
    let quotes: HashSet<String> = items.iter().map(|i| norm(&i.quote)).filter(|q| !q.is_empty()).collect();
    let notes: HashSet<String> = items.iter().map(|i| norm(&i.note)).filter(|q| !q.is_empty()).collect();
    let mut problems = vec![];
    let mut in_sources = false;
    for (n, line) in markdown.lines().enumerate() {
        let l = line.trim();
        if l.starts_with('#') {
            in_sources = l == "## Sources";
            continue;
        }
        if l.is_empty() {
            continue;
        }
        if in_sources {
            if !l.starts_with("- **") {
                problems.push(format!("line {}: not a source: {l}", n + 1));
            }
            continue;
        }
        if l.starts_with(&format!("*{SUMMARY}*")) || (n <= 3 && l.starts_with('*') && l.ends_with('*')) {
            continue;
        }
        let Some(rest) = l.strip_prefix("- ") else {
            problems.push(format!("line {}: text that is no highlight or note: {l}", n + 1));
            continue;
        };
        // the citation at the end: " ([label](lunta://open?id=…)…)"
        let body = match rest.rfind(" ([") {
            Some(i) if rest.ends_with(')') && rest[i..].contains("](lunta://open?id=") => &rest[..i],
            _ => {
                problems.push(format!("line {}: a point without its citation: {l}", n + 1));
                continue;
            }
        };
        let with_note = format!("” — *{NOTE}*");
        let (quote, note) = if let Some(q) = body.strip_prefix('“') {
            if let Some(e) = q.find(&with_note) {
                (Some(md_unescape(&q[..e])), Some(md_unescape(q[e + with_note.len()..].trim())))
            } else if let Some(q) = q.strip_suffix('”') {
                (Some(md_unescape(q)), None)
            } else {
                problems.push(format!("line {}: text after the quote that is no note: {body}", n + 1));
                continue;
            }
        } else if let Some(x) = body.strip_prefix(&format!("*{NOTE}*")) {
            (None, Some(md_unescape(x.trim())))
        } else {
            problems.push(format!("line {}: a point that is no highlight or note: {body}", n + 1));
            continue;
        };
        if let Some(q) = quote {
            if !quotes.contains(&norm(&q)) {
                problems.push(format!("line {}: a quote that isn't a highlight: “{q}”", n + 1));
            }
        }
        if let Some(t) = note {
            if !notes.contains(&norm(&t)) {
                problems.push(format!("line {}: a note that isn't the researcher's: {t}", n + 1));
            }
        }
    }
    problems
}

/// An outline check, exported: problems with `markdown` given these highlights (empty: fine).
#[uniffi::export]
pub fn outline_check(markdown: String, items: Vec<OutlineItem>) -> Vec<String> {
    check(&markdown, &items)
}

// ---------------------------------------------------------------- HTML

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Inline Markdown (as written here) as HTML: links, **bold**, *emphasis*, escapes.
fn inline_html(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let (mut em, mut strong) = (false, false);
    while i < cs.len() {
        let c = cs[i];
        if c == '\\' && i + 1 < cs.len() {
            out += &html_escape(&cs[i + 1].to_string());
            i += 2;
            continue;
        }
        if c == '*' && cs.get(i + 1) == Some(&'*') {
            out += if strong { "</strong>" } else { "<strong>" };
            strong = !strong;
            i += 2;
            continue;
        }
        if c == '*' {
            out += if em { "</em>" } else { "<em>" };
            em = !em;
            i += 1;
            continue;
        }
        if c == '[' {
            // [text](url), the text may have escapes
            let mut j = i + 1;
            let mut text = String::new();
            while j < cs.len() && cs[j] != ']' {
                if cs[j] == '\\' && j + 1 < cs.len() {
                    j += 1;
                }
                text.push(cs[j]);
                j += 1;
            }
            if j + 1 < cs.len() && cs[j + 1] == '(' {
                if let Some(k) = cs[j + 2..].iter().position(|&x| x == ')') {
                    let url: String = cs[j + 2..j + 2 + k].iter().collect();
                    out += &format!("<a href=\"{}\">{}</a>", html_escape(&url), html_escape(&text));
                    i = j + 3 + k;
                    continue;
                }
            }
            // a citation key [@key]
            out += &html_escape(&format!("[{text}]"));
            i = j + 1;
            continue;
        }
        out += &html_escape(&c.to_string());
        i += 1;
    }
    if em {
        out += "</em>";
    }
    if strong {
        out += "</strong>";
    }
    out
}

/// The outline's Markdown as HTML (headings, paragraphs, lists, links, emphasis).
pub fn markdown_html(md: &str) -> String {
    let mut out = String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>\n");
    let mut in_list = false;
    for line in md.lines() {
        let l = line.trim_end();
        let is_item = l.starts_with("- ");
        if in_list && !is_item {
            out += "</ul>\n";
            in_list = false;
        }
        if l.trim().is_empty() {
            continue;
        }
        if let Some(h) = ["### ", "## ", "# "].iter().find(|h| l.starts_with(**h)) {
            let n = h.len() - 1;
            out += &format!("<h{n}>{}</h{n}>\n", inline_html(&l[h.len()..]));
        } else if is_item {
            if !in_list {
                out += "<ul>\n";
                in_list = true;
            }
            out += &format!("<li>{}</li>\n", inline_html(&l[2..]));
        } else {
            out += &format!("<p>{}</p>\n", inline_html(l));
        }
    }
    if in_list {
        out += "</ul>\n";
    }
    out + "</body></html>\n"
}

// ---------------------------------------------------------------- the library

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Library {
    /// The papers (in order, those that exist) and their highlights and notes, in each paper's order.
    fn outline_material(&self, ids: &[String]) -> (Vec<Paper>, Vec<OutlineItem>) {
        let mut seen = HashSet::new();
        let papers: Vec<Paper> = ids.iter().filter(|i| seen.insert((*i).clone())).filter_map(|i| self.paper(i.clone())).collect();
        let mut items = vec![];
        for p in &papers {
            let anns = self.annotations(p.id.clone());
            if anns.is_empty() {
                continue;
            }
            let heads = anns.iter().any(|a| !a.location.starts_with("pdf")).then(|| {
                self.file_path(p.id.clone(), FileKind::Epub)
                    .and_then(|_| self.open_epub(p.id.clone()).ok())
                    .and_then(|b| std::fs::read_to_string(b.combined).ok())
                    .map(|x| headings(&x))
                    .unwrap_or_default()
            });
            let mut mine: Vec<OutlineItem> = anns
                .iter()
                .filter(|a| !one_line(&a.quote).is_empty() || !one_line(&a.note).is_empty())
                .map(|a| {
                    let (section, order) = place(a, heads.as_ref());
                    let context = if crate::figmark::is_mark(a) { one_line(&a.suffix) } else { String::new() };
                    OutlineItem { annotation_id: a.id.clone(), paper_id: p.id.clone(), quote: a.quote.trim().to_string(), note: a.note.trim().to_string(), section, order, context }
                })
                .collect();
            mine.sort_by(|a, b| a.order.cmp(&b.order).then(a.annotation_id.cmp(&b.annotation_id)));
            items.extend(mine);
        }
        (papers, items)
    }

    /// The items for a topic: those that mention it, unless none does. And a note on what was left out.
    fn about_topic(items: Vec<OutlineItem>, topic: &str) -> (Vec<OutlineItem>, String) {
        let words = topic_words(topic);
        if words.is_empty() {
            return (items, String::new());
        }
        let n = items.len();
        let on: Vec<OutlineItem> = items.iter().filter(|i| about(i, &words)).cloned().collect();
        if on.is_empty() {
            return (items, format!("No highlight or note mentions “{}”: all are listed.", topic.trim()));
        }
        let left = n - on.len();
        let note = if left == 0 { String::new() } else { format!("{} that don't mention “{}” left out.", plural(left, "highlight or note", "highlights and notes"), topic.trim()) };
        (on, note)
    }
}

#[uniffi::export]
impl Library {
    /// The highlights and notes of these papers, in each paper's order, with their sections.
    pub fn outline_items(&self, ids: Vec<String>) -> Vec<OutlineItem> {
        self.outline_material(&ids).1
    }

    /// The model's prompt for grouping these papers' highlights into themes (at most
    /// `max_items` highlights: about 40 fit Apple's on-device model, 120 the open model). Give
    /// its answer to `outline_from_highlights`.
    pub fn outline_prompt(&self, ids: Vec<String>, topic: String, max_items: u32) -> ModelPrompt {
        let (papers, items) = self.outline_material(&ids);
        prompt_for(&items, &labels(&papers), &topic, max_items as usize)
    }

    /// The outline of these papers' highlights and notes. With the model's `answer` to
    /// `outline_prompt` (the same `prompt`), its themes; else by paper and section. With a
    /// `topic` and no model, only the highlights that mention it (all when none does).
    pub fn outline_from_highlights(&self, ids: Vec<String>, topic: String, prompt: Option<ModelPrompt>, answer: Option<String>) -> Outline {
        let (papers, all) = self.outline_material(&ids);
        let topic = topic.trim().to_string();
        let title = if topic.is_empty() { "Highlights".to_string() } else { topic.clone() };
        let model = match (&prompt, &answer) {
            (Some(p), Some(a)) => parse_themes(p, a),
            _ => vec![],
        };
        let by_model = !model.is_empty();
        let (items, mut note) = if by_model { (all, String::new()) } else { Self::about_topic(all, &topic) };
        let mut themes = model;
        let label = labels(&papers);
        if by_model {
            // what the model left out is kept, under its paper
            let placed: HashSet<&String> = themes.iter().flat_map(|t| &t.annotation_ids).collect();
            let rest: Vec<String> = items.iter().filter(|i| !placed.contains(&i.annotation_id)).map(|i| i.annotation_id.clone()).collect();
            // each theme's points in the papers' order
            let pos: HashMap<&String, usize> = items.iter().enumerate().map(|(i, x)| (&x.annotation_id, i)).collect();
            for t in themes.iter_mut() {
                t.annotation_ids.sort_by_key(|id| pos.get(id).copied().unwrap_or(usize::MAX));
            }
            if !rest.is_empty() {
                themes.push(OutlineTheme { title: "Other highlights".into(), summary: String::new(), annotation_ids: rest });
            }
        } else {
            for p in &papers {
                let ids: Vec<String> = items.iter().filter(|i| i.paper_id == p.id).map(|i| i.annotation_id.clone()).collect();
                if !ids.is_empty() {
                    let t: String = crate::metadata::strip_tags(&p.meta.title);
                    themes.push(OutlineTheme { title: format!("{}: {}", label.get(&p.id).cloned().unwrap_or_default(), one_line(&t)), summary: String::new(), annotation_ids: ids });
                }
            }
        }
        let in_outline: HashSet<&String> = items.iter().map(|i| &i.paper_id).collect();
        let cited: Vec<Paper> = papers.iter().filter(|p| in_outline.contains(&p.id)).cloned().collect();
        let mut intro = format!(
            "From {} in {}. Each point is a highlight, word for word, or the researcher's own note (marked Note).",
            plural(items.len(), "highlight or note", "highlights and notes"),
            plural(cited.len(), "paper", "papers")
        );
        intro += if by_model { " Themes grouped and named by the on-device model; its summaries are marked." } else { " Grouped by paper and section." };
        if items.is_empty() {
            note = if papers.is_empty() { "No papers were chosen.".into() } else { "These papers have no highlights or notes yet.".into() };
        }
        let markdown = write(&title, &intro, &themes, &items, &cited, by_model);
        Outline {
            html: markdown_html(&markdown),
            markdown,
            title,
            themes,
            items: items.len() as u32,
            papers: cited.len() as u32,
            by_model,
            note,
        }
    }
}

#[cfg(feature = "llm")]
#[uniffi::export]
impl Library {
    /// The outline with the open model's themes (desktop, Android): `outline_prompt` answered
    /// by `engine`; by paper and section if the model gives nothing usable. Blocking.
    pub fn outline_with_model(&self, ids: Vec<String>, topic: String, engine: std::sync::Arc<crate::llm::AiEngine>, max_items: u32) -> Outline {
        let p = self.outline_prompt(ids.clone(), topic.clone(), max_items);
        let a = engine.plain_answer(&p.instructions, &p.prompt, p.max_tokens).ok();
        self.outline_from_highlights(ids, topic, Some(p), a)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn lib() -> (tempfile::TempDir, Arc<Library>) {
        let d = tempfile::tempdir().unwrap();
        let l = Library::open(d.path().join("lib").to_string_lossy().into(), d.path().join("cache").to_string_lossy().into(), "test".into()).unwrap();
        (d, l)
    }

    const TEXT_A: &str = "Multiple sclerosis lesions evolve over years. Chronic active lesions have a rim of iron-laden microglia. \
        Remyelination is more common in early lesions. Oligodendrocytes near the rim show stress responses.";
    const TEXT_B: &str = "In situ sequencing maps hundreds of genes in tissue sections. Probe design limits which transcripts are seen. \
        Microglia states differ between white and grey matter.";

    /// Two papers with highlights (one with a note, one with *markup* characters), a note on its
    /// own, and a paper without highlights.
    fn library() -> (tempfile::TempDir, Arc<Library>, Vec<String>) {
        let (d, l) = lib();
        let a = l
            .add_paper(
                PaperMeta { title: "Cellular architecture of evolving neuroinflammatory lesions".into(), authors: vec!["Petra Kukanja".into(), "C. M. Langseth".into(), "G. Castelo-Branco".into()], date: "2024".into(), journal: "Cell".into(), doi: "10.1016/j.cell.2024.02.030".into(), ..Default::default() },
                None,
                None,
                Some(TEXT_A.into()),
            )
            .unwrap()
            .id;
        let b = l
            .add_paper(PaperMeta { title: "In situ sequencing for RNA analysis".into(), authors: vec!["Rongqin Ke".into(), "Mats Nilsson".into()], date: "2013".into(), journal: "Nature Methods".into(), ..Default::default() }, None, None, Some(TEXT_B.into()))
            .unwrap()
            .id;
        let c = l.add_paper(PaperMeta { title: "A paper not highlighted".into(), authors: vec!["Nobody".into()], date: "2020".into(), ..Default::default() }, None, None, None).unwrap().id;
        l.add_highlight(a.clone(), "Chronic active lesions have a rim of iron-laden microglia.".into(), None, "yellow".into(), "compare with *our* MRI data".into()).unwrap();
        l.add_highlight(a.clone(), "Remyelination is more common in early lesions.".into(), None, "green".into(), String::new()).unwrap();
        l.add_highlight(b.clone(), "Microglia states differ between white and grey matter.".into(), None, "yellow".into(), String::new()).unwrap();
        l.add_highlight(b.clone(), "Probe design limits which transcripts are seen.".into(), None, "blue".into(), String::new()).unwrap();
        let note = Annotation {
            id: String::new(),
            paper_id: b.clone(),
            kind: AnnotationKind::Note,
            color: "yellow".into(),
            location: "pdf:2".into(),
            position: String::new(),
            quote: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            note: "Use this [method] for the_thesis chapter 3".into(),
            created: 0,
            modified: 0,
        };
        l.save_annotation(note).unwrap();
        (d, l, vec![a, b, c])
    }

    #[test]
    fn labels_and_citations() {
        let m = |a: &[&str], y: &str| PaperMeta { authors: a.iter().map(|s| s.to_string()).collect(), date: y.into(), title: "On things".into(), ..Default::default() };
        assert_eq!(author_year(&m(&["Petra Kukanja", "B", "C"], "2024-04-11")), "Kukanja et al. 2024");
        assert_eq!(author_year(&m(&["Smith, John", "Lee, Ann"], "2019")), "Smith and Lee 2019");
        assert_eq!(author_year(&m(&["Ludwig van Beethoven"], "")), "van Beethoven n.d.");
        assert_eq!(author_year(&m(&[], "2001")), "“On things” 2001");
        assert_eq!(md_unescape(&md_escape("a *b* [c](d) \\ e_f #1 <g>")), "a *b* [c](d) \\ e_f #1 <g>");
    }

    #[test]
    fn by_paper_and_section_without_a_model() {
        let (_d, l, ids) = library();
        let o = l.outline_from_highlights(ids.clone(), String::new(), None, None);
        assert!(!o.by_model);
        assert_eq!((o.items, o.papers), (5, 2), "the paper without highlights isn't a source");
        assert_eq!(o.themes.len(), 2);
        assert!(o.themes[0].title.starts_with("Kukanja et al. 2024: Cellular architecture"));
        let md = &o.markdown;
        assert!(md.starts_with("# Highlights\n"));
        // the highlight word for word (escaped), its note marked, its citation with link and key
        let p = l.paper(ids[0].clone()).unwrap();
        let line = format!(
            "- “Chronic active lesions have a rim of iron-laden microglia.” — *Note:* compare with \\*our\\* MRI data ([Kukanja et al. 2024](lunta://open?id={}), [@{}])",
            ids[0], p.cite_key
        );
        assert!(md.contains(&line), "{md}");
        // a note on its own; the PDF page as its section
        assert!(md.contains("### Page 2"), "{md}");
        assert!(md.contains("- *Note:* Use this \\[method\\] for the\\_thesis chapter 3 ([Ke and Nilsson 2013](lunta://open?id="), "{md}");
        // the sources
        let src = md.split("## Sources").nth(1).unwrap();
        assert!(src.contains("**Kukanja et al. 2024**") && src.contains("https://doi.org/10.1016/j.cell.2024.02.030") && src.contains("[Open in Lunta](lunta://open?id="));
        assert!(!src.contains("not highlighted"));
        assert_eq!(check(md, &l.outline_items(ids.clone())), Vec::<String>::new());
        // HTML for rich text
        assert!(o.html.contains("<h2>Kukanja et al. 2024: Cellular architecture"));
        assert!(o.html.contains(&format!("<a href=\"lunta://open?id={}\">Kukanja et al. 2024</a>", ids[0])));
        assert!(o.html.contains("<em>Note:</em> compare with *our* MRI data"));
    }

    #[test]
    fn a_topic_without_a_model_keeps_what_mentions_it() {
        let (_d, l, ids) = library();
        let o = l.outline_from_highlights(ids.clone(), "microglia".into(), None, None);
        assert_eq!(o.items, 2);
        assert!(o.markdown.starts_with("# microglia\n"));
        assert!(o.note.contains("3 highlights and notes that don't mention “microglia” left out"), "{}", o.note);
        let o = l.outline_from_highlights(ids, "astrocytes".into(), None, None);
        assert_eq!(o.items, 5);
        assert!(o.note.starts_with("No highlight or note mentions"));
    }

    #[test]
    fn themes_from_the_model_never_add_text() {
        let (_d, l, ids) = library();
        let p = l.outline_prompt(ids.clone(), "microglia in MS".into(), 40);
        assert_eq!(p.ids.len(), 5);
        assert!(p.prompt.contains("The outline is about: microglia in MS"));
        assert!(p.prompt.contains("1. (Kukanja et al. 2024) \"Remyelination is more common in early lesions.\"") || p.prompt.contains("1. (Kukanja et al. 2024) \"Chronic active"), "{}", p.prompt);
        let items = l.outline_items(ids.clone());
        let n = |q: &str| p.ids.iter().position(|id| items.iter().any(|i| &i.annotation_id == id && i.quote.starts_with(q))).unwrap() + 1;
        let answer = format!(
            "<think>let me see</think>\nTheme: **Microglia across lesions**\nSummary: Microglia sit at lesion rims and differ by matter.\nItems: {}, {}, 99\n\n\
             Theme: Repair\nSummary: Remyelination happens early.\nItems: {}, {}\n- The cortex is made of cheese (Smith 2020).\n\
             Theme: Empty theme\nItems: 42\n",
            n("Chronic"),
            n("Microglia states"),
            n("Remyelination"),
            n("Chronic"),
        );
        let o = l.outline_from_highlights(ids.clone(), "microglia in MS".into(), Some(p.clone()), Some(answer));
        assert!(o.by_model);
        let titles: Vec<&str> = o.themes.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["Microglia across lesions", "Repair", "Other highlights"]);
        // a highlight once, in the first theme naming it; the rest kept under Other highlights
        assert_eq!(o.themes[0].annotation_ids.len(), 2);
        assert_eq!(o.themes[1].annotation_ids.len(), 1);
        assert_eq!(o.themes[2].annotation_ids.len(), 2);
        let md = &o.markdown;
        assert!(!md.contains("cheese"), "the model's own text is not a point");
        assert!(md.contains("*Summary (written by the on-device model; check it against the quotes):* Microglia sit at lesion rims"));
        assert_eq!(o.items, 5);
        assert_eq!(check(md, &items), Vec::<String>::new());
        // without a usable answer: by paper
        let o = l.outline_from_highlights(ids, String::new(), Some(p), Some("I cannot help with that.".into()));
        assert!(!o.by_model);
    }

    #[test]
    fn the_check_finds_invented_text() {
        let (_d, l, ids) = library();
        let items = l.outline_items(ids.clone());
        let md = l.outline_from_highlights(ids.clone(), String::new(), None, None).markdown;
        let cite = format!(" ([Ke and Nilsson 2013](lunta://open?id={}))", ids[1]);
        let invented = md.replacen("## Sources", &format!("- “Microglia cause multiple sclerosis.”{cite}\n\n## Sources"), 1);
        assert_eq!(check(&invented, &items).len(), 1);
        let altered = md.replace("Remyelination is more common in early lesions.", "Remyelination is common in all lesions.");
        assert_eq!(check(&altered, &items).len(), 1);
        let prose = md.replacen("## Sources", "Microglia are therefore the cause.\n\n## Sources", 1);
        assert_eq!(check(&prose, &items).len(), 1);
        let paraphrase = md.replacen("## Sources", &format!("- Microglia matter{cite}\n\n## Sources"), 1);
        assert_eq!(check(&paraphrase, &items).len(), 1);
        let note = md.replace("compare with \\*our\\* MRI data", "this proves our theory");
        assert_eq!(check(&note, &items).len(), 1);
    }

    #[test]
    fn sections_of_the_reading_view() {
        let x = "<html><head><title>t</title></head><body><h1>Paper</h1><p>Abstract text.</p><h2>Introduction</h2><p>Some intro.</p><h3>A detail</h3><p>Detail here.</p><h2>Results</h2><p>Found it.</p></body></html>";
        let h = headings(x);
        let names: Vec<&str> = h.iter().map(|x| x.1.as_str()).collect();
        assert_eq!(names, vec!["Paper", "Introduction", "Results"]);
        let text = crate::highlight::dom_text(x);
        let at = |q: &str| text[..text.find(q).unwrap()].encode_utf16().count().to_string();
        let a = |pos: String| Annotation { id: "1".into(), paper_id: "p".into(), kind: AnnotationKind::Highlight, color: String::new(), location: "epub".into(), position: pos, quote: String::new(), prefix: String::new(), suffix: String::new(), note: String::new(), created: 0, modified: 0 };
        assert_eq!(place(&a(at("Some intro")), Some(&h)).0, "Introduction");
        assert_eq!(place(&a(at("Detail here")), Some(&h)).0, "Introduction");
        assert_eq!(place(&a(at("Found")), Some(&h)).0, "Results");
    }
}
