//! The words and phrases (two or three words) of a paper's details that say what it is about, for grouping papers
//! into topics and suggesting tags: the title's words and two-word phrases, the keywords, and
//! (weaker) the abstract's. Folded to ASCII and lower case, plurals made singular ("organoids" and
//! "organoid" are one term), with the form most often written kept for labels ("RNA-seq").

use crate::model::PaperMeta;
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// Words that never make a topic.
const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "of", "in", "on", "at", "to", "for", "from", "by", "with", "without", "into", "onto", "over",
    "under", "via", "as", "is", "are", "was", "were", "be", "been", "being", "its", "their", "this", "that", "these", "those",
    "it", "we", "our", "us", "not", "no", "but", "than", "then", "can", "could", "may", "might", "will", "would", "should",
    "do", "does", "did", "has", "have", "had", "which", "who", "whom", "what", "when", "where", "why", "how", "between",
    "among", "after", "before", "during", "through", "across", "within", "upon", "about", "against", "toward", "towards",
    "both", "each", "all", "any", "some", "more", "most", "less", "least", "other", "such", "only", "also", "here", "there",
    "while", "whereas", "if", "so", "up", "down", "out", "off", "one", "two", "three", "four", "five", "first", "second",
    "new", "use", "used", "using", "uses", "based", "versus", "vs", "et", "al", "i", "ii", "iii", "iv", "de", "la", "le",
    "der", "die", "und", "en", "el", "los", "las", "del", "di", "da", "per", "un", "une", "des", "du",
];

/// Words too general to be a topic alone (they may still be part of a phrase: "gene expression").
const GENERIC: &[&str] = &[
    "study", "studies", "analysis", "analyses", "method", "methods", "approach", "approaches", "result", "results", "data",
    "model", "models", "modeling", "modelling", "effect", "effects", "role", "roles", "review", "reviews", "evidence",
    "novel", "large", "small", "high", "low", "human", "humans", "case", "cases", "report", "reports", "development",
    "application", "applications", "system", "systems", "framework", "toward", "insight", "insights", "impact", "level",
    "levels", "function", "functions", "functional", "mechanism", "mechanisms", "factor", "factors", "process", "processes",
    "research", "patient", "patients", "associated", "association", "associations", "related", "different", "multiple",
    "specific", "potential", "single", "general", "improved", "improving", "improve", "towards", "paper",
    "papers", "work", "time", "type", "types", "state", "states", "change", "changes", "response", "responses", "identification",
    "identifies", "identify", "reveals", "reveal", "revealed", "shows", "show", "role", "regulation", "regulates", "regulate",
    "control", "controls", "early", "late", "long", "short", "term", "terms", "following", "against", "versus",
    "comparison", "characterization", "measurement", "measurements", "assessment", "evaluation",
    "toward", "perspective", "perspectives", "overview", "update", "challenges", "opportunities", "future", "recent",
    "advances", "current", "need", "needs", "make", "makes", "making", "can", "toolkit", "tool", "tools", "resource",
    "atlas", "map", "maps", "mapping", "profile", "profiles", "profiling", "dynamic", "dynamics", "activity", "formation",
    "outcome", "outcomes",
];

pub(crate) fn is_stop(w: &str) -> bool {
    STOP.contains(&w)
}

pub(crate) fn is_generic(w: &str) -> bool {
    GENERIC.contains(&w)
}

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[\p{L}\p{N}]+(?:['’-][\p{L}\p{N}]+)*").unwrap())
}

fn segment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[.,;:!?()\[\]{}“”\x22/|]+|\s[-–—]\s").unwrap())
}

/// A word as compared: folded, lower case, singular ("Organoids" → "organoid"; "genomics",
/// "analysis", "virus" and "class" stay as they are).
pub(crate) fn word_key(w: &str) -> String {
    let f = crate::export::ascii_fold(w).to_lowercase().replace('’', "'");
    singular(&f)
}

fn singular(w: &str) -> String {
    let n = w.chars().count();
    if n > 4 && w.ends_with("ies") && !w.ends_with("eies") {
        return format!("{}y", &w[..w.len() - 3]);
    }
    if n > 4 && w.ends_with('s') && !(w.ends_with("ss") || w.ends_with("is") || w.ends_with("us") || w.ends_with("ics") || w.ends_with("'s")) {
        return w[..w.len() - 1].to_string();
    }
    w.to_string()
}

/// A phrase's key: its words' keys joined by spaces.
pub(crate) fn phrase_key(p: &str) -> String {
    token_re().find_iter(p).map(|m| word_key(m.as_str())).collect::<Vec<_>>().join(" ")
}

