//! Ask on the open model: questions about one paper, answered from its passages that best match
//! the question, and questions about the library, answered from the papers a search finds. The
//! Apple apps' Ask (apple/Shared/Assistant.swift) lets the model call tools to look things up;
//! a small model on a phone or a CPU does better, and much faster, when the app looks them up
//! first and hands it just those (one call, no tool round trips). Its rules are the same: answer
//! only from what it is given, cite by number, keep numbers exact, never invent papers.

use super::prompt::{digitize, ChatTurn};
use crate::model::Paper;
use std::collections::HashMap;

/// Something an answer cites as [n]: a passage of the paper, or a paper of the library.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AskSource {
    pub n: u32,
    /// The passage (paper questions) or the paper's title, author and year (library questions).
    pub text: String,
    /// The paper it is from, or the paper itself.
    pub paper_id: String,
}

/// What to give the model for one question.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AskPrompt {
    pub system: String,
    /// The conversation so far and, last, the question with what was looked up.
    pub turns: Vec<ChatTurn>,
    pub sources: Vec<AskSource>,
}

/// Answer tokens allowed.
pub const ANSWER_TOKENS: u32 = 700;

const STOP: [&str; 70] = [
    "the", "and", "for", "are", "was", "were", "with", "that", "this", "from", "have", "has", "what", "which", "who",
    "how", "why", "when", "does", "did", "they", "their", "there", "these", "those", "into", "about", "than", "then",
    "also", "can", "not", "but", "all", "any", "our", "its", "his", "her", "you", "your", "paper", "study", "authors",
    "use", "used", "using", "show", "shows", "find", "found", "between", "tell", "explain", "papers", "library",
    "many", "much", "some", "where", "would", "could", "should", "been", "being", "will", "more", "most", "other",
    "give",
];

/// The words of a query that carry meaning (Retriever.terms).
pub fn terms(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.chars().count() > 2 && !STOP.contains(w))
        .map(String::from)
        .collect()
}

/// The full text cut into passages: one paragraph per line; short lines (headings) join the
/// paragraph after them; long paragraphs are split at sentence ends (as lunta-mcp's read_paper).
pub fn passages_of(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = vec![];
    let mut pending = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.chars().count() < 80 {
            pending.push_str(line);
            pending.push_str(" — ");
            continue;
        }
        let mut cur = std::mem::take(&mut pending);
        for sentence in line.split_inclusive(". ") {
            if cur.len() + sentence.len() > 1200 && cur.len() > 300 {
                chunks.push(std::mem::take(&mut cur));
            }
            cur.push_str(sentence);
        }
        chunks.push(cur);
    }
    // short lines at the end (or a text of short lines only)
    let tail = pending.trim_end_matches(" — ").to_string();
    if !tail.is_empty() {
        chunks.push(tail);
    }
    chunks.into_iter().filter(|p| p.chars().count() > 40).collect()
}

/// Indices of the passages that best match the query, best first (BM25, as Retriever.rank).
pub fn rank(passages: &[String], query: &str, top: usize) -> Vec<usize> {
    let q: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        terms(query).into_iter().filter(|t| seen.insert(t.clone())).collect()
    };
    if q.is_empty() || passages.is_empty() {
        return vec![];
    }
    let docs: Vec<Vec<String>> = passages.iter().map(|p| terms(p)).collect();
    let n = docs.len() as f64;
    let avg = (docs.iter().map(Vec::len).sum::<usize>() as f64 / n).max(1.0);
    let mut df: HashMap<&str, f64> = HashMap::new();
    for t in &q {
        // a query word matches a passage word it begins (so "patient" finds "patients")
        df.insert(t, docs.iter().filter(|d| d.iter().any(|w| w.starts_with(t.as_str()))).count() as f64);
    }
    let mut scored: Vec<(usize, f64)> = docs
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let mut s = 0.0;
            for t in &q {
                let f = d.iter().filter(|w| w.starts_with(t.as_str())).count() as f64;
                if f == 0.0 {
                    continue;
                }
                let dfi = df[t.as_str()];
                let idf = (1.0 + (n - dfi + 0.5) / (dfi + 0.5)).ln();
                s += idf * f * 2.2 / (f + 1.2 * (0.25 + 0.75 * d.len() as f64 / avg));
            }
            (s > 0.0).then_some((i, s))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().take(top).map(|(i, _)| i).collect()
}

