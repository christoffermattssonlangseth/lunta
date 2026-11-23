//! cargo run --example csl_bib_check -- <library copy> <cache dir> <style id> > entries.tsv
//!
//! Every paper's reference list entry in a journal style, one per line (id, work type, journal,
//! authors, DOI, entry), to look over the formatting. Open a *copy* of the library (its ops/
//! folder is enough): opening writes the copy's index. The cache dir needs csl/<style>.csl (the
//! Mac's are in ~/Library/Application Support/Margins/csl).
use lunta_core::model::{PaperFilter, SortKey};
use lunta_core::Library;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = Library::open(a[1].clone(), a[2].clone(), "csl-check".into()).unwrap();
    let papers = lib.papers(PaperFilter { collection: None, tag: None, starred_only: false, unread_only: false, must_read_only: false }, SortKey::Added);
    let ids: Vec<Vec<String>> = papers.iter().map(|p| vec![p.id.clone()]).collect();
    let r = lib.cite_csl(a[2].clone(), a[3].clone(), ids).unwrap();
    for e in &r.bibliography {
        let p = papers.iter().find(|p| p.id == e.id).unwrap();
        println!("{}\t{}\t{}\t{}\t{}\t{}", e.id, p.meta.work_type, p.meta.journal, p.meta.authors.join("; "), p.meta.doi, e.text);
    }
}
