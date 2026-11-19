//! cargo run --example bib_keep_keys -- <refs.bib> <out.bib> [folder name]
//!
//! Imports a LaTeX project's .bib into a throwaway library (in the temp folder, never yours) with
//! its citation keys, looking entries up online, and writes the .bib Lunta would keep for the
//! folder to <out.bib>. Prints the import report.
use lunta_core::model::*;
use lunta_core::*;
use std::sync::Arc;

struct Print;
impl ProgressListener for Print {
    fn on_progress(&self, message: String) {
        eprintln!("{message}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (src, out) = (args.get(1).expect("refs.bib"), args.get(2).expect("out.bib"));
    let folder = args.get(3).cloned().unwrap_or_else(|| "Imported".into());
    let tmp = std::env::temp_dir().join(format!("lunta-bib-{}", std::process::id()));
    let lib = Library::open(tmp.join("lib").to_string_lossy().into(), tmp.join("cache").to_string_lossy().into(), "bib".into()).unwrap();
    let c = lib.create_collection(folder, None).unwrap();
    let text = std::fs::read_to_string(src).unwrap();
    let r = lib.import_bibtex_keep_keys(text, c.id.clone(), true, Some(Arc::new(Print))).unwrap();
    println!("{}", r.text);
    std::fs::write(out, lib.kept_bibtex(Some(c.id)).unwrap_or_default()).unwrap();
    let _ = lib.papers(PaperFilter::default(), SortKey::Added);
    std::fs::remove_dir_all(&tmp).ok();
}
