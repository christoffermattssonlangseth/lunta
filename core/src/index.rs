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
        let mut st = self.db.prepare(
            "SELECT paper_id,
                    snippet(docs, -1, '[', ']', '…', 14),
                    bm25(docs, 0.0, 10.0, 6.0, 3.0, 2.0, 3.0, 1.0)
             FROM docs WHERE docs MATCH ?1 ORDER BY 3 LIMIT ?2",
        )?;
        let rows = st.query_map(params![q, limit], |r| Ok((r.get(0)?, r.get(1)?, -r.get::<_, f64>(2)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

/// Turns free text into an FTS5 query: every word must match, as a prefix; "quoted phrases" stay phrases.
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
            let word: String = rest[..end].chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
            let word = word.trim_matches('-').to_string();
            if !word.is_empty() {
                parts.push(format!("\"{}\"*", word));
            }
            rest = rest[end..].trim_start();
        }
    }
    parts.join(" AND ")
}
