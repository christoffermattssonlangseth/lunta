//! **Check Citations**: does the cited paper plausibly say what the citing sentence says?
//!
//! For each citation in a manuscript (Word, Google Docs, LaTeX) the citing sentence is taken
//! (`citing_sentence`, `tex_claims`: the sentence the citation is in, the citation and the other
//! citations in it taken out, and the sentence before it for context), and the cited paper's text
//! (its fulltext.txt, the reading view's text; else its abstract, which is said) is cut into
//! passages: paragraphs, long ones in runs of sentences, each a verbatim piece of the text with
//! the section it is in (`passages`). The passages that best match the sentence are found with
//! BM25 (`rank`, nothing leaves the device), the top 3 shown with their *word overlap* (the share
//! of the sentence's content words that are in the passage).
//!
//! Lunta proposes, the researcher decides; nothing is certain:
//!
//! - With an on-device model (`ClaimJudge`: Apple's on the Mac, Qwen3 on Windows and Linux) the
//!   model reads the sentence and the passages and answers Supported / Partly supported / Not
//!   found in this paper / Contradicted, quoting the passage words it relies on. The code checks
//!   the quotes (`judge`): a quote must be word for word in a passage given (case, spacing,
//!   quotes and dashes aside; "…" may join pieces of one passage); a verdict of support or
//!   contradiction without such a quote is taken as *Not found*.
//! - Without a model the passages are shown as *Possible support* with their overlap, for the
//!   researcher to judge; with no passage sharing the sentence's words, *No matching passage*.
//!
//! The citation API (`claims_request`: POST /api/claims on the Mac, /cite/claims on Windows and
//! Linux) answers the Word and Google Docs panels; MCP's check_citation gives agents the passages
//! and overlaps (no model there: the agent judges).

use crate::model::*;
use crate::tidy::ModelPrompt;
use crate::Library;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

/// The on-device model, as the app gives it (blocking).
#[uniffi::export(with_foreign)]
pub trait ClaimJudge: Send + Sync {
    /// The model's answer; None when there is no model here (or it failed).
    fn answer(&self, instructions: String, prompt: String, max_tokens: u32) -> Option<String>;
}

/// What the paper's text says about the sentence.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClaimVerdict {
    /// The model: the passages state it (with a quote found in the paper).
    Supported,
    /// The model: they support part of it, or a weaker or narrower version.
    Partly,
    /// The model: the passages don't address it (or its quotes weren't in them). Without a
    /// model: no passage shares the sentence's words.
    NotFound,
    /// The model: they say the opposite (with a quote found in the paper).
    Contradicted,
    /// No model: passages that share words with the sentence, for the researcher to judge.
    Possible,
    /// No text of the paper here (not in the library, or neither full text nor abstract).
    NoText,
}

impl ClaimVerdict {
    pub fn key(self) -> &'static str {
        match self {
            ClaimVerdict::Supported => "supported",
            ClaimVerdict::Partly => "partly",
            ClaimVerdict::NotFound => "notFound",
            ClaimVerdict::Contradicted => "contradicted",
            ClaimVerdict::Possible => "possible",
            ClaimVerdict::NoText => "noText",
        }
    }

    /// What the apps show ("Not found in this paper" is the model's; without one, "No matching passage").
    pub fn label(self, by_model: bool) -> &'static str {
        match self {
            ClaimVerdict::Supported => "Supported",
            ClaimVerdict::Partly => "Partly supported",
            ClaimVerdict::NotFound if by_model => "Not found in this paper",
            ClaimVerdict::NotFound => "No matching passage",
            ClaimVerdict::Contradicted => "Contradicted",
            ClaimVerdict::Possible => "Possible support",
            ClaimVerdict::NoText => "No text to check",
        }
    }
}

/// Which of the paper's texts was searched.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClaimTextSource {
    FullText,
    /// Only the abstract (no reading view, or its text isn't on this device).
    Abstract,
    None,
    /// The paper isn't in the library.
    Missing,
}

impl ClaimTextSource {
    pub fn key(self) -> &'static str {
        match self {
            ClaimTextSource::FullText => "fullText",
            ClaimTextSource::Abstract => "abstract",
            ClaimTextSource::None => "none",
            ClaimTextSource::Missing => "missing",
        }
    }
}

/// A piece of the paper's text, word for word.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClaimPassage {
    pub text: String,
    /// The heading it is under ("" when the text has none before it).
    pub section: String,
    /// Word overlap: the share (0–1) of the sentence's content words in the passage.
    pub score: f64,
    /// Its BM25 score for the sentence (for ranking).
    pub bm25: f64,
    /// Where it starts in the text, in characters.
    pub start: u32,
    /// lunta://open?id=…&find=… : the paper, at the passage.
    pub link: String,
}

/// A quote the model gave that is word for word in the paper.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClaimEvidence {
    pub quote: String,
    pub section: String,
    pub link: String,
}

/// One cited paper, ready for the model: its best passages and the prompt.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClaimPaper {
    pub paper_id: String,
    pub title: String,
    /// "Kukanja et al. 2024"
    pub label: String,
    pub source: ClaimTextSource,
    pub passages: Vec<ClaimPassage>,
    /// None when there is nothing to ask about (no text, or no passage shares a word).
    pub prompt: Option<ModelPrompt>,
}

/// The check of one sentence against one cited paper.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClaimResult {
    pub paper_id: String,
    pub title: String,
    pub label: String,
    pub source: ClaimTextSource,
    pub verdict: ClaimVerdict,
    /// The verdict is the on-device model's (checked).
    pub by_model: bool,
    /// The model's one-line reason ("" without one).
    pub reason: String,
    /// Quotes found word for word in the passages.
    pub evidence: Vec<ClaimEvidence>,
    pub passages: Vec<ClaimPassage>,
    /// What to know about this result ("Only the abstract…", "The model's quote wasn't…").
    pub note: String,
}

/// The check of one citing sentence.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClaimReport {
    /// The sentence as checked (citations taken out).
    pub claim: String,
    /// The sentence before it.
    pub context: String,
    /// Whether the on-device model judged (else the passages are for the researcher to judge).
    pub judged: bool,
    pub note: String,
    pub results: Vec<ClaimResult>,
}

/// A citation's place in a paragraph, in UTF-16 code units (as JavaScript counts).
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CiteSpan {
    pub start: u32,
    pub end: u32,
}

/// The sentence a citation is in.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct CitingSentence {
    /// As written, citations included.
    pub sentence: String,
    /// The sentence before it ("" at the start).
    pub previous: String,
    /// The sentence with the citations taken out: what is checked.
    pub claim: String,
}

/// A citation in a .tex file.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TexClaim {
    pub keys: Vec<String>,
    /// The command as written ("\citep[p.~4]{smith2019,lee2020}").
    pub command: String,
    /// 1-based line of the command.
    pub line: u32,
    pub sentence: String,
    pub previous: String,
    pub claim: String,
}

/// The citation API's answer: HTTP status and JSON body.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ClaimsAnswer {
    pub status: u16,
    pub body: String,
}

