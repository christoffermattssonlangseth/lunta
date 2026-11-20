//! Tags and folders to suggest.
//!
//! **Topics** (new tags or folders, up to ten, each with the papers it covers): with the
//! on-device model, it is given the papers' titles (numbered) and names topics by their numbers;
//! without it, terms shared by several papers (title words and phrases, keywords, the abstract's
//! start) are scored by how many papers they cover and how specific they are, and picked so
//! that no two cover mostly the same papers. Journals with many papers are candidates too.
//! Topics named like an existing tag or folder are left out.
//!
//! **Existing tags for untagged papers**: a tag whose papers share distinctive terms with the
//! paper (or whose name is in its title or keywords).

use super::text::{self, Terms};
use super::{GroupProposal, ModelPrompt, TagSuggestion};
use crate::model::Paper;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

/// At most this many topics are proposed.
pub const MAX_TOPICS: usize = 10;

pub(crate) struct Corpus<'a> {
    pub papers: &'a [Paper],
    pub terms: Vec<Terms>,
    /// term → papers (indices) with it
    pub postings: HashMap<String, Vec<usize>>,
}

impl<'a> Corpus<'a> {
    pub fn new(papers: &'a [Paper]) -> Self {
        let terms: Vec<Terms> = papers.iter().map(|p| text::terms(&p.meta)).collect();
        let mut postings: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, t) in terms.iter().enumerate() {
            for k in t.keys() {
                postings.entry(k.clone()).or_default().push(i);
            }
        }
        Corpus { papers, terms, postings }
    }

    fn idf(&self, k: &str) -> f64 {
        let df = self.postings.get(k).map(|v| v.len()).unwrap_or(0).max(1) as f64;
        (self.papers.len().max(1) as f64 / df).ln().max(0.0)
    }
}

fn existing_names(tags: &[String], folders: &[String]) -> HashSet<String> {
    tags.iter().chain(folders).map(|n| text::name_key(n)).filter(|k| !k.is_empty()).collect()
}

fn jaccard(a: &BTreeSet<usize>, b: &BTreeSet<usize>) -> f64 {
    let i = a.intersection(b).count() as f64;
    let u = a.union(b).count() as f64;
    if u == 0.0 { 0.0 } else { i / u }
}

/// Topics found without a model: shared terms and frequent journals.
pub(crate) fn topics(papers: &[Paper], tags: &[String], folders: &[String]) -> Vec<GroupProposal> {
    let n = papers.len();
    if n < 6 {
        return vec![];
    }
    let c = Corpus::new(papers);
    let taken = existing_names(tags, folders);
    let min_df = 3.max((n as f64 * 0.02).ceil() as usize);
    let max_df = (n as f64 * 0.5).floor() as usize;
    struct Cand {
        key: String,
        score: f64,
        papers: BTreeSet<usize>,
        why: String,
    }
    let mut cands: Vec<Cand> = vec![];
    for (k, list) in &c.postings {
        let df = list.len();
        if df < min_df || df > max_df.max(min_df) || taken.contains(k) {
            continue;
        }
        // most of its papers have it in the title or keywords, not only in the abstract
        let strong = list.iter().filter(|&&i| c.terms[i].strong(k)).count();
        if strong * 3 < df * 2 {
            continue;
        }
        let words = k.split(' ').count();
        // a phrase is a better label than its words; specific terms over common ones
        let specificity = (n as f64 / df as f64).ln().max(0.3);
        let score = (df as f64).sqrt() * specificity * match words {
            1 => 1.0,
            2 => 1.6,
            _ => 1.8,
        };
        cands.push(Cand { key: k.clone(), score, papers: list.iter().copied().collect(), why: "shared words".into() });
    }
    // a word or phrase whose papers mostly have a longer phrase with it ("transcriptomics" /
    // "spatial transcriptomics", "protein structure" / "protein structure prediction") gives way to it
    let longer: Vec<(String, usize)> = cands.iter().filter(|c| c.key.contains(' ')).map(|c| (format!(" {} ", c.key), c.papers.len())).collect();
    cands.retain(|cd| {
        let me = format!(" {} ", cd.key);
        !longer.iter().any(|(pk, n)| pk.len() > me.len() && pk.contains(&me) && *n as f64 >= 0.6 * cd.papers.len() as f64)
    });
    // journals
    let mut journals: HashMap<String, (String, BTreeSet<usize>)> = HashMap::new();
    for (i, p) in papers.iter().enumerate() {
        let j = p.meta.journal.trim();
        if j.is_empty() || crate::checks::is_preprint(&p.meta) {
            continue;
        }
        let e = journals.entry(text::name_key(j)).or_insert_with(|| (j.to_string(), BTreeSet::new()));
        e.1.insert(i);
    }
    let mut journal_cands: Vec<Cand> = journals
        .into_iter()
        .filter(|(k, (_, ps))| ps.len() >= 4.max(min_df) && ps.len() <= max_df.max(4) && !taken.contains(k))
        .map(|(k, (name, ps))| Cand { key: format!("journal:{k}"), score: (ps.len() as f64).sqrt() * 0.8, papers: ps, why: name })
        .collect();
    journal_cands.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.key.cmp(&b.key)));
    cands.extend(journal_cands.into_iter().take(2));
    cands.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.key.cmp(&b.key)));

    let mut chosen: Vec<&Cand> = vec![];
    for cd in &cands {
        if chosen.len() >= MAX_TOPICS {
            break;
        }
        // not mostly the same papers as a topic already chosen, and not a word of one
        let overlaps = chosen.iter().any(|x| {
            jaccard(&x.papers, &cd.papers) > 0.5
                || (!x.key.starts_with("journal:") && !cd.key.starts_with("journal:") && (x.key.split(' ').any(|w| cd.key.split(' ').any(|v| v == w))) && jaccard(&x.papers, &cd.papers) > 0.25)
        });
        if !overlaps {
            chosen.push(cd);
        }
    }
    chosen
        .into_iter()
        .map(|cd| {
            let ids: Vec<String> = cd.papers.iter().map(|&i| papers[i].id.clone()).collect();
            if cd.key.starts_with("journal:") {
                return GroupProposal { name: cd.why.clone(), paper_ids: ids, why: "papers in this journal".into(), by_model: false };
            }
            let mut surfaces: HashMap<String, usize> = HashMap::new();
            for &i in &cd.papers {
                if let Some((s, strong)) = c.terms[i].map.get(&cd.key) {
                    *surfaces.entry(s.clone()).or_default() += if *strong { 2 } else { 1 };
                }
            }
            GroupProposal { name: text::label(&surfaces), paper_ids: ids, why: format!("“{}” in their titles, keywords or abstracts", cd.key), by_model: false }
        })
        .collect()
}

