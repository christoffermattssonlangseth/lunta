//! **Summarize Highlights**: a short condensation, by the on-device model, of the highlights and
//! notes an outline is made of (`Library::outline_items`), where every sentence cites the
//! highlights it is based on ("… iron-laden microglia [1, 4].").
//!
//! Lunta proposes, the researcher decides; nothing is invented. The model may only condense what
//! the highlights and notes say, and the code checks what it wrote (`validate`):
//!
//! - the answer is split into sentences (lines, bullets, sentence ends; "et al.", "Fig. 2", "e.g."
//!   and decimals don't end one);
//! - a sentence without a citation is dropped, and one citing a number that wasn't given;
//! - a sentence must be *supported* by the highlights it cites: most of its content words
//!   (≥ 60 %, plurals and word endings allowed) are in them, every number in it is in them, and a
//!   negation ("not", "no", "never", "without"…) only when they have one;
//! - the same sentence twice is kept once; at most the length's sentences are kept.
//!
//! How many were dropped is said. When nothing is left the summary is not shown (`ok` false):
//! the apps show the outline instead.
//!
//! Long sets don't fit the model's context (Apple's model about 4,000 tokens, Qwen3 on phones
//! 4,096): `summary_plan` numbers the highlights [1]…[n] once and cuts them, paper by paper, into
//! parts of at most `max_items`; each part's prompt numbers its own highlights 1…k (`ModelPrompt.ids`
//! maps them back), the model summarizes each part, `summary_combine_prompt` gives the checked
//! sentences of all parts (with the original numbers) to the model to combine, and
//! `summary_from_answers` checks the combined summary again (only numbers the parts cited).
//! If combining gives nothing usable, the parts' checked sentences are the summary. The final
//! summary's citations are renumbered 1, 2, 3… in the order they are first cited, and the cited
//! highlights listed under it, footnote style ("[2] “quote” — Kukanja et al. 2024").
//!
//! No model here: the apps run it (Apple's FoundationModels, Qwen3 via MLX, or `AiEngine` with the
//! `llm` feature: `Library::summary_with_model`). MCP has no model and no summary.

use super::{labels, md_escape, one_line, paper_link, topic_words, OutlineItem, NOTE};
use crate::model::*;
use crate::tidy::ModelPrompt;
use crate::Library;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

/// How long the summary is.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SummaryLength {
    /// 3 to 5 sentences.
    #[default]
    Short,
    /// 6 to 10 sentences.
    Longer,
}

/// A paragraph, or one point per sentence.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SummaryStyle {
    #[default]
    Prose,
    Bullets,
}

impl SummaryLength {
    fn range(self) -> (usize, usize) {
        match self {
            SummaryLength::Short => (3, 5),
            SummaryLength::Longer => (6, 10),
        }
    }
    /// Sentences for one part of a long set.
    fn part_range(self) -> (usize, usize) {
        match self {
            SummaryLength::Short => (2, 3),
            SummaryLength::Longer => (3, 4),
        }
    }
}

/// A highlight or note the summary may cite, with its number.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SummarySource {
    pub number: u32,
    pub annotation_id: String,
    pub paper_id: String,
    /// The highlighted text (empty for a note on the paper).
    pub quote: String,
    pub note: String,
    /// "Results", "Page 4" or "".
    pub section: String,
    /// "Kukanja et al. 2024" (2019a, 2019b when two are alike).
    pub label: String,
    pub cite_key: String,
    /// The paper as a reference (APA, one line).
    pub reference: String,
    /// lunta://open?id=<paper>&highlight=<annotation>: the paper, at the highlight.
    pub link: String,
    /// A region marked on a figure: the start of the figure's caption (`OutlineItem::context`),
    /// given to the model and counted as the mark's words; "" for highlights and notes.
    #[serde(default)]
    #[uniffi(default = "")]
    pub context: String,
}

/// What to ask the model, part by part.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SummaryPlan {
    pub title: String,
    pub topic: String,
    pub length: SummaryLength,
    pub style: SummaryStyle,
    /// The highlights and notes given to the model, numbered 1…n.
    pub sources: Vec<SummarySource>,
    /// One prompt per part (one when everything fits). Number k in a part's prompt is the
    /// highlight `ids[k - 1]`.
    pub parts: Vec<ModelPrompt>,
    /// Highlights and notes not given to the model (more than fit).
    pub left_out: u32,
    pub papers: u32,
    /// What was left out and why ("" when nothing was).
    pub note: String,
}

/// A sentence of the summary and the highlights it cites (numbers of `HighlightSummary.sources`).
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SummarySentence {
    /// Without its citations.
    pub text: String,
    pub citations: Vec<u32>,
}

