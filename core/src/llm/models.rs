//! The open models the apps download: Qwen3 (Apache-2.0) as GGUF, 4-bit (Q4_K_M), pinned to a
//! revision of the repository on Hugging Face and checked against its SHA-256 (the LFS object id
//! Hugging Face lists for the file).

/// One downloadable model file.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AiModel {
    /// What a summary made with it is marked with in summary.json ("qwen3-4b", as the Apple apps
    /// write for their Qwen3 4B; "qwen3-1.7b").
    pub id: String,
    /// "Qwen3 4B"
    pub name: String,
    pub file_name: String,
    pub url: String,
    pub size: u64,
    /// lower-case hex
    pub sha256: String,
    /// Tokens of context the apps use with it.
    pub context: u32,
}

fn hf(repo: &str, revision: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/{revision}/{file}")
}

/// Windows and Linux: Qwen3 4B, Qwen's own GGUF (Qwen/Qwen3-4B-GGUF), 2.5 GB.
pub fn desktop_model() -> AiModel {
    AiModel {
        id: "qwen3-4b".into(),
        name: "Qwen3 4B".into(),
        file_name: "Qwen3-4B-Q4_K_M.gguf".into(),
        url: hf("Qwen/Qwen3-4B-GGUF", "bc640142c66e1fdd12af0bd68f40445458f3869b", "Qwen3-4B-Q4_K_M.gguf"),
        size: 2_497_280_256,
        sha256: "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5".into(),
        context: 4096,
    }
}

/// Android: Qwen3 1.7B, 1.3 GB. Qwen publishes the 1.7B only as Q8_0 (1.8 GB, slower on a
/// phone); this Q4_K_M is llama.cpp's own conversion (ggml-org/Qwen3-1.7B-GGUF).
pub fn phone_model() -> AiModel {
    AiModel {
        id: "qwen3-1.7b".into(),
        name: "Qwen3 1.7B".into(),
        file_name: "Qwen3-1.7B-Q4_K_M.gguf".into(),
        url: hf("ggml-org/Qwen3-1.7B-GGUF", "daeb8e2d528a760970442092f6bf1e55c3b659eb", "Qwen3-1.7B-Q4_K_M.gguf"),
        size: 1_282_439_264,
        sha256: "d2387ca2dbfee2ffabce7120d3770dadca0b293052bc2f0e138fdc940d9bc7b5".into(),
        context: 4096,
    }
}

/// "2.5 GB"
pub fn size_label(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}
