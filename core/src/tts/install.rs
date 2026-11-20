//! The natural voice's files: what to download (pinned by size and SHA-256), from where (the
//! sherpa-onnx project's GitHub releases, nothing else), and how it is unpacked into the app's
//! data folder. A download is checked before anything is unpacked, and unpacked into a temporary
//! folder that is renamed into place, so a half-finished or tampered download never counts.

use crate::model::{CoreError, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// One archive to download.
#[derive(Debug, Clone, Copy)]
pub struct Package {
    /// Folder it is unpacked into, inside the voice folder.
    pub dir: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    /// What to keep; the rest is skipped. A file name keeps that file (wherever it is in the
    /// archive) at the top of the folder; "name/" keeps that folder with everything in it.
    pub keep: &'static [&'static str],
}

/// Kokoro-82M v1.0 (Apache-2.0) as sherpa-onnx packages it: 334 MB to download, about 370 MB on
/// disk with only the English files kept (the Chinese lexicon and dictionaries are skipped). The
/// full-precision model: on ARM (Apple silicon, phones) the int8 one is two to three times slower.
/// espeak-ng's data reads the words the lexicon doesn't have (numbers, acronyms, new terms).
pub const MODEL: Package = Package {
    dir: "kokoro-multi-lang-v1_0",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2",
    sha256: "60703707bc05ccd48d60e1eb723f38b07d6e474795a0b6a588d7220400e0f7a4",
    size: 349_886_476,
    keep: &["model.onnx", "voices.bin", "tokens.txt", "lexicon-us-en.txt", "lexicon-gb-en.txt", "LICENSE", "espeak-ng-data/"],
};

/// The model file inside `MODEL.dir`.
pub const MODEL_FILE: &str = "model.onnx";

/// sherpa-onnx's C API and ONNX Runtime for this computer (Windows, Linux, macOS), loaded at run
/// time; None on Android, where they are in the APK.
pub fn engine_package() -> Option<Package> {
    #[allow(unused_mut, unused_assignments)]
    let mut p: Option<Package> = None;
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        p = Some(Package {
            dir: "sherpa-onnx-v1.13.8",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2",
            sha256: "b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7",
            size: 8_032_957,
            keep: &["sherpa-onnx-c-api.dll", "onnxruntime.dll", "onnxruntime_providers_shared.dll"],
        });
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        p = Some(Package {
            dir: "sherpa-onnx-v1.13.8",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-arm64-shared-MT-Release-lib.tar.bz2",
            sha256: "22cd2b2b5e35c1132abc74a91ee77f683645437382617af6a4bd5818b9c509f4",
            size: 7_622_849,
            keep: &["sherpa-onnx-c-api.dll", "onnxruntime.dll", "onnxruntime_providers_shared.dll"],
        });
    }
    #[cfg(all(target_os = "linux", not(target_os = "android"), target_arch = "x86_64"))]
    {
        p = Some(Package {
            dir: "sherpa-onnx-v1.13.8",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-linux-x64-shared-lib.tar.bz2",
            sha256: "3892d184be41027e18165e67f549cd4e4cdd8dcd73ac5579e97afd55e14e30b6",
            size: 9_816_899,
            keep: &["libsherpa-onnx-c-api.so", "libonnxruntime.so"],
        });
    }
    #[cfg(all(target_os = "linux", not(target_os = "android"), target_arch = "aarch64"))]
    {
        p = Some(Package {
            dir: "sherpa-onnx-v1.13.8",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-linux-aarch64-shared-cpu-lib.tar.bz2",
            sha256: "fb98b80628a383909bf83626daca2e0d0ffc93bf1f7d222eaaa4282b388f072f",
            size: 12_595_068,
            keep: &["libsherpa-onnx-c-api.so", "libonnxruntime.so"],
        });
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        p = Some(Package {
            dir: "sherpa-onnx-v1.13.8",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-osx-arm64-shared-lib.tar.bz2",
            sha256: "ae77050cdae565496059d96f5ab33d77b397a0864282a4e4a26e3b3b3effb948",
            size: 8_773_198,
            keep: &["libsherpa-onnx-c-api.dylib", "libonnxruntime.dylib"],
        });
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        p = Some(Package {
            dir: "sherpa-onnx-v1.13.8",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-osx-x64-shared-lib.tar.bz2",
            sha256: "0f88371565a06372889c76266e253abd1b415fcc6f98232cabd968933efb0711",
            size: 10_014_197,
            keep: &["libsherpa-onnx-c-api.dylib", "libonnxruntime.dylib"],
        });
    }
    p
}

/// Everything this device needs downloaded for the natural voice, in order.
pub fn packages() -> Vec<Package> {
    engine_package().into_iter().chain(std::iter::once(MODEL)).collect()
}

/// Is the package unpacked in `root`?
pub fn installed(root: &Path, p: &Package) -> bool {
    let dir = root.join(p.dir);
    p.keep.iter().filter(|f| **f != "LICENSE").all(|f| match f.strip_suffix('/') {
        Some(d) => dir.join(d).is_dir(),
        None => dir.join(f).is_file(),
    })
}

