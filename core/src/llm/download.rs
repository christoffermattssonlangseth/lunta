//! Downloading a model: into `<dir>/<file>.part`, resumed with a Range request after an
//! interruption (or a cancel), checked against the model's SHA-256, and only then renamed to
//! `<dir>/<file>`. So a file under its own name is always a complete, verified model.

use super::models::AiModel;
use super::AiError;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What a GET from a byte offset gave.
pub struct Fetched {
    /// 200 (the whole file) or 206 (from the offset asked for); 416 when the offset is at or past the end.
    pub status: u16,
    pub body: Box<dyn Read + Send>,
}

/// HTTP, behind a trait so the tests can stand in for the server.
pub trait Fetch: Send + Sync {
    /// GET `url`, from byte `from` (a Range request when `from` > 0).
    fn get(&self, url: &str, from: u64) -> Result<Fetched, AiError>;
}

/// The real thing (ureq; follows Hugging Face's redirect to its CDN).
pub struct HttpFetch;

impl Fetch for HttpFetch {
    fn get(&self, url: &str, from: u64) -> Result<Fetched, AiError> {
        static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
        let agent = AGENT.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(30)))
                .timeout_per_read(Some(Duration::from_secs(60)))
                .http_status_as_error(false)
                .build()
                .into()
        });
        let mut req = agent.get(url).header("User-Agent", "Lunta (on-device AI model download)");
        if from > 0 {
            req = req.header("Range", format!("bytes={from}-"));
        }
        let r = req.call().map_err(|e| AiError::Network(e.to_string()))?;
        let status = r.status().as_u16();
        if !(status == 200 || status == 206 || status == 416) {
            return Err(AiError::Network(format!("the server answered {status}")));
        }
        Ok(Fetched { status, body: Box::new(r.into_body().into_reader()) })
    }
}

pub fn model_path(model: &AiModel, dir: &Path) -> PathBuf {
    dir.join(&model.file_name)
}

fn part_path(model: &AiModel, dir: &Path) -> PathBuf {
    dir.join(format!("{}.part", model.file_name))
}

/// The model's file, if it is there (complete: only a verified download gets its name).
pub fn installed(model: &AiModel, dir: &Path) -> Option<PathBuf> {
    let p = model_path(model, dir);
    std::fs::metadata(&p).ok().filter(|m| m.len() == model.size).map(|_| p)
}

/// Bytes of an unfinished download (0 if none).
pub fn partial_bytes(model: &AiModel, dir: &Path) -> u64 {
    std::fs::metadata(part_path(model, dir)).map(|m| m.len()).unwrap_or(0)
}

/// Removes the model and any unfinished download.
pub fn remove(model: &AiModel, dir: &Path) -> Result<(), AiError> {
    for p in [model_path(model, dir), part_path(model, dir)] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(AiError::Io(e.to_string())),
        }
    }
    Ok(())
}

fn io(e: std::io::Error) -> AiError {
    AiError::Io(e.to_string())
}

/// Downloads the model into `dir` (made if needed) and returns its path. `progress(done, total)`
/// is called after every chunk (256 KB; throttle what you show); returning false stops the download (the part is kept, and the
/// next call goes on from there). A dropped connection is retried a few times from where it got to.
pub fn download(
    model: &AiModel,
    dir: &Path,
    fetch: &dyn Fetch,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<PathBuf, AiError> {
    if let Some(p) = installed(model, dir) {
        return Ok(p);
    }
    std::fs::create_dir_all(dir).map_err(io)?;
    let part = part_path(model, dir);
    let mut have = partial_bytes(model, dir);
    if have > model.size {
        std::fs::remove_file(&part).map_err(io)?;
        have = 0;
    }
    // the hash of what is already there
    let mut hasher = Sha256::new();
    if have > 0 {
        let mut f = File::open(&part).map_err(io)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).map_err(io)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }
    let mut failures = 0;
    if !progress(have, model.size) {
        return Err(AiError::Cancelled);
    }
    while have < model.size {
        let got = fetch.get(&model.url, have);
        let fetched = match got {
            Ok(f) => f,
            Err(e) => {
                failures += 1;
                if failures > 3 {
                    return Err(e);
                }
                std::thread::sleep(Duration::from_millis(500 * failures));
                continue;
            }
        };
        let mut out = match fetched.status {
            206 => OpenOptions::new().append(true).create(true).open(&part).map_err(io)?,
            416 => break, // nothing past what we have: check it below
            _ => {
                // the server sent the whole file (no Range support): start again
                have = 0;
                hasher = Sha256::new();
                File::create(&part).map_err(io)?
            }
        };
        let mut body = fetched.body;
        let mut buf = vec![0u8; 256 * 1024];
        let mut broke = false;
        loop {
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => {
                    broke = true;
                    break;
                }
            };
            let n = n.min((model.size - have) as usize);
            out.write_all(&buf[..n]).map_err(io)?;
            hasher.update(&buf[..n]);
            have += n as u64;
            if !progress(have, model.size) {
                out.flush().map_err(io)?;
                return Err(AiError::Cancelled);
            }
            if have >= model.size {
                break;
            }
        }
        out.flush().map_err(io)?;
        drop(out);
        if have < model.size {
            failures += 1;
            if failures > 3 {
                return Err(AiError::Network(if broke {
                    "the connection dropped; try again to go on where it stopped".into()
                } else {
                    "the download ended early; try again to go on where it stopped".into()
                }));
            }
            std::thread::sleep(Duration::from_millis(500 * failures));
        }
    }
    let size = partial_bytes(model, dir);
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if size != model.size || digest != model.sha256.to_lowercase() {
        let _ = std::fs::remove_file(&part);
        return Err(AiError::Checksum(format!(
            "the downloaded model is not the expected file ({size} bytes, SHA-256 {}…); it was deleted, try again",
            &digest[..12]
        )));
    }
    let dest = model_path(model, dir);
    std::fs::rename(&part, &dest).map_err(io)?;
    Ok(dest)
}
