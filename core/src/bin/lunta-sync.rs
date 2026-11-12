//! Command-line tool for trying Google Drive sync by hand.
//!
//! ```text
//! lunta-sync connect    --library DIR     sign in to Google (opens the browser)
//! lunta-sync sync       --library DIR     one sync round
//! lunta-sync status     --library DIR     what is pending (no network)
//! lunta-sync fetch ID   --library DIR     download all files of one paper (lazy mode)
//! lunta-sync disconnect --library DIR     revoke and forget the tokens
//!
//! options: --state-dir DIR (default LIBRARY/.lunta-sync: tokens, sync state, device id)
//!          --device ID  --lazy  --folder-name NAME (default Lunta)  --folder-id ID
//!          --scope full|file  --client-id ID --client-secret S (or LUNTA_GOOGLE_CLIENT_ID / _SECRET)
//!          --remote-dir DIR   use a plain folder as the "remote" instead of Google Drive (testing)
//! ```

use lunta_core::drive::*;
use lunta_core::model::FileKind;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Args {
    cmd: String,
    positional: Vec<String>,
    flags: std::collections::HashMap<String, String>,
}

fn parse() -> Args {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_default();
    let (mut positional, mut flags) = (vec![], std::collections::HashMap::new());
    while let Some(a) = it.next() {
        if let Some(k) = a.strip_prefix("--") {
            if k == "lazy" {
                flags.insert(k.into(), "1".into());
            } else {
                flags.insert(k.into(), it.next().unwrap_or_default());
            }
        } else {
            positional.push(a);
        }
    }
    Args { cmd, positional, flags }
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("lunta-sync: {msg}");
    std::process::exit(1);
}

fn main() {
    let a = parse();
    if !["connect", "disconnect", "sync", "status", "fetch"].contains(&a.cmd.as_str()) {
        eprintln!("usage: lunta-sync connect|sync|status|fetch <paper-id>|disconnect --library DIR [options]\n(see the header of core/src/bin/lunta-sync.rs)");
        std::process::exit(2);
    }
    let library = PathBuf::from(a.flags.get("library").cloned().unwrap_or_else(|| fail("--library DIR is required")));
    let state_dir = a.flags.get("state-dir").map(PathBuf::from).unwrap_or_else(|| library.join(".lunta-sync"));
    std::fs::create_dir_all(&state_dir).unwrap_or_else(|e| fail(e));

    let device = a.flags.get("device").cloned().unwrap_or_else(|| {
        let f = state_dir.join("device-id");
        std::fs::read_to_string(&f).map(|s| s.trim().to_string()).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
            let id = format!("{}-{}", std::env::consts::OS, &uuid::Uuid::new_v4().simple().to_string()[..8]);
            let _ = std::fs::write(&f, &id);
            id
        })
    });

    let scope = match a.flags.get("scope").map(|s| s.as_str()) {
        Some("file") => Scope::File,
        _ => Scope::Full,
    };
    let auth = || {
        let client = match a.flags.get("client-id") {
            Some(id) => OAuthClient::new(id.clone(), a.flags.get("client-secret").cloned().or_else(|| std::env::var("LUNTA_GOOGLE_CLIENT_SECRET").ok())),
            None => OAuthClient::from_env().unwrap_or_else(|e| fail(e)),
        }
        .with_scope(scope);
        Authenticator::new(client, Arc::new(FileTokenStore::new(state_dir.join("google-tokens.json"))))
    };

    match a.cmd.as_str() {
        "connect" => {
            let au = auth();
            let r = au.connect_loopback(
                &|url| {
                    println!("Open this URL in your browser to sign in to Google:\n\n{url}\n");
                    let opener = if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" };
                    let _ = std::process::Command::new(opener).arg(url).spawn();
                },
                Duration::from_secs(300),
            );
            match r {
                Ok(()) => println!("Connected. Tokens saved in {}", state_dir.join("google-tokens.json").display()),
                Err(e) => fail(e),
            }
        }
        "disconnect" => {
            auth().disconnect().unwrap_or_else(|e| fail(e));
            println!("Disconnected.");
        }
        _ => {
            let remote: Box<dyn RemoteStore> = match a.flags.get("remote-dir") {
                Some(d) => Box::new(LocalDirRemote::new(d)),
                None => {
                    let au = auth();
                    if !au.is_connected() {
                        fail("not connected; run `lunta-sync connect` first");
                    }
                    let root = match a.flags.get("folder-id") {
                        Some(id) => DriveRoot::Id(id.clone()),
                        None => DriveRoot::Named { name: a.flags.get("folder-name").cloned().unwrap_or_else(|| "Lunta".into()), create: true },
                    };
                    Box::new(GoogleDrive::new(au, root))
                }
            };
            let mut opts = SyncOptions::new(state_dir.join("sync-state.json"));
            opts.lazy = a.flags.contains_key("lazy");
            opts.progress = Some(Arc::new(|l| println!("  {l}")));
            let syncer = Syncer::new(&library, device.clone(), remote, opts).unwrap_or_else(|e| fail(e));
            match a.cmd.as_str() {
                "sync" => {
                    println!("device {device}");
                    let r = syncer.sync_now().unwrap_or_else(|e| fail(e));
                    println!(
                        "uploaded {}, downloaded {}, deleted {} local / {} remote, {} conflicts, {} left on remote (lazy), {} errors",
                        r.uploaded, r.downloaded, r.deleted_local, r.deleted_remote, r.conflicts.len(), r.skipped_lazy, r.errors.len()
                    );
                    for c in &r.conflicts {
                        println!("  conflict: {c}");
                    }
                    for e in &r.errors {
                        eprintln!("  error: {e}");
                    }
                    if !r.errors.is_empty() {
                        std::process::exit(1);
                    }
                }
                "status" => {
                    let s = syncer.status();
                    println!("device {device}\nremote: {}\nlast sync: {}", s.remote, if s.last_sync_ms == 0 { "never".into() } else { format!("{} ms since epoch", s.last_sync_ms) });
                    println!("tracked files {}, to upload {}, to download {}, remote only {}", s.tracked_files, s.pending_uploads, s.pending_downloads, s.not_downloaded);
                }
                "fetch" => {
                    let id = a.positional.first().cloned().unwrap_or_else(|| fail("fetch needs a paper id"));
                    let kinds: Vec<FileKind> = vec![];
                    let n = syncer.fetch_paper(&id, &kinds).unwrap_or_else(|e| fail(e));
                    println!("downloaded {n} file(s)");
                }
                _ => unreachable!(),
            }
        }
    }
}
