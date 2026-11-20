//! What the desktop app remembers between runs: which folder holds the library, and this
//! install's device id (the core writes this device's changes to `ops/<device-id>.jsonl`, so the
//! id must be stable and unique per install; it never changes once made).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Settings {
    /// The library folder (inside Google Drive, Dropbox, OneDrive, or anywhere local).
    #[serde(default)]
    pub library: Option<String>,
    #[serde(default)]
    pub device_id: String,
    /// .bib files kept up to date: "*" (all papers) or a folder id -> file path (bibfiles.rs).
    #[serde(default)]
    pub bib_files: std::collections::BTreeMap<String, String>,
    /// "Check for Updates Automatically" (updates.rs); unset means on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
    /// The Google Docs sidebar's pairing key for the citation server (citeserver.rs).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub citation_token: String,
}

impl Settings {
    pub fn check_updates(&self) -> bool {
        self.auto_update.unwrap_or(true)
    }
}

fn file(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Loads the settings, making (and saving) the device id on first run.
pub fn load(config_dir: &Path) -> Settings {
    let mut s: Settings = std::fs::read_to_string(file(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if s.device_id.trim().is_empty() {
        // like the apps' "mac-1a2b3c4d" / "ios-…": the platform, then 8 random hex digits
        let os = match std::env::consts::OS {
            "windows" => "windows",
            "linux" => "linux",
            "macos" => "macos",
            other => other,
        };
        let hex = uuid::Uuid::new_v4().simple().to_string();
        s.device_id = format!("{os}-{}", &hex[..8]);
        let _ = save(config_dir, &s);
    }
    s
}

pub fn save(config_dir: &Path, s: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let tmp = config_dir.join(".settings.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s).unwrap_or_default())?;
    std::fs::rename(tmp, file(config_dir))
}

/// What the "continued from your …" note calls this device on the other devices.
pub fn device_label() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows PC",
        "linux" => "Linux PC",
        "macos" => "Mac",
        _ => "computer",
    }
}