/// Passages shown and given to the model per paper.
pub const TOP: usize = 3;
/// What the panels and windows say about privacy.
pub const PRIVACY: &str = "Checked on this device: the document's sentences and the papers stay on it, nothing is sent anywhere.";
pub const NO_MODEL: &str = "No on-device model here, so Lunta shows the passages that share the most words with each sentence (word overlap). Read them and judge for yourself.";
pub const BY_MODEL: &str = "Suggested by the on-device model from the passages shown, and checked: every quote is word for word in the paper. It can be wrong; read the passages and decide.";

// ---------------------------------------------------------------- words

const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "in", "on", "at", "to", "for", "from", "by", "with", "as", "is", "are", "was", "were", "be", "been",
    "being", "this", "that", "these", "those", "it", "its", "which", "who", "whom", "whose", "what", "when", "where", "while", "than", "then",
    "there", "their", "they", "them", "we", "our", "us", "he", "she", "his", "her", "has", "have", "had", "do", "does", "did", "can", "could",
    "may", "might", "will", "would", "shall", "should", "must", "also", "into", "onto", "over", "under", "such", "both", "each", "other",
    "more", "most", "some", "any", "all", "only", "very", "via", "per", "thus", "however", "here", "using", "used", "use", "et", "al", "eg",
    "ie", "not", "no", "between", "within", "about", "after", "before", "during", "through", "across", "among", "because", "so", "if",
    "whether", "how", "one", "two", "show", "shown", "shows", "showed", "found", "find", "finds", "study", "studies", "reported", "report",
    "reports", "suggest", "suggests", "suggested", "demonstrated", "demonstrate", "demonstrates", "previously", "recent", "recently",
    "known", "well", "further", "furthermore", "moreover", "including", "include", "includes", "particular", "particularly", "respectively",
];

fn stop(w: &str) -> bool {
    STOP.contains(&w)
}

/// A word's stem: lower case, plural and common endings taken off ("microglia", "lesions" → "lesion").
pub fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    let n = w.chars().count();
    if w.chars().any(|c| c.is_ascii_digit()) {
        return w;
    }
    let cut = |s: &str, k: usize| s.chars().take(s.chars().count() - k).collect::<String>();
    if n > 4 && w.ends_with("ies") {
        return cut(&w, 3) + "y";
    }
    if n > 5 && w.ends_with("ing") {
        return cut(&w, 3);
    }
    if n > 4 && w.ends_with("ed") {
        return cut(&w, 2);
    }
    if n > 4 && (w.ends_with("ses") || w.ends_with("xes") || w.ends_with("ches") || w.ends_with("shes")) {
        return cut(&w, 2);
    }
    if n > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is") {
        return cut(&w, 1);
    }
    w
}

/// The words of a text (letters and digits; "IL-6", "CD8+" kept whole), lower case.
fn words(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || ((c == '-' || c == '+') && !cur.is_empty()) {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|w| w.trim_end_matches('-').to_lowercase()).filter(|w| !w.is_empty()).collect()
}

/// The content words' stems: no stop words, at least 3 letters or with a digit.
pub fn terms(s: &str) -> Vec<String> {
    words(s)
        .into_iter()
        .filter(|w| !stop(w) && (w.chars().count() >= 3 || w.chars().any(|c| c.is_ascii_digit())))
        .map(|w| stem(&w))
        .collect()
}

/// The share of the claim's content words (distinct) that are in the passage.
pub fn overlap(claim: &str, passage: &str) -> f64 {
    let q: HashSet<String> = terms(claim).into_iter().collect();
    if q.is_empty() {
        return 0.0;
    }
    let p: HashSet<String> = terms(passage).into_iter().collect();
    q.iter().filter(|w| p.contains(*w)).count() as f64 / q.len() as f64
}

// ---------------------------------------------------------------- sentences

const ABBREVIATIONS: &[&str] = &[
    "al", "e.g", "i.e", "eg", "ie", "fig", "figs", "eq", "eqs", "ref", "refs", "vs", "cf", "ca", "approx", "no", "nos", "vol", "pp", "p", "dr", "prof", "st",
    "resp", "suppl", "sect", "sec", "ch", "mr", "ms", "mrs", "inc", "ltd", "co", "jr", "sr", "etc", "viz", "ed", "eds", "n.b", "nb",
];

/// Where the sentences of a text are: [start, end) in characters, whitespace between them left out.
pub fn sentence_bounds(chars: &[char]) -> Vec<(usize, usize)> {
    let n = chars.len();
    let mut out = vec![];
    let mut start = 0;
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if matches!(c, '.' | '!' | '?') {
            // the end runs on through closing quotes and brackets
            let mut j = i + 1;
            while j < n && matches!(chars[j], '"' | '\'' | '”' | '’' | ')' | ']') {
                j += 1;
            }
            let mut k = j;
            while k < n && chars[k].is_whitespace() {
                k += 1;
            }
            let at_end = k >= n;
            let spaced = k > j;
            let next_starts = k < n && (chars[k].is_uppercase() || chars[k].is_ascii_digit() || matches!(chars[k], '"' | '“' | '‘' | '(' | '[' | '\''));
            if at_end || (spaced && next_starts && !(c == '.' && abbreviation(chars, start, i))) {
                out.push((start, j));
                start = k;
                i = k;
                continue;
            }
        }
        i += 1;
    }
    if start < n {
        let mut e = n;
        while e > start && chars[e - 1].is_whitespace() {
            e -= 1;
        }
        if e > start {
            out.push((start, e));
        }
    }
    // leading whitespace off each
    out.into_iter()
        .filter_map(|(mut s, e)| {
            while s < e && chars[s].is_whitespace() {
                s += 1;
            }
            (s < e).then_some((s, e))
        })
        .collect()
}

/// Whether the full stop at `dot` ends an abbreviation ("et al.", "Fig.", "e.g.", an initial "J.").
fn abbreviation(chars: &[char], from: usize, dot: usize) -> bool {
    let mut s = dot;
    while s > from && (chars[s - 1].is_alphanumeric() || chars[s - 1] == '.') {
        s -= 1;
    }
    let word: String = chars[s..dot].iter().collect::<String>().to_lowercase();
    if word.is_empty() {
        return false;
    }
    if ABBREVIATIONS.contains(&word.as_str()) {
        return true;
    }
    // a single capital (an initial), or dotted initials
    let raw: String = chars[s..dot].iter().collect();
    (raw.chars().count() == 1 && raw.chars().all(|c| c.is_uppercase())) || (raw.contains('.') && raw.chars().filter(|c| c.is_alphabetic()).count() <= 3)
}

fn utf16_to_char(chars: &[char], u: u32) -> usize {
    let mut n = 0u32;
    for (i, c) in chars.iter().enumerate() {
        if n >= u {
            return i;
        }
        n += c.len_utf16() as u32;
    }
    chars.len()
}

