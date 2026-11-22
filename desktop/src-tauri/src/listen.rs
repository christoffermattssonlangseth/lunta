//! Listen: the natural voice (Kokoro-82M through the core's `tts`) for the reader's player
//! (src/listen.ts). The voice is downloaded on demand into the app's local data folder (never
//! the library, never roaming profiles); clips are made one sentence piece at a time, as the
//! player asks for them, and come back as WAV for an <audio> element. A Mac-made narration
//! (narration.m4a + narration.json next to the paper) is handed over as is.

use crate::{AppState, R};
use lunta_core::model::FileKind;
use lunta_core::tts::{self, text, NaturalVoice, VoiceInfo, VoiceStatus};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Listen {
    voice: Mutex<Option<Arc<NaturalVoice>>>,
    /// The player's current generation: clips asked for under an older one (before a skip or a
    /// stop) are dropped before they are made, so the voice works on what will play next.
    generation: AtomicU64,
}

impl Listen {
    pub fn new() -> Self {
        Listen { voice: Mutex::new(None), generation: AtomicU64::new(0) }
    }

    fn voice(&self, app: &AppHandle) -> R<Arc<NaturalVoice>> {
        let mut g = self.voice.lock().unwrap();
        if let Some(v) = g.as_ref() {
            return Ok(v.clone());
        }
        let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("voice");
        let v = NaturalVoice::new(dir.to_string_lossy().to_string());
        *g = Some(v.clone());
        Ok(v)
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn listen_voices() -> Vec<VoiceInfo> {
    tts::tts_voices()
}

#[tauri::command]
pub fn listen_status(app: AppHandle, listen: State<'_, Listen>) -> R<VoiceStatus> {
    Ok(listen.voice(&app)?.status())
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    done: u64,
    total: u64,
}

struct Emit(AppHandle);
impl tts::DownloadListener for Emit {
    fn on_progress(&self, done: u64, total: u64) {
        let _ = self.0.emit("listen-download", DownloadProgress { done, total });
    }
}

/// Downloads the natural voice (progress as "listen-download" events).
#[tauri::command]
pub async fn listen_download(app: AppHandle, listen: State<'_, Listen>) -> R<VoiceStatus> {
    let v = listen.voice(&app)?;
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        v.download(Arc::new(Emit(a))).map_err(err)?;
        Ok(v.status())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn listen_cancel_download(app: AppHandle, listen: State<'_, Listen>) -> R<()> {
    listen.voice(&app)?.cancel_download();
    Ok(())
}

#[tauri::command]
pub fn listen_remove(app: AppHandle, listen: State<'_, Listen>) -> R<VoiceStatus> {
    let v = listen.voice(&app)?;
    v.remove().map_err(err)?;
    Ok(v.status())
}

/// One narration item, made ready for speech and cut into the pieces the voice makes one at a time.
#[derive(Serialize)]
pub struct SpokenItem {
    id: String,
    kind: String,
    pieces: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct Item {
    id: String,
    kind: String,
    text: String,
}

/// The reading view's narration items as the voice will say them (the Apple apps' rules).
#[tauri::command]
pub fn listen_prepare_text(items: Vec<Item>, max: Option<u32>) -> Vec<SpokenItem> {
    let max = max.unwrap_or(300) as usize;
    items
        .into_iter()
        .map(|i| {
            let said = text::spoken(&i.text, &i.kind);
            SpokenItem { pieces: if said.is_empty() { vec![] } else { text::chunks(&said, max) }, id: i.id, kind: i.kind }
        })
        .collect()
}

/// The player's new generation (a skip, a new paper, stop): clips still waiting for an older one are dropped.
#[tauri::command]
pub fn listen_generation(listen: State<'_, Listen>, generation: u64) {
    listen.generation.store(generation, Ordering::SeqCst);
}

/// Loads the model for this voice ahead of the first clip.
#[tauri::command]
pub async fn listen_warm_up(app: AppHandle, listen: State<'_, Listen>, voice: String) -> R<()> {
    let v = listen.voice(&app)?;
    tauri::async_runtime::spawn_blocking(move || v.prepare(voice).map_err(err)).await.map_err(err)?
}

/// One piece in the natural voice, as WAV, with `before`/`after` seconds of silence. Empty if
/// the generation moved on before it was its turn.
#[tauri::command]
pub async fn listen_clip(
    app: AppHandle,
    listen: State<'_, Listen>,
    text: String,
    voice: String,
    generation: u64,
    before: f32,
    after: f32,
) -> R<tauri::ipc::Response> {
    let v = listen.voice(&app)?;
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let current = a.state::<Listen>().generation.load(Ordering::SeqCst);
        if generation != current {
            return Ok(tauri::ipc::Response::new(Vec::new()));
        }
        // speed is the player's (playbackRate), so a clip is made once whatever the speed
        let wav = v.synthesize_wav(text, voice, 1.0, before, after).map_err(err)?;
        Ok(tauri::ipc::Response::new(wav))
    })
    .await
    .map_err(err)?
}

/// A narration made on the Mac ("Make Natural Narration"), if this paper has one here: its
/// timing map (narration.json, as text). The audio comes from `listen_paper_audio`.
#[tauri::command]
pub fn listen_paper_timings(state: State<'_, AppState>, id: String) -> R<Option<String>> {
    let lib = state.lib()?;
    // the audio may be only in Google Drive (a library synced on demand): listen_paper_audio fetches it
    let audio_here = lib.file_path(id.clone(), FileKind::Audio).is_some_and(|a| std::path::Path::new(&a).is_file())
        || lib.file_status(id.clone(), FileKind::Audio) == lunta_core::model::FileStatus::InCloud;
    let Some(index) = lib.file_path(id, FileKind::AudioIndex) else {
        return Ok(None);
    };
    if !audio_here {
        return Ok(None);
    }
    Ok(std::fs::read_to_string(index).ok())
}

/// The Mac-made narration's audio (AAC in an .m4a).
#[tauri::command]
pub async fn listen_paper_audio(app: tauri::AppHandle, state: State<'_, AppState>, id: String) -> R<tauri::ipc::Response> {
    let lib = state.lib()?;
    let data = tauri::async_runtime::spawn_blocking(move || {
        crate::drivesync::ensure_local(&app, &id, &[FileKind::Audio])?;
        let path = lib.file_path(id, FileKind::Audio).ok_or("This paper has no narration on this computer.")?;
        std::fs::read(path).map_err(err)
    })
    .await
    .map_err(err)??;
    Ok(tauri::ipc::Response::new(data))
}
