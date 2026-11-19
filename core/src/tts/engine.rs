//! Kokoro-82M through sherpa-onnx's C API, loaded at run time with libloading: the library is
//! downloaded with the model on Windows, Linux and macOS (see `install`), and is in the APK on
//! Android. Nothing is linked at build time, so the app builds anywhere without it.
//!
//! The structs below are sherpa-onnx v1.13.8's `c-api.h` (the version `install` pins: the last
//! one that reads words its lexicon lacks with espeak-ng; v2 skips them, numbers included).

use crate::model::{CoreError, Result};
use libloading::Library;
use std::ffi::{c_char, c_float, c_void, CString};
use std::path::{Path, PathBuf};

#[repr(C)]
#[derive(Clone, Copy)]
struct VitsConfig {
    model: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: c_float,
    noise_scale_w: c_float,
    length_scale: c_float,
    dict_dir: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct MatchaConfig {
    acoustic_model: *const c_char,
    vocoder: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: c_float,
    length_scale: c_float,
    dict_dir: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct KokoroConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: c_float,
    dict_dir: *const c_char,
    lexicon: *const c_char,
    lang: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct KittenConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: c_float,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ZipvoiceConfig {
    tokens: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    vocoder: *const c_char,
    data_dir: *const c_char,
    lexicon: *const c_char,
    feat_scale: c_float,
    t_shift: c_float,
    target_rms: c_float,
    guidance_scale: c_float,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct PocketConfig {
    lm_flow: *const c_char,
    lm_main: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    text_conditioner: *const c_char,
    vocab_json: *const c_char,
    token_scores_json: *const c_char,
    voice_embedding_cache_capacity: i32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct SupertonicConfig {
    duration_predictor: *const c_char,
    text_encoder: *const c_char,
    vector_estimator: *const c_char,
    vocoder: *const c_char,
    tts_json: *const c_char,
    unicode_indexer: *const c_char,
    voice_style: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ModelConfig {
    vits: VitsConfig,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
    matcha: MatchaConfig,
    kokoro: KokoroConfig,
    kitten: KittenConfig,
    zipvoice: ZipvoiceConfig,
    pocket: PocketConfig,
    supertonic: SupertonicConfig,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct TtsConfig {
    model: ModelConfig,
    rule_fsts: *const c_char,
    max_num_sentences: i32,
    rule_fars: *const c_char,
    silence_scale: c_float,
}
#[repr(C)]
struct GeneratedAudio {
    samples: *const f32,
    n: i32,
    sample_rate: i32,
}

type CreateFn = unsafe extern "C" fn(*const TtsConfig) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type SampleRateFn = unsafe extern "C" fn(*const c_void) -> i32;
type GenerateFn = unsafe extern "C" fn(*const c_void, *const c_char, i32, c_float) -> *const GeneratedAudio;
type DestroyAudioFn = unsafe extern "C" fn(*const GeneratedAudio);

/// A Kokoro voice: its id (as the Apple apps name it), label and speaker number in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voice {
    pub id: &'static str,
    pub name: &'static str,
    pub speaker: i32,
}

/// The Apple apps' voices (NarrationRenderer.voices), in their order; `af_heart` is the default.
pub const VOICES: &[Voice] = &[
    Voice { id: "af_heart", name: "Heart · US female", speaker: 3 },
    Voice { id: "af_bella", name: "Bella · US female", speaker: 2 },
    Voice { id: "af_nicole", name: "Nicole · US female", speaker: 6 },
    Voice { id: "am_michael", name: "Michael · US male", speaker: 16 },
    Voice { id: "am_fenrir", name: "Fenrir · US male", speaker: 14 },
    Voice { id: "bf_emma", name: "Emma · UK female", speaker: 21 },
    Voice { id: "bm_george", name: "George · UK male", speaker: 26 },
    Voice { id: "bm_fable", name: "Fable · UK male", speaker: 25 },
];

pub const DEFAULT_VOICE: &str = "af_heart";

pub fn voice(id: &str) -> &'static Voice {
    VOICES.iter().find(|v| v.id == id).unwrap_or(&VOICES[0])
}

/// American or British English: the voice id's first letter (a…, b…), as in Kokoro.
fn british(id: &str) -> bool {
    id.starts_with('b')
}

/// The loaded model for one accent.
pub struct Kokoro {
    tts: *const c_void,
    destroy: DestroyFn,
    generate: GenerateFn,
    destroy_audio: DestroyAudioFn,
    pub sample_rate: u32,
    british: bool,
    // keeps the library (and the function pointers above) loaded; dropped last
    _libs: Vec<Library>,
}

// sherpa-onnx's OfflineTts may be used from any thread, one call at a time (the owner holds it in a Mutex).
unsafe impl Send for Kokoro {}

impl Drop for Kokoro {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.tts) }
    }
}

/// File names of the C API library and what it needs, per platform.
fn lib_names() -> (&'static str, &'static [&'static str]) {
    if cfg!(target_os = "windows") {
        ("sherpa-onnx-c-api.dll", &["onnxruntime_providers_shared.dll", "onnxruntime.dll"])
    } else if cfg!(target_os = "macos") {
        ("libsherpa-onnx-c-api.dylib", &["libonnxruntime.dylib"])
    } else {
        ("libsherpa-onnx-c-api.so", &["libonnxruntime.so"])
    }
}

unsafe fn open(path: &Path) -> std::result::Result<Library, libloading::Error> {
    #[cfg(windows)]
    {
        // look for its DLLs next to it first (Windows 11 has an onnxruntime.dll of its own in System32)
        let flags = libloading::os::windows::LOAD_WITH_ALTERED_SEARCH_PATH;
        libloading::os::windows::Library::load_with_flags(path, flags).map(Into::into)
    }
    #[cfg(not(windows))]
    {
        Library::new(path)
    }
}

impl Kokoro {
    /// Loads the model in `model_dir` for the accent of `voice_id`. `lib_dir`: the folder with
    /// sherpa-onnx's libraries, or None to find them by name (Android: in the APK).
    pub fn load(lib_dir: Option<&Path>, model_dir: &Path, voice_id: &str, threads: i32) -> Result<Kokoro> {
        let (main, deps) = lib_names();
        let mut libs = Vec::new();
        let lib = unsafe {
            match lib_dir {
                Some(d) => {
                    // its dependencies first, from the same folder, so the right copies are used
                    for dep in deps {
                        let p = d.join(dep);
                        if p.is_file() {
                            libs.push(open(&p).map_err(|e| CoreError::Invalid(format!("couldn't load {dep}: {e}")))?);
                        }
                    }
                    open(&d.join(main))
                }
                None => Library::new(main),
            }
        }
        .map_err(|e| CoreError::Invalid(format!("couldn't load the speech engine ({main}): {e}")))?;
        let british = british(voice_id);
        let file = |name: &str| -> Result<CString> {
            let p: PathBuf = model_dir.join(name);
            if !p.is_file() {
                return Err(CoreError::NotFound(format!("natural voice file {}", p.display())));
            }
            CString::new(p.to_string_lossy().as_bytes()).map_err(|_| CoreError::Invalid("path".into()))
        };
        let model = file(super::install::MODEL_FILE)?;
        let voices = file("voices.bin")?;
        let tokens = file("tokens.txt")?;
        let lexicon = file(if british { "lexicon-gb-en.txt" } else { "lexicon-us-en.txt" })?;
        let data_dir = model_dir.join("espeak-ng-data");
        if !data_dir.is_dir() {
            return Err(CoreError::NotFound(format!("natural voice folder {}", data_dir.display())));
        }
        let data_dir = CString::new(data_dir.to_string_lossy().as_bytes()).map_err(|_| CoreError::Invalid("path".into()))?;
        let provider = CString::new("cpu").unwrap();
        unsafe {
            let create: CreateFn = *lib.get::<CreateFn>(b"SherpaOnnxCreateOfflineTts\0").map_err(sym)?;
            let destroy: DestroyFn = *lib.get::<DestroyFn>(b"SherpaOnnxDestroyOfflineTts\0").map_err(sym)?;
            let rate: SampleRateFn = *lib.get::<SampleRateFn>(b"SherpaOnnxOfflineTtsSampleRate\0").map_err(sym)?;
            let generate: GenerateFn = *lib.get::<GenerateFn>(b"SherpaOnnxOfflineTtsGenerate\0").map_err(sym)?;
            let destroy_audio: DestroyAudioFn = *lib.get::<DestroyAudioFn>(b"SherpaOnnxDestroyOfflineTtsGeneratedAudio\0").map_err(sym)?;
            let mut cfg: TtsConfig = std::mem::zeroed();
            cfg.model.kokoro.model = model.as_ptr();
            cfg.model.kokoro.voices = voices.as_ptr();
            cfg.model.kokoro.tokens = tokens.as_ptr();
            cfg.model.kokoro.lexicon = lexicon.as_ptr();
            cfg.model.kokoro.data_dir = data_dir.as_ptr();
            cfg.model.kokoro.length_scale = 1.0;
            cfg.model.num_threads = threads.max(1);
            cfg.model.provider = provider.as_ptr();
            cfg.max_num_sentences = 1;
            cfg.silence_scale = 0.2;
            let tts = create(&cfg);
            if tts.is_null() {
                return Err(CoreError::Invalid("the natural voice couldn't start (model files damaged? delete and download it again)".into()));
            }
            let sample_rate = rate(tts).max(1) as u32;
            libs.push(lib);
            Ok(Kokoro { tts, destroy, generate, destroy_audio, sample_rate, british, _libs: libs })
        }
    }

    /// Is this model loaded for the accent of `voice_id`?
    pub fn fits(&self, voice_id: &str) -> bool {
        self.british == british(voice_id)
    }

    /// Speech for `text` in the voice `voice_id` at `speed` (1.0 = normal): mono samples at
    /// `sample_rate`, from -1 to 1.
    pub fn synthesize(&self, text: &str, voice_id: &str, speed: f32) -> Result<Vec<f32>> {
        let text = CString::new(text.replace('\0', " ")).unwrap();
        let sid = voice(voice_id).speaker;
        unsafe {
            let a = (self.generate)(self.tts, text.as_ptr(), sid, if speed > 0.0 { speed } else { 1.0 });
            if a.is_null() {
                return Err(CoreError::Invalid("the natural voice made no audio".into()));
            }
            let out = if (*a).samples.is_null() || (*a).n <= 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts((*a).samples, (*a).n as usize).to_vec()
            };
            (self.destroy_audio)(a);
            Ok(out)
        }
    }
}

fn sym(e: libloading::Error) -> CoreError {
    CoreError::Invalid(format!("the speech engine is not the expected version: {e}"))
}

/// A 16-bit PCM WAV file of `samples` (mono, `rate` Hz), with silence before and after (seconds).
pub fn wav(samples: &[f32], rate: u32, before: f32, after: f32) -> Vec<u8> {
    let pad = |s: f32| (s.max(0.0) * rate as f32) as usize;
    let n = pad(before) + samples.len() + pad(after);
    let data_len = (n * 2) as u32;
    let mut out = Vec::with_capacity(44 + n * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(out.len() + pad(before) * 2, 0);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.resize(out.len() + pad(after) * 2, 0);
    out
}

/// Silence around a narration item, as the Mac's renderer puts it (lunta_tts.py PAUSE): seconds
/// before and after.
pub fn pauses(kind: &str) -> (f32, f32) {
    match kind {
        "heading" => (0.55, 0.45),
        "caption" => (0.2, 0.4),
        _ => (0.0, 0.35),
    }
}
