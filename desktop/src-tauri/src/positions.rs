//! Reading positions, in the same files the Mac and iPhone apps use
//! (apple/Shared/ReadingPosition.swift): each device writes only its own
//! `<library>/positions/<device-id>.json`, a map paper id → `{offset, at, device}`, and a reader
//! opens at the newest position from any device. `offset` is a character offset into the page's
//! text (the same on every device); `at` is a Swift `Date` as JSONEncoder writes it by default,
//! i.e. seconds since 2001-01-01; `device` is a label for the "continuing where you left off on
//! your …" note.
//!
//! Positions are not in the core's op log (they change all the time); this module is the desktop
//! counterpart of ReadingPositions in Swift.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Seconds between 1970-01-01 and 2001-01-01 (Swift's reference date).
const SWIFT_EPOCH: f64 = 978_307_200.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Spot {
    pub offset: i64,
    pub at: f64,
    pub device: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Latest {
    pub offset: i64,
    pub device: String,
    /// True if this device saved it.
    pub mine: bool,
}

fn folder(root: &Path) -> PathBuf {
    root.join("positions")
}

fn read(path: &Path) -> BTreeMap<String, Spot> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn now_swift() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
        - SWIFT_EPOCH
}

/// Saves this device's position for a paper (skipped if it moved fewer than 40 characters, like
/// the apps). Only ever writes this device's own file, atomically.
pub fn save(root: &Path, device_id: &str, label: &str, paper: &str, offset: i64) -> std::io::Result<()> {
    let dir = folder(root);
    let path = dir.join(format!("{device_id}.json"));
    let mut mine = read(&path);
    if let Some(old) = mine.get(paper) {
        if (old.offset - offset).abs() < 40 {
            return Ok(());
        }
    }
    mine.insert(paper.to_string(), Spot { offset, at: now_swift(), device: label.to_string() });
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{device_id}.json.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(&mine).unwrap_or_default())?;
    std::fs::rename(tmp, path)
}

/// The most recent position for a paper from any device.
pub fn latest(root: &Path, device_id: &str, paper: &str) -> Option<Latest> {
    let rd = std::fs::read_dir(folder(root)).ok()?;
    let mut best: Option<(Spot, bool)> = None;
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || p.extension().map_or(true, |x| x != "json") {
            continue;
        }
        if let Some(s) = read(&p).remove(paper) {
            if best.as_ref().map_or(true, |(b, _)| s.at > b.at) {
                best = Some((s, name == format!("{device_id}.json")));
            }
        }
    }
    best.map(|(s, mine)| Latest { offset: s.offset, device: s.device, mine })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_newest_wins() {
        let root = std::env::temp_dir().join(format!("lunta-pos-{}", uuid::Uuid::new_v4()));
        // another device's file, as the Swift app writes it
        std::fs::create_dir_all(root.join("positions")).unwrap();
        std::fs::write(root.join("positions/mac-1.json"), r#"{"p1":{"offset":500,"at":100.5,"device":"Mac"}}"#).unwrap();
        assert_eq!(latest(&root, "linux-1", "p1").unwrap().device, "Mac");
        save(&root, "linux-1", "Linux PC", "p1", 900).unwrap();
        let l = latest(&root, "linux-1", "p1").unwrap();
        assert_eq!((l.offset, l.mine), (900, true));
        // the other device's file is untouched
        assert!(std::fs::read_to_string(root.join("positions/mac-1.json")).unwrap().contains("500"));
        let _ = std::fs::remove_dir_all(root);
    }
}