fn tidy_spaces(s: &str) -> String {
    static RE: OnceLock<[Regex; 3]> = OnceLock::new();
    let re = RE.get_or_init(|| {
        [Regex::new(r"\s+").unwrap(), Regex::new(r"\s+([.,;:!?)\]])").unwrap(), Regex::new(r"([(\[])\s+").unwrap()]
    });
    let s = re[0].replace_all(s.trim(), " ");
    let s = re[1].replace_all(&s, "$1");
    re[2].replace_all(&s, "$1").trim().to_string()
}

/// Citations typed as text, taken out of a claim: "[3]", "[1–4, 7]", "(Smith et al., 2019; Lee 2020)", "(ref. 4)".
pub fn strip_citations(s: &str) -> String {
    static RE: OnceLock<[Regex; 4]> = OnceLock::new();
    let re = RE.get_or_init(|| {
        [
            Regex::new(r"\[\s*\d+[a-z]?(?:\s*[,–—\-;]\s*\d+[a-z]?)*\s*\]").unwrap(),
            Regex::new(r"\((?:e\.g\.,?\s*|see\s+|cf\.\s*)?[^()]*?\b(?:1[89]|20)\d{2}[a-z]?(?:,\s*(?:p|pp)\.?\s*[\d–-]+)?(?:\s*;\s*[^()]*?\b(?:1[89]|20)\d{2}[a-z]?)*\)").unwrap(),
            Regex::new(r"\((?:refs?\.?|references?)\s*[\d,–\-\s]+\)").unwrap(),
            Regex::new(r"\(\s*[,;]?\s*\)|\[\s*[,;]?\s*\]").unwrap(),
        ]
    });
    let mut out = s.to_string();
    for r in re.iter() {
        out = r.replace_all(&out, " ").to_string();
    }
    tidy_spaces(&out)
}

/// The sentence a citation at [start, end) (UTF-16) of a paragraph is in, and the one before it
/// (in the paragraph, else the last of `before`, the paragraph before). `others`: the other
/// citations in the paragraph, taken out of the claim too. A citation with no letters ("[3]",
/// a superscript "3") that starts a sentence is the sentence before's: "…microglia.³ Next…".
pub fn citing_sentence(text: &str, start: u32, end: u32, others: &[CiteSpan], before: &str) -> CitingSentence {
    let chars: Vec<char> = text.chars().collect();
    let (s, e) = (utf16_to_char(&chars, start), utf16_to_char(&chars, end.max(start)));
    let mut spans: Vec<(usize, usize)> = others.iter().map(|o| (utf16_to_char(&chars, o.start), utf16_to_char(&chars, o.end.max(o.start)))).collect();
    spans.push((s, e));
    // citations masked, so their brackets and dots don't end sentences: a numeric one as space
    // (a full stop before it still ends the sentence), "Smith et al. (2020)" as a word that may
    // start one
    let mut masked = chars.clone();
    for &(a, b) in &spans {
        let b = b.min(chars.len());
        let letters = chars[a.min(b)..b].iter().any(|c| c.is_alphabetic());
        for c in masked.iter_mut().take(b).skip(a) {
            *c = if letters { 'X' } else { ' ' };
        }
    }
    let bounds = sentence_bounds(&masked);
    if bounds.is_empty() {
        return CitingSentence::default();
    }
    let mut k = bounds.iter().position(|&(a, b)| s < b && e > a).or_else(|| bounds.iter().rposition(|&(a, _)| a <= s)).unwrap_or(0);
    let cite: String = chars[s..e.min(chars.len())].iter().collect();
    let numeric = !cite.chars().any(|c| c.is_alphabetic());
    if numeric && k > 0 && s <= bounds[k].0 + 1 {
        k -= 1;
    }
    // a numeric citation right after the last sentence's full stop is that sentence's too
    let (a, mut b) = bounds[k];
    if numeric && e > b && masked[b..e.min(masked.len())].iter().all(|c| c.is_whitespace()) {
        b = e.min(chars.len());
    }
    // a citation before the sentence's first word ("Smith et al. (2020) showed…") is in it
    let shown_from = if s < a && (k == 0 || s >= bounds[k - 1].1) { s } else { a };
    let sentence: String = chars[shown_from..b].iter().collect();
    // the claim: the sentence without the citations in it
    let mut kept = String::new();
    for (i, c) in chars.iter().enumerate().take(b).skip(a) {
        if spans.iter().any(|&(x, y)| i >= x && i < y) {
            if !kept.ends_with(' ') {
                kept.push(' ');
            }
        } else {
            kept.push(*c);
        }
    }
    let previous: String = if k > 0 {
        let (pa, pb) = bounds[k - 1];
        chars[pa..pb].iter().collect()
    } else {
        let bc: Vec<char> = before.chars().collect();
        sentence_bounds(&bc).last().map(|&(pa, pb)| bc[pa..pb].iter().collect::<String>()).unwrap_or_default()
    };
    CitingSentence { sentence: tidy_spaces(&sentence), previous: strip_citations(&previous), claim: strip_citations(&kept) }
}

// ---------------------------------------------------------------- passages

/// Sections whose text isn't evidence (a reference list matches every sentence's words).
const SKIP_SECTIONS: &[&str] = &[
    "references", "reference", "bibliography", "literature cited", "works cited", "acknowledgements", "acknowledgments", "acknowledgement",
    "acknowledgment", "funding", "author contributions", "authors' contributions", "competing interests", "conflict of interest",
    "conflicts of interest", "declaration of interests", "declaration of competing interest", "data availability", "code availability",
    "data and code availability", "ethics declarations", "additional information", "supplementary information", "supplementary material",
];

fn heading(line: &str) -> bool {
    let n = line.chars().count();
    if !(2..80).contains(&n) || line.ends_with(['.', ',', ';']) {
        return false;
    }
    let low = line.to_lowercase();
    if low.starts_with("figure") || low.starts_with("fig.") || low.starts_with("table") || low.starts_with("doi") || low.starts_with("http") {
        return false;
    }
    let first = line.chars().next().unwrap_or(' ');
    line.split_whitespace().count() <= 12 && (first.is_uppercase() || first.is_ascii_digit()) && line.chars().any(char::is_alphabetic)
}

fn section_name(line: &str) -> String {
    // "2.1 Methods", "II. RESULTS" → as written, numbers kept
    line.trim().to_string()
}

fn skipped(section: &str) -> bool {
    let low = section.to_lowercase();
    let bare = low.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
    // "II. References"
    let bare = match bare.split_once(". ") {
        Some((r, rest)) if !r.is_empty() && r.chars().all(|c| "ivxlc".contains(c)) => rest,
        _ => bare,
    }
    .trim();
    SKIP_SECTIONS.iter().any(|s| bare == *s || bare.starts_with(&format!("{s} ")))
}

