//! Updates from GitHub Releases, through tauri-plugin-updater: `latest.json` on the latest release
//! (made by scripts/update-feeds.sh from the installers' signatures; the public key and the
//! endpoint are in tauri.conf.json). The frontend (src/updates.ts) checks quietly at start and once
//! a day when "Check for Updates Automatically" is on, and on demand from Settings.
//!
//! Windows (NSIS, MSI) and the Linux AppImage replace themselves and restart. A .deb or .rpm
//! belongs to the system's package manager, so there the app offers the release page instead.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{settings, AppState, R};

pub const RELEASES: &str = "https://github.com/christoffermattssonlangseth/lunta/releases";

/// The update found by the last check, kept for Install.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Serialize, Debug, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    /// What's new, as Markdown (the release notes)
    pub notes: String,
    /// false: show Download (the release page) instead of Install and Restart
    pub can_install: bool,
    pub page: String,
}

/// Whether this install can replace itself: the Windows installers and the Linux AppImage (which
/// runs with $APPIMAGE set). Not a .deb or .rpm, and not the macOS build (Lunta for Mac is the Mac
/// app; this one is for testing).
pub fn can_self_update() -> bool {
    use tauri::utils::config::BundleType;
    if cfg!(target_os = "windows") {
        return true;
    }
    if cfg!(target_os = "linux") {
        let appimage = std::env::var_os("APPIMAGE").is_some_and(|p| !p.is_empty());
        return appimage && !matches!(tauri::utils::platform::bundle_type(), Some(BundleType::Deb | BundleType::Rpm));
    }
    false
}

fn info(u: &Update, can_install: bool) -> UpdateInfo {
    UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone().unwrap_or_default(),
        can_install,
        page: format!("{RELEASES}/tag/v{}", u.version),
    }
}

/// Asks GitHub for latest.json; None when this is the newest version.
#[tauri::command]
pub async fn update_check(app: AppHandle, pending: State<'_, Pending>) -> R<Option<UpdateInfo>> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| format!("Couldn't check for updates: {e}"))?;
    let out = found.as_ref().map(|u| info(u, can_self_update()));
    *pending.0.lock().unwrap() = found;
    Ok(out)
}

/// Downloads the update found by the last check, checks its signature, installs it and restarts
/// (on Windows the installer closes the app and opens it again).
#[tauri::command]
pub async fn update_install(app: AppHandle, pending: State<'_, Pending>) -> R<()> {
    if !can_self_update() {
        return Err("This copy of Lunta was installed by the system's package manager: download the new version from the release page.".into());
    }
    let update = pending.0.lock().unwrap().take().ok_or("Check for updates first.")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("Couldn't install the update: {e}"))?;
    app.restart()
}

#[tauri::command]
pub fn update_auto(state: State<AppState>) -> bool {
    state.settings.lock().unwrap().check_updates()
}

#[tauri::command]
pub fn set_update_auto(state: State<AppState>, on: bool) -> R<()> {
    let mut s = state.settings.lock().unwrap();
    s.auto_update = Some(on);
    settings::save(&state.config_dir, &s).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use tauri_plugin_updater::RemoteRelease;

    /// latest.json as scripts/update-feeds.sh makes it, read as the updater reads it.
    fn feed() -> Option<String> {
        if !cfg!(unix) || std::process::Command::new("jq").arg("--version").output().is_err() {
            eprintln!("skipped: needs bash and jq");
            return None;
        }
        let dir = std::env::temp_dir().join(format!("lunta-feed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["Lunta-1.0.10-Windows-Setup.exe", "Lunta-1.0.10-Windows.msi", "Lunta-1.0.10-Linux.AppImage"] {
            std::fs::write(dir.join(format!("{f}.sig")), format!("sig-of-{f}")).unwrap();
        }
        // the release notes as written: what's new, then the template's text about the downloads
        let notes = "\r\n## What's new in 1.0.10\r\n\r\n- **Updates:** \"quoted\" too\r\n\r\nLunta is a reference manager for scientific papers: …\r\n\r\n## Which file do I need?\r\n\r\n| table |\r\n";
        std::fs::write(dir.join("notes.md"), notes).unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/update-feeds.sh");
        let out = std::process::Command::new("bash")
            .args([script, "desktop", "v1.0.10"])
            .env("DRY_RUN", "1")
            .env("FEEDS_LOCAL_DIR", &dir)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Some(String::from_utf8(out.stdout).unwrap())
    }

    #[test]
    fn feed_from_script_reads_as_the_updater_expects() {
        let Some(json) = feed() else { return };
        let r: RemoteRelease = serde_json::from_str(&json).unwrap();
        assert_eq!(r.version.to_string(), "1.0.10");
        assert_eq!(r.notes.as_deref(), Some("## What's new in 1.0.10\n\n- **Updates:** \"quoted\" too"));
        let dl = "https://github.com/christoffermattssonlangseth/lunta/releases/download/v1.0.10/";
        for (target, file) in [
            ("windows-x86_64", "Lunta-1.0.10-Windows-Setup.exe"),
            ("windows-x86_64-nsis", "Lunta-1.0.10-Windows-Setup.exe"),
            ("windows-x86_64-msi", "Lunta-1.0.10-Windows.msi"),
            ("linux-x86_64", "Lunta-1.0.10-Linux.AppImage"),
            ("linux-x86_64-appimage", "Lunta-1.0.10-Linux.AppImage"),
        ] {
            assert_eq!(r.download_url(target).unwrap().as_str(), format!("{dl}{file}"), "{target}");
            assert_eq!(r.signature(target).unwrap(), &format!("sig-of-{file}"), "{target}");
        }
        // a .deb install finds no entry of its own (and falls back to linux-x86_64, which it
        // doesn't install: can_self_update)
        assert!(r.download_url("linux-x86_64-deb").is_err());
    }

    /// The updater offers a release only when its version is newer (semver, so 1.0.10 > 1.0.9).
    #[test]
    fn versions_compare_as_semver() {
        let v = |s: &str| -> RemoteRelease {
            serde_json::from_value(serde_json::json!({"version": s, "platforms": {}})).unwrap()
        };
        let current = semver::Version::parse("1.0.9").unwrap();
        assert!(v("1.0.10").version > current);
        assert!(v("v1.1.0").version > current); // a leading v is accepted
        assert!(v("1.0.9").version <= current);
        assert!(v("1.0.5").version <= current);
    }
}