pub(crate) fn clip(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= n {
        return t.to_string();
    }
    let mut c: String = t.chars().take(n).collect();
    // end at a word
    if let Some(i) = c.rfind(' ').filter(|i| *i > n / 2) {
        c.truncate(i);
    }
    c + "…"
}

/// A passage cut to about 100 characters, for a list of sources.
pub fn clip_for_display(s: &str) -> String {
    clip(s, 100)
}

/// "Smith et al. 2020, Nature"
pub(crate) fn byline(p: &Paper) -> String {
    let first = p.meta.authors.first().map(|a| a.split(',').next().unwrap_or(a).trim().to_string()).unwrap_or_default();
    let mut s = first;
    if p.meta.authors.len() > 1 {
        s += " et al.";
    }
    let year = p.meta.year();
    if !year.is_empty() {
        s += &format!("{}{year}", if s.is_empty() { "" } else { " " });
    }
    if !p.meta.journal.is_empty() {
        s += &format!("{}{}", if s.is_empty() { "" } else { ", " }, p.meta.journal);
    }
    s
}

/// Earlier turns kept with a new question: the last few, shortened, so the passages fit.
fn history(prev: &[ChatTurn]) -> Vec<ChatTurn> {
    let keep = prev.len().saturating_sub(4);
    prev[keep..].iter().map(|t| ChatTurn { role: t.role.clone(), text: clip(&t.text, 700) }).collect()
}

/// A question about one paper. `text`: its full text (fulltext.txt, or the reading view's);
/// `highlights`: the user's (quote, note) pairs in it, so "what did I mark?" works.
pub fn paper_prompt(paper: &Paper, text: &str, highlights: &[(String, String)], question: &str, prev: &[ChatTurn]) -> AskPrompt {
    let all = passages_of(text);
    // a follow-up ("and the women?") is looked up with the question before it
    let last_question = prev.iter().rev().find(|t| t.role == "user").map(|t| t.text.as_str()).unwrap_or("");
    let mut picked = rank(&all, question, 5);
    if picked.len() < 5 && !last_question.is_empty() {
        for i in rank(&all, &format!("{question} {last_question}"), 5) {
            if picked.len() < 5 && !picked.contains(&i) {
                picked.push(i);
            }
        }
    }
    // nothing matched ("summarize this"): the start of the paper
    if picked.is_empty() {
        picked = (0..all.len().min(4)).collect();
    }
    let sources: Vec<AskSource> = picked
        .iter()
        .enumerate()
        .map(|(k, &i)| AskSource { n: k as u32 + 1, text: clip(&all[i], 700), paper_id: paper.id.clone() })
        .collect();
    let abstract_text = digitize(&clip(&paper.meta.abstract_text, 1200));
    let mut marks = String::new();
    for (quote, note) in highlights.iter().take(12) {
        let line = format!("- “{}”{}\n", clip(quote, 160), if note.trim().is_empty() { String::new() } else { format!(" — note: {}", clip(note, 120)) });
        if marks.len() + line.len() > 1200 {
            break;
        }
        marks += &line;
    }
    let system = format!(
        "You are the reading assistant in Lunta, helping a researcher understand one scientific paper: “{}” ({}).\n\
         Answer from the paper itself: the abstract and the numbered passages in the message, which the app found for \
         the question (they are only part of the paper). Cite what you use like [2] or [2][5]. Copy numbers exactly as \
         they appear in the passages and never state a number that isn't in them; quote the paper's words for key claims. \
         If the passages don't answer the question, say the paper doesn't seem to say, rather than guessing. \
         Be concise and precise; use short paragraphs or bullet points; keep numbers and units exact.\n\
         Abstract: {}{}",
        paper.meta.title.trim(),
        byline(paper),
        if abstract_text.is_empty() { "(none)".to_string() } else { abstract_text },
        if marks.is_empty() { String::new() } else { format!("\nThe user's highlights and notes in this paper:\n{marks}") },
    );
    let found = sources.iter().map(|s| format!("[{}] {}", s.n, digitize(&s.text))).collect::<Vec<_>>().join("\n\n");
    let mut turns = history(prev);
    turns.push(ChatTurn { role: "user".into(), text: format!("Passages from the paper:\n\n{found}\n\nQuestion: {}", question.trim()) });
    AskPrompt { system, turns, sources }
}

/// What the library is like, for questions about it.
pub struct LibraryFacts {
    pub all: usize,
    pub unread: usize,
    pub starred: usize,
    pub tags: Vec<String>,
    pub collections: Vec<String>,
}

