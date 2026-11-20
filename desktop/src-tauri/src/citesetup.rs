//! Citations in Word & Google Docs… on Windows and Linux (the Mac's `apple/Mac/CitationsView.swift`,
//! `CitationsSetup`): trust the certificate, install the Word add-in, status, remove.
//!
//! - **Trust, Windows:** the authority goes into this user's Root store (`CertAddEncodedCertificateToStore`
//!   on `CERT_SYSTEM_STORE_CURRENT_USER\Root`): no administrator needed, and Windows itself asks
//!   ("You are about to install a certificate from a certification authority… Do you want to install
//!   this certificate?"). Word, Edge and Chrome use that store. Its key is protected with DPAPI.
//! - **Trust, Linux:** browsers keep their own (NSS) databases: Chrome/Chromium's `~/.pki/nssdb` and
//!   each Firefox profile. With `certutil` (libnss3-tools) Lunta adds the authority to them; without
//!   it the window says how to import `ca.pem` in the browser's settings.
//! - **Word on Windows:** the add-in's manifest is written to `<app data>/citations/manifest.xml` and
//!   registered under `HKCU\Software\Microsoft\Office\16.0\WEF\Developer` (value: the add-in's id,
//!   data: the manifest's path). That is how Office's own tools sideload an add-in for one user
//!   (office-addin-dev-settings); no administrator, no network share (a shared-folder catalog
//!   needs a UNC share, which needs an administrator to make). Word shows it after a restart.
//! - **Word on the web** (any system): Home › Add-ins › More Settings › Upload My Add-in, with the
//!   manifest saved from the window.

use crate::citecerts;
use crate::citeserver;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// The add-in's id in manifest.xml (the registry value's name).
#[cfg_attr(not(windows), allow(dead_code))]
pub const ADDIN_ID: &str = "6a1f3b2c-6d0e-4c8e-9b8e-4d2f3a7c9e15";
/// Its name in the browsers' certificate databases (Linux).
const NSS_NICK: &str = "Lunta Local CA";

fn citations_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("Lunta")).join("citations")
}

fn manifest_path(app: &AppHandle) -> PathBuf {
    citations_dir(app).join("manifest.xml")
}

