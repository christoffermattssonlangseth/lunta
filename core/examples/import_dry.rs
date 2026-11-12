//! cargo run --release --example import_dry -- <file.bib> : imports into a throwaway library
use lunta_core::*;
use lunta_core::model::*;
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let tmp = std::env::temp_dir().join(format!("lunta-dry-{}", std::process::id()));
    let lib = Library::open(tmp.join("lib").to_string_lossy().into(), tmp.join("cache").to_string_lossy().into(), "dry".into()).unwrap();
    let t = std::time::Instant::now();
    let entries = parse_import(std::fs::read_to_string(&path).unwrap());
    let r = lib.import_entries(entries).unwrap();
    println!("import: added {} merged {} skipped {} in {:.1?}", r.added, r.merged, r.skipped, t.elapsed());
    for q in ["in situ sequencing", "oligodendrocyte multiple sclerosis", "\"spatial transcriptomics\"", "Langseth"] {
        let t = std::time::Instant::now();
        let hits = lib.search(q.into(), 200).unwrap();
        println!("search {q:40} {:4} hits in {:>6.1?}  top: {}", hits.len(), t.elapsed(), hits.first().map(|h| h.paper.meta.title.chars().take(60).collect::<String>()).unwrap_or_default());
    }
    let t = std::time::Instant::now();
    let lib2 = Library::open(tmp.join("lib").to_string_lossy().into(), tmp.join("cache").to_string_lossy().into(), "dry".into()).unwrap();
    println!("reopen with {} papers in {:.1?}", lib2.papers(PaperFilter::default(), SortKey::Added).len(), t.elapsed());
    let ids: Vec<String> = lib2.papers(PaperFilter::default(), SortKey::Added).into_iter().take(1).map(|p| p.id).collect();
    println!("{}", lib2.citation(ids[0].clone(), CitationStyle::Apa).unwrap());
    std::fs::remove_dir_all(&tmp).ok();
}