/// The summary, checked.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct HighlightSummary {
    pub title: String,
    pub sentences: Vec<SummarySentence>,
    pub bullets: bool,
    /// The highlights cited, renumbered 1, 2… in the order the summary first cites them.
    pub sources: Vec<SummarySource>,
    /// The summary, the cited highlights (footnote style) and the sources, as Markdown.
    pub markdown: String,
    /// The same as HTML (for copying as rich text).
    pub html: String,
    /// The model's sentences left out by the check.
    pub dropped: u32,
    /// Sentences kept; false: nothing held up, show the outline instead.
    pub ok: bool,
    /// Highlights and notes given to the model, and their papers.
    pub items: u32,
    pub papers: u32,
    /// What the check left out, what didn't fit, what to do ("" when there is nothing to say).
    pub note: String,
}

/// Said with every summary (the apps show it too).
pub const SUMMARY_LABEL: &str = "Written by the on-device model from your highlights. Check it against the quotes below.";

/// The link to a highlight in its paper.
pub fn highlight_link(paper_id: &str, annotation_id: &str) -> String {
    format!("{}&highlight={annotation_id}", paper_link(paper_id))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---------------------------------------------------------------- prompts

fn style_words(style: SummaryStyle) -> &'static str {
    match style {
        SummaryStyle::Prose => "as one paragraph",
        SummaryStyle::Bullets => "as bullet points, one sentence per bullet starting with \"- \"",
    }
}

const RULES: &str = "Use only what the highlights and notes say: add no facts, numbers, names, causes or conclusions of your own, \
and no introduction. End every sentence with the numbers of the highlights it is based on, in square brackets, like [2] or [3, 7]. \
Use only the numbers given. Write nothing else.";

fn instructions(n: (usize, usize), style: SummaryStyle) -> String {
    format!(
        "You condense what a researcher highlighted in scientific papers. You get their highlights and notes, numbered. \
         Write {} to {} short sentences {} that say what the highlights say, grouping related ones. {RULES}",
        n.0,
        n.1,
        style_words(style)
    )
}

fn combine_instructions(n: (usize, usize), style: SummaryStyle) -> String {
    format!(
        "You combine partial summaries of what a researcher highlighted in scientific papers into one summary. \
         Every sentence of the partial summaries ends with the numbers of the highlights it is based on, in square brackets. \
         Write {} to {} short sentences {}. Use only what the partial summaries say: add no facts, numbers, names, causes or \
         conclusions of your own. End every sentence with the bracketed numbers of the sentences it draws on, copied from them, \
         like [4, 12]. Use only numbers that appear in the partial summaries. Write nothing else.",
        n.0,
        n.1,
        style_words(style)
    )
}

fn about_line(topic: &str) -> String {
    if topic.is_empty() { String::new() } else { format!("The summary is about: {topic}. Say what the highlights say about it.\n\n") }
}

/// A part's prompt: its highlights numbered 1…k.
fn part_prompt(part: &[&SummarySource], topic: &str, n: (usize, usize), style: SummaryStyle) -> ModelPrompt {
    let mut lines = String::new();
    for (i, s) in part.iter().enumerate() {
        let q: String = one_line(&s.quote).chars().take(320).collect();
        let note: String = one_line(&s.note).chars().take(180).collect();
        let mut l = format!("[{}] ({}", i + 1, s.label);
        if !s.section.is_empty() {
            l += &format!(", {}", s.section);
        }
        l.push(')');
        if !q.is_empty() {
            l += &format!(" \"{q}\"");
        }
        if !s.context.is_empty() {
            // a region marked on a figure: the model knows the figure only by its caption
            let c: String = s.context.chars().take(240).collect();
            l += &format!(" (a region the researcher marked on the figure; its caption: \"{c}\")");
        }
        if !note.is_empty() {
            l += &format!(" Researcher's note: {note}");
        }
        lines += &l;
        lines.push('\n');
    }
    ModelPrompt {
        instructions: instructions(n, style),
        prompt: format!("{}Highlights:\n{lines}\nWrite the summary ({} to {} sentences, each ending with its numbers in brackets).", about_line(topic), n.0, n.1),
        max_tokens: (n.1 as u32) * 70 + 80,
        ids: part.iter().map(|s| s.annotation_id.clone()).collect(),
    }
}