/// Where an archive entry goes in the package folder, if it is kept.
fn kept(p: &Package, path: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = path.components().filter_map(|c| match c {
        std::path::Component::Normal(s) => s.to_str(),
        _ => None,
    }).collect();
    let name = *parts.last()?;
    if p.keep.contains(&name) {
        return Some(PathBuf::from(name));
    }
    for k in p.keep {
        if let Some(d) = k.strip_suffix('/') {
            if let Some(i) = parts.iter().position(|x| *x == d) {
                if i + 1 < parts.len() {
                    return Some(parts[i..].iter().collect());
                }
            }
        }
    }
    None
}

/// Download progress: bytes so far of all packages, and the total.
pub type Progress<'a> = &'a dyn Fn(u64, u64);

/// Downloads and unpacks what's missing into `root`. `cancel` stops it (the partial download is
/// removed). Contacts only the packages' URLs.
pub fn install(root: &Path, progress: Progress, cancel: &AtomicBool) -> Result<()> {
    let todo: Vec<Package> = packages().into_iter().filter(|p| !installed(root, p)).collect();
    let total: u64 = todo.iter().map(|p| p.size).sum();
    let mut done = 0u64;
    fs::create_dir_all(root)?;
    for p in todo {
        let part = root.join(format!("{}.download", p.dir));
        let result = (|| {
            let mut out = fs::File::create(&part)?;
            let resp = agent().get(p.url).header("User-Agent", "Lunta (natural voice download)").call().map_err(|e| CoreError::Network(e.to_string()))?;
            let mut body = resp.into_body();
            let base = done;
            copy_verified(&mut body.as_reader(), &mut out, &p, &|n| progress(base + n, total), cancel)?;
            out.sync_all()?;
            drop(out);
            unpack(io::BufReader::new(fs::File::open(&part)?), root, &p)
        })();
        let _ = fs::remove_file(&part);
        result?;
        done += p.size;
        progress(done, total);
    }
    Ok(())
}

/// Removes the natural voice (model and engine) from `root`.
pub fn remove(root: &Path) -> Result<()> {
    for p in packages() {
        let d = root.join(p.dir);
        if d.exists() {
            fs::remove_dir_all(&d)?;
        }
    }
    Ok(())
}

/// Bytes the natural voice takes on disk in `root`.
pub fn disk_size(root: &Path) -> u64 {
    fn walk(p: &Path) -> u64 {
        match fs::metadata(p) {
            Ok(m) if m.is_dir() => fs::read_dir(p).map(|r| r.flatten().map(|e| walk(&e.path())).sum()).unwrap_or(0),
            Ok(m) => m.len(),
            Err(_) => 0,
        }
    }
    packages().iter().map(|p| walk(&root.join(p.dir))).sum()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
            .tls_config(crate::tls::config())
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// Copies `from` to `to`, checking the size and SHA-256 against the package's.
pub fn copy_verified(from: &mut dyn Read, to: &mut dyn Write, p: &Package, progress: &dyn Fn(u64), cancel: &AtomicBool) -> Result<()> {
    let mut hash = Sha256::new();
    let mut n = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    let mut last = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Invalid("download cancelled".into()));
        }
        let k = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(k) => k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CoreError::Network(e.to_string())),
        };
        n += k as u64;
        if n > p.size {
            return Err(CoreError::Invalid(format!("the download of {} is larger than expected", p.dir)));
        }
        hash.update(&buf[..k]);
        to.write_all(&buf[..k])?;
        if n - last >= 1 << 20 {
            last = n;
            progress(n);
        }
    }
    progress(n);
    if n != p.size {
        return Err(CoreError::Network(format!("the download of {} stopped early ({n} of {} bytes)", p.dir, p.size)));
    }
    let got = hex(&hash.finalize());
    if !got.eq_ignore_ascii_case(p.sha256) {
        return Err(CoreError::Invalid(format!("the download of {} is not the expected file (SHA-256 {got})", p.dir)));
    }
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Unpacks the kept files of a (verified) tar.bz2 into `root/<dir>`, through a temporary folder.
pub fn unpack(archive: impl Read, root: &Path, p: &Package) -> Result<()> {
    let tmp = root.join(format!("{}.unpacking", p.dir));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;
    let r = (|| {
        let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(archive));
        for entry in tar.entries()? {
            let mut e = entry?;
            if !e.header().entry_type().is_file() {
                continue;
            }
            let path = e.path()?.into_owned();
            let Some(rel) = kept(p, &path) else { continue };
            let to = tmp.join(&rel);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = fs::File::create(&to)?;
            io::copy(&mut e, &mut f)?;
        }
        let missing: Vec<&str> = p.keep.iter().copied().filter(|f| *f != "LICENSE" && !match f.strip_suffix('/') {
            Some(d) => tmp.join(d).is_dir(),
            None => tmp.join(f).is_file(),
        }).collect();
        if !missing.is_empty() {
            return Err(CoreError::Invalid(format!("the download of {} lacks {}", p.dir, missing.join(", "))));
        }
        let dest = root.join(p.dir);
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        fs::rename(&tmp, &dest)?;
        Ok(())
    })();
    if r.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    r
}

