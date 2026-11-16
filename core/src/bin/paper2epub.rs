//! Command-line front end for the Rust paper → EPUB converter.
//!
//! Usage: paper2epub [--offline] <DOI | PMCID | PubMed ID | arXiv id | link | file.pdf | file.xml | page.webarchive> [output-dir]
//!
//! A PDF is converted by layout analysis (needs the PDFium library, see the README) unless an
//! open-access XML version is found through its DOI; `--offline` skips every network lookup.
//!
//! `--page <url>` with a saved HTML file converts it as the browser extension's "Use this page's
//! full text" does; `--images <dir>`: the figure images the browser would send (matched by file
//! name; an optional `urls.tsv` of "file<TAB>address" lines adds the addresses). A Safari
//! `.webarchive` is converted as a page with its images. `--fulltext <file>` writes the plain
//! full text (what Lunta stores as the paper's fulltext.txt).

use lunta_core::convert::{self, Options};
use std::path::{Path, PathBuf};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let offline = args.iter().any(|a| a == "--offline");
    args.retain(|a| a != "--offline");
    let mut take = |flag: &str| -> Option<String> {
        let i = args.iter().position(|a| a == flag)?;
        if i + 1 >= args.len() {
            return None;
        }
        let v = args.remove(i + 1);
        args.remove(i);
        Some(v)
    };
    let page_url = take("--page");
    let images_dir = take("--images");
    let fulltext = take("--fulltext");
    let Some(input) = args.first().cloned() else {
        eprintln!("usage: paper2epub [--offline] <DOI|PMCID|arXiv id|link|file.pdf|file.xml> [output-dir]");
        std::process::exit(2);
    };
    let out = args.get(1).map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap());
    let opts = Options { log: Some(Box::new(|m| eprintln!("{m}"))), offline };
    let r = if input.to_lowercase().ends_with(".webarchive") {
        match std::fs::read(&input) {
            Ok(d) => convert::convert_webarchive_with(&d, &out, &opts),
            Err(e) => Err(e.into()),
        }
    } else if let Some(url) = page_url {
        let data = std::fs::read(&input).unwrap_or_default();
        let html = String::from_utf8_lossy(&data).into_owned();
        let mut imgs = Vec::new();
        if let Some(dir) = images_dir {
            let dir = PathBuf::from(dir);
            let mut addr: std::collections::HashMap<String, Vec<String>> = Default::default();
            if let Ok(tsv) = std::fs::read_to_string(dir.join("urls.tsv")) {
                for line in tsv.lines() {
                    let p: Vec<&str> = line.split('\t').collect();
                    if p.len() >= 2 {
                        addr.entry(p[0].to_string()).or_default().extend(p[1..].iter().map(|s| s.to_string()));
                    }
                }
            }
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name == "urls.tsv" {
                    continue;
                }
                if let Ok(d) = std::fs::read(e.path()) {
                    let mut urls = vec![name.clone()];
                    urls.extend(addr.get(&name).cloned().unwrap_or_default());
                    imgs.push(convert::PageImage { urls, data: d });
                }
            }
        }
        eprintln!("reader: {}; {} image file(s) offered", convert::page_reader(&url, &html), imgs.len());
        convert::convert_page_with(&html, &url, imgs, &out, &opts)
    } else if Path::new(&input).exists() {
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
            if let Some(f) = fulltext {
                if let Err(e) = std::fs::write(&f, &r.fulltext) {
                    eprintln!("couldn't write {f}: {e}");
                }
            }
        }
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    }
}
