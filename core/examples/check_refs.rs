//! cargo run --example check_refs -- <refs.bib | refs.ris | list.txt> [report.md] [report.csv]
//!
//! Checks a reference list against Crossref, DataCite, OpenAlex and PubMed (what Check a Reference
//! List does in the apps) and prints the report as Markdown. Reads the file only; asks the services
//! politely (answers kept a day in the temp folder).
use lunta_core::checks::refcheck::check_references;
use lunta_core::ProgressListener;
use std::sync::Arc;

struct Print;
impl ProgressListener for Print {
    fn on_progress(&self, message: String) {
        eprintln!("{message}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(args.get(1).expect("a .bib, .ris or text file")).unwrap();
    let cache = std::env::temp_dir().join("lunta-check-refs");
    let r = check_references(text, cache.to_string_lossy().into(), Some(Arc::new(Print)));
    println!("{}", r.markdown);
    if let Some(md) = args.get(2) {
        std::fs::write(md, &r.markdown).unwrap();
    }
    if let Some(csv) = args.get(3) {
        std::fs::write(csv, &r.csv).unwrap();
    }
}
