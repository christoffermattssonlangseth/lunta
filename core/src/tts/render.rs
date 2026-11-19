//! A whole paper read in the natural voice: one audio track and its timing map, as the Mac's
//! "Make Natural Narration" writes them (tts/lunta_tts.py): `narration.json` is
//! `{"voice", "model", "segments": [{"id", "kind", "start", "end"}]}` in seconds, with the same
//! pauses around headings, paragraphs and captions. The app encodes the track as AAC
//! (`narration.m4a`) and attaches both to the paper, so the narration syncs and plays everywhere.

use super::{engine, text, NaturalVoice};
use crate::model::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// One narration item from the reading view (narration.js's luntaNarration()).
#[derive(uniffi::Record, Clone, Debug, Deserialize, Serialize)]
pub struct NarrationSegment {
    pub id: String,
    pub kind: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Timing {
    pub id: String,
    pub kind: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Timings {
    pub voice: String,
    pub model: String,
    pub segments: Vec<Timing>,
}

/// Receives render progress: items done, items in all.
#[uniffi::export(with_foreign)]
pub trait RenderListener: Send + Sync {
    fn on_progress(&self, done: u32, total: u32);
}

/// Longest piece of text given to the voice at once while rendering.
const PIECE: usize = 400;

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Renders `items` to a 16-bit mono WAV at `wav_path` with `synth` (text → samples at `rate`),
/// returning the timing map. Items with nothing to say are left out, as on the Mac.
pub fn render_with(
    items: &[NarrationSegment],
    voice: &str,
    rate: u32,
    wav_path: &Path,
    synth: &mut dyn FnMut(&str) -> Result<Vec<f32>>,
    progress: &dyn Fn(u32, u32),
    cancelled: &dyn Fn() -> bool,
) -> Result<Timings> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(wav_path)?);
    f.write_all(&engine::wav(&[], rate, 0.0, 0.0))?; // header; sizes filled in at the end
    let mut frames: u64 = 0;
    let mut segments = Vec::new();
    let silence = |f: &mut dyn Write, secs: f32, frames: &mut u64| -> std::io::Result<()> {
        let n = (secs * rate as f32) as usize;
        f.write_all(&vec![0u8; n * 2])?;
        *frames += n as u64;
        Ok(())
    };
    let total = items.len() as u32;
    for (i, item) in items.iter().enumerate() {
        if cancelled() {
            return Err(CoreError::Invalid("cancelled".into()));
        }
        let said = text::spoken(&item.text, &item.kind);
        if said.is_empty() {
            progress(i as u32 + 1, total);
            continue;
        }
        let (before, after) = engine::pauses(&item.kind);
        silence(&mut f, before, &mut frames)?;
        let start = frames as f64 / rate as f64;
        for piece in text::chunks(&said, PIECE) {
            let samples = synth(&piece)?;
            for s in &samples {
                f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())?;
            }
            frames += samples.len() as u64;
        }
        let end = frames as f64 / rate as f64;
        segments.push(Timing { id: item.id.clone(), kind: item.kind.clone(), start: round3(start), end: round3(end) });
        silence(&mut f, after, &mut frames)?;
        progress(i as u32 + 1, total);
    }
    // the WAV header's sizes
    let mut file = f.into_inner().map_err(|e| CoreError::Io(e.to_string()))?;
    let data = (frames * 2) as u32;
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&(36 + data).to_le_bytes())?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&data.to_le_bytes())?;
    file.sync_all()?;
    Ok(Timings { voice: voice.to_string(), model: format!("Kokoro-82M ({}, sherpa-onnx)", super::install::MODEL.dir), segments })
}

#[uniffi::export]
impl NaturalVoice {
    /// Renders a whole paper in the natural voice to a WAV file at `wav_path` (blocking) and
    /// returns narration.json. `cancel_download()` stops it.
    pub fn render(&self, items: Vec<NarrationSegment>, voice: String, wav_path: String, listener: Arc<dyn RenderListener>) -> Result<String> {
        self.cancel.store(false, Ordering::Relaxed);
        self.prepare(voice.clone())?;
        let rate = self.sample_rate();
        let t = render_with(
            &items,
            &voice,
            rate,
            Path::new(&wav_path),
            &mut |s| self.synthesize(s, &voice, 1.0),
            &|d, n| listener.on_progress(d, n),
            &|| self.cancel.load(Ordering::Relaxed),
        )?;
        serde_json::to_string(&t).map_err(|e| CoreError::Invalid(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timings_follow_the_macs_pauses_and_skip_empty_items() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("n.wav");
        let items = vec![
            NarrationSegment { id: "h".into(), kind: "heading".into(), text: "Methods".into() },
            NarrationSegment { id: "x".into(), kind: "text".into(), text: "[12]".into() },
            NarrationSegment { id: "p".into(), kind: "text".into(), text: "One. Two.".into() },
        ];
        // a fake voice: 0.1 s of audio per character
        let rate = 1000;
        let mut synth = |s: &str| Ok(vec![0.5f32; s.chars().count() * 100]);
        let t = render_with(&items, "af_heart", rate, &wav, &mut synth, &|_, _| {}, &|| false).unwrap();
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[0], Timing { id: "h".into(), kind: "heading".into(), start: 0.55, end: 0.55 + 0.7 });
        // the heading's 0.45 s after it, none before the paragraph
        let p = &t.segments[1];
        assert!((p.start - (1.25 + 0.45)).abs() < 1e-9, "{p:?}");
        assert!((p.end - (p.start + 0.9)).abs() < 1e-9, "{p:?}");
        let bytes = std::fs::read(&wav).unwrap();
        let frames = ((p.end + 0.35) * rate as f64).round() as usize;
        assert_eq!(bytes.len(), 44 + frames * 2);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize, frames * 2);
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.starts_with(r#"{"voice":"af_heart","model":"#), "{json}");
    }
}
