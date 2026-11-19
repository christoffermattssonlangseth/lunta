//! Measures the natural voice (Kokoro-82M through sherpa-onnx) on this computer: load time,
//! time to the first sentence, and the real-time factor (seconds of work per second of speech).
//!
//!   cargo run --release --example tts_bench -- <voice folder> [voice] [threads]
//!
//! Downloads the voice into the folder first if it isn't there (the same pinned files as the apps).
use lunta_core::tts::{engine::Kokoro, install, text, NaturalVoice};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const PARAGRAPH: &str = "Bacterial meningitis remains a medical emergency with high mortality worldwide [1–3]. \
Cerebrospinal fluid (CSF) lactate has been proposed as a rapid marker to distinguish bacterial from viral meningitis \
(Sakushima et al., 2011). In this cohort of 352 patients, a CSF lactate above 3.5 mmol/L had a sensitivity of 93% \
and a specificity of 96%, i.e. it performed better than the CSF to blood glucose ratio. Fig. 2 shows the receiver \
operating characteristic curves for both markers, with 95% confidence intervals.";

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: tts_bench <voice folder> [voice] [threads]");
    let voice = args.next().unwrap_or_else(|| "af_heart".into());
    let threads: i32 = args.next().and_then(|t| t.parse().ok()).unwrap_or(0);
    let v = NaturalVoice::new(root.clone());
    if !v.status().installed {
        eprintln!("downloading the natural voice into {root} ({} MB)…", v.status().download_bytes / 1_000_000);
        let t = Instant::now();
        install::install(std::path::Path::new(&root), &|d, n| eprint!("\r{} / {} MB", d / 1_000_000, n / 1_000_000), &AtomicBool::new(false)).unwrap();
        eprintln!("\ndownloaded in {:.0} s", t.elapsed().as_secs_f32());
    }
    eprintln!("on disk: {} MB", v.status().disk_bytes / 1_000_000);

    let threads = if threads > 0 { threads } else { (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(1)).clamp(1, 4) as i32 };
    let lib_dir = install::engine_package().map(|p| install::package_dir(std::path::Path::new(&root), &p));
    let t = Instant::now();
    let k = Kokoro::load(lib_dir.as_deref(), &install::package_dir(std::path::Path::new(&root), &install::MODEL), &voice, threads).unwrap();
    println!("load: {:.2} s ({threads} threads)", t.elapsed().as_secs_f32());

    let said = text::spoken(PARAGRAPH, "text");
    let pieces = text::chunks(&said, 300);
    let (mut work, mut audio) = (0f32, 0f32);
    let mut all = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let t = Instant::now();
        let s = k.synthesize(p, &voice, 1.0).unwrap();
        let w = t.elapsed().as_secs_f32();
        let a = s.len() as f32 / k.sample_rate as f32;
        println!("piece {} ({} chars): {:.2} s for {:.2} s of speech (RTF {:.2})", i + 1, p.chars().count(), w, a, w / a);
        if i > 0 || pieces.len() == 1 {
            work += w;
            audio += a;
        }
        all.extend(s);
    }
    println!("first piece ready in the time above; the rest: RTF {:.2} ({:.1}x faster than real time)", work / audio, audio / work);
    let out = std::env::temp_dir().join("lunta-tts-bench.wav");
    std::fs::write(&out, lunta_core::tts::engine::wav(&all, k.sample_rate, 0.0, 0.3)).unwrap();
    println!("wrote {}", out.display());
}