/// Where a package's files are once unpacked.
pub fn package_dir(root: &Path, p: &Package) -> PathBuf {
    root.join(p.dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tar.bz2 with these files (name, contents), and a package that expects it.
    fn archive(files: &[(&str, &[u8])], keep: &'static [&'static str]) -> (Vec<u8>, Package) {
        let mut tar = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, name, *data).unwrap();
        }
        let raw = tar.into_inner().unwrap();
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&raw).unwrap();
        let bytes = bz.finish().unwrap();
        let sha: &'static str = Box::leak(hex(&Sha256::digest(&bytes)).into_boxed_str());
        (bytes.clone(), Package { dir: "voice-test", url: "https://example.invalid/x.tar.bz2", sha256: sha, size: bytes.len() as u64, keep })
    }

    const KEEP: &[&str] = &["model.int8.onnx", "voices.bin", "LICENSE", "espeak-ng-data/"];

    #[test]
    fn verified_download_is_unpacked_with_only_the_kept_files() {
        let (bytes, p) = archive(&[
            ("pkg/model.int8.onnx", b"model"), ("pkg/voices.bin", b"voices"), ("pkg/dict/zh.txt", b"skip me"),
            ("pkg/espeak-ng-data/en_dict", b"en"), ("pkg/espeak-ng-data/voices/en/en-us", b"us"),
        ], KEEP);
        let root = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        let seen = std::cell::Cell::new(0u64);
        copy_verified(&mut &bytes[..], &mut out, &p, &|n| seen.set(n), &AtomicBool::new(false)).unwrap();
        assert_eq!(out, bytes);
        assert_eq!(seen.get(), bytes.len() as u64);
        unpack(&out[..], root.path(), &p).unwrap();
        let dir = root.path().join("voice-test");
        assert_eq!(fs::read(dir.join("model.int8.onnx")).unwrap(), b"model");
        assert_eq!(fs::read(dir.join("voices.bin")).unwrap(), b"voices");
        assert!(!dir.join("zh.txt").exists() && !dir.join("dict").exists());
        assert_eq!(fs::read(dir.join("espeak-ng-data/voices/en/en-us")).unwrap(), b"us");
        assert_eq!(fs::read(dir.join("espeak-ng-data/en_dict")).unwrap(), b"en");
        assert!(installed(root.path(), &p), "LICENSE is optional");
        assert!(!root.path().join("voice-test.unpacking").exists());
    }

    #[test]
    fn a_changed_or_short_download_is_refused() {
        let (bytes, p) = archive(&[("model.int8.onnx", b"model"), ("voices.bin", b"v"), ("espeak-ng-data/x", b"x")], KEEP);
        let never = AtomicBool::new(false);
        // one byte changed: same size, other hash
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xff;
        let e = copy_verified(&mut &bad[..], &mut Vec::new(), &p, &|_| {}, &never).unwrap_err();
        assert!(e.to_string().contains("not the expected file"), "{e}");
        // cut short
        let e = copy_verified(&mut &bytes[..bytes.len() - 3], &mut Vec::new(), &p, &|_| {}, &never).unwrap_err();
        assert!(e.to_string().contains("stopped early"), "{e}");
        // longer than announced
        let mut long = bytes.clone();
        long.extend_from_slice(b"extra");
        let e = copy_verified(&mut &long[..], &mut Vec::new(), &p, &|_| {}, &never).unwrap_err();
        assert!(e.to_string().contains("larger than expected"), "{e}");
        // cancelled
        let e = copy_verified(&mut &bytes[..], &mut Vec::new(), &p, &|_| {}, &AtomicBool::new(true)).unwrap_err();
        assert!(e.to_string().contains("cancelled"), "{e}");
    }

    #[test]
    fn an_archive_without_the_needed_files_leaves_nothing_behind() {
        let (bytes, p) = archive(&[("model.int8.onnx", b"model"), ("espeak-ng-data/x", b"x")], KEEP);
        let root = tempfile::tempdir().unwrap();
        let e = unpack(&bytes[..], root.path(), &p).unwrap_err();
        assert!(e.to_string().contains("voices.bin"), "{e}");
        assert!(!root.path().join("voice-test").exists());
        assert!(!root.path().join("voice-test.unpacking").exists());
        assert!(!installed(root.path(), &p));
    }

    #[test]
    fn pinned_packages_come_from_sherpa_onnx_releases() {
        for p in packages() {
            assert!(p.url.starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/"), "{}", p.url);
            assert_eq!(p.sha256.len(), 64);
            assert!(p.size > 1_000_000);
        }
        assert_eq!(packages().last().unwrap().dir, MODEL.dir);
    }
}
