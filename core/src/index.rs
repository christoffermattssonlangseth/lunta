//! Local full-text index (SQLite FTS5). Lives in the device's cache folder, never in iCloud;
//! it is derived from the library and can be deleted at any time.

use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Index {
    db: Connection,
}

/// What a paper's index row was built from; when it changes the row is rebuilt.
pub fn signature(meta: &PaperMeta, notes: &str, fulltext_len: u64) -> String {
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a
    for b in serde_json::to_string(meta).unwrap_or_default().bytes().chain(notes.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}-{fulltext_len}")
}

impl Index {
    pub fn open(path: &Path) -> Result<Index> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(
                 paper_id UNINDEXED, title, authors, abstract, journal, notes, fulltext,
                 tokenize = 'unicode61 remove_diacritics 2');
             CREATE TABLE IF NOT EXISTS indexed(paper_id TEXT PRIMARY KEY, sig TEXT NOT NULL);",
        )?;
        Ok(Index { db })
    }

    pub fn signature_of(&self, id: &str) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT sig FROM indexed WHERE paper_id = ?1", [id], |r| r.get(0)).optional()?)
    }

    pub fn upsert(&mut self, id: &str, meta: &PaperMeta, notes: &str, fulltext: &str, sig: &str) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM docs WHERE paper_id = ?1", [id])?;
        tx.execute(
            "INSERT INTO docs(paper_id, title, authors, abstract, journal, notes, fulltext) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, meta.title, meta.authors.join("; "), meta.abstract_text, meta.journal, notes, fulltext],
        )?;
        tx.execute("INSERT OR REPLACE INTO indexed(paper_id, sig) VALUES (?1, ?2)", params![id, sig])?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_missing(&mut self, keep: &std::collections::BTreeSet<String>) -> Result<()> {
        let ids: Vec<String> = {
            let mut st = self.db.prepare("SELECT paper_id FROM indexed")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        for id in ids.into_iter().filter(|i| !keep.contains(i)) {
            self.db.execute("DELETE FROM docs WHERE paper_id = ?1", [&id])?;
            self.db.execute("DELETE FROM indexed WHERE paper_id = ?1", [&id])?;
        }
        Ok(())
    }

    /// Returns (paper id, snippet, score); lower bm25 is better, so the score is negated.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<(String, String, f64)>> {
        let q = fts_query(query);
        if q.is_empty() {
            return Ok(vec![]);
        }
        // Ranking first, without snippets: making a snippet reads through the paper's full text,
        // which for a few hundred hits took most of a second per keystroke (seconds for a
        // one-letter search). Snippets are then made for the top hits only; the rest get none.
        let mut st = self.db.prepare(
            "SELECT rowid, paper_id, bm25(docs, 0.0, 10.0, 6.0, 3.0, 2.0, 3.0, 1.0)
             FROM docs WHERE docs MATCH ?1 ORDER BY 3 LIMIT ?2",
        )?;
        let ranked: Vec<(i64, String, f64)> = st
            .query_map(params![q, limit], |r| Ok((r.get(0)?, r.get(1)?, -r.get::<_, f64>(2)?)))?
            .filter_map(|r| r.ok())
            .collect();
        let top: Vec<String> = ranked.iter().take(SNIPPETS).map(|(rowid, _, _)| rowid.to_string()).collect();
        let mut snippets = std::collections::HashMap::new();
        if !top.is_empty() {
            let mut st = self.db.prepare(&format!(
                "SELECT rowid, snippet(docs, -1, '[', ']', '…', 14) FROM docs WHERE docs MATCH ?1 AND rowid IN ({})",
                top.join(",")
            ))?;
            for row in st.query_map(params![q], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?.flatten() {
                snippets.insert(row.0, row.1);
            }
        }
        Ok(ranked
            .into_iter()
            .map(|(rowid, id, score)| (id, snippets.remove(&rowid).unwrap_or_default(), score))
            .collect())
    }
}

/// Snippets are made for this many of the best hits (the rest of a long result list gets none).
const SNIPPETS: usize = 40;

/// Turns free text into an FTS5 query: every word must match, as a prefix; "quoted phrases" stay phrases.
/// A word with punctuation inside (a DOI, "single-cell") is a phrase of its parts.
/// A single letter matches only itself (as a prefix it matched nearly every word, and was slow).
pub fn fts_query(input: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = input.trim();
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('"') {
            let end = stripped.find('"').unwrap_or(stripped.len());
            let phrase: String = stripped[..end].chars().filter(|c| *c != '"').collect();
            if !phrase.trim().is_empty() {
                parts.push(format!("\"{}\"", phrase.trim()));
            }
            rest = stripped.get(end + 1..).unwrap_or("").trim_start();
        } else {
            let end = rest.find(|c: char| c.is_whitespace() || c == '"').unwrap_or(rest.len());
            // Punctuation inside a word stays: in a quoted string FTS5 splits the word into its
            // parts as a phrase, as it did the text ("10.1038/nmeth.2563" → 10 1038 nmeth 2563,
            // "Hebb's" → hebb s). Taken out, the parts ran together into a word found nowhere.
            let word = rest[..end].trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            if word.chars().filter(|c| c.is_alphanumeric()).count() == 1 {
                parts.push(format!("\"{}\"", word));
            } else if !word.is_empty() {
                parts.push(format!("\"{}\"*", word));
            }
            rest = rest[end..].trim_start();
        }
    }
    parts.join(" AND ")
}