/// The highlights cut into parts of at most `max` (a paper's together while they fit).
fn cut(sources: &[SummarySource], max: usize) -> Vec<Vec<&SummarySource>> {
    let mut parts: Vec<Vec<&SummarySource>> = vec![];
    let mut cur: Vec<&SummarySource> = vec![];
    let mut i = 0;
    while i < sources.len() {
        let pid = &sources[i].paper_id;
        let j = sources[i..].iter().position(|s| &s.paper_id != pid).map(|k| i + k).unwrap_or(sources.len());
        let paper: Vec<&SummarySource> = sources[i..j].iter().collect();
        if !cur.is_empty() && cur.len() + paper.len() > max {
            parts.push(std::mem::take(&mut cur));
        }
        for chunk in paper.chunks(max) {
            if !cur.is_empty() && cur.len() + chunk.len() > max {
                parts.push(std::mem::take(&mut cur));
            }
            cur.extend_from_slice(chunk);
        }
        i = j;
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// The plan for these highlights (numbered in this order).
pub fn plan(items: &[OutlineItem], papers: &[Paper], topic: &str, length: SummaryLength, style: SummaryStyle, max_items: usize, max_parts: usize, topic_note: String) -> SummaryPlan {
    let max_items = max_items.max(1);
    let cap = max_items * max_parts.max(1);
    let label = labels(papers);
    let paper: HashMap<&str, &Paper> = papers.iter().map(|p| (p.id.as_str(), p)).collect();
    let given: Vec<&OutlineItem> = items.iter().filter(|i| paper.contains_key(i.paper_id.as_str())).take(cap).collect();
    let left_out = items.len().saturating_sub(given.len());
    let sources: Vec<SummarySource> = given
        .iter()
        .enumerate()
        .map(|(n, it)| {
            let p = paper[it.paper_id.as_str()];
            SummarySource {
                number: n as u32 + 1,
                annotation_id: it.annotation_id.clone(),
                paper_id: it.paper_id.clone(),
                quote: one_line(&it.quote),
                note: one_line(&it.note),
                section: it.section.clone(),
                label: label.get(&p.id).cloned().unwrap_or_default(),
                cite_key: p.cite_key.clone(),
                reference: one_line(&crate::export::citation(p, CitationStyle::Apa)),
                link: highlight_link(&p.id, &it.annotation_id),
                context: one_line(&it.context),
            }
        })
        .collect();
    let parts = cut(&sources, max_items);
    let range = if parts.len() > 1 { length.part_range() } else { length.range() };
    let prompts: Vec<ModelPrompt> = parts.iter().map(|p| part_prompt(p, topic, range, style_for_parts(parts.len(), style))).collect();
    let papers_in = sources.iter().map(|s| s.paper_id.as_str()).collect::<HashSet<&str>>().len();
    drop(parts);
    let mut note = topic_note;
    if left_out > 0 {
        let more = format!("The first {} were summarized; {} didn't fit the model.", sources.len(), plural(left_out, "highlight or note", "highlights and notes"));
        note = if note.is_empty() { more } else { format!("{note} {more}") };
    }
    let topic = topic.trim().to_string();
    SummaryPlan {
        title: if topic.is_empty() { "Summary of the highlights".into() } else { format!("Summary: {topic}") },
        topic,
        length,
        style,
        sources,
        parts: prompts,
        left_out: left_out as u32,
        papers: papers_in as u32,
        note,
    }
}

/// Parts of a long set are written as prose (they are combined).
fn style_for_parts(parts: usize, style: SummaryStyle) -> SummaryStyle {
    if parts > 1 { SummaryStyle::Prose } else { style }
}

// ---------------------------------------------------------------- reading the answer

fn cite_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\[\s*(?:highlights?\s*|nos?\.?\s*|#)?(\d+(?:\s*[-–]\s*\d+)?(?:\s*(?:[,;]\s*(?:and\s+)?|and\s+)#?\d+(?:\s*[-–]\s*\d+)?)*)\s*\]").unwrap())
}

/// The numbers in a citation's brackets ("2, 5", "3–6"; a range of at most 20).
fn numbers(inner: &str) -> Vec<u32> {
    static RANGE: OnceLock<Regex> = OnceLock::new();
    let range = RANGE.get_or_init(|| Regex::new(r"(\d+)(?:\s*[-–]\s*(\d+))?").unwrap());
    let mut out = vec![];
    for c in range.captures_iter(inner) {
        let a: u32 = c[1].parse().unwrap_or(0);
        match c.get(2).and_then(|b| b.as_str().parse::<u32>().ok()) {
            Some(b) if b >= a && b - a <= 20 => out.extend(a..=b),
            Some(b) => out.extend([a, b]),
            None => out.push(a),
        }
    }
    out
}

/// Words before a full stop that don't end a sentence.
const ABBREVIATIONS: &[&str] = &["al", "e.g", "eg", "i.e", "ie", "fig", "figs", "eq", "eqs", "vs", "approx", "ca", "cf", "ref", "refs", "no", "nos", "dr", "prof", "sp", "spp", "resp", "st", "mr", "ms"];

/// The answer's sentences, each with its citations still in it. Headings and lines that only
/// introduce ("Here is a summary:") are left out; bullets and numbering taken off.
pub fn sentences(answer: &str) -> Vec<String> {
    static BULLET: OnceLock<Regex> = OnceLock::new();
    let bullet = BULLET.get_or_init(|| Regex::new(r"^\s*(?:[-*•–]|\d+[.)])\s+").unwrap());
    let answer = crate::tidy::without_thinking(answer);
    let mut out: Vec<String> = vec![];
    for line in answer.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with("```") {
            continue;
        }
        let l = bullet.replace(l, "").replace("**", "");
        let l = l.trim();
        // "Summary:", "Here is the summary:" (no citation, ends with a colon)
        if l.ends_with(':') && !cite_re().is_match(l) {
            continue;
        }
        let l = l.strip_prefix("Summary:").map(str::trim).unwrap_or(l);
        for s in split_line(l) {
            let only_cites = cite_re().replace_all(&s, "").trim_matches(|c: char| c.is_whitespace() || ".,;".contains(c)).is_empty();
            match out.last_mut() {
                // "… lesions. [2, 5]": the citation belongs to the sentence before
                Some(prev) if only_cites => {
                    prev.push(' ');
                    prev.push_str(s.trim());
                }
                _ if only_cites => {}
                _ => out.push(s),
            }
        }
    }
    out
}

/// One line cut at its sentence ends (a citation right after the full stop stays with it).
fn split_line(l: &str) -> Vec<String> {
    let cs: Vec<char> = l.chars().collect();
    let mut out = vec![];
    let mut start = 0;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if !matches!(c, '.' | '!' | '?') {
            i += 1;
            continue;
        }
        // the word before it
        let mut w = i;
        while w > 0 && (cs[w - 1].is_alphanumeric() || cs[w - 1] == '.') {
            w -= 1;
        }
        let word: String = cs[w..i].iter().collect::<String>().to_lowercase();
        let decimal = i > 0 && cs[i - 1].is_ascii_digit() && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        let initial = word.chars().count() == 1 && cs[w].is_uppercase();
        if c == '.' && (decimal || initial || ABBREVIATIONS.contains(&word.trim_end_matches('.'))) {
            i += 1;
            continue;
        }
        // closing quotes and brackets, then citations, stay with the sentence
        let mut j = i + 1;
        while j < cs.len() && matches!(cs[j], '"' | '”' | '’' | '\'' | ')') {
            j += 1;
        }
        loop {
            let mut k = j;
            while k < cs.len() && cs[k] == ' ' {
                k += 1;
            }
            if k < cs.len() && cs[k] == '[' {
                let rest: String = cs[k..].iter().collect();
                if let Some(m) = cite_re().find(&rest).filter(|m| m.start() == 0) {
                    j = k + rest[..m.end()].chars().count();
                    while j < cs.len() && matches!(cs[j], '.' | ',' | ';') {
                        j += 1;
                    }
                    continue;
                }
            }
            break;
        }
        // a sentence ends where the next starts: a space, then not a lower-case letter
        let mut k = j;
        while k < cs.len() && cs[k].is_whitespace() {
            k += 1;
        }
        if k >= cs.len() || (k > j && !cs[k].is_lowercase()) {
            out.push(cs[start..j].iter().collect::<String>().trim().to_string());
            start = k;
            i = k;
            continue;
        }
        i += 1;
    }
    let rest: String = cs[start.min(cs.len())..].iter().collect::<String>().trim().to_string();
    if !rest.is_empty() {
        out.push(rest);
    }
    out.retain(|s| !s.is_empty());
    out
}

/// A sentence's text without its citations, and the numbers it cites (in order, once).
pub fn citations(sentence: &str) -> (String, Vec<u32>) {
    let mut nums = vec![];
    for c in cite_re().captures_iter(sentence) {
        for n in numbers(&c[1]) {
            if !nums.contains(&n) {
                nums.push(n);
            }
        }
    }
    let text = cite_re().replace_all(sentence, "");
    let text = one_line(&text);
    // the space a citation left before punctuation, and doubled punctuation
    static SPACE: OnceLock<Regex> = OnceLock::new();
    let space = SPACE.get_or_init(|| Regex::new(r"\s+([.,;:!?)])").unwrap());
    let text = space.replace_all(&text, "$1").to_string();
    let text = text.replace("..", ".").replace(",.", ".").replace(";.", ".");
    let text = text.trim_matches(|c: char| c == '*' || c == '_' || c == ',' || c == ';' || c.is_whitespace()).to_string();
    (text, nums)
}

// ---------------------------------------------------------------- support

/// Words a summary may use that the highlights needn't have.
const GLUE: &[&str] = &[
    "highlight", "highlighted", "highlights", "note", "notes", "noted", "researcher", "author", "authors", "study", "studie", "studies",
    "paper", "work", "research", "show", "shows", "shown", "showed", "suggest", "suggests", "suggested", "report", "reports", "reported",
    "find", "finds", "found", "finding", "findings", "describe", "describes", "described", "indicate", "indicates", "indicated",
    "observe", "observes", "observed", "propose", "proposes", "proposed", "argue", "argues", "argued", "discuss", "discusses",
    "discussed", "compare", "compares", "compared", "emphasize", "emphasizes", "emphasise", "emphasises", "point", "points",
    "mention", "mentions", "mentioned", "according", "however", "thus", "together", "overall", "including", "include", "includes",
    "several", "many", "various", "another", "further", "furthermore", "moreover", "additionally", "addition", "particular",
    "particularly", "especially", "respectively", "likewise", "similarly", "contrast", "instead", "rather", "whether", "because",
    "since", "although", "though", "yet", "still", "even", "well", "like", "same", "own", "per", "researchers", "evidence",
    "reference", "references", "focus", "focuses", "mainly", "key", "important", "notably", "related", "relate", "relates",
];

/// Words that turn a statement around.
const NEGATIONS: &[&str] = &["not", "no", "never", "neither", "nor", "none", "without", "cannot", "unlike", "lack", "lacks", "lacking", "absent", "absence", "fail", "fails", "failed"];

fn tokens(s: &str) -> Vec<String> {
    crate::tidy::text::phrase_key(&s.replace("n't", " not")).split(' ').filter(|w| !w.is_empty()).map(String::from).collect()
}

fn content(w: &str) -> bool {
    let letters = w.chars().filter(|c| c.is_alphabetic()).count();
    let digits = w.chars().any(|c| c.is_ascii_digit());
    (digits || letters >= 3) && !crate::tidy::text::is_stop(w) && !GLUE.contains(&w) && !NEGATIONS.contains(&w)
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// The word is in the vocabulary, as it is or with another ending ("differ" ~ "differences",
/// "remyelination" ~ "remyelinate"; not "microglia" ~ "microscopy").
fn known(w: &str, vocab: &HashSet<String>) -> bool {
    if vocab.contains(w) {
        return true;
    }
    if w.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    let n = w.chars().count();
    if n < 5 {
        return false;
    }
    vocab.iter().any(|v| {
        let m = v.chars().count();
        m >= 4 && common_prefix(w, v) >= 5.max(n.min(m).saturating_sub(3))
    })
}

/// Share of a sentence's content words that must be in the highlights it cites.
pub const SUPPORT: f64 = 0.6;

/// Whether the sentence says only what the cited highlights (and notes) say, by their words:
/// most of its content words, all its numbers, a negation only when they have one.
pub fn supported(text: &str, cited: &[&SummarySource]) -> bool {
    let mut vocab: HashSet<String> = HashSet::new();
    for s in cited {
        for t in tokens(&format!("{} {} {} {}", s.quote, s.note, s.label, s.context)) {
            vocab.insert(t);
        }
    }
    let words = tokens(text);
    let negated = words.iter().any(|w| NEGATIONS.contains(&w.as_str()));
    if negated && !vocab.iter().any(|w| NEGATIONS.contains(&w.as_str())) {
        return false;
    }
    let content: Vec<&String> = words.iter().filter(|w| content(w)).collect();
    if content.is_empty() {
        return false;
    }
    if content.iter().any(|w| w.chars().any(|c| c.is_ascii_digit()) && !vocab.contains(w.as_str())) {
        return false;
    }
    let ok = content.iter().filter(|w| known(w, &vocab)).count();
    ok as f64 >= SUPPORT * content.len() as f64
}

/// Why sentences were dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drops {
    pub uncited: usize,
    pub unknown: usize,
    pub unsupported: usize,
}

impl Drops {
    pub fn total(&self) -> usize {
        self.uncited + self.unknown + self.unsupported
    }
    fn add(&mut self, o: Drops) {
        self.uncited += o.uncited;
        self.unknown += o.unknown;
        self.unsupported += o.unsupported;
    }
    fn says(&self) -> String {
        if self.total() == 0 {
            return String::new();
        }
        let mut why = vec![];
        if self.uncited > 0 {
            why.push(format!("{} without a citation", self.uncited));
        }
        if self.unknown > 0 {
            why.push(format!("{} citing a highlight that doesn't exist", self.unknown));
        }
        if self.unsupported > 0 {
            why.push(format!("{} saying more than the highlights {} cited", self.unsupported, if self.unsupported == 1 { "it" } else { "they" }));
        }
        format!("Left out {} of the model's sentences: {}.", self.total(), why.join(", "))
    }
}

/// The answer's sentences that hold up, with their citations as the plan's numbers. `ids` maps
/// the prompt's numbers (k → `ids[k - 1]`); `allowed`, if given, the plan's numbers that may be
/// cited. At most `max` sentences.
pub fn validate(answer: &str, ids: &[String], sources: &[SummarySource], allowed: Option<&HashSet<u32>>, max: usize) -> (Vec<SummarySentence>, Drops) {
    let by_id: HashMap<&str, &SummarySource> = sources.iter().map(|s| (s.annotation_id.as_str(), s)).collect();
    let mut kept: Vec<SummarySentence> = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    let mut drops = Drops::default();
    for s in sentences(answer) {
        let (text, nums) = citations(&s);
        if text.chars().filter(|c| c.is_alphabetic()).count() < 3 {
            continue;
        }
        if nums.is_empty() {
            drops.uncited += 1;
            continue;
        }
        let cited: Option<Vec<&SummarySource>> = nums
            .iter()
            .map(|n| (*n as usize).checked_sub(1).and_then(|k| ids.get(k)).and_then(|id| by_id.get(id.as_str()).copied()))
            .map(|s| s.filter(|s| allowed.is_none_or(|a| a.contains(&s.number))))
            .collect();
        let Some(cited) = cited else {
            drops.unknown += 1;
            continue;
        };
        if !supported(&text, &cited) {
            drops.unsupported += 1;
            continue;
        }
        if !seen.insert(crate::tidy::text::phrase_key(&text)) || kept.len() >= max {
            continue;
        }
        let mut c: Vec<u32> = cited.iter().map(|s| s.number).collect();
        c.dedup();
        kept.push(SummarySentence { text, citations: c });
    }
    (kept, drops)
}

// ---------------------------------------------------------------- combining

/// The parts' checked sentences (plan numbers) and what was dropped.
fn checked_parts(plan: &SummaryPlan, answers: &[String]) -> (Vec<Vec<SummarySentence>>, Drops) {
    let mut drops = Drops::default();
    let max = if plan.parts.len() > 1 { plan.length.part_range().1 + 1 } else { plan.length.range().1 };
    let parts = plan
        .parts
        .iter()
        .zip(answers.iter())
        .map(|(p, a)| {
            let (s, d) = validate(a, &p.ids, &plan.sources, None, max);
            drops.add(d);
            s
        })
        .collect();
    (parts, drops)
}

fn with_cites(s: &SummarySentence) -> String {
    let (body, end) = split_end(&s.text);
    format!("{body} [{}]{end}", s.citations.iter().map(u32::to_string).collect::<Vec<_>>().join(", "))
}

/// "Text." → ("Text", "."); text without an end gets a full stop.
fn split_end(t: &str) -> (&str, &'static str) {
    for e in [".", "!", "?"] {
        if let Some(b) = t.strip_suffix(e) {
            return (b.trim_end(), if e == "!" { "!" } else if e == "?" { "?" } else { "." });
        }
    }
    (t, ".")
}

/// The prompt to combine the parts' summaries (the model's `answers`, one per part), or none
/// when there is one part or only one part gave sentences that hold up. Its numbers are the
/// plan's.
pub fn combine_prompt(plan: &SummaryPlan, answers: &[String]) -> Option<ModelPrompt> {
    let (parts, _) = checked_parts(plan, answers);
    if plan.parts.len() < 2 || parts.iter().filter(|p| !p.is_empty()).count() < 2 {
        return None;
    }
    let by_num: HashMap<u32, &SummarySource> = plan.sources.iter().map(|s| (s.number, s)).collect();
    let mut text = String::new();
    for (k, p) in parts.iter().enumerate().filter(|(_, p)| !p.is_empty()) {
        let mut who: Vec<String> = vec![];
        for s in p.iter().flat_map(|s| &s.citations) {
            if let Some(l) = by_num.get(s).map(|x| x.label.clone()) {
                if !who.contains(&l) {
                    who.push(l);
                }
            }
        }
        text += &format!("Part {} ({}):\n", k + 1, who.join("; "));
        for s in p {
            text += &with_cites(s);
            text.push('\n');
        }
        text.push('\n');
    }
    let n = plan.length.range();
    Some(ModelPrompt {
        instructions: combine_instructions(n, plan.style),
        prompt: format!("{}Partial summaries:\n{text}Combine them into one summary ({} to {} sentences, each ending with its numbers in brackets).", about_line(&plan.topic), n.0, n.1),
        max_tokens: (n.1 as u32) * 70 + 80,
        ids: plan.sources.iter().map(|s| s.annotation_id.clone()).collect(),
    })
}

// ---------------------------------------------------------------- the summary

/// The summary from the model's answers to the plan's parts (in order) and, for several parts,
/// its answer to `combine_prompt`.
pub fn from_answers(plan: &SummaryPlan, answers: &[String], combined: Option<&str>) -> HighlightSummary {
    let (parts, mut drops) = checked_parts(plan, answers);
    let n = plan.length.range();
    let mut extra = String::new();
    let sentences: Vec<SummarySentence> = if plan.parts.len() < 2 {
        parts.into_iter().next().unwrap_or_default()
    } else {
        let allowed: HashSet<u32> = parts.iter().flatten().flat_map(|s| s.citations.iter().copied()).collect();
        let ids: Vec<String> = plan.sources.iter().map(|s| s.annotation_id.clone()).collect();
        let combined = combined.map(|c| validate(c, &ids, &plan.sources, Some(&allowed), n.1));
        match combined {
            Some((s, d)) if !s.is_empty() => {
                drops.add(d);
                s
            }
            other => {
                if let Some((_, d)) = other {
                    drops.add(d);
                }
                let all: Vec<SummarySentence> = parts.into_iter().flatten().collect();
                if !all.is_empty() {
                    extra = "The model couldn't combine the parts: the summaries of each part are shown.".into();
                }
                all
            }
        }
    };
    write(plan, sentences, drops, extra)
}

fn write(plan: &SummaryPlan, sentences: Vec<SummarySentence>, drops: Drops, extra: String) -> HighlightSummary {
    // renumbered in the order first cited
    let mut order: Vec<u32> = vec![];
    for n in sentences.iter().flat_map(|s| &s.citations) {
        if !order.contains(n) {
            order.push(*n);
        }
    }
    let new: HashMap<u32, u32> = order.iter().enumerate().map(|(i, n)| (*n, i as u32 + 1)).collect();
    let by_num: HashMap<u32, &SummarySource> = plan.sources.iter().map(|s| (s.number, s)).collect();
    let sources: Vec<SummarySource> = order.iter().filter_map(|n| by_num.get(n).map(|s| SummarySource { number: new[n], ..(*s).clone() })).collect();
    let sentences: Vec<SummarySentence> = sentences
        .into_iter()
        .map(|s| {
            let mut c: Vec<u32> = s.citations.iter().map(|n| new[n]).collect();
            c.sort_unstable();
            c.dedup();
            SummarySentence { text: s.text, citations: c }
        })
        .collect();
    let ok = !sentences.is_empty();
    let mut notes: Vec<String> = vec![];
    if !ok {
        notes.push(if plan.sources.is_empty() {
            "There are no highlights or notes to summarize.".to_string()
        } else {
            "The model's summary didn't hold up: none of its sentences cited the highlights it was based on and said only what they say, so it isn't shown. The outline is shown instead.".to_string()
        });
    }
    let d = drops.says();
    if ok && !d.is_empty() {
        notes.push(d);
    }
    if !extra.is_empty() {
        notes.push(extra);
    }
    if !plan.note.is_empty() {
        notes.push(plan.note.clone());
    }
    let bullets = plan.style == SummaryStyle::Bullets;
    let (markdown, html) = if ok { (markdown(plan, &sentences, &sources, bullets), html(plan, &sentences, &sources, bullets)) } else { (String::new(), String::new()) };
    HighlightSummary {
        title: plan.title.clone(),
        sentences,
        bullets,
        sources,
        markdown,
        html,
        dropped: drops.total() as u32,
        ok,
        items: plan.sources.len() as u32,
        papers: plan.papers,
        note: notes.join(" "),
    }
}

fn cites_md(c: &[u32]) -> String {
    // plain brackets: without a link after them Markdown leaves them as they are
    format!("[{}]", c.iter().map(u32::to_string).collect::<Vec<_>>().join(", "))
}

/// A cited highlight, footnote style: "[2] “quote” — *Note:* … — [Kukanja et al. 2024](link), [@key]".
fn footnote_md(s: &SummarySource) -> String {
    let mut l = format!("[{}] ", s.number);
    if !s.quote.is_empty() {
        l += &format!("“{}”", md_escape(&s.quote));
        if !s.note.is_empty() {
            l += &format!(" — *{NOTE}* {}", md_escape(&s.note));
        }
    } else {
        l += &format!("*{NOTE}* {}", md_escape(&s.note));
    }
    let key = if s.cite_key.is_empty() { String::new() } else { format!(", [@{}]", s.cite_key) };
    l + &format!(" — [{}]({}){key}", md_escape(&s.label), s.link)
}

fn markdown(plan: &SummaryPlan, sentences: &[SummarySentence], sources: &[SummarySource], bullets: bool) -> String {
    let mut md = format!("# {}\n\n*{}*\n\n", md_escape(&plan.title), md_escape(SUMMARY_LABEL));
    let lines: Vec<String> = sentences
        .iter()
        .map(|s| {
            let (body, end) = split_end(&s.text);
            format!("{} {}{end}", md_escape(body), cites_md(&s.citations))
        })
        .collect();
    if bullets {
        for l in &lines {
            md += &format!("- {l}\n");
        }
        md.push('\n');
    } else {
        md += &lines.join(" ");
        md += "\n\n";
    }
    md += "## Highlights cited\n\n";
    for s in sources {
        md += &footnote_md(s);
        md += "\n\n";
    }
    // the papers, once
    let mut seen = HashSet::new();
    let mut papers: BTreeMap<String, &SummarySource> = BTreeMap::new();
    for s in sources {
        if seen.insert(s.paper_id.clone()) {
            papers.insert(s.label.to_lowercase(), s);
        }
    }
    md += "## Sources\n\n";
    for s in papers.values() {
        let key = if s.cite_key.is_empty() { String::new() } else { format!(" [@{}]", s.cite_key) };
        md += &format!("- **{}**{key}: {} [Open in Lunta]({})\n", md_escape(&s.label), s.reference, paper_link(&s.paper_id));
    }
    md
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn html(plan: &SummaryPlan, sentences: &[SummarySentence], sources: &[SummarySource], bullets: bool) -> String {
    let link: HashMap<u32, &str> = sources.iter().map(|s| (s.number, s.link.as_str())).collect();
    let cites = |c: &[u32]| {
        let inner: Vec<String> = c.iter().map(|n| format!("<a href=\"{}\">{n}</a>", esc(link.get(n).copied().unwrap_or("")))).collect();
        format!("[{}]", inner.join(", "))
    };
    let mut h = format!("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>\n<h1>{}</h1>\n<p><em>{}</em></p>\n", esc(&plan.title), esc(SUMMARY_LABEL));
    let lines: Vec<String> = sentences
        .iter()
        .map(|s| {
            let (body, end) = split_end(&s.text);
            format!("{} {}{end}", esc(body), cites(&s.citations))
        })
        .collect();
    if bullets {
        h += "<ul>\n";
        for l in &lines {
            h += &format!("<li>{l}</li>\n");
        }
        h += "</ul>\n";
    } else {
        h += &format!("<p>{}</p>\n", lines.join(" "));
    }
    h += "<h2>Highlights cited</h2>\n";
    for s in sources {
        let mut l = format!("<p>[{}] ", s.number);
        if !s.quote.is_empty() {
            l += &format!("“{}”", esc(&s.quote));
            if !s.note.is_empty() {
                l += &format!(" — <em>{NOTE}</em> {}", esc(&s.note));
            }
        } else {
            l += &format!("<em>{NOTE}</em> {}", esc(&s.note));
        }
        let key = if s.cite_key.is_empty() { String::new() } else { format!(", [@{}]", esc(&s.cite_key)) };
        l += &format!(" — <a href=\"{}\">{}</a>{key}</p>\n", esc(&s.link), esc(&s.label));
        h += &l;
    }
    h += "<h2>Sources</h2>\n<ul>\n";
    let mut seen = HashSet::new();
    let mut papers: BTreeMap<String, &SummarySource> = BTreeMap::new();
    for s in sources {
        if seen.insert(s.paper_id.clone()) {
            papers.insert(s.label.to_lowercase(), s);
        }
    }
    for s in papers.values() {
        // the reference as plain text (its *journal* markers taken off)
        h += &format!("<li><strong>{}</strong>: {} <a href=\"{}\">Open in Lunta</a></li>\n", esc(&s.label), esc(&s.reference.replace('*', "")), esc(&paper_link(&s.paper_id)));
    }
    h + "</ul>\n</body></html>\n"
}

// ---------------------------------------------------------------- exported

/// The prompt to combine a long set's parts (see `SummaryPlan`), none when not needed.
#[uniffi::export]
pub fn summary_combine_prompt(plan: SummaryPlan, answers: Vec<String>) -> Option<ModelPrompt> {
    combine_prompt(&plan, &answers)
}

/// The checked summary from the model's answers to the plan's parts and (several parts) to
/// `summary_combine_prompt`.
#[uniffi::export]
pub fn summary_from_answers(plan: SummaryPlan, answers: Vec<String>, combined: Option<String>) -> HighlightSummary {
    from_answers(&plan, &answers, combined.as_deref())
}

#[uniffi::export]
impl Library {
    /// What to ask the model to summarize these papers' highlights and notes (the outline's, with
    /// a topic only those that mention it): at most `max_items` per prompt (about 25 for Apple's
    /// on-device model, 45 for Qwen3 with 4,096 tokens), at most `max_parts` prompts.
    pub fn summary_plan(&self, ids: Vec<String>, topic: String, length: SummaryLength, style: SummaryStyle, max_items: u32, max_parts: u32) -> SummaryPlan {
        let (papers, all) = self.outline_material(&ids);
        let (items, note) = if topic_words(&topic).is_empty() { (all, String::new()) } else { Self::about_topic(all, &topic) };
        plan(&items, &papers, &topic, length, style, max_items as usize, max_parts as usize, note)
    }
}

#[cfg(feature = "llm")]
#[uniffi::export]
impl Library {
    /// The summary written by the open model (desktop, Android): every part, then combined.
    /// Blocking.
    pub fn summary_with_model(&self, ids: Vec<String>, topic: String, length: SummaryLength, style: SummaryStyle, engine: std::sync::Arc<crate::llm::AiEngine>, max_items: u32) -> HighlightSummary {
        let plan = self.summary_plan(ids, topic, length, style, max_items, 4);
        let answers: Vec<String> = plan.parts.iter().map(|p| engine.plain_answer(&p.instructions, &p.prompt, p.max_tokens).unwrap_or_default()).collect();
        let combined = combine_prompt(&plan, &answers).and_then(|p| engine.plain_answer(&p.instructions, &p.prompt, p.max_tokens).ok());
        from_answers(&plan, &answers, combined.as_deref())
    }
}

#[cfg(test)]
mod tests;