/// A word that can be (part of) a topic: not a stop word, not a number, at least three letters
/// (or an acronym/symbol with a digit: "p53", "CD8").
fn usable(key: &str) -> bool {
    let letters = key.chars().filter(|c| c.is_alphabetic()).count();
    !is_stop(key) && letters >= 2 && (key.chars().count() >= 3 || key.chars().any(|c| c.is_ascii_digit()))
}

/// One paper's terms: key → (surface form as written, strong: in the title or the keywords).
#[derive(Clone, Debug, Default)]
pub(crate) struct Terms {
    pub map: BTreeMap<String, (String, bool)>,
}

impl Terms {
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }
    pub fn has(&self, k: &str) -> bool {
        self.map.contains_key(k)
    }
    pub fn strong(&self, k: &str) -> bool {
        self.map.get(k).is_some_and(|x| x.1)
    }
    fn add(&mut self, key: String, surface: &str, strong: bool) {
        let e = self.map.entry(key).or_insert_with(|| (surface.to_string(), strong));
        if strong && !e.1 {
            *e = (surface.to_string(), true);
        }
    }
}

/// Words and two-word phrases of a text (not across punctuation).
fn add_text(t: &mut Terms, text: &str, strong: bool) {
    let text = crate::metadata::strip_tags(text);
    for seg in segment_re().split(&text) {
        let toks: Vec<(String, &str)> = token_re().find_iter(seg).map(|m| (word_key(m.as_str()), m.as_str())).collect();
        for (i, (k, s)) in toks.iter().enumerate() {
            if usable(k) && !is_generic(k) {
                t.add(k.clone(), s, strong);
            }
            if let Some((k2, s2)) = toks.get(i + 1) {
                // a phrase: neither word a stop word, not both generic ("case study")
                if usable(k) && usable(k2) && !(is_generic(k) && is_generic(k2)) && k != k2 {
                    t.add(format!("{k} {k2}"), &format!("{s} {s2}"), strong);
                    // and three words ("protein structure prediction")
                    if let Some((k3, s3)) = toks.get(i + 2) {
                        if usable(k3) && !is_generic(k3) && k3 != k2 {
                            t.add(format!("{k} {k2} {k3}"), &format!("{s} {s2} {s3}"), strong);
                        }
                    }
                }
            }
        }
    }
}

/// The terms of a paper's title, keywords and abstract.
pub(crate) fn terms(m: &PaperMeta) -> Terms {
    let mut t = Terms::default();
    add_text(&mut t, &m.title, true);
    for k in &m.keywords {
        let key = phrase_key(k);
        let n = key.split(' ').count();
        if !key.is_empty() && n <= 4 && key.split(' ').any(|w| usable(w) && !is_generic(w)) && !(n == 1 && !usable(&key)) {
            t.add(key, k.trim(), true);
        }
    }
    // the abstract's first 250 words (the start says what the paper is about)
    let abs: String = m.abstract_text.split_whitespace().take(250).collect::<Vec<_>>().join(" ");
    add_text(&mut t, &abs, false);
    t
}

/// The label for a term: the form written most often, its first letter upper case unless the
/// word is written in lower case on purpose ("mRNA", "scRNA-seq" stay).
pub(crate) fn label(surfaces: &HashMap<String, usize>) -> String {
    let best = surfaces.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|x| x.0.clone()).unwrap_or_default();
    // "SPATIAL TRANSCRIPTOMICS" from a shouting title: lower case it
    let best = if best.chars().filter(|c| c.is_alphabetic()).count() > 6 && !best.chars().any(|c| c.is_lowercase()) { best.to_lowercase() } else { best };
    // a title's capitalized words ("Spatial Transcriptomics") as a sentence: "Spatial transcriptomics"
    let words: Vec<String> = best
        .split(' ')
        .enumerate()
        .map(|(i, w)| {
            let mut cs = w.chars();
            let first = cs.next();
            let rest: String = cs.collect();
            let plain_capital = first.is_some_and(|c| c.is_uppercase()) && !rest.is_empty() && rest.chars().all(|c| c.is_lowercase() || !c.is_alphabetic());
            if i > 0 && plain_capital {
                w.to_lowercase()
            } else {
                w.to_string()
            }
        })
        .collect();
    let s = words.join(" ");
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if c.is_lowercase() && !cs.clone().any(|x| x.is_uppercase()) => c.to_uppercase().collect::<String>() + cs.as_str(),
        _ => s,
    }
}

/// A tag's or folder's name as compared with a term ("Spatial Transcriptomics" = "spatial
/// transcriptomic").
pub(crate) fn name_key(name: &str) -> String {
    phrase_key(name)
}