/// A paper's identifier typed into the search field: a DOI (lower case), PubMed ID, PMCID
/// ("PMC…") or arXiv ID (without its version).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryId {
    Doi(String),
    Pmid(String),
    Pmcid(String),
    Arxiv(String),
}

impl QueryId {
    /// The identifier as text, for searching full texts that mention it.
    pub fn text(&self) -> &str {
        match self {
            QueryId::Doi(s) | QueryId::Pmid(s) | QueryId::Pmcid(s) | QueryId::Arxiv(s) => s,
        }
    }

    /// Whether the paper has this identifier.
    pub fn matches(&self, m: &PaperMeta) -> bool {
        match self {
            QueryId::Doi(d) => !m.doi.is_empty() && crate::metadata::normalize_doi(&m.doi) == *d,
            QueryId::Pmid(n) => m.pmid.trim().trim_start_matches(|c: char| !c.is_ascii_digit()) == n,
            QueryId::Pmcid(p) => {
                let own = m.pmcid.trim().to_uppercase();
                !own.is_empty() && (own == *p || format!("PMC{own}") == *p)
            }
            QueryId::Arxiv(a) => {
                let own = |s: &str| crate::metadata::arxiv_in(&format!("arxiv:{}", s.trim()));
                (!m.arxiv.trim().is_empty() && own(&m.arxiv).as_deref() == Some(a.as_str()))
                    || crate::metadata::normalize_doi(&m.doi) == format!("10.48550/arxiv.{a}")
            }
        }
    }
}

/// The identifiers in a search query — DOIs (bare "10.…/…", "doi:10.…", "https://doi.org/10.…",
/// case ignored, trailing punctuation dropped), PubMed IDs ("12345678", "pmid:…", a PubMed link),
/// PMCIDs ("PMC…") and arXiv IDs ("2101.01234", "arXiv:…", an arXiv link) — each with the word it
/// was, and the query's other words (labels like "doi:" before an identifier left out).
pub fn query_ids(input: &str) -> (Vec<(QueryId, String)>, Vec<String>) {
    use regex::Regex;
    use std::sync::OnceLock;
    static PMC: OnceLock<Regex> = OnceLock::new();
    static PMID: OnceLock<Regex> = OnceLock::new();
    let pmc = PMC.get_or_init(|| Regex::new(r"(?i)^(?:pmcid:|pmc:|https?://\S*/)?(PMC\d{4,})/?$").unwrap());
    let pmid = PMID.get_or_init(|| Regex::new(r"(?i)^(?:pmid:|https?://pubmed\.ncbi\.nlm\.nih\.gov/)?(\d{5,9})/?$").unwrap());
    let mut ids = Vec::new();
    let mut rest = Vec::new();
    let mut labels = Vec::new();
    let mut quoted = false;
    for word in input.split_whitespace() {
        // words inside a "quoted phrase" stay in the phrase
        let in_quote = quoted || word.starts_with('"');
        quoted ^= word.matches('"').count() % 2 == 1;
        if in_quote {
            rest.push(word.to_string());
            continue;
        }
        let core = word.trim_matches(|c: char| "()[]{}<>,;'".contains(c)).trim_end_matches(['.', ':']);
        let lower = core.to_lowercase();
        if matches!(lower.as_str(), "doi" | "pmid" | "pmcid" | "arxiv") && word.ends_with(':') {
            labels.push(word.to_string());
            continue;
        }
        let before = ids.len();
        if lower.contains("10.") {
            if let Some(d) = crate::metadata::doi_in(core) {
                ids.push((QueryId::Doi(d.to_lowercase()), word.to_string()));
            }
        }
        if let Some(a) = crate::metadata::arxiv_in(core) {
            ids.push((QueryId::Arxiv(a), word.to_string()));
        } else if let Some(c) = pmc.captures(core) {
            ids.push((QueryId::Pmcid(c[1].to_uppercase()), word.to_string()));
        } else if let Some(c) = pmid.captures(core) {
            ids.push((QueryId::Pmid(c[1].to_string()), word.to_string()));
        }
        if ids.len() == before {
            rest.push(word.to_string());
        }
    }
    if ids.is_empty() {
        rest.extend(labels);
    }
    (ids, rest)
}