/// The passages of a text: its paragraphs (one per line, as fulltext.txt has them), long ones cut
/// in runs of sentences; each with the heading it is under. Reference lists, acknowledgements and
/// the like are left out. Scores are 0 (see `rank`).
pub fn passages(text: &str) -> Vec<ClaimPassage> {
    let mut out = vec![];
    let mut section = String::new();
    let mut skip = false;
    let mut offset = 0usize; // characters before the line
    for raw in text.split_inclusive('\n') {
        let line_start = offset;
        offset += raw.chars().count();
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let lead = raw.chars().take_while(|c| c.is_whitespace()).count();
        if heading(line) {
            section = section_name(line);
            skip = skipped(&section);
            continue;
        }
        if skip || line.chars().count() < 40 {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        if chars.len() <= 700 {
            out.push(ClaimPassage { text: line.to_string(), section: section.clone(), score: 0.0, bm25: 0.0, start: (line_start + lead) as u32, link: String::new() });
            continue;
        }
        // a long paragraph: runs of sentences of about 300–600 characters
        let bounds = sentence_bounds(&chars);
        let mut run: Option<(usize, usize)> = None;
        for (a, b) in bounds {
            run = match run {
                Some((ra, rb)) if b - ra <= 600 || rb - ra < 300 => Some((ra, b)),
                Some((ra, rb)) => {
                    out.push(ClaimPassage { text: chars[ra..rb].iter().collect(), section: section.clone(), score: 0.0, bm25: 0.0, start: (line_start + lead + ra) as u32, link: String::new() });
                    Some((a, b))
                }
                None => Some((a, b)),
            };
        }
        if let Some((ra, rb)) = run {
            out.push(ClaimPassage { text: chars[ra..rb].iter().collect(), section: section.clone(), score: 0.0, bm25: 0.0, start: (line_start + lead + ra) as u32, link: String::new() });
        }
    }
    out
}

/// The passages that best match the claim, best first (BM25 over the paper's passages), at most
/// `top`, each with its word overlap; only passages sharing at least one content word.
pub fn rank(all: &[ClaimPassage], claim: &str, top: usize) -> Vec<ClaimPassage> {
    let q: Vec<String> = {
        let mut seen = HashSet::new();
        terms(claim).into_iter().filter(|t| seen.insert(t.clone())).collect()
    };
    if q.is_empty() || all.is_empty() {
        return vec![];
    }
    let docs: Vec<Vec<String>> = all.iter().map(|p| terms(&p.text)).collect();
    let n = docs.len() as f64;
    let avg = (docs.iter().map(Vec::len).sum::<usize>() as f64 / n).max(1.0);
    let mut df: HashMap<&str, f64> = HashMap::new();
    for t in &q {
        df.insert(t, docs.iter().filter(|d| d.contains(t)).count() as f64);
    }
    let (k1, b) = (1.2, 0.75);
    let mut scored: Vec<(f64, usize)> = docs
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let len = d.len() as f64;
            let mut s = 0.0;
            for t in &q {
                let tf = d.iter().filter(|w| *w == t).count() as f64;
                if tf == 0.0 {
                    continue;
                }
                let idf = ((n - df[t.as_str()] + 0.5) / (df[t.as_str()] + 0.5) + 1.0).ln();
                s += idf * tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * len / avg));
            }
            (s > 0.0).then_some((s, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(top)
        .map(|(s, i)| {
            let mut p = all[i].clone();
            p.bm25 = (s * 100.0).round() / 100.0;
            p.score = (overlap(claim, &p.text) * 100.0).round() / 100.0;
            p
        })
        .collect()
}

/// The best passages of a text for a claim (`passages`, then `rank`).
#[uniffi::export]
pub fn claim_passages(text: String, claim: String, top: u32) -> Vec<ClaimPassage> {
    rank(&passages(&text), &claim, top as usize)
}

fn percent(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// lunta://open?id=<paper>&find=<the passage's first words>: the paper, found at the passage.
pub fn passage_link(paper_id: &str, text: &str) -> String {
    // the first words, up to a sentence end inside them (the reader finds text within a paragraph)
    let mut ws: Vec<&str> = vec![];
    for w in text.split_whitespace() {
        ws.push(w);
        if ws.len() >= 8 || (ws.len() >= 4 && w.ends_with(['.', ';', ':'])) {
            break;
        }
    }
    let find = ws.join(" ");
    let find = find.trim_end_matches(['.', ',', ';', ':']);
    format!("lunta://open?id={}&find={}", percent(paper_id), percent(find))
}

// ---------------------------------------------------------------- the model

fn instructions() -> String {
    "You check whether a scientific paper supports a sentence that a researcher wrote and cited the paper for. \
     Use only the numbered passages from the paper given in the message; they are the parts of it that best match the sentence. \
     Answer in exactly this form:\n\
     VERDICT: SUPPORTED, PARTLY, NOT FOUND or CONTRADICTED\n\
     EVIDENCE: [n] \"words copied exactly from passage n\"\n\
     REASON: one short sentence\n\
     SUPPORTED: the passages state what the sentence says. PARTLY: they support part of it, or a weaker, narrower or different version \
     (another species, condition or measure). CONTRADICTED: they state the opposite. NOT FOUND: they don't address it. \
     Give one or two EVIDENCE lines, each a short quote copied word for word from one passage (no paraphrase, no changes); none for NOT FOUND. \
     Don't use what you know about the topic, only the passages."
        .to_string()
}

fn prompt_for(claim: &str, context: &str, paper: &Paper, label: &str, source: ClaimTextSource, ps: &[ClaimPassage]) -> String {
    let mut s = format!("Sentence (it cites this paper): {claim}\n");
    if !context.trim().is_empty() {
        s += &format!("The sentence before it (context only): {}\n", context.trim());
    }
    s += &format!("Paper: {} ({label})\n", paper.meta.title.trim());
    if source == ClaimTextSource::Abstract {
        s += "Only the paper's abstract is available.\n";
    }
    s += "Passages:\n";
    for (i, p) in ps.iter().enumerate() {
        let sec = if p.section.is_empty() { String::new() } else { format!("({}) ", p.section) };
        s += &format!("[{}] {sec}{}\n", i + 1, p.text);
    }
    s
}

/// For comparing quotes with the text: lower case, one kind of quote, dash and space, ligatures undone.
pub fn normalized(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for c in s.chars() {
        let mapped: &str = match c {
            '‘' | '’' | '′' | '`' | '´' => "'",
            '“' | '”' | '″' | '„' => "\"",
            '–' | '—' | '−' | '‐' | '‑' | '‒' => "-",
            'ﬁ' => "fi",
            'ﬂ' => "fl",
            'ﬀ' => "ff",
            '\u{00AD}' => "",
            _ if c.is_whitespace() => " ",
            _ => "",
        };
        if mapped == " " {
            if !space {
                out.push(' ');
            }
            space = true;
            continue;
        }
        if mapped.is_empty() && c != '\u{00AD}' {
            for l in c.to_lowercase() {
                out.push(l);
            }
        } else {
            out.push_str(mapped);
        }
        space = false;
    }
    out.trim().to_string()
}

/// The passage (index) a quote is word for word in: "…" or "..." may join pieces of one passage,
/// each of at least three words; quotes, dashes, case and spacing aside.
pub fn quote_in(quote: &str, passages: &[ClaimPassage]) -> Option<usize> {
    let q = quote.trim().trim_matches(|c: char| c == '"' || c == '“' || c == '”' || c == '\'' || c == ' ');
    let pieces: Vec<String> = q
        .replace('…', "...")
        .split("...")
        .map(|p| normalized(p).trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '.' | '[' | ']' | '(' | ')')).to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if pieces.is_empty() || pieces.iter().any(|p| p.split_whitespace().count() < 3) {
        return None;
    }
    passages.iter().position(|p| {
        let t = normalized(&p.text);
        pieces.iter().all(|piece| t.contains(piece.as_str()))
    })
}

fn verdict_of(answer: &str) -> Option<ClaimVerdict> {
    let low = answer.to_lowercase();
    let line = low.lines().find(|l| l.trim_start().starts_with("verdict")).map(|l| l.to_string()).unwrap_or_else(|| low.lines().next().unwrap_or("").to_string());
    let pick = |l: &str| -> Option<ClaimVerdict> {
        if l.contains("contradict") {
            Some(ClaimVerdict::Contradicted)
        } else if l.contains("partly") || l.contains("partial") {
            Some(ClaimVerdict::Partly)
        } else if l.contains("not found") || l.contains("not supported") || l.contains("unsupported") || l.contains("no support") || l.contains("not addressed") {
            Some(ClaimVerdict::NotFound)
        } else if l.contains("support") {
            Some(ClaimVerdict::Supported)
        } else {
            None
        }
    };
    pick(&line).or_else(|| pick(&low))
}

fn quotes_of(answer: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"["“]([^"“”]{6,}?)["”]"#).unwrap());
    re.captures_iter(answer).map(|c| c[1].trim().to_string()).collect()
}

fn reason_of(answer: &str) -> String {
    answer
        .lines()
        .find_map(|l| {
            let t = l.trim();
            let low = t.to_lowercase();
            low.starts_with("reason").then(|| t.split_once(':').map(|(_, r)| r.trim().to_string()).unwrap_or_default())
        })
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect()
}

fn without_thinking(s: &str) -> String {
    match (s.find("<think>"), s.find("</think>")) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &s[..a], &s[b + "</think>".len()..]),
        (None, Some(b)) => s[b + "</think>".len()..].to_string(),
        _ => s.to_string(),
    }
}

