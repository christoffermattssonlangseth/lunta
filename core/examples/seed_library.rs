//! Makes a scratch library from public papers, for testing the apps (e.g. as the "remote" of a
//! LocalDirRemote sync): cargo run --example seed_library -- <dir> <doi|link|arxiv-id>...
use lunta_core::model::*;
use lunta_core::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("library dir"));
    let cache = std::env::temp_dir().join(format!("lunta-seed-{}", std::process::id()));
    let lib = Library::open(dir.to_string_lossy().into(), cache.to_string_lossy().into(), "seed".into()).unwrap();
    for input in args {
        match lib.add_from_input(input.clone(), None) {
            Ok(o) => println!("added {} ({}){}", o.paper.meta.title, o.paper.id, o.note.map(|n| format!(" - no reading view: {n}")).unwrap_or_default()),
            Err(e) => println!("failed {input}: {e}"),
        }
    }
    let first = lib.papers(PaperFilter::default(), SortKey::Added);
    if let Some(p) = first.first() {
        let f = lib.create_collection("Test folder".into(), None).unwrap();
        lib.set_membership(f.id, p.id.clone(), true).unwrap();
        lib.set_tag(p.id.clone(), "demo".into(), true).unwrap();
        lib.set_starred(p.id.clone(), true).unwrap();
    }
    println!("{} papers in {}", lib.papers(PaperFilter::default(), SortKey::Added).len(), dir.display());
    let _ = std::fs::remove_dir_all(cache);
}
