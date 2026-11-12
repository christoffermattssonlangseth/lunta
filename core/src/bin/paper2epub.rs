//! Command-line front end for the Rust paper → EPUB converter.
//!
//! Usage: paper2epub [--offline] <DOI | PMCID | PubMed ID | arXiv id | link | file.xml> [output-dir]

use margins_core::convert::{self, Options};
use std::path::{Path, PathBuf};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let offline = args.iter().any(|a| a == "--offline");
    args.retain(|a| a != "--offline");
    let Some(input) = args.first().cloned() else {
        eprintln!("usage: paper2epub [--offline] <DOI|PMCID|arXiv id|link|file.xml> [output-dir]");
        std::process::exit(2);
    };
    let out = args.get(1).map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap());
    let opts = Options { log: Some(Box::new(|m| eprintln!("{m}"))), offline };
    let r = if Path::new(&input).exists() {
        convert::convert_file_with(Path::new(&input), &out, &opts)
    } else {
        convert::convert_with(&input, &out, &opts)
    };
    match r {
        Ok(r) => {
            println!("✓ {}", r.file.display());
            println!("  source: {}   words: {}   images: {}", r.source, r.words, r.images);
            for n in &r.notes {
                println!("  note: {n}");
            }
        }
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    }
}