fn ca_der(app: &AppHandle) -> Result<Vec<u8>, String> {
    citecerts::cert_der(&citeserver::tls_dir(app).join("ca.pem")).ok_or_else(|| "Lunta hasn’t made its certificate yet. Try again in a moment.".into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// "windows", "linux", "macos"
    platform: &'static str,
    serving: bool,
    problem: Option<String>,
    port: u16,
    /// None: can't tell (Linux without certutil)
    trusted: Option<bool>,
    /// None: no desktop Word here
    word_installed: Option<bool>,
    /// seconds since an add-in last asked
    last_request: Option<u64>,
    certutil: bool,
    /// the Google Docs sidebar's pairing key
    token: String,
    ca_path: String,
}

#[tauri::command]
pub async fn citations_status(app: AppHandle) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (serving, problem, last_request) = citeserver::status();
        let ca = ca_der(&app).ok();
        Status {
            platform: if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" },
            serving,
            problem,
            port: citeserver::PORT,
            trusted: ca.as_deref().and_then(platform::is_trusted),
            word_installed: platform::word_installed(&manifest_path(&app)),
            last_request,
            certutil: platform::has_certutil(),
            token: citeserver::token(&app),
            ca_path: citeserver::tls_dir(&app).join("ca.pem").to_string_lossy().to_string(),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Trust Certificate: Windows asks the user to confirm; the answer says what happened.
#[tauri::command]
pub async fn citations_trust(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let der = ca_der(&app)?;
        platform::trust(&der, &citeserver::tls_dir(&app).join("ca.pem"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install in Word (Windows): the manifest into the app's data, registered for this user.
#[tauri::command]
pub async fn citations_install_word(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = write_manifest(&app)?;
        platform::install_word(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn write_manifest(app: &AppHandle) -> Result<PathBuf, String> {
    let path = manifest_path(app);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, citeserver::file("word/manifest.xml").unwrap_or_default()).map_err(|e| format!("Couldn’t write the add-in’s manifest: {e}"))?;
    Ok(path)
}

/// Saves the add-in's manifest where the user chose (for Word on the web's Upload My Add-in).
#[tauri::command]
pub fn citations_save_manifest(path: String) -> Result<(), String> {
    std::fs::write(&path, citeserver::file("word/manifest.xml").unwrap_or_default()).map_err(|e| e.to_string())
}

/// Saves the authority's certificate (ca.pem) where the user chose, to import it by hand.
#[tauri::command]
pub fn citations_save_certificate(app: AppHandle, path: String) -> Result<(), String> {
    std::fs::copy(citeserver::tls_dir(&app).join("ca.pem"), &path).map(|_| ()).map_err(|e| e.to_string())
}

/// Code.gs and Sidebar.html for the Google Docs Apps Script (the sidebar with this computer's key).
#[tauri::command]
pub fn citations_google_files(app: AppHandle) -> (String, String) {
    let text = |p: &str| String::from_utf8_lossy(citeserver::file(p).unwrap_or_default()).to_string();
    let sidebar = text("gdocs/Sidebar.html").replace("{{COMMON}}", &text("common.js")).replace("{{CSS}}", &text("panel.css")).replace("{{TOKEN}}", &citeserver::token(&app));
    (text("gdocs/Code.gs"), sidebar)
}

/// Remove: the add-in from Word, the authority from the trust stores, and a new authority made
/// (the old key gone), so nothing trusted remains that could vouch for localhost.
#[tauri::command]
pub async fn citations_remove(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut notes = Vec::new();
        if let Err(e) = platform::uninstall_word() {
            notes.push(e);
        }
        let _ = std::fs::remove_file(manifest_path(&app));
        if let Ok(der) = ca_der(&app) {
            if let Err(e) = platform::untrust(&der) {
                notes.push(e);
            }
        }
        let dir = citeserver::tls_dir(&app);
        let _ = std::fs::remove_dir_all(&dir);
        citeserver::refresh_certificates(&dir);
        Ok(if notes.is_empty() {
            "Removed. Word no longer loads Lunta Citations (after a restart), and this computer no longer trusts Lunta’s certificate. To cite again, start from step 1.".to_string()
        } else {
            format!("Removed, except: {}", notes.join(" "))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- Windows ----

#[cfg(windows)]
pub(crate) mod win {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{LocalFree, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Cryptography::*;
    use windows::Win32::System::Registry::*;

    pub const DEVELOPER_KEY: &str = r"Software\Microsoft\Office\16.0\WEF\Developer";

    /// DPAPI for this user: protect (`seal`) or unprotect.
    pub fn dpapi(data: &[u8], seal: bool) -> Result<Vec<u8>, String> {
        unsafe {
            let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
            let mut out = CRYPT_INTEGER_BLOB::default();
            let r = if seal {
                CryptProtectData(&input, &HSTRING::from("Lunta citation certificate key"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            };
            r.map_err(|e| format!("Windows couldn’t {} the key ({e})", if seal { "protect" } else { "read" }))?;
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
            Ok(v)
        }
    }

    /// This user's Root store ("Trusted Root Certification Authorities", which also shows the
    /// computer's).
    unsafe fn root_store() -> Result<HCERTSTORE, String> {
        let name = HSTRING::from("Root");
        CertOpenStore(CERT_STORE_PROV_SYSTEM_W, CERT_QUERY_ENCODING_TYPE(0), None, CERT_OPEN_STORE_FLAGS(CERT_SYSTEM_STORE_CURRENT_USER), Some(name.as_ptr() as *const _))
            .map_err(|e| format!("Couldn’t open the certificate store ({e})."))
    }

    /// The certificates in the store whose bytes are `der` (each duplicated; free with CertFreeCertificateContext).
    unsafe fn find(store: HCERTSTORE, der: &[u8]) -> Vec<*mut CERT_CONTEXT> {
        let mut found = Vec::new();
        let mut prev: Option<*const CERT_CONTEXT> = None;
        loop {
            let c = CertEnumCertificatesInStore(store, prev);
            if c.is_null() {
                break;
            }
            let bytes = std::slice::from_raw_parts((*c).pbCertEncoded, (*c).cbCertEncoded as usize);
            if bytes == der {
                found.push(CertDuplicateCertificateContext(Some(c)));
            }
            prev = Some(c);
        }
        found
    }

    pub fn is_trusted(der: &[u8]) -> Option<bool> {
        unsafe {
            let store = root_store().ok()?;
            let found = find(store, der);
            for c in &found {
                let _ = CertFreeCertificateContext(Some(*c));
            }
            let _ = CertCloseStore(Some(store), 0);
            Some(!found.is_empty())
        }
    }

    pub fn trust(der: &[u8]) -> Result<String, String> {
        unsafe {
            let store = root_store()?;
            // Windows asks the user here (a security warning with Yes / No); this thread waits
            let r = CertAddEncodedCertificateToStore(Some(store), X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, der, CERT_STORE_ADD_REPLACE_EXISTING, None);
            let _ = CertCloseStore(Some(store), 0);
            match r {
                Ok(()) => Ok("Certificate trusted. Word, Edge and Chrome on this computer can now reach Lunta.".into()),
                Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => Err("Not trusted: you answered No. Click Trust Certificate again and choose Yes.".into()),
                Err(e) => Err(format!("Windows didn’t add the certificate ({e}). Your organisation may not allow it; ask IT, or save the certificate and import it with them.")),
            }
        }
    }

    pub fn untrust(der: &[u8]) -> Result<(), String> {
        unsafe {
            let store = root_store()?;
            let mut err = None;
            for c in find(store, der) {
                // Windows asks the user to confirm removing a root certificate; this frees `c`
                if let Err(e) = CertDeleteCertificateFromStore(c) {
                    err = Some(format!("The certificate is still trusted ({e})."));
                }
            }
            let _ = CertCloseStore(Some(store), 0);
            err.map_or(Ok(()), Err)
        }
    }

    pub fn word_manifest() -> Option<String> {
        unsafe {
            let (key, name) = (HSTRING::from(DEVELOPER_KEY), HSTRING::from(super::ADDIN_ID));
            let mut len: u32 = 0;
            if RegGetValueW(HKEY_CURRENT_USER, &key, &name, RRF_RT_REG_SZ, None, None, Some(&mut len)) != ERROR_SUCCESS || len == 0 {
                return None;
            }
            let mut buf = vec![0u16; len as usize / 2 + 1];
            if RegGetValueW(HKEY_CURRENT_USER, &key, &name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) != ERROR_SUCCESS {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }

    pub fn install_word(manifest: &std::path::Path) -> Result<(), String> {
        unsafe {
            let mut key = HKEY::default();
            let r = RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(DEVELOPER_KEY), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None);
            if r != ERROR_SUCCESS {
                return Err(format!("Couldn’t register the add-in with Word (registry error {}).", r.0));
            }
            let wide: Vec<u16> = manifest.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            let r = RegSetValueExW(key, &HSTRING::from(super::ADDIN_ID), None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(key);
            if r != ERROR_SUCCESS {
                return Err(format!("Couldn’t register the add-in with Word (registry error {}).", r.0));
            }
            Ok(())
        }
    }

    pub fn uninstall_word() -> Result<(), String> {
        unsafe {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(DEVELOPER_KEY), &HSTRING::from(super::ADDIN_ID));
            if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(format!("Word still lists the add-in (registry error {}).", r.0))
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::path::Path;

    pub fn has_certutil() -> bool {
        false
    }
    pub fn is_trusted(der: &[u8]) -> Option<bool> {
        super::win::is_trusted(der)
    }
    pub fn trust(der: &[u8], _pem: &Path) -> Result<String, String> {
        super::win::trust(der)
    }
    pub fn untrust(der: &[u8]) -> Result<(), String> {
        super::win::untrust(der)
    }
    pub fn word_installed(manifest: &Path) -> Option<bool> {
        Some(super::win::word_manifest().is_some_and(|p| Path::new(&p) == manifest && manifest.exists()))
    }
    pub fn install_word(manifest: &Path) -> Result<String, String> {
        super::win::install_word(manifest)?;
        Ok("Installed. Quit Word completely and open it again: Cite is on the Home tab (or Home › Add-ins, under Developer Add-ins).".into())
    }
    pub fn uninstall_word() -> Result<(), String> {
        super::win::uninstall_word()
    }
}

// ---- Linux (and macOS, where this app runs only for testing) ----

#[cfg(not(windows))]
mod platform {
    use super::NSS_NICK;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub fn has_certutil() -> bool {
        cfg!(target_os = "linux") && Command::new("certutil").arg("-H").output().is_ok()
    }

    /// The browsers' certificate databases: Chrome/Chromium's, and every Firefox profile's
    /// (also the Snap and Flatpak ones).
    pub(crate) fn nss_dbs(home: &Path) -> Vec<PathBuf> {
        let mut out = vec![home.join(".pki/nssdb")];
        for base in [".mozilla/firefox", "snap/firefox/common/.mozilla/firefox", ".var/app/org.mozilla.firefox/.mozilla/firefox"] {
            for e in std::fs::read_dir(home.join(base)).into_iter().flatten().flatten() {
                if e.path().join("cert9.db").exists() {
                    out.push(e.path());
                }
            }
        }
        out
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    fn certutil(db: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
        Command::new("certutil").arg("-d").arg(format!("sql:{}", db.display())).args(args).output()
    }

    pub fn is_trusted(der: &[u8]) -> Option<bool> {
        if !has_certutil() {
            return None;
        }
        let db = home()?.join(".pki/nssdb");
        let out = certutil(&db, &["-L", "-n", NSS_NICK, "-r"]).ok()?;
        Some(out.status.success() && out.stdout == der)
    }

    pub fn trust(_der: &[u8], pem: &Path) -> Result<String, String> {
        if cfg!(target_os = "macos") {
            return Err("On a Mac, Lunta for Mac sets up citing in Word.".into());
        }
        if !has_certutil() {
            return Err("Lunta needs certutil to add its certificate to the browsers (sudo apt install libnss3-tools, or dnf install nss-tools), or import the certificate yourself: Save Certificate…, then in Chrome Settings › Privacy and security › Security › Manage certificates › Authorities › Import, in Firefox Settings › Privacy & Security › View Certificates › Authorities › Import (tick “Trust this CA to identify websites”).".into());
        }
        let home = home().ok_or("No home folder.")?;
        let chrome = home.join(".pki/nssdb");
        if !chrome.join("cert9.db").exists() {
            std::fs::create_dir_all(&chrome).map_err(|e| e.to_string())?;
            let _ = certutil(&chrome, &["-N", "--empty-password"]);
        }
        let mut done = Vec::new();
        let mut failed = Vec::new();
        for db in nss_dbs(&home) {
            let _ = certutil(&db, &["-D", "-n", NSS_NICK]); // an earlier authority
            match certutil(&db, &["-A", "-t", "C,,", "-n", NSS_NICK, "-i", &pem.to_string_lossy()]) {
                Ok(o) if o.status.success() => done.push(db),
                Ok(o) => failed.push(format!("{}: {}", db.display(), String::from_utf8_lossy(&o.stderr).trim())),
                Err(e) => failed.push(format!("{}: {e}", db.display())),
            }
        }
        let firefox = done.iter().filter(|d| d.to_string_lossy().contains("firefox")).count();
        let mut msg = format!("Certificate trusted in Chrome/Chromium{}. Restart the browser.", if firefox > 0 { format!(" and {firefox} Firefox profile(s)") } else { String::new() });
        if !failed.is_empty() {
            msg += &format!(" Not in: {}", failed.join("; "));
        }
        Ok(msg)
    }

    pub fn untrust(_der: &[u8]) -> Result<(), String> {
        if !has_certutil() {
            return Ok(());
        }
        if let Some(home) = home() {
            for db in nss_dbs(&home) {
                let _ = certutil(&db, &["-D", "-n", NSS_NICK]);
            }
        }
        Ok(())
    }

    pub fn word_installed(_manifest: &Path) -> Option<bool> {
        None
    }
    pub fn install_word(_manifest: &Path) -> Result<String, String> {
        Err("There is no Word for this system: use Word on the web (Save Manifest…, then Upload My Add-in).".into())
    }
    pub fn uninstall_word() -> Result<(), String> {
        Ok(())
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn finds_the_browsers_certificate_databases() {
        let home = std::env::temp_dir().join(format!("lunta-nss-test-{}", uuid::Uuid::new_v4()));
        for p in [".mozilla/firefox/abc.default-release", "snap/firefox/common/.mozilla/firefox/x.default", ".mozilla/firefox/empty"] {
            std::fs::create_dir_all(home.join(p)).unwrap();
        }
        std::fs::write(home.join(".mozilla/firefox/abc.default-release/cert9.db"), b"").unwrap();
        std::fs::write(home.join("snap/firefox/common/.mozilla/firefox/x.default/cert9.db"), b"").unwrap();
        let dbs = platform::nss_dbs(&home);
        assert_eq!(dbs.len(), 3, "{dbs:?}");
        assert_eq!(dbs[0], home.join(".pki/nssdb"));
        assert!(!dbs.iter().any(|d| d.ends_with("empty")));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_registered_id_is_the_manifests() {
        let m = String::from_utf8_lossy(citeserver::file("word/manifest.xml").unwrap()).to_string();
        assert!(m.contains(&format!("<Id>{ADDIN_ID}</Id>")));
    }

    #[test]
    fn the_google_sidebar_gets_the_key() {
        let html = String::from_utf8_lossy(citeserver::file("gdocs/Sidebar.html").unwrap()).to_string();
        assert!(html.contains("{{TOKEN}}") && html.contains("{{COMMON}}"));
    }
}