fn source_note(source: ClaimTextSource) -> &'static str {
    match source {
        ClaimTextSource::Abstract => "Only the abstract is in your library (no reading view of the full text here), so the paper may say more than was checked.",
        ClaimTextSource::None => "Lunta has no text of this paper: no reading view and no abstract. Find its PDF to check it.",
        ClaimTextSource::Missing => "This paper isn’t in your Lunta library, so there is nothing to check it against.",
        ClaimTextSource::FullText => "",
    }
}

fn join_notes(a: &str, b: &str) -> String {
    [a, b].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ")
}

/// The result for one paper: the model's answer checked (`answer` Some), or without a model the
/// passages as possible support.
pub fn judge(paper: &ClaimPaper, answer: Option<&str>) -> ClaimResult {
    let mut r = ClaimResult {
        paper_id: paper.paper_id.clone(),
        title: paper.title.clone(),
        label: paper.label.clone(),
        source: paper.source,
        verdict: ClaimVerdict::NoText,
        by_model: false,
        reason: String::new(),
        evidence: vec![],
        passages: paper.passages.clone(),
        note: source_note(paper.source).to_string(),
    };
    if matches!(paper.source, ClaimTextSource::None | ClaimTextSource::Missing) {
        return r;
    }
    let lexical = |mut r: ClaimResult, extra: &str| {
        r.verdict = if r.passages.is_empty() { ClaimVerdict::NotFound } else { ClaimVerdict::Possible };
        r.by_model = false;
        r.note = join_notes(&r.note, extra);
        r
    };
    let Some(answer) = answer.map(without_thinking).filter(|a| !a.trim().is_empty()) else {
        return lexical(r, "");
    };
    let Some(v) = verdict_of(&answer) else {
        return lexical(r, "The model’s answer couldn’t be read, so the passages are shown for you to judge.");
    };
    r.by_model = true;
    r.reason = reason_of(&answer);
    let mut seen = HashSet::new();
    for q in quotes_of(&answer) {
        if let Some(i) = quote_in(&q, &paper.passages) {
            if seen.insert(normalized(&q)) && r.evidence.len() < 3 {
                let p = &paper.passages[i];
                r.evidence.push(ClaimEvidence { link: passage_link(&paper.paper_id, &q.replace('…', " ").replace("...", " ")), quote: q, section: p.section.clone() });
            }
        }
    }
    r.verdict = v;
    match v {
        ClaimVerdict::NotFound => r.evidence.clear(),
        ClaimVerdict::Supported | ClaimVerdict::Partly | ClaimVerdict::Contradicted if r.evidence.is_empty() => {
            r.verdict = ClaimVerdict::NotFound;
            r.note = join_notes(&r.note, &format!(
                "The model said “{}” but gave no quote that is word for word in the paper, so it is shown as not found. Read the passages and decide.",
                v.label(true).to_lowercase()
            ));
        }
        _ => {}
    }
    r
}

/// The checked result for one paper from the model's answer (None: no model).
#[uniffi::export]
pub fn claim_result(paper: ClaimPaper, answer: Option<String>) -> ClaimResult {
    judge(&paper, answer.as_deref())
}

/// The sentence a citation is in (see `citing_sentence`).
#[uniffi::export]
pub fn find_citing_sentence(text: String, start: u32, end: u32, others: Vec<CiteSpan>, before: String) -> CitingSentence {
    citing_sentence(&text, start, end, &others, &before)
}

// ---------------------------------------------------------------- LaTeX

const CITE_COMMANDS: &[&str] = &[
    "cite", "citep", "citet", "citealp", "citealt", "citeauthor", "citeyear", "citeyearpar", "parencite", "textcite", "autocite", "footcite",
    "supercite", "smartcite", "fullcite", "Cite", "Citep", "Citet", "Parencite", "Textcite", "Autocite", "Smartcite", "cites", "parencites",
    "textcites", "autocites",
];
/// Commands dropped with their argument.
const DROP_WITH_ARG: &[&str] = &[
    "label", "includegraphics", "vspace", "hspace", "vskip", "hskip", "bibliographystyle", "bibliography", "addbibresource", "nocite", "input",
    "include", "usepackage", "documentclass", "newcommand", "renewcommand", "setlength", "url", "pagestyle", "thispagestyle", "begin", "end",
    "printbibliography",
];
/// Commands that stand for a number in the text ("Fig. \ref{f1}").
const REF_COMMANDS: &[&str] = &["ref", "eqref", "pageref", "cref", "Cref", "autoref", "nameref"];
const BREAK_COMMANDS: &[&str] = &[
    "section", "subsection", "subsubsection", "chapter", "paragraph", "subparagraph", "part", "title", "author", "date", "caption", "item",
    "maketitle", "par", "newline", "bigskip", "medskip", "smallskip", "noindent",
];
/// Environments whose content isn't prose.
const SKIP_ENVS: &[&str] = &[
    "equation", "equation*", "align", "align*", "gather", "gather*", "multline", "multline*", "eqnarray", "eqnarray*", "tabular", "tabular*",
    "verbatim", "lstlisting", "minted", "thebibliography", "tikzpicture", "displaymath", "math", "comment",
];

