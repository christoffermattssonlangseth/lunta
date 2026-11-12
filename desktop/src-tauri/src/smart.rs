//! Smart folders, read from `<library>/smart-folders.json` (written by the Mac and iPhone apps;
//! apple/Shared/SmartFolders.swift) and evaluated the same way. The desktop app shows and opens
//! them; creating and editing them is left to the Apple apps for now.

use lunta_core::model::{Paper, PaperFilter, SortKey};
use lunta_core::Library;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SmartFolder {
    pub id: String,
    pub name: String,
    pub text: String,
    pub tag: String,
    pub folder: String,
    pub unread_only: bool,
    pub starred_only: bool,
    pub year_from: String,
    pub year_to: String,
    pub added_days: i64,
    /// "any", "has" or "missing"
    pub pdf: String,
    pub highlights_only: bool,
}

pub fn load(root: &Path) -> Vec<SmartFolder> {
    std::fs::read_to_string(root.join("smart-folders.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn year(p: &Paper) -> Option<i64> {
    p.meta.date.chars().take(4).collect::<String>().parse().ok()
}

/// The papers that match, in the given order (SmartFolder.papers in Swift).
pub fn papers(f: &SmartFolder, lib: &Library, sort: SortKey) -> Vec<Paper> {
    let filter = PaperFilter {
        collection: (!f.folder.is_empty()).then(|| f.folder.clone()),
        tag: (!f.tag.is_empty()).then(|| f.tag.clone()),
        starred_only: f.starred_only,
        unread_only: f.unread_only,
    };
    let mut list = lib.papers(filter, sort);
    let words = f.text.trim();
    if !words.is_empty() {
        let hits: HashSet<String> = lib.search(words.to_string(), 2000).unwrap_or_default().into_iter().map(|h| h.paper.id).collect();
        list.retain(|p| hits.contains(&p.id));
    }
    if let Ok(from) = f.year_from.trim().parse::<i64>() {
        list.retain(|p| year(p).unwrap_or(0) >= from);
    }
    if let Ok(to) = f.year_to.trim().parse::<i64>() {
        list.retain(|p| year(p).unwrap_or(9999) <= to);
    }
    if f.added_days > 0 {
        let since = lunta_core::model::now_ms() - f.added_days * 86_400_000;
        list.retain(|p| p.added >= since);
    }
    match f.pdf.as_str() {
        "has" => list.retain(|p| p.has_pdf),
        "missing" => list.retain(|p| !p.has_pdf),
        _ => {}
    }
    if f.highlights_only {
        list.retain(|p| p.annotation_count > 0);
    }
    list
}
