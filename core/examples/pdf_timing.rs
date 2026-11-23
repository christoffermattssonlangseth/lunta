//! Times the add → PDF steps for a few papers (network): metadata (Crossref, Europe PMC),
//! the open-access PDF candidates, and the download. No library is touched.
//!
//!   cargo run --release --example pdf_timing -- [DOI | arXiv id | PMCID …]

use lunta_core::metadata;
use std::time::Instant;

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = [
            "10.1038/s41592-019-0686-2",  // Nature Methods (OA, in PMC)
            "10.1101/2020.03.22.002386",  // bioRxiv preprint
            "10.1016/j.cell.2020.02.052", // Cell (OA, in PMC)
            "arXiv:1706.03762",           // arXiv
            "PMC4718556",                 // PMC
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
    }
    for id in ids {
        println!("== {id}");
        let t = Instant::now();
        let m = match metadata::resolve(&id) {
            Ok(m) => m,
            Err(e) => {
                println!("  metadata failed: {e}");
                continue;
            }
        };
        println!("  metadata (resolve)      {:>6} ms  {}", t.elapsed().as_millis(), m.title.chars().take(50).collect::<String>());
        let t = Instant::now();
        let cands = metadata::pdf_candidates(&m);
        println!("  candidates              {:>6} ms  {} found", t.elapsed().as_millis(), cands.len());
        for c in &cands {
            println!("    {} [{}] {}", c.label, c.version, c.url);
        }
        let t = Instant::now();
        let r = metadata::find_first_pdf(&cands);
        match r {
            Some((c, d)) => println!("  download (first valid)  {:>6} ms  {} ({} KB)", t.elapsed().as_millis(), c.label, d.len() / 1024),
            None => println!("  download (first valid)  {:>6} ms  none", t.elapsed().as_millis()),
        }
        let t = Instant::now();
        let r = metadata::find_pdf_for(&m);
        match r {
            Some((c, d)) => println!("  find_pdf_for (total)    {:>6} ms  {} ({} KB)", t.elapsed().as_millis(), c.label, d.len() / 1024),
            None => println!("  find_pdf_for (total)    {:>6} ms  none", t.elapsed().as_millis()),
        }
    }
}