/// Plain text of LaTeX made so far, with the citations found in it.
struct TexPara {
    text: String,
    /// [start, end) in UTF-16 of `text`, the keys, the command, its line.
    cites: Vec<(u32, u32, Vec<String>, String, u32)>,
}

fn tex_without_comments(src: &str) -> String {
    src.lines()
        .map(|l| {
            let b = l.as_bytes();
            let mut cut = l.len();
            for i in 0..b.len() {
                if b[i] == b'%' {
                    let mut k = 0;
                    let mut j = i;
                    while j > 0 && b[j - 1] == b'\\' {
                        k += 1;
                        j -= 1;
                    }
                    if k % 2 == 0 {
                        cut = i;
                        break;
                    }
                }
            }
            &l[..cut]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The {…} group starting at `i` (which must be '{'): its content and the index after it.
fn group(c: &[char], i: usize) -> Option<(String, usize)> {
    if c.get(i) != Some(&'{') {
        return None;
    }
    let mut depth = 0;
    for j in i..c.len() {
        match c[j] {
            '\\' => {}
            '{' if j == i || c[j - 1] != '\\' => depth += 1,
            '}' if c[j - 1] != '\\' => {
                depth -= 1;
                if depth == 0 {
                    return Some((c[i + 1..j].iter().collect(), j + 1));
                }
            }
            _ => {}
        }
    }
    None
}

fn skip_spaces(c: &[char], mut i: usize) -> usize {
    while i < c.len() && (c[i] == ' ' || c[i] == '\t') {
        i += 1;
    }
    i
}

/// The [..] options at `i`, skipped.
fn skip_options(c: &[char], mut i: usize) -> usize {
    loop {
        let j = skip_spaces(c, i);
        if c.get(j) != Some(&'[') {
            return i;
        }
        let mut depth = 0;
        let mut k = j;
        while k < c.len() {
            match c[k] {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        i = (k + 1).min(c.len());
    }
}

/// The citations of a .tex file, each with its sentence (see `citing_sentence`). Comments, the
/// preamble, equations, tables and the bibliography are skipped; formatting commands are read as
/// their text, \ref as "X".
pub fn tex_claims(src: &str) -> Vec<TexClaim> {
    let clean = tex_without_comments(src);
    let chars: Vec<char> = clean.chars().collect();
    // the body only, when there is one
    let body_from = clean.find("\\begin{document}").map(|b| clean[..b].chars().count() + "\\begin{document}".len()).unwrap_or(0);
    let body_to = clean.find("\\end{document}").map(|b| clean[..b].chars().count()).unwrap_or(chars.len());
    let mut line_at = vec![1u32; chars.len() + 1];
    let mut ln = 1;
    for (i, c) in chars.iter().enumerate() {
        line_at[i] = ln;
        if *c == '\n' {
            ln += 1;
        }
    }
    line_at[chars.len()] = ln;

    let mut paras: Vec<TexPara> = vec![];
    let mut cur = TexPara { text: String::new(), cites: vec![] };
    let mut u16len = 0u32;
    let flush = |cur: &mut TexPara, paras: &mut Vec<TexPara>, u16len: &mut u32| {
        if !cur.text.trim().is_empty() {
            paras.push(std::mem::replace(cur, TexPara { text: String::new(), cites: vec![] }));
        } else {
            cur.text.clear();
            cur.cites.clear();
        }
        *u16len = 0;
    };
    let push = |cur: &mut TexPara, u16len: &mut u32, s: &str| {
        cur.text.push_str(s);
        *u16len += s.encode_utf16().count() as u32;
    };
    let mut skip_env: Option<String> = None;
    let mut i = body_from.min(chars.len());
    while i < body_to.min(chars.len()) {
        let c = chars[i];
        if let Some(env) = &skip_env {
            let end = format!("\\end{{{env}}}");
            let rest: String = chars[i..(i + end.chars().count()).min(chars.len())].iter().collect();
            if rest == end {
                skip_env = None;
                i += end.chars().count();
            } else {
                i += 1;
            }
            continue;
        }
        match c {
            '\\' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j == i + 1 {
                    // a symbol: \% \& \_ \# \$ \{ \}, \\ (a line break), \, \; \ (spaces)
                    let s = chars.get(j).copied().unwrap_or(' ');
                    match s {
                        '%' | '&' | '_' | '#' | '$' | '{' | '}' => push(&mut cur, &mut u16len, &s.to_string()),
                        _ => push(&mut cur, &mut u16len, " "),
                    }
                    i = j + 1;
                    continue;
                }
                let name: String = chars[i + 1..j].iter().collect();
                let mut k = j;
                if chars.get(k) == Some(&'*') {
                    k += 1;
                }
                if CITE_COMMANDS.contains(&name.as_str()) {
                    // \cite[pre][post]{a,b} (biblatex's \cites{a}{b} too)
                    let mut keys = vec![];
                    let mut e = skip_options(&chars, k);
                    loop {
                        let s = skip_spaces(&chars, e);
                        let Some((g, after)) = group(&chars, s) else { break };
                        keys.extend(g.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()));
                        e = skip_options(&chars, after);
                        if !name.ends_with("cites") {
                            break;
                        }
                    }
                    let command: String = chars[i..e].iter().collect();
                    let mark = "[cite]";
                    let start = u16len;
                    push(&mut cur, &mut u16len, mark);
                    if !keys.is_empty() {
                        cur.cites.push((start, u16len, keys, command, line_at[i]));
                    }
                    i = e;
                    continue;
                }
                if name == "begin" {
                    if let Some((env, after)) = group(&chars, skip_spaces(&chars, k)) {
                        if SKIP_ENVS.contains(&env.as_str()) {
                            skip_env = Some(env);
                        }
                        flush(&mut cur, &mut paras, &mut u16len);
                        i = skip_options(&chars, after);
                        continue;
                    }
                }
                if REF_COMMANDS.contains(&name.as_str()) {
                    let after = group(&chars, skip_spaces(&chars, k)).map(|g| g.1).unwrap_or(k);
                    push(&mut cur, &mut u16len, "X");
                    i = after;
                    continue;
                }
                if DROP_WITH_ARG.contains(&name.as_str()) {
                    let mut e = skip_options(&chars, k);
                    if let Some((_, after)) = group(&chars, skip_spaces(&chars, e)) {
                        e = after;
                    }
                    if name == "end" {
                        flush(&mut cur, &mut paras, &mut u16len);
                    }
                    i = e;
                    continue;
                }
                if name == "href" {
                    // \href{url}{text}: the text
                    let e = group(&chars, skip_spaces(&chars, k)).map(|g| g.1).unwrap_or(k);
                    i = e;
                    continue;
                }
                if BREAK_COMMANDS.contains(&name.as_str()) {
                    flush(&mut cur, &mut paras, &mut u16len);
                    let mut e = skip_options(&chars, k);
                    if matches!(name.as_str(), "section" | "subsection" | "subsubsection" | "chapter" | "paragraph" | "subparagraph" | "part" | "title" | "author" | "date") {
                        // a heading: not prose
                        if let Some((_, after)) = group(&chars, skip_spaces(&chars, e)) {
                            e = after;
                        }
                    }
                    i = e;
                    continue;
                }
                // \emph{…}, \textbf{…}, \footnote{…} and the rest: their text (braces are dropped below)
                i = skip_options(&chars, k);
                if name.len() > 1 && chars.get(i) != Some(&'{') {
                    push(&mut cur, &mut u16len, " ");
                }
            }
            '{' | '}' => i += 1,
            '~' => {
                push(&mut cur, &mut u16len, " ");
                i += 1;
            }
            '$' => {
                // inline math: its text, dollars dropped ($$…$$ as one)
                i += 1;
            }
            '\n' => {
                // a blank line ends a paragraph
                let mut j = i + 1;
                while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                    j += 1;
                }
                if chars.get(j) == Some(&'\n') {
                    flush(&mut cur, &mut paras, &mut u16len);
                    i = j + 1;
                } else {
                    push(&mut cur, &mut u16len, " ");
                    i += 1;
                }
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                let three = chars.get(i + 2) == Some(&'-');
                push(&mut cur, &mut u16len, if three { "—" } else { "–" });
                i += if three { 3 } else { 2 };
            }
            '`' if chars.get(i + 1) == Some(&'`') => {
                push(&mut cur, &mut u16len, "“");
                i += 2;
            }
            '\'' if chars.get(i + 1) == Some(&'\'') => {
                push(&mut cur, &mut u16len, "”");
                i += 2;
            }
            _ => {
                let mut b = [0u8; 4];
                push(&mut cur, &mut u16len, c.encode_utf8(&mut b));
                i += 1;
            }
        }
    }
    flush(&mut cur, &mut paras, &mut u16len);

    let mut out = vec![];
    let mut before = String::new();
    for p in &paras {
        for (n, (s, e, keys, command, line)) in p.cites.iter().enumerate() {
            let others: Vec<CiteSpan> = p.cites.iter().enumerate().filter(|(m, _)| *m != n).map(|(_, c)| CiteSpan { start: c.0, end: c.1 }).collect();
            let cs = citing_sentence(&p.text, *s, *e, &others, &before);
            out.push(TexClaim { keys: keys.clone(), command: command.clone(), line: *line, sentence: cs.sentence, previous: cs.previous, claim: cs.claim });
        }
        before = p.text.replace("[cite]", "");
    }
    out
}

/// The .bib files a .tex file names (\bibliography{refs,more}, \addbibresource{refs.bib}), as
/// paths relative to it, ".bib" added where left out.
pub fn tex_bib_files(src: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\\(?:bibliography|addbibresource|addglobalbib)\s*(?:\[[^\]]*\])?\s*\{([^}]*)\}").unwrap());
    let mut out: Vec<String> = vec![];
    for c in re.captures_iter(&tex_without_comments(src)) {
        for f in c[1].split(',').map(str::trim).filter(|f| !f.is_empty()) {
            let f = if f.to_lowercase().ends_with(".bib") { f.to_string() } else { format!("{f}.bib") };
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// The .bib files a .tex file names (see `tex_bib_files`).
#[uniffi::export]
pub fn find_tex_bib_files(text: String) -> Vec<String> {
    tex_bib_files(&text)
}

/// The citations of a .tex file with their sentences.
#[uniffi::export]
pub fn find_tex_claims(text: String) -> Vec<TexClaim> {
    tex_claims(&text)
}

// ---------------------------------------------------------------- the library

impl Library {
    /// The paper's text to search: its full text when on this device, else its abstract.
    fn claim_text(&self, p: &Paper) -> (ClaimTextSource, String) {
        let full = self.file_path(p.id.clone(), FileKind::Fulltext).and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
        if !passages(&full).is_empty() {
            return (ClaimTextSource::FullText, full);
        }
        let a = p.meta.abstract_text.trim();
        if a.is_empty() {
            return (ClaimTextSource::None, String::new());
        }
        // the abstract's sentences, one per line, under "Abstract"
        let chars: Vec<char> = a.chars().collect();
        let lines: Vec<String> = sentence_bounds(&chars).into_iter().map(|(s, e)| chars[s..e].iter().collect()).collect();
        (ClaimTextSource::Abstract, format!("Abstract\n{}", lines.join("\n")))
    }

    fn claim_paper(&self, claim: &str, context: &str, id: &str) -> ClaimPaper {
        let Some(p) = self.paper(id.to_string()) else {
            return ClaimPaper { paper_id: id.to_string(), title: String::new(), label: String::new(), source: ClaimTextSource::Missing, passages: vec![], prompt: None };
        };
        let label = crate::outline::author_year(&p.meta);
        let (source, text) = self.claim_text(&p);
        let all = passages(&text);
        // an abstract's sentences are short: passages of two
        let all = if source == ClaimTextSource::Abstract { pairs(&all) } else { all };
        let mut top = rank(&all, claim, TOP);
        for t in top.iter_mut() {
            t.link = passage_link(&p.id, &t.text);
        }
        let prompt = (!top.is_empty()).then(|| ModelPrompt { instructions: instructions(), prompt: prompt_for(claim, context, &p, &label, source, &top), max_tokens: 220, ids: vec![p.id.clone()] });
        ClaimPaper { paper_id: p.id.clone(), title: p.meta.title.clone(), label, source, passages: top, prompt }
    }

    /// The library's paper for each citation key (`""` when none): its own key (case aside), else
    /// the .bib's entry for the key (`bib`, the document's .bib text) found by DOI or title.
    pub fn papers_for_keys(&self, keys: &[String], bib: &str) -> Vec<String> {
        let by_key: HashMap<String, String> = {
            let g = self.inner.lock().unwrap();
            g.state.cite_keys.iter().map(|(id, k)| (k.to_lowercase(), id.clone())).collect()
        };
        let entries = if bib.trim().is_empty() { vec![] } else { crate::import::parse_bibtex_raw(bib) };
        keys.iter()
            .map(|k| {
                if let Some(id) = by_key.get(&k.to_lowercase()) {
                    return id.clone();
                }
                entries
                    .iter()
                    .find(|e| e.key.eq_ignore_ascii_case(k))
                    .and_then(|e| {
                        let title = e.get("title").replace(['{', '}'], "");
                        self.find_cited(e.get("doi"), title)
                    })
                    .map(|p| p.id)
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The check of a sentence against the papers it cites; `judge` the on-device model (None:
    /// the passages for the researcher to judge). Blocking (the model: seconds per paper).
    pub fn check_claim_with(&self, claim: &str, context: &str, ids: &[String], judge: Option<&dyn ClaimJudge>) -> ClaimReport {
        let mut results = vec![];
        let mut judged = false;
        let mut model_gone = judge.is_none();
        let mut seen = HashSet::new();
        for id in ids.iter().filter(|i| !i.trim().is_empty() && seen.insert(i.to_string())) {
            let p = self.claim_paper(claim, context, id);
            let answer = match (&p.prompt, judge) {
                (Some(pr), Some(j)) if !model_gone => {
                    let a = j.answer(pr.instructions.clone(), pr.prompt.clone(), pr.max_tokens);
                    if a.is_none() {
                        model_gone = true; // no model after all: don't wait for it again
                    }
                    a
                }
                _ => None,
            };
            let r = self::judge(&p, answer.as_deref());
            judged |= r.by_model;
            results.push(r);
        }
        let note = if judged { BY_MODEL } else { NO_MODEL };
        ClaimReport { claim: claim.to_string(), context: context.to_string(), judged, note: format!("{note} {PRIVACY}"), results }
    }

    /// The answer to the citation API's POST /api/claims (the Mac; /cite/claims on Windows and
    /// Linux):
    ///   {text, start, end, others?: [[start, end]], before?} (a paragraph and the citation's place
    ///   in it, UTF-16) or {claim, context?}; ids: [paper id]; papers?: {id: details} (a paper the
    ///   document cites under another library's id is found by DOI or title); judge? (default true)
    ///   → {claim, sentence, context, judged, note, privacy, results: [{id, libraryId?, title, label,
    ///      source, verdict, verdictLabel, byModel, reason, note, evidence: [{quote, section, link}],
    ///      passages: [{text, section, score, link}]}]}
    pub fn claims_answer(&self, body: &str, judge: Option<&dyn ClaimJudge>) -> (u16, Value) {
        let Ok(Value::Object(j)) = serde_json::from_str::<Value>(body) else {
            return (400, json!({"error": "Expected {text, start, end, ids} or {claim, ids}."}));
        };
        let s = |k: &str| j.get(k).and_then(Value::as_str).map(|v| v.chars().take(20_000).collect::<String>()).unwrap_or_default();
        let ids: Vec<String> = j.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).take(50).collect()).unwrap_or_default();
        if ids.is_empty() {
            return (400, json!({"error": "Expected ids."}));
        }
        let (claim, sentence, context) = if j.get("text").is_some() {
            let num = |k: &str| j.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
            let others: Vec<CiteSpan> = j
                .get("others")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|o| Some(CiteSpan { start: o.get(0)?.as_u64()? as u32, end: o.get(1)?.as_u64()? as u32 })).take(500).collect())
                .unwrap_or_default();
            let cs = citing_sentence(&s("text"), num("start"), num("end"), &others, &s("before"));
            (cs.claim, cs.sentence, cs.previous)
        } else {
            let c = s("claim");
            (strip_citations(&c), c, s("context"))
        };
        if terms(&claim).is_empty() {
            return (200, json!({"claim": claim, "sentence": sentence, "context": context, "judged": false, "privacy": PRIVACY,
                "note": "The sentence has no words to check (only the citation?).", "results": []}));
        }
        // a paper the document has under another library's id: the library's copy
        let given = j.get("papers").and_then(Value::as_object).cloned().unwrap_or_default();
        let mut swap: HashMap<String, String> = HashMap::new();
        let lib_ids: Vec<String> = ids
            .iter()
            .map(|id| {
                if self.paper(id.clone()).is_some() {
                    return id.clone();
                }
                let m = given.get(id);
                let f = |k: &str| m.and_then(|m| m.get(k)).and_then(Value::as_str).unwrap_or("").to_string();
                match self.find_cited(f("doi"), f("title")) {
                    Some(p) => {
                        swap.insert(p.id.clone(), id.clone());
                        p.id
                    }
                    None => id.clone(),
                }
            })
            .collect();
        let use_model = j.get("judge").and_then(Value::as_bool).unwrap_or(true);
        let report = self.check_claim_with(&claim, &context, &lib_ids, if use_model { judge } else { None });
        let results: Vec<Value> = report
            .results
            .iter()
            .map(|r| {
                let mut d = json!({
                    "id": swap.get(&r.paper_id).cloned().unwrap_or_else(|| r.paper_id.clone()),
                    "title": r.title, "label": r.label, "source": r.source.key(),
                    "verdict": r.verdict.key(), "verdictLabel": r.verdict.label(r.by_model), "byModel": r.by_model,
                    "reason": r.reason, "note": r.note,
                    "evidence": r.evidence.iter().map(|e| json!({"quote": e.quote, "section": e.section, "link": e.link})).collect::<Vec<_>>(),
                    "passages": r.passages.iter().map(|p| json!({"text": p.text, "section": p.section, "score": p.score, "link": p.link})).collect::<Vec<_>>(),
                });
                if swap.contains_key(&r.paper_id) {
                    d["libraryId"] = json!(r.paper_id);
                }
                d
            })
            .collect();
        (200, json!({"claim": report.claim, "sentence": sentence, "context": report.context, "judged": report.judged,
            "note": if report.judged { BY_MODEL } else { NO_MODEL }, "privacy": PRIVACY, "results": results}))
    }
}

/// An abstract's sentences joined two by two (overlapping), so a passage has some context.
fn pairs(all: &[ClaimPassage]) -> Vec<ClaimPassage> {
    if all.len() < 2 {
        return all.to_vec();
    }
    all.windows(2)
        .map(|w| ClaimPassage { text: format!("{} {}", w[0].text, w[1].text), section: w[0].section.clone(), score: 0.0, bm25: 0.0, start: w[0].start, link: String::new() })
        .collect()
}

#[uniffi::export]
impl Library {
    /// The best passages of each cited paper for a sentence, with the prompt for the model.
    pub fn claim_papers(&self, claim: String, context: String, ids: Vec<String>) -> Vec<ClaimPaper> {
        ids.iter().map(|id| self.claim_paper(&claim, &context, id)).collect()
    }

    /// The check of a sentence against the papers it cites, the model asked through `judge`
    /// (it answers None without a model). Blocking.
    pub fn check_claim(&self, claim: String, context: String, ids: Vec<String>, judge: Arc<dyn ClaimJudge>) -> ClaimReport {
        self.check_claim_with(&claim, &context, &ids, Some(judge.as_ref()))
    }

    /// The library's paper for each citation key of a .tex file ("" when none; see `papers_for_keys`).
    pub fn cite_key_papers(&self, keys: Vec<String>, bib: String) -> Vec<String> {
        self.papers_for_keys(&keys, &bib)
    }

    /// The citation API's /api/claims (see `claims_answer`).
    pub fn claims_request(&self, body: String, judge: Arc<dyn ClaimJudge>) -> ClaimsAnswer {
        let (status, v) = self.claims_answer(&body, Some(judge.as_ref()));
        ClaimsAnswer { status, body: v.to_string() }
    }
}

#[cfg(test)]
mod tests;
