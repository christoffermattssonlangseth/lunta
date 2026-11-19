//! Trying the on-device AI (core/src/llm) by hand:
//!
//!   cargo run --release --features llm --example ai -- download desktop|phone <dir>
//!   cargo run --release --features llm --example ai -- summarize <model.gguf> <source.txt> [title]
//!   cargo run --release --features llm --example ai -- ask <model.gguf> <fulltext.txt> <question>
//!
//! <source.txt> is what the reader's luntaSummarySource gives for a paper
//! (scripts/summary-source.mjs <all.xhtml> prints it).

use lunta_core::llm::{ask, download, models, AiEngine};
use lunta_core::model::{Paper, PaperMeta};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_else(|| usage());
    match args.first().map(String::as_str) {
        Some("download") => {
            let m = if arg(1) == "phone" { models::phone_model() } else { models::desktop_model() };
            let started = Instant::now();
            let mut last = 0;
            let r = download::download(&m, Path::new(&arg(2)), &download::HttpFetch, &mut |d, t| {
                if d - last > 50_000_000 || d == t {
                    last = d;
                    eprint!("\r{} of {}", models::size_label(d), models::size_label(t));
                }
                true
            });
            eprintln!();
            match r {
                Ok(p) => println!("{} in {:.0} s (SHA-256 checked)", p.display(), started.elapsed().as_secs_f64()),
                Err(e) => {
                    eprintln!("failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some("summarize") => {
            let source = std::fs::read_to_string(arg(2)).expect("source text");
            let title = args.get(3).cloned().unwrap_or_default();
            let t0 = Instant::now();
            let e = AiEngine::load_model(Path::new(&arg(1)), "test", 4096, None).expect("model");
            eprintln!("loaded in {:.1} s; {} words of source", t0.elapsed().as_secs_f64(), source.split_whitespace().count());
            let t1 = Instant::now();
            let mut first: Option<f64> = None;
            let mut shown = 0;
            let s = e
                .write_summary(&title, &source, &mut |t| {
                    first.get_or_insert(t1.elapsed().as_secs_f64());
                    eprint!("{}", &t[shown.min(t.len())..]);
                    let _ = std::io::stderr().flush();
                    shown = t.len();
                    true
                })
                .expect("summary");
            eprintln!();
            println!(
                "first words after {:.1} s, done in {:.1} s\n{}",
                first.unwrap_or(0.0),
                t1.elapsed().as_secs_f64(),
                s.unwrap_or_else(|| "(not a summary)".into())
            );
        }
        Some("ask") => {
            let text = std::fs::read_to_string(arg(2)).expect("full text");
            let e = AiEngine::load_model(Path::new(&arg(1)), "test", 4096, None).expect("model");
            let paper = Paper {
                id: "p".into(),
                meta: PaperMeta { title: "The paper".into(), ..Default::default() },
                added: 0, modified: 0, has_pdf: false, has_epub: true, has_audio: false, read: true, starred: false,
                must_read: false, collections: vec![], tags: vec![], annotation_count: 0, cite_key: String::new(),
            };
            let p = ask::paper_prompt(&paper, &text, &[], &arg(3), &[]);
            for s in &p.sources {
                eprintln!("[{}] {}", s.n, ask::clip_for_display(&s.text));
            }
            let t = Instant::now();
            let a = e.write_answer(&p, &mut |_| true).expect("answer");
            println!("{a}\n({:.1} s)", t.elapsed().as_secs_f64());
        }
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!("usage: ai download desktop|phone <dir> | summarize <model.gguf> <source.txt> [title] | ask <model.gguf> <fulltext.txt> <question>");
    std::process::exit(2)
}
