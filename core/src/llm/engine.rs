//! The model runtime: llama.cpp through the llama-cpp-2 crate, on the CPU (Metal on a Mac, where
//! llama.cpp turns it on by itself; the desktop app runs there only for testing). One model is
//! loaded at a time and kept loaded; generations run one at a time.

use super::AiError;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn backend() -> Result<&'static LlamaBackend, AiError> {
    static B: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
    B.get_or_init(|| {
        LlamaBackend::init().map(|mut b| {
            // llama.cpp logs every tensor it loads to stderr; LUNTA_LLM_LOG=1 keeps that
            if std::env::var_os("LUNTA_LLM_LOG").is_none() {
                b.void_logs();
            }
            b
        })
        .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| AiError::Model(e.clone()))
}

/// Can this computer run the model at all? (None: yes; else why not.) llama.cpp is built for
/// x86-64 with AVX2 (every PC since about 2015) and, on Android, for ARMv8.2 with dot products
/// (phones since about 2018); older processors would crash in it.
pub fn unsupported_reason() -> Option<String> {
    // (Android's x86_64 build, for emulators, is cross-compiled for plain x86-64: no check)
    #[cfg(all(target_arch = "x86_64", not(target_os = "android")))]
    {
        if !std::is_x86_feature_detected!("avx2") || !std::is_x86_feature_detected!("fma") {
            return Some("On-device AI needs a processor with AVX2 (most PCs made since 2015); this one doesn't have it.".into());
        }
    }
    #[cfg(all(target_arch = "aarch64", target_os = "android"))]
    {
        if !std::arch::is_aarch64_feature_detected!("dotprod") {
            return Some("On-device AI needs a newer processor (ARMv8.2 with dot-product instructions); this phone's doesn't have them.".into());
        }
    }
    None
}

fn cpu_only() -> bool {
    std::env::var_os("LUNTA_LLM_CPU").is_some()
}

/// Settings for one generation.
#[derive(Clone, Debug)]
pub struct GenOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    /// Tokens of context (prompt and answer together).
    pub context: u32,
}

/// A loaded model.
pub struct Engine {
    model: LlamaModel,
    path: PathBuf,
    threads: i32,
    /// one generation at a time (the context and its memory are made per generation)
    busy: Mutex<()>,
}

/// Threads for llama.cpp: the performance cores, roughly. On a phone the big cores (4 of 8); on a
/// PC half the logical processors (hyper-threads don't help), at most 8.
pub fn default_threads() -> i32 {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) as i32;
    if cfg!(target_os = "android") {
        n.clamp(1, 4)
    } else if n <= 4 {
        n
    } else {
        (n / 2).clamp(4, 8)
    }
}

impl Engine {
    /// Loads a GGUF model (a few seconds; the file is memory-mapped, not copied).
    pub fn load(path: &Path, threads: Option<i32>) -> Result<Engine, AiError> {
        if let Some(why) = unsupported_reason() {
            return Err(AiError::Unsupported(why));
        }
        if !path.is_file() {
            return Err(AiError::Model(format!("no model at {}", path.display())));
        }
        let b = backend()?;
        // LUNTA_LLM_CPU=1: no GPU even where llama.cpp has one (Metal on a Mac), to time the CPU
        let params = if cpu_only() { LlamaModelParams::default().with_n_gpu_layers(0) } else { LlamaModelParams::default() };
        let model = LlamaModel::load_from_file(b, path, &params).map_err(|e| AiError::Model(format!("couldn't load the model: {e}")))?;
        Ok(Engine { model, path: path.to_path_buf(), threads: threads.filter(|t| *t > 0).unwrap_or_else(default_threads), busy: Mutex::new(()) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The model's own chat template uses ChatML (Qwen and most others): prompt::chatml fits it.
    pub fn uses_chatml(&self) -> bool {
        self.model.chat_template(None).ok().and_then(|t| t.to_string().ok()).map_or(true, |t| t.contains("<|im_start|>"))
    }

    /// Tokens a text takes.
    pub fn count_tokens(&self, text: &str) -> usize {
        self.model.vocab().tokenize(text.as_bytes(), false, true).len()
    }

    /// Runs the model on a prompt (already in the chat template) and returns the text it wrote.
    /// `on_text` gets the whole text so far after every token; returning false stops it there.
    pub fn generate(&self, prompt: &str, opts: &GenOptions, on_text: &mut dyn FnMut(&str) -> bool) -> Result<String, AiError> {
        let _one = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        let b = backend()?;
        let vocab = self.model.vocab();
        let tokens = vocab.tokenize(prompt.as_bytes(), false, true);
        let n_ctx = opts.context.max(512);
        if tokens.len() as u32 + opts.max_tokens.min(64) >= n_ctx {
            return Err(AiError::TooLong(format!("the prompt takes {} tokens, more than the model's {n_ctx}", tokens.len())));
        }
        let max_new = opts.max_tokens.min(n_ctx - tokens.len() as u32 - 1);
        let n_batch = 512u32;
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(n_batch)
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads)
            .with_offload_kqv(!cpu_only());
        let mut ctx = self.model.new_context(b, params).map_err(|e| AiError::Model(format!("couldn't make a context: {e}")))?;

        // the prompt, in batches; logits only for its last token
        let mut batch = LlamaBatch::new(n_batch as usize, 1);
        let last = tokens.len() - 1;
        for (start, chunk) in tokens.chunks(n_batch as usize).enumerate() {
            batch.clear();
            for (k, t) in chunk.iter().enumerate() {
                let i = start * n_batch as usize + k;
                batch.add(*t, i as i32, &[0], i == last).map_err(|e| AiError::Model(e.to_string()))?;
            }
            ctx.decode(&mut batch).map_err(|e| AiError::Model(format!("couldn't read the prompt: {e}")))?;
        }

        // Qwen3's settings without thinking (top-k 20, top-p 0.8), at the temperature asked for
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
        let mut sampler = if opts.temperature <= 0.0 {
            LlamaSampler::greedy()
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::penalties(self.model.n_vocab(), 64, 1.05, 0.0, 0.0),
                LlamaSampler::top_k(20),
                LlamaSampler::top_p(0.8, 1),
                LlamaSampler::temp(opts.temperature),
                LlamaSampler::dist(seed),
            ])
        };
        let mut bytes: Vec<u8> = Vec::new();
        let mut piece = Vec::new();
        let mut pos = tokens.len() as i32;
        let mut logits_at = batch.n_tokens() - 1;
        for _ in 0..max_new {
            let token = sampler.sample(&ctx, logits_at);
            sampler.accept(token);
            if vocab.is_eog(token) {
                break;
            }
            piece.clear();
            vocab.token_to_piece_into(token, &mut piece, false, None);
            bytes.extend_from_slice(&piece);
            // a character can take several tokens: show only the complete ones
            let valid = match std::str::from_utf8(&bytes) {
                Ok(s) => s,
                Err(e) => std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or(""),
            };
            if !on_text(valid) {
                return Ok(valid.to_string());
            }
            batch.clear();
            batch.add(token, pos, &[0], true).map_err(|e| AiError::Model(e.to_string()))?;
            pos += 1;
            logits_at = 0;
            ctx.decode(&mut batch).map_err(|e| AiError::Model(format!("generation failed: {e}")))?;
        }
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}
