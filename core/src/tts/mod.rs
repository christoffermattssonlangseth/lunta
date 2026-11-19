//! Listen: reading a paper aloud on Windows, Linux and Android, as the Apple apps do
//! (apple/Shared/Narrator.swift).
//!
//! - `text`: the narration items' text made ready for speech (the Apple apps' rules) and cut into
//!   sentence pieces, so the voice can start on the first sentence of a paragraph at once.
//! - `install`: the natural voice's download (Kokoro-82M packaged by sherpa-onnx, and on desktops
//!   sherpa-onnx's C library), pinned by size and SHA-256, into the app's data folder.
//! - `engine`: Kokoro through sherpa-onnx's C API, loaded at run time.
//! - `NaturalVoice`: all of it behind one object, exported to the Android app through UniFFI and
//!   used directly by the desktop app.
//! - `render`: a whole paper to one audio track and its timing map (narration.json), the format
//!   the Mac's "Make Natural Narration" writes, so a narration made anywhere plays everywhere.

pub mod engine;
pub mod install;
pub mod render;
pub mod text;

use crate::model::{CoreError, Result};
use engine::Kokoro;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A voice the app can offer.
#[derive(uniffi::Record, Clone, Debug, serde::Serialize)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
}

/// Where the natural voice stands on this device.
#[derive(uniffi::Record, Clone, Debug, serde::Serialize)]
pub struct VoiceStatus {
    /// Model (and engine) downloaded.
    pub installed: bool,
    /// Bytes still to download (0 when installed).
    pub download_bytes: u64,
    /// Bytes it takes on disk.
    pub disk_bytes: u64,
}

/// Receives download progress (bytes so far, total).
#[uniffi::export(with_foreign)]
pub trait DownloadListener: Send + Sync {
    fn on_progress(&self, done: u64, total: u64);
}

/// The natural voice (Kokoro-82M): download, delete, and speech for a piece of text.
#[derive(uniffi::Object)]
pub struct NaturalVoice {
    root: PathBuf,
    cancel: AtomicBool,
    engine: Mutex<Option<Kokoro>>,
    threads: i32,
}

#[uniffi::export]
impl NaturalVoice {
    /// `root`: a folder of its own in the app's data (not synced, not backed up).
    #[uniffi::constructor]
    pub fn new(root: String) -> Arc<Self> {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        // leave a core or two for the app and the audio; Kokoro gains little past 4 threads
        let threads = (threads.saturating_sub(1)).clamp(1, 4) as i32;
        Arc::new(NaturalVoice { root: PathBuf::from(root), cancel: AtomicBool::new(false), engine: Mutex::new(None), threads })
    }

    pub fn status(&self) -> VoiceStatus {
        let missing: u64 = install::packages().iter().filter(|p| !install::installed(&self.root, p)).map(|p| p.size).sum();
        VoiceStatus { installed: missing == 0, download_bytes: missing, disk_bytes: install::disk_size(&self.root) }
    }

    /// Downloads what's missing (blocking; call it off the main thread).
    pub fn download(&self, listener: Arc<dyn DownloadListener>) -> Result<()> {
        self.cancel.store(false, Ordering::Relaxed);
        install::install(&self.root, &|d, t| listener.on_progress(d, t), &self.cancel)
    }

    pub fn cancel_download(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Deletes the downloaded voice.
    pub fn remove(&self) -> Result<()> {
        *self.engine.lock().unwrap() = None;
        install::remove(&self.root)
    }

    /// Loads the model for this voice's accent ahead of the first sentence (a second or two).
    pub fn prepare(&self, voice: String) -> Result<()> {
        let mut g = self.engine.lock().unwrap();
        self.ensure(&mut g, &voice)
    }

    /// Speech for one piece of text: 16-bit little-endian mono PCM at `sample_rate()`.
    pub fn synthesize_pcm(&self, text: String, voice: String, speed: f32) -> Result<Vec<u8>> {
        let samples = self.synthesize(&text, &voice, speed)?;
        let mut out = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
        }
        Ok(out)
    }

    /// Speech for one piece of text as a WAV file, with the item kind's pauses around it when
    /// `before`/`after` are set.
    pub fn synthesize_wav(&self, text: String, voice: String, speed: f32, before: f32, after: f32) -> Result<Vec<u8>> {
        let samples = self.synthesize(&text, &voice, speed)?;
        Ok(engine::wav(&samples, self.sample_rate(), before, after))
    }

    pub fn sample_rate(&self) -> u32 {
        self.engine.lock().unwrap().as_ref().map(|k| k.sample_rate).unwrap_or(24_000)
    }
}

impl NaturalVoice {
    fn ensure(&self, g: &mut Option<Kokoro>, voice: &str) -> Result<()> {
        if g.as_ref().is_some_and(|k| k.fits(voice)) {
            return Ok(());
        }
        if !self.status().installed {
            return Err(CoreError::NotFound("the natural voice is not downloaded".into()));
        }
        *g = None;
        let lib_dir = install::engine_package().map(|p| install::package_dir(&self.root, &p));
        let model_dir = install::package_dir(&self.root, &install::MODEL);
        *g = Some(Kokoro::load(lib_dir.as_deref(), &model_dir, voice, self.threads)?);
        Ok(())
    }

    /// Mono samples (-1…1) at `sample_rate()`.
    pub fn synthesize(&self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>> {
        let mut g = self.engine.lock().unwrap();
        self.ensure(&mut g, voice)?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        g.as_ref().unwrap().synthesize(text, voice, speed)
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
}

/// The voices to offer (the Apple apps' eight), the default first.
#[uniffi::export]
pub fn tts_voices() -> Vec<VoiceInfo> {
    engine::VOICES.iter().map(|v| VoiceInfo { id: v.id.into(), name: v.name.into() }).collect()
}

/// A narration item's text made ready for speech (`kind`: heading, text or caption).
#[uniffi::export]
pub fn tts_spoken(text: String, kind: String) -> String {
    text::spoken(&text, &kind)
}

/// Pieces of a (spoken) paragraph for the voice, up to `max` characters each.
#[uniffi::export]
pub fn tts_chunks(text: String, max: u32) -> Vec<String> {
    text::chunks(&text, max as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the natural voice downloaded: `LUNTA_TTS_DIR=<folder> cargo test tts -- --ignored`
    /// (`cargo run --release --example tts_bench -- <folder>` downloads it there).
    #[test]
    #[ignore]
    fn synthesizes_a_sentence_with_the_downloaded_voice() {
        let Ok(root) = std::env::var("LUNTA_TTS_DIR") else {
            eprintln!("LUNTA_TTS_DIR not set: skipped");
            return;
        };
        let v = NaturalVoice::new(root);
        assert!(v.status().installed, "the natural voice is not downloaded there");
        let text = text::spoken("Cerebrospinal fluid lactate was higher in bacterial meningitis (Smith et al., 2020) [12].", "text");
        let samples = v.synthesize(&text, "af_heart", 1.0).unwrap();
        let secs = samples.len() as f32 / v.sample_rate() as f32;
        assert!(secs > 2.0 && secs < 12.0, "{secs} s");
        assert!(samples.iter().any(|s| s.abs() > 0.05), "silent");
        // the British voices load the model again with the other lexicon
        let gb = v.synthesize("Good morning.", "bf_emma", 1.0).unwrap();
        assert!(!gb.is_empty());
    }
}
