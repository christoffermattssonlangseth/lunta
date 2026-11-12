//! cargo run --example import_preview -- <file.bib|file.ris>
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let text = std::fs::read_to_string(&path).unwrap();
    let e = lunta_core::parse_import(text);
    let n = e.len();
    let c = |f: &dyn Fn(&lunta_core::import::ImportEntry) -> bool| e.iter().filter(|x| f(x)).count();
    println!("entries {n}");
    println!("  title {}  authors {}  doi {}  pmid {}  pmcid {}  arxiv {}  abstract {}  date {}",
        c(&|x| !x.meta.title.is_empty()), c(&|x| !x.meta.authors.is_empty()), c(&|x| !x.meta.doi.is_empty()),
        c(&|x| !x.meta.pmid.is_empty()), c(&|x| !x.meta.pmcid.is_empty()), c(&|x| !x.meta.arxiv.is_empty()),
        c(&|x| !x.meta.abstract_text.is_empty()), c(&|x| !x.meta.date.is_empty()));
    let leftovers: Vec<_> = e.iter().filter(|x| x.meta.title.contains('\\') || x.meta.title.contains('{')
        || x.meta.authors.iter().any(|a| a.contains('\\') || a.contains('{'))).collect();
    println!("  with LaTeX leftovers: {}", leftovers.len());
    for x in leftovers.iter().take(5) { println!("    {} | {:?}", x.meta.title, x.meta.authors.first()); }
    let mut dup = std::collections::HashMap::new();
    for x in &e { if !x.meta.doi.is_empty() { *dup.entry(x.meta.doi.to_lowercase()).or_insert(0) += 1; } }
    println!("  duplicate DOIs: {}", dup.values().filter(|v| **v > 1).count());
    for i in [0usize, n / 2, n - 1] {
        let m = &e[i].meta;
        println!("\n[{i}] {}\n    {} authors, first {:?} | {} | {} | doi {} | pmcid {} | kw {:?}",
            m.title, m.authors.len(), m.authors.first(), m.journal, m.date, m.doi, m.pmcid, m.keywords);
    }
}