// ---------------------------------------------------------------- with the model

pub const TOPIC_INSTRUCTIONS: &str = "You organize a researcher's library of scientific papers into topics. \
You get the papers as a numbered list of titles (with keywords). Propose at most 10 topics, each covering at least 3 of the papers, \
named in 1 to 4 words the way a researcher would name a folder (for example \"Spatial transcriptomics\" or \"Multiple sclerosis\"). \
Name the topics after the research, never after the papers' kind (no \"Reviews\", \"Methods\", \"Other\", \"Miscellaneous\"). \
Answer only with one line per topic, in the form: Topic name: 3, 8, 15. Use only the numbers given. No other text.";

/// The model's prompt for topics: the papers' titles (and keywords), at most `max_papers` of
/// them (the latest added first; Apple's on-device model has a small context, ~60 fit).
pub(crate) fn topic_prompt(papers: &[Paper], tags: &[String], folders: &[String], max_papers: usize) -> ModelPrompt {
    let mut ps: Vec<&Paper> = papers.iter().filter(|p| !p.meta.title.trim().is_empty()).collect();
    ps.sort_by(|a, b| b.added.cmp(&a.added));
    ps.truncate(max_papers.max(1));
    let mut lines = String::new();
    for (i, p) in ps.iter().enumerate() {
        let title: String = crate::metadata::strip_tags(&p.meta.title).chars().take(140).collect();
        let kw: Vec<&str> = p.meta.keywords.iter().take(4).map(|s| s.as_str()).collect();
        let kw = if kw.is_empty() { String::new() } else { format!(" [{}]", kw.join("; ")) };
        lines += &format!("{}. {}{}\n", i + 1, title.trim(), kw);
    }
    let have: Vec<&String> = tags.iter().chain(folders).collect();
    let have = if have.is_empty() {
        String::new()
    } else {
        format!("\nThe library already has these tags and folders (don't propose them again): {}.\n", have.iter().take(40).map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
    };
    ModelPrompt {
        instructions: TOPIC_INSTRUCTIONS.into(),
        prompt: format!("Papers:\n{lines}{have}\nPropose the topics, one per line: Topic name: numbers."),
        max_tokens: 400,
        ids: ps.iter().map(|p| p.id.clone()).collect(),
    }
}

fn topic_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(?:[-*•]\s*|\d+[.)]\s+)?\**\s*([^:\n]{2,60}?)\s*\**\s*[:–—-]\s*\**\s*((?:#?\d+\s*(?:,|and|&|\s)\s*)*#?\d+)\s*\.?\s*$").unwrap())
}

/// A topic's name as the model wrote it, tidied: no quotes, markdown or "Topic 1:".
fn clean_name(s: &str) -> String {
    let s = s.trim().trim_matches(|c: char| matches!(c, '"' | '“' | '”' | '\'' | '*' | '_' | '`' | '#')).trim();
    let s = Regex::new(r"(?i)^(topic|folder|tag)\s*\d*\s*[:.\-–]?\s*").unwrap().replace(s, "");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The topics in the model's answer: each line's name and paper numbers (numbers not in the
/// prompt are dropped; a topic needs two papers; names already used are left out). The model
/// only names and groups: every paper id comes from the prompt.
pub(crate) fn parse_topics(prompt: &ModelPrompt, answer: &str, tags: &[String], folders: &[String]) -> Vec<GroupProposal> {
    let answer = crate::tidy::without_thinking(answer);
    let taken = existing_names(tags, folders);
    const VAGUE: [&str; 8] = ["other", "miscellaneous", "misc", "general", "various", "uncategorized", "review", "method"];
    let mut out: Vec<GroupProposal> = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    for line in answer.lines() {
        let Some(c) = topic_line().captures(line) else { continue };
        let name = clean_name(&c[1]);
        let key = text::name_key(&name);
        if key.is_empty() || name.chars().count() > 48 || taken.contains(&key) || VAGUE.contains(&key.as_str()) || !seen.insert(key) {
            continue;
        }
        let mut ids: Vec<String> = vec![];
        for n in Regex::new(r"\d+").unwrap().find_iter(&c[2]) {
            if let Some(id) = n.as_str().parse::<usize>().ok().filter(|&n| n >= 1).and_then(|n| prompt.ids.get(n - 1)) {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        if ids.len() >= 2 {
            out.push(GroupProposal { name, paper_ids: ids, why: "proposed by the on-device model".into(), by_model: true });
        }
        if out.len() >= MAX_TOPICS {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------- existing tags

/// Existing tags for untagged papers: at most two per paper, the likeliest first.
pub(crate) fn tag_suggestions(papers: &[Paper], dismissed: &BTreeSet<String>) -> Vec<TagSuggestion> {
    let c = Corpus::new(papers);
    // tag → its papers
    let mut by_tag: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, p) in papers.iter().enumerate() {
        for t in &p.tags {
            by_tag.entry(t.clone()).or_default().push(i);
        }
    }
    let mut out = vec![];
    for (i, p) in papers.iter().enumerate() {
        if !p.tags.is_empty() {
            continue;
        }
        let mut scored: Vec<(f64, String, String)> = vec![];
        for (tag, members) in &by_tag {
            if dismissed.contains(&format!("tag:{}:{tag}", p.id)) {
                continue;
            }
            let name = text::name_key(tag);
            // the tag's name is in the title or keywords
            if !name.is_empty() && c.terms[i].strong(&name) && name.chars().count() >= 3 {
                scored.push((10.0 + members.len() as f64 * 0.01, tag.clone(), format!("“{tag}” is in its title or keywords")));
                continue;
            }
            if members.len() < 2 {
                continue;
            }
            let mut score = 0.0;
            let mut shared: Vec<(f64, String)> = vec![];
            for k in c.terms[i].keys() {
                let have = members.iter().filter(|&&m| c.terms[m].has(k)).count();
                let share = have as f64 / members.len() as f64;
                let df = c.postings.get(k).map(|v| v.len()).unwrap_or(0);
                // shared by a good part of the tag's papers, and more common among them than in the library
                if have >= 2 && share >= 0.3 && share > 2.0 * df as f64 / papers.len() as f64 {
                    let w = c.idf(k) * share;
                    score += w;
                    shared.push((w, c.terms[i].map[k].0.clone()));
                }
            }
            if shared.len() >= 2 && score >= 2.5 {
                shared.sort_by(|a, b| b.0.total_cmp(&a.0));
                let words: Vec<String> = shared.iter().take(3).map(|s| format!("“{}”", s.1.to_lowercase())).collect();
                scored.push((score, tag.clone(), format!("shares {} with {} papers tagged {tag}", words.join(", "), members.len())));
            }
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (score, tag, because) in scored.into_iter().take(2) {
            out.push(TagSuggestion { paper_id: p.id.clone(), tag, because, score });
        }
    }
    out
}