/// One paper of the library as the model sees it.
fn paper_line(p: &Paper) -> String {
    let mut flags = vec![];
    if !p.read {
        flags.push("unread".to_string());
    }
    if p.starred {
        flags.push("starred".to_string());
    }
    if !p.tags.is_empty() {
        flags.push(format!("tags: {}", p.tags.join(", ")));
    }
    format!(
        "{} — {}{}",
        clip(&p.meta.title, 160),
        byline(p),
        if flags.is_empty() { String::new() } else { format!(" ({})", flags.join("; ")) }
    )
}

/// A question about the library, with the papers a search found (best first) and a snippet each.
pub fn library_prompt(facts: &LibraryFacts, found: &[(Paper, String)], question: &str, prev: &[ChatTurn]) -> AskPrompt {
    let sources: Vec<AskSource> = found
        .iter()
        .enumerate()
        .map(|(k, (p, _))| AskSource { n: k as u32 + 1, text: format!("{} — {}", p.meta.title.trim(), byline(p)), paper_id: p.id.clone() })
        .collect();
    let tags = facts.tags.iter().take(40).cloned().collect::<Vec<_>>().join(", ");
    let cols = facts.collections.iter().take(30).cloned().collect::<Vec<_>>().join(", ");
    let system = format!(
        "You are the research assistant in Lunta, the user's reference manager. The library has {} papers ({} unread, {} starred). \
         Tags: {}. Collections: {}.\n\
         The app searched the library for the question; the papers it found are numbered in the message. Use only those papers; \
         never invent papers, findings or citations. Cite them like [3], once per paper, right after its title. If none of them \
         fits, say the library doesn't seem to have such papers. You cannot change the library. \
         Be concise; use short bullet lists of papers with their numbers.",
        facts.all,
        facts.unread,
        facts.starred,
        if tags.is_empty() { "none" } else { &tags },
        if cols.is_empty() { "none" } else { &cols },
    );
    let list = if found.is_empty() {
        "(the search found no papers)".to_string()
    } else {
        found
            .iter()
            .enumerate()
            .map(|(k, (p, snippet))| {
                let abs = clip(&p.meta.abstract_text, 360);
                let mut s = format!("[{}] {}", k + 1, paper_line(p));
                if !abs.is_empty() {
                    s += &format!("\nAbstract: {}", digitize(&abs));
                } else if !snippet.is_empty() {
                    s += &format!("\nMatch: {}", snippet.replace(['[', ']'], ""));
                }
                s
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let mut turns = history(prev);
    turns.push(ChatTurn { role: "user".into(), text: format!("Papers found in the library:\n\n{list}\n\nQuestion: {}", question.trim()) });
    AskPrompt { system, turns, sources }
}

/// Library search for a question: all its words first; when that finds few, each word on its
/// own, papers ranked by how many words found them and how well (reciprocal rank fusion).
pub fn search_for(search: &dyn Fn(&str, u32) -> Vec<(Paper, String)>, question: &str, top: usize) -> Vec<(Paper, String)> {
    let words = terms(question);
    if words.is_empty() {
        return vec![];
    }
    let all = search(&words.join(" "), top as u32);
    if all.len() >= top || words.len() == 1 {
        return all;
    }
    let mut score: HashMap<String, (f64, Paper, String)> = HashMap::new();
    for (r, (p, s)) in all.into_iter().enumerate() {
        score.insert(p.id.clone(), (2.0 / (10.0 + r as f64), p, s));
    }
    for w in words.iter().take(8) {
        for (r, (p, s)) in search(w, 20).into_iter().enumerate() {
            let e = score.entry(p.id.clone()).or_insert((0.0, p, s));
            e.0 += 1.0 / (10.0 + r as f64);
        }
    }
    let mut v: Vec<(f64, Paper, String)> = score.into_values().collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    v.into_iter().take(top).map(|(_, p, s)| (p, s)).collect()
}

/// The answer without its thinking, and with source numbers the prompt didn't give taken out
/// (a small model sometimes cites [7] of five).
pub fn clean_answer(raw: &str, sources: usize) -> String {
    use regex::Regex;
    use std::sync::OnceLock;
    static CITE: OnceLock<Regex> = OnceLock::new();
    let cite = CITE.get_or_init(|| Regex::new(r"\[(\d{1,3})\]").unwrap());
    let s = super::prompt::without_thinking(raw);
    cite.replace_all(&s, |c: &regex::Captures| {
        let n: usize = c[1].parse().unwrap_or(0);
        if n >= 1 && n <= sources { c[0].to_string() } else { String::new() }
    })
    .trim()
    .to_string()
}
