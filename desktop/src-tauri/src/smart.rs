//! Smart folders, read from `<library>/smart-folders.json` (written by the Mac and iPhone apps;
//! apple/Shared/SmartFolders.swift) and evaluated the same way. The desktop app shows and opens
//! them, and creates, edits and deletes them (same JSON, sorted by name, like saveSmartFolder).

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

fn store(root: &Path, list: &[SmartFolder]) -> Result<(), String> {
    // pretty-printed with sorted keys, like the Swift encoder; every field is written (Swift's
    // Codable needs them all)
    let value = serde_json::to_value(list).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    let tmp = root.join("smart-folders.json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, root.join("smart-folders.json")).map_err(|e| e.to_string())
}

pub fn save(root: &Path, mut f: SmartFolder) -> Result<(), String> {
    if f.id.is_empty() {
        f.id = uuid_like();
    }
    if !matches!(f.pdf.as_str(), "has" | "missing") {
        f.pdf = "any".into();
    }
    let mut list = load(root);
    match list.iter().position(|x| x.id == f.id) {
        Some(i) => list[i] = f,
        None => list.push(f),
    }
    list.sort_by_key(|x| x.name.to_lowercase());
    store(root, &list)
}

pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    let mut list = load(root);
    list.retain(|x| x.id != id);
    store(root, &list)
}

fn uuid_like() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
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
