use super::*;

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("lunta-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn open(root: &PathBuf, device: &str) -> Arc<Library> {
    let cache = root.parent().unwrap().join(format!("{}-cache-{device}", root.file_name().unwrap().to_string_lossy()));
    Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), device.into()).unwrap()
}

fn sample() -> PaperMeta {
    PaperMeta {
        title: "Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology".into(),
        authors: vec!["Petra Kukanja".into(), "Christoffer M. Langseth".into(), "Gonçalo Castelo-Branco".into()],
        date: "2024-04-11".into(),
        journal: "Cell".into(),
        volume: "187".into(),
        issue: "8".into(),
        pages: "1990-2009".into(),
        doi: "10.1016/j.cell.2024.02.030".into(),
        ..Default::default()
    }
}

#[test]
fn two_devices_sync_through_shared_folder() {
    let root = tmp().join("lib");
    let mac = open(&root, "mac");
    let p = mac
        .add_paper(sample(), Some(b"%PDF-1.7 fake".to_vec()), None, Some("in situ sequencing of oligodendrocytes".into()))
        .unwrap();
    assert!(p.has_pdf && !p.has_epub);

    // the phone opens the same (synced) folder
    let phone = open(&root, "phone");
    assert_eq!(phone.papers(PaperFilter::default(), SortKey::Added).len(), 1);
    // full text is indexed in the background on the second device
    assert!(phone.search("oligodendrocytes".into(), 10).unwrap().is_empty());
    assert_eq!(phone.index_fulltext(50).unwrap(), 1);
    assert_eq!(phone.index_fulltext(50).unwrap(), 0);
    assert_eq!(phone.search("oligodendrocytes".into(), 10).unwrap().len(), 1);
    let a = phone
        .save_annotation(Annotation {
            id: String::new(),
            paper_id: p.id.clone(),
            kind: AnnotationKind::Highlight,
            color: "yellow".into(),
            location: "epub:ch05.xhtml".into(),
            position: "{}".into(),
            quote: "centrifugal manner".into(),
            prefix: String::new(),
            suffix: String::new(),
            note: "key result".into(),
            created: 0,
            modified: 0,
        })
        .unwrap();
    phone.set_tag(p.id.clone(), "MS".into(), true).unwrap();
    let col = phone.create_collection("KAW AI4S".into(), None).unwrap();
    phone.set_membership(col.id.clone(), p.id.clone(), true).unwrap();

    assert!(mac.refresh().unwrap());
    mac.reindex().unwrap();
    let anns = mac.annotations(p.id.clone());
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].id, a.id);
    let mp = mac.paper(p.id.clone()).unwrap();
    assert_eq!(mp.tags, vec!["MS".to_string()]);
    assert_eq!(mp.collections, vec![col.id.clone()]);
    assert_eq!(mac.collections()[0].paper_count, 1);
    // annotation text is searchable on the other device
    assert_eq!(mac.search("\"key result\"".into(), 10).unwrap().len(), 1);
    // nothing new: refresh is a no-op
    assert!(!mac.refresh().unwrap());

    // later edit wins
    let mut m = sample();
    m.title = "Edited on the phone".into();
    std::thread::sleep(std::time::Duration::from_millis(5));
    phone.update_meta(p.id.clone(), m).unwrap();
    mac.refresh().unwrap();
    assert_eq!(mac.paper(p.id.clone()).unwrap().meta.title, "Edited on the phone");

    // delete propagates
    mac.delete_paper(p.id.clone()).unwrap();
    phone.refresh().unwrap();
    assert!(phone.papers(PaperFilter::default(), SortKey::Added).is_empty());
    assert!(phone.search("oligodendrocytes".into(), 10).unwrap().is_empty());
}

#[test]
fn duplicate_doi_attaches_instead_of_adding() {
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let a = lib.add_paper(sample(), None, None, None).unwrap();
    let mut m2 = PaperMeta { doi: "https://doi.org/10.1016/J.CELL.2024.02.030".into(), ..Default::default() };
    m2.pmid = "38513664".into();
    let b = lib.add_paper(m2, None, Some(b"PK epub".to_vec()), None).unwrap();
    assert_eq!(a.id, b.id);
    assert!(b.has_epub);
    assert_eq!(b.meta.pmid, "38513664");
    assert_eq!(b.meta.title, sample().title);
    assert_eq!(lib.papers(PaperFilter::default(), SortKey::Added).len(), 1);
}

#[test]
fn full_text_search_with_snippets() {
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    lib.add_paper(sample(), None, None, Some("We used in situ sequencing to map oligodendrocyte states across lesions.".into())).unwrap();
    let mut other = sample();
    other.doi = "10.1/xyz".into();
    other.title = "Optimal transport for single cells".into();
    lib.add_paper(other, None, None, Some("Wasserstein distances between distributions.".into())).unwrap();

    let hits = lib.search("oligodendro".into(), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].snippet.contains('['), "snippet: {}", hits[0].snippet);
    assert_eq!(lib.search("transport wasserstein".into(), 10).unwrap().len(), 1);
    assert_eq!(lib.search("Gonçalo".into(), 10).unwrap().len(), 2); // author field, diacritics
    assert_eq!(lib.search("goncalo".into(), 10).unwrap().len(), 2);
    assert!(lib.search("\"no such phrase\"".into(), 10).unwrap().is_empty());
    assert!(lib.search("   ".into(), 10).unwrap().is_empty());
    assert!(lib.search("AND OR (".into(), 10).is_ok());
}

#[test]
fn citations_and_exports() {
    let p = Paper {
        id: "x".into(),
        meta: sample(),
        added: 0,
        modified: 0,
        has_pdf: false,
        has_epub: false,
        has_audio: false,
        read: false,
        starred: false,
        must_read: false,
        collections: vec![],
        tags: vec![],
        annotation_count: 0,
    };
    let bib = export::bibtex(&[p.clone(), p.clone()]);
    assert!(bib.contains("@article{Kukanja2024cellular,"), "{bib}");
    assert!(bib.contains("@article{Kukanja2024cellularb,"));
    assert!(bib.contains("author = {Kukanja, Petra and Langseth, Christoffer M. and Castelo-Branco, Gonçalo}"));
    assert!(bib.contains("pages = {1990--2009}"));
    let ris = export::ris(&[p.clone()]);
    assert!(ris.contains("AU  - Langseth, Christoffer M.\r\n") && ris.contains("SP  - 1990\r\n") && ris.contains("EP  - 2009"));
    assert_eq!(
        export::citation(&p, CitationStyle::Vancouver),
        "Kukanja P, Langseth CM, Castelo-Branco G. Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. Cell. 2024;187(8):1990-2009. doi:10.1016/j.cell.2024.02.030"
    );
    let apa = export::citation(&p, CitationStyle::Apa);
    assert!(apa.starts_with("Kukanja, P., Langseth, C. M., & Castelo-Branco, G. (2024)."), "{apa}");
    let nat = export::citation(&p, CitationStyle::Nature);
    assert!(nat.starts_with("Kukanja, P., Langseth, C. M. & Castelo-Branco, G. Cellular"), "{nat}");
    assert_eq!(export::split_name("Ludwig van Beethoven"), ("van Beethoven".into(), "Ludwig".into()));
    for (style, want) in [
        (CitationStyle::Harvard, "Kukanja, P., Langseth, C.M. and Castelo-Branco, G. (2024) ‘Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology’, *Cell*, 187(8), pp. 1990–2009. Available at: https://doi.org/10.1016/j.cell.2024.02.030."),
        (CitationStyle::Chicago, "Kukanja, Petra, Christoffer M. Langseth, and Gonçalo Castelo-Branco. 2024. “Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology.” *Cell* 187 (8): 1990–2009. https://doi.org/10.1016/j.cell.2024.02.030."),
        (CitationStyle::Mla, "Kukanja, Petra, et al. “Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology.” *Cell*, vol. 187, no. 8, 2024, pp. 1990–2009, https://doi.org/10.1016/j.cell.2024.02.030."),
        (CitationStyle::Cell, "Kukanja, P., Langseth, C.M., and Castelo-Branco, G. (2024). Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. Cell *187*, 1990–2009. https://doi.org/10.1016/j.cell.2024.02.030."),
        (CitationStyle::Ieee, "P. Kukanja, C. M. Langseth, and G. Castelo-Branco, “Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology,” *Cell*, vol. 187, no. 8, pp. 1990–2009, 2024, doi: 10.1016/j.cell.2024.02.030."),
        (CitationStyle::Ama, "Kukanja P, Langseth CM, Castelo-Branco G. Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. *Cell*. 2024;187(8):1990-2009. doi:10.1016/j.cell.2024.02.030"),
        (CitationStyle::Science, "P. Kukanja, C. M. Langseth, G. Castelo-Branco, Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. *Cell* **187**, 1990–2009 (2024)."),
        (CitationStyle::Acs, "Kukanja, P.; Langseth, C. M.; Castelo-Branco, G. Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology. *Cell* **2024**, *187* (8), 1990–2009. DOI: 10.1016/j.cell.2024.02.030."),
    ] {
        assert_eq!(export::citation(&p, style), want, "{}", style.name());
    }
    // every style has a key that maps back to it
    for s in CitationStyle::ALL {
        assert_eq!(CitationStyle::from_key(s.key()), Some(s));
    }
}

#[test]
fn query_and_doi_parsing() {
    assert_eq!(index::fts_query("spatial omics"), "\"spatial\"* AND \"omics\"*");
    assert_eq!(index::fts_query("\"cell niches\" moscot"), "\"cell niches\" AND \"moscot\"*");
    assert_eq!(metadata::doi_in("https://doi.org/10.1038/s41588-024-01664-3").unwrap(), "10.1038/s41588-024-01664-3");
    assert_eq!(metadata::doi_in("see doi:10.1016/j.cell.2024.02.030.").unwrap(), "10.1016/j.cell.2024.02.030");
}

#[test]
#[ignore] // network: cargo test -- --ignored
fn resolves_metadata_online() {
    let m = metadata::resolve("https://doi.org/10.1016/j.cell.2024.02.030").unwrap();
    assert!(m.title.starts_with("Cellular architecture"));
    assert_eq!(m.authors.len(), 14);
    assert_eq!(m.pmid, "38513664");
    let a = metadata::resolve("arXiv:1706.03762").unwrap();
    assert_eq!(a.title, "Attention Is All You Need");
    let p = metadata::resolve("PMC10937399").unwrap();
    assert!(p.title.starts_with("BANKSY"));
}

const BIB: &str = r#"
@string{cell = "Cell"}
@article{Kukanja2024,
  author = {Kukanja, Petra and Langseth, Christoffer M. and Rubio Rodr{\'\i}guez-Kirby, Leslie A. and Castelo-Branco, Gon{\c c}alo},
  title = {{Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology}},
  journal = cell,
  year = 2024,
  month = apr,
  volume = {187},
  number = {8},
  pages = {1990--2009},
  doi = {10.1016/j.cell.2024.02.030},
  file = {Full Text PDF:/tmp/lunta-import-test/kukanja.pdf:application/pdf},
  keywords = {multiple sclerosis, spatial transcriptomics},
  mendeley-tags = {MS,ISS}
}
@comment{ignored}
@inproceedings{vaswani,
  title="Attention is All you {N}eed",
  author="Vaswani, Ashish and Shazeer, Noam and others",
  booktitle = "Advances in Neural Information Processing Systems",
  year = "2017",
  eprint = {1706.03762}, archivePrefix = {arXiv}
}
"#;

const RIS: &str = "TY  - JOUR\r\nAU  - Lopez, Romain\r\nAU  - Yosef, Nir\r\nTI  - Deep generative modeling for single-cell transcriptomics\r\nJO  - Nature Methods\r\nPY  - 2018/12/01\r\nVL  - 15\r\nSP  - 1053\r\nEP  - 1058\r\nDO  - 10.1038/s41592-018-0229-2\r\nAB  - Single-cell transcriptome measurements can reveal\r\n  unexplored biological diversity.\r\nKW  - scVI\r\nER  - \r\n";

#[test]
fn parses_bibtex_exports() {
    let e = import::parse_bibtex(BIB);
    assert_eq!(e.len(), 2);
    let k = &e[0];
    assert_eq!(k.meta.title, "Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology");
    assert_eq!(k.meta.authors, vec!["Petra Kukanja", "Christoffer M. Langseth", "Leslie A. Rubio Rodríguez-Kirby", "Gonçalo Castelo-Branco"]);
    assert_eq!(k.meta.journal, "Cell");
    assert_eq!(k.meta.date, "2024-04");
    assert_eq!(k.meta.pages, "1990-2009");
    assert_eq!(k.meta.doi, "10.1016/j.cell.2024.02.030");
    assert_eq!(k.files, vec!["/tmp/lunta-import-test/kukanja.pdf"]);
    assert_eq!(k.tags, vec!["MS", "ISS"]);
    assert_eq!(k.meta.keywords.len(), 2);
    let v = &e[1];
    assert_eq!(v.meta.title, "Attention is All you Need");
    assert_eq!(v.meta.authors, vec!["Ashish Vaswani", "Noam Shazeer"]);
    assert_eq!(v.meta.arxiv, "1706.03762");
    assert_eq!(v.meta.journal, "Advances in Neural Information Processing Systems");
}

#[test]
fn parses_ris_exports() {
    let e = import::parse_any(RIS);
    assert_eq!(e.len(), 1);
    let m = &e[0].meta;
    assert_eq!(m.authors, vec!["Romain Lopez", "Nir Yosef"]);
    assert_eq!(m.date, "2018-12-01");
    assert_eq!(m.pages, "1053-1058");
    assert_eq!(m.abstract_text, "Single-cell transcriptome measurements can reveal unexplored biological diversity.");
    assert_eq!(m.doi, "10.1038/s41592-018-0229-2");
}

#[test]
fn import_merges_duplicates_and_copies_pdfs() {
    std::fs::create_dir_all("/tmp/lunta-import-test").unwrap();
    std::fs::write("/tmp/lunta-import-test/kukanja.pdf", b"%PDF-1.7 test").unwrap();
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    lib.add_paper(sample(), None, None, None).unwrap(); // already in the library, without PDF
    let mut entries = import::parse_bibtex(BIB);
    entries.extend(import::parse_any(RIS));
    entries.push(import::ImportEntry::default()); // empty → skipped
    let r = lib.import_entries(entries).unwrap();
    assert_eq!((r.added, r.merged, r.skipped, r.pdfs_copied), (2, 1, 1, 1));
    let k = lib.find_by_doi("10.1016/j.cell.2024.02.030".into()).unwrap();
    assert!(k.has_pdf);
    assert_eq!(k.tags, vec!["ISS".to_string(), "MS".to_string()]);
    // importing again changes nothing
    let r2 = lib.import_entries(import::parse_bibtex(BIB)).unwrap();
    assert_eq!((r2.added, r2.merged), (0, 2));
    assert_eq!(lib.papers(PaperFilter::default(), SortKey::Added).len(), 3);
}

#[test]
fn opens_epub_for_reading() {
    // an EPUB made by the converter: LUNTA_TEST_EPUB=/path/to/attention-is-all-you-need.epub cargo test
    let Ok(path) = std::env::var("LUNTA_TEST_EPUB") else { return };
    let src = std::path::Path::new(&path);
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let p = lib.add_paper(PaperMeta { title: "Attention".into(), ..Default::default() }, None, Some(std::fs::read(src).unwrap()), None).unwrap();
    let book = lib.open_epub(p.id.clone()).unwrap();
    assert!(book.chapters.len() > 3, "{:?}", book.chapters.len());
    assert!(book.chapters.iter().all(|c| std::path::Path::new(&c.path).exists()));
    assert!(book.toc.iter().any(|t| t.title.contains("Model Architecture")), "{:?}", book.toc.iter().map(|t| &t.title).collect::<Vec<_>>());
    let all = std::fs::read_to_string(&book.combined).unwrap();
    assert!(all.contains("id=\"section-1\"") && all.contains("Model Architecture"));
    assert!(!all.contains("href=\"ch0"), "links between sections should be in-page");
    // second open reuses the unpacked copy
    assert_eq!(lib.open_epub(p.id).unwrap().dir, book.dir);
}

#[test]
#[ignore] // network
fn finds_open_access_pdfs() {
    let m = metadata::resolve("10.1016/j.cell.2024.02.030").unwrap();
    let c = metadata::pdf_candidates(&m);
    println!("{c:#?}");
    // a published (Cell) paper never gets its bioRxiv preprint
    assert!(!c.iter().any(|x| x.url.contains("biorxiv") || x.version == "preprint"), "no preprints for a published paper");
    assert!(metadata::is_preprint_doi("10.1101/2024.01.02.573123") && !metadata::is_preprint_doi("10.1016/j.cell.2024.02.030"));
    // a preprint itself still finds its own PDF
    let pre = metadata::resolve("10.1101/2023.04.13.536804").unwrap();
    assert!(metadata::pdf_candidates(&pre).iter().any(|x| x.version == "preprint" || x.url.contains("rxiv")), "{:#?}", metadata::pdf_candidates(&pre));
    let banksy = metadata::resolve("PMC10937399").unwrap();
    let c2 = metadata::pdf_candidates(&banksy);
    assert_eq!(c2[0].label, "PubMed Central");
    let pdf = metadata::download_pdf(&c2[0].url).unwrap();
    assert!(pdf.len() > 100_000);
}

#[test]
fn icloud_placeholders_count_as_present() {
    let root = tmp().join("lib");
    let lib = open(&root, "phone");
    let p = lib.add_paper(sample(), None, None, None).unwrap();
    let dir = root.join("papers").join(&p.id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".book.epub.icloud"), b"").unwrap(); // what iOS shows before download
    lib.reindex().unwrap();
    assert!(lib.paper(p.id.clone()).unwrap().has_epub);
    assert_eq!(lib.file_status(p.id.clone(), FileKind::Epub), FileStatus::InCloud);
    assert_eq!(lib.file_status(p.id.clone(), FileKind::Pdf), FileStatus::Missing);
    assert!(lib.expected_file_path(p.id.clone(), FileKind::Epub).ends_with("book.epub"));
    assert_eq!(lib.pending_downloads().len(), 1);
    lib.attach_file(p.id.clone(), FileKind::AudioIndex, b"{}".to_vec()).unwrap();
    assert!(lib.paper(p.id.clone()).unwrap().has_audio);
}

#[test]
#[ignore] // network
fn discovers_papers_on_openalex() {
    let hits = metadata::discover("spatial transcriptomics multiple sclerosis lesions", 10).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|h| !h.meta.abstract_text.is_empty() && !h.meta.authors.is_empty()));
    for h in hits.iter().take(5) { println!("{} | {} | {} | cited {}", h.meta.title, h.meta.journal, h.meta.date, h.cited_by); }
}

#[test]
fn folders_nest_and_refuse_cycles() {
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let a = lib.create_collection("Thesis".into(), None).unwrap();
    let b = lib.create_collection("Chapter 1".into(), None).unwrap();
    lib.move_collection(b.id.clone(), Some(a.id.clone())).unwrap();
    let cols = lib.collections();
    assert_eq!(cols.iter().find(|c| c.id == b.id).unwrap().parent.as_deref(), Some(a.id.as_str()));
    assert!(lib.move_collection(a.id.clone(), Some(b.id.clone())).is_err());
    assert!(lib.move_collection(a.id.clone(), Some(a.id.clone())).is_err());
    lib.move_collection(b.id.clone(), None).unwrap();
    assert!(lib.collections().iter().all(|c| c.parent.is_none()));
}

#[test]
#[ignore] // online
fn watches_find_recent_papers() {
    use crate::metadata::{journal_name, watch_new, WatchKind};
    let since = "2024-01-01";
    let t = watch_new(WatchKind::Topic, "velocity-based training", since, 5).unwrap();
    assert!(!t.is_empty() && t.iter().all(|h| h.meta.date.as_str() >= since), "{:?}", t.iter().map(|h| &h.meta.date).collect::<Vec<_>>());
    let a = watch_new(WatchKind::Author, "Jonathon Weakley", since, 5).unwrap();
    assert!(!a.is_empty());
    let c = watch_new(WatchKind::Citing, "10.1519/SSC.0000000000000560", "2021-01-01", 5).unwrap();
    assert_eq!(journal_name("nature neuroscience").unwrap().as_deref(), Some("Nature Neuroscience"));
    let n = watch_new(WatchKind::Journal, "Nature Neuroscience", "2025-06-01", 5).unwrap();
    assert!(!n.is_empty() && n.iter().all(|h| h.meta.journal == "Nature Neuroscience"), "{:?}", n.iter().map(|h| &h.meta.journal).collect::<Vec<_>>());
    assert!(!c.is_empty());
    println!("topic {} author {} citing {}", t.len(), a.len(), c.len());
    for h in t.iter().chain(&a).chain(&c) { println!("{} | {}", h.meta.date, h.meta.title); }
}

#[test]
#[ignore] // network: cargo test -- --ignored
fn journal_styles_online() {
    let dir = tmp();
    let cache = dir.to_str().unwrap();
    let p = Paper {
        id: "p1".into(),
        meta: sample(),
        added: 0,
        modified: 0,
        has_pdf: false,
        has_epub: false,
        has_audio: false,
        read: false,
        starred: false,
        must_read: false,
        collections: vec![],
        tags: vec![],
        annotation_count: 0,
    };
    let hits = csl::search(cache, "nature neuroscience", 5).unwrap();
    assert_eq!(hits[0].id, "nature-neuroscience", "{hits:?}");
    let mut q = p.clone();
    q.id = "p2".into();
    q.meta.authors = vec!["Ada Lee".into()];
    q.meta.date = "2021".into();
    for style in ["nature-neuroscience", "elife", "cell-reports"] {
        let r = csl::render(cache, style, &[p.clone(), q.clone()], &[vec!["p1".into(), "p2".into()], vec!["p2".into()]]).unwrap();
        println!("{style} [{}]: {:?}", r.style_title, r.citations);
        for e in &r.bibliography { println!("   {}", e.text); }
        assert_eq!(r.citations.len(), 2);
        assert_eq!(r.bibliography.len(), 2);
    }
}

#[test]
fn must_read_list() {
    let root = tmp();
    let lib = open(&root, "mac");
    let mut m = sample();
    m.doi = "10.1/mr".into();
    let p = lib.add_paper(m, None, None, None).unwrap();
    let only = |lib: &Library| lib.papers(PaperFilter { must_read_only: true, ..Default::default() }, SortKey::Added).len();
    assert_eq!(only(&lib), 0);
    // on the list: also unread
    lib.set_read(p.id.clone(), true).unwrap();
    lib.set_must_read(p.id.clone(), true).unwrap();
    let q = lib.paper(p.id.clone()).unwrap();
    assert!(q.must_read && !q.read);
    assert_eq!(only(&lib), 1);
    // opening it keeps it on the list
    lib.mark_opened(p.id.clone()).unwrap();
    let q = lib.paper(p.id.clone()).unwrap();
    assert!(q.must_read && q.read);
    // marking it read takes it off
    lib.set_read(p.id.clone(), false).unwrap();
    lib.set_read(p.id.clone(), true).unwrap();
    assert!(!lib.paper(p.id.clone()).unwrap().must_read);
    assert_eq!(only(&lib), 0);
    // syncs to another device
    lib.set_must_read(p.id.clone(), true).unwrap();
    let phone = open(&root, "phone");
    assert!(phone.paper(p.id.clone()).unwrap().must_read);
}

// ---- highlights by quote, and across the library ----

/// A small two-section EPUB like the converter's (body text with inline markup, a script, entities).
fn tiny_epub() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut add = |name: &str, data: &str| {
        zw.start_file(name, o).unwrap();
        zw.write_all(data.as_bytes()).unwrap();
    };
    add("mimetype", "application/epub+zip");
    add("META-INF/container.xml", r#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#);
    add("EPUB/content.opf", r#"<package><manifest><item id="c1" href="text/ch01.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/ch02.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#);
    let page = |body: &str| format!("<?xml version=\"1.0\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title><style>p {{ x: 1 }}</style></head><body>\n{body}\n</body></html>");
    add("EPUB/text/ch01.xhtml", &page("<h1>Introduction</h1>\n<p>Strains of <i>Escherichia</i>\n   coli grew &amp; divided within 20&#160;min.</p>\n<script>var t = 'grew';</script>\n<p>The cells were washed twice.</p>"));
    add("EPUB/text/ch02.xhtml", &page("<h2>Methods</h2>\n<p>Before imaging, the cells were washed twice.</p>\n<p>Results are “robust” — see <a href=\"#f1\">Fig.&#160;2</a>.</p>"));
    drop(add);
    zw.finish().unwrap().into_inner()
}

#[test]
fn highlights_by_quote() {
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let p = lib.add_paper(sample(), None, Some(tiny_epub()), None).unwrap();
    let book = lib.open_epub(p.id.clone()).unwrap();
    // the coordinate system: the page's body text, as the reader's textNodes() join it
    let text = highlight::dom_text(&std::fs::read_to_string(&book.combined).unwrap());
    assert!(text.contains("Strains of Escherichia\n   coli grew & divided within 20\u{a0}min."));
    assert!(!text.contains("var t"), "script text is not page text");

    // across inline markup, with whitespace and the no-break space written as plain spaces
    let a = lib.add_highlight(p.id.clone(), "Escherichia coli grew & divided within 20 min".into(), None, "green".into(), "".into()).unwrap();
    assert_eq!(a.location, "epub");
    assert_eq!(a.quote, "Escherichia\n   coli grew & divided within 20\u{a0}min");
    assert_eq!(a.kind, AnnotationKind::Highlight);
    assert_eq!(a.color, "green");
    // position: UTF-16 offset of the quote in the page text; prefix and suffix are the text around it
    let start: usize = a.position.parse().unwrap();
    let utf16: Vec<u16> = text.encode_utf16().collect();
    assert_eq!(String::from_utf16(&utf16[start..start + a.quote.encode_utf16().count()]).unwrap(), a.quote);
    assert!(a.prefix.ends_with("Strains of ") && text.contains(&format!("{}{}{}", a.prefix, a.quote, a.suffix)));

    // across a paragraph boundary and with straight quotes / hyphen for curly quotes / em dash
    let b = lib.add_highlight(p.id.clone(), "Results are \"robust\" - see Fig. 2".into(), None, "".into(), "check this".into()).unwrap();
    assert_eq!(b.quote, "Results are “robust” — see Fig.\u{a0}2");
    assert_eq!((b.kind, b.color.as_str()), (AnnotationKind::Note, "yellow"));

    // ambiguous: twice in the paper, until the text before it picks one
    let err = lib.add_highlight(p.id.clone(), "the cells were washed twice".into(), None, "".into(), "".into()).unwrap_err();
    assert!(err.to_string().contains("occurs 2 times"), "{err}");
    let c = lib.add_highlight(p.id.clone(), "the cells were washed twice".into(), Some("Before imaging,".into()), "pink".into(), "".into()).unwrap();
    assert!(c.prefix.ends_with("Before imaging, "));
    let c_start: usize = c.position.parse().unwrap();
    assert!(c_start > start);
    // not there at all: says how far it matched
    let err = lib.add_highlight(p.id.clone(), "Strains of Escherichia coli died".into(), None, "".into(), "".into()).unwrap_err();
    assert!(err.to_string().contains("continues"), "{err}");
    assert!(lib.add_highlight(p.id.clone(), "grew".into(), None, "purple".into(), "".into()).is_err());

    // listing across the library: newest first, by colour, words in quote or note, tag
    let other = lib.add_paper(PaperMeta { title: "Another paper".into(), doi: "10.1/other".into(), ..Default::default() }, None, None, None).unwrap();
    lib.save_annotation(Annotation {
        id: String::new(), paper_id: other.id.clone(), kind: AnnotationKind::Note, color: "agent".into(),
        location: "paper".into(), position: String::new(), quote: String::new(), prefix: String::new(),
        suffix: String::new(), note: "Compare with E. coli growth".into(), created: 0, modified: 0,
    }).unwrap();
    let all = lib.find_annotations(Default::default());
    assert_eq!(all.len(), 4);
    assert!(all.windows(2).all(|w| w[0].created >= w[1].created));
    let f = |filter: highlight::AnnotationFilter| lib.find_annotations(filter).into_iter().map(|a| a.id).collect::<Vec<_>>();
    assert_eq!(f(highlight::AnnotationFilter { color: Some("Green".into()), ..Default::default() }), vec![a.id.clone()]);
    assert_eq!(f(highlight::AnnotationFilter { query: Some("ESCHERICHIA divided".into()), ..Default::default() }), vec![a.id.clone()]);
    assert_eq!(f(highlight::AnnotationFilter { query: Some("check".into()), ..Default::default() }), vec![b.id.clone()]);
    assert_eq!(f(highlight::AnnotationFilter { paper: Some(other.id.clone()), ..Default::default() }).len(), 1);
    lib.set_tag(other.id.clone(), "growth".into(), true).unwrap();
    assert_eq!(f(highlight::AnnotationFilter { tag: Some("growth".into()), ..Default::default() }).len(), 1);
    assert_eq!(lib.annotation(c.id.clone()).unwrap().color, "pink");

    // highlights and notes are in the library search
    let hits = lib.search("robust".into(), 10).unwrap();
    assert!(hits.iter().any(|h| h.paper.id == p.id));
    let hits = lib.search("growth compare".into(), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].paper.id, other.id);

    // the full text stands in when there is no reading view
    let ft = lib.add_paper(PaperMeta { title: "Text only".into(), doi: "10.1/ft".into(), ..Default::default() }, None, None, Some("Line one.\nWe measured lactate in CSF.\n".into())).unwrap();
    let d = lib.add_highlight(ft.id.clone(), "measured lactate in  CSF".into(), None, "blue".into(), "".into()).unwrap();
    assert_eq!((d.location.as_str(), d.quote.as_str()), ("pdf", "measured lactate in CSF"));
    let none = lib.add_paper(PaperMeta { title: "Nothing".into(), doi: "10.1/none".into(), ..Default::default() }, None, None, None).unwrap();
    assert!(lib.add_highlight(none.id, "anything at all".into(), None, "".into(), "".into()).is_err());
}

#[test]
fn dom_text_like_the_page() {
    let x = "<html><head><title>x</title></head><body class=\"c\"><p a=\"1>2\">A<!-- no --><b>B</b>&lt;&#x41;&unknown;</p><style>s{}</style><br/>C</body></html>";
    assert_eq!(highlight::dom_text(x), "AB<A&unknown;C");
}

#[test]
fn abstracts_keep_less_than_signs() {
    // NEJM via Crossref: "P<0.001" was taken for the start of a tag and half the abstract vanished
    let a = metadata::strip_tags("<jats:p>Recurrence-free survival was 82.2% vs 70.0% (two-sided P&lt;0.001; hazard ratio, 0.62). Distant metastasis &gt; 1 site.</jats:p><jats:sec><jats:title>Conclusions</jats:title><jats:p>Adjuvant therapy prolonged survival.</jats:p></jats:sec>");
    assert!(a.contains("(two-sided P<0.001; hazard ratio, 0.62)"), "{a}");
    assert!(a.contains("Distant metastasis > 1 site."), "{a}");
    assert!(a.contains("Conclusions Adjuvant therapy prolonged survival."), "{a}");
    // already-decoded text with a bare "<" and ">" around words
    let b = metadata::strip_tags("<p>Patients with n<5 lesions and age >60 years.</p> <i>P</i> < 0.05 <br/>done");
    assert_eq!(b, "Patients with n<5 lesions and age >60 years. P < 0.05 done");
}

#[test]
fn orcid_ids_are_checked() {
    use metadata::normalize_orcid as n;
    let ok = Some("0000-0002-1825-0097".to_string());
    assert_eq!(n("0000-0002-1825-0097"), ok);
    assert_eq!(n(" https://orcid.org/0000-0002-1825-0097 "), ok);
    assert_eq!(n("orcid:0000000218250097"), ok);
    assert_eq!(n("ORCID: 0000-0002-1825-0097"), ok);
    // check digit X
    assert_eq!(n("0000-0002-1694-233x"), Some("0000-0002-1694-233X".to_string()));
    assert_eq!(n("0000-0001-5109-3700"), Some("0000-0001-5109-3700".to_string()));
    // wrong check digit, too short, letters
    assert_eq!(n("0000-0002-1825-0098"), None);
    assert_eq!(n("0000-0002-1825-009"), None);
    assert_eq!(n("0000-000A-1825-0097"), None);
    assert_eq!(n("Jane Smith"), None);
    assert_eq!(n(""), None);
}

#[test]
fn author_watch_values() {
    use metadata::{author_ref, AuthorRef};
    assert_eq!(author_ref("0000-0002-1825-0097"), AuthorRef::Orcid("0000-0002-1825-0097".into()));
    assert_eq!(author_ref("orcid:0000-0002-1825-0097"), AuthorRef::Orcid("0000-0002-1825-0097".into()));
    assert_eq!(author_ref("https://orcid.org/0000-0002-1825-0097"), AuthorRef::Orcid("0000-0002-1825-0097".into()));
    assert_eq!(author_ref("A5023888391"), AuthorRef::OpenAlex("A5023888391".into()));
    assert_eq!(author_ref("https://openalex.org/A5023888391"), AuthorRef::OpenAlex("A5023888391".into()));
    assert_eq!(author_ref("https://openalex.org/authors/a5023888391/"), AuthorRef::OpenAlex("A5023888391".into()));
    // names stay names (even short or odd ones)
    assert_eq!(author_ref("Josiah Carberry"), AuthorRef::Name("Josiah Carberry".into()));
    assert_eq!(author_ref("Abe"), AuthorRef::Name("Abe".into()));
    assert_eq!(author_ref("A123"), AuthorRef::OpenAlex("A123".into()));
    assert_eq!(author_ref("A12"), AuthorRef::Name("A12".into()));
    // a malformed ORCID iD is not an id
    assert_eq!(author_ref("0000-0002-1825-0098"), AuthorRef::Name("0000-0002-1825-0098".into()));
}

#[test]
fn coauthors_from_works() {
    let w = serde_json::json!({"results": [
        {"authorships": [
            {"author": {"id": "https://openalex.org/A1", "display_name": "Me", "orcid": "https://orcid.org/0000-0002-1825-0097"}},
            {"author": {"id": "https://openalex.org/A2", "display_name": "Ann", "orcid": "https://orcid.org/0000-0001-5109-3700"}},
            {"author": {"id": "https://openalex.org/A3", "display_name": "Bo", "orcid": null}}]},
        {"authorships": [
            {"author": {"id": "https://openalex.org/A9", "display_name": "Me again", "orcid": "https://orcid.org/0000-0002-1825-0097"}},
            {"author": {"id": "https://openalex.org/A3", "display_name": "Bo", "orcid": null}},
            {"author": {"id": "https://openalex.org/A3", "display_name": "Bo", "orcid": null}}]}
    ]});
    let c = metadata::coauthors(&w, "A1", "0000-0002-1825-0097", 10);
    assert_eq!(c.iter().map(|x| (x.name.as_str(), x.shared)).collect::<Vec<_>>(), vec![("Bo", 2), ("Ann", 1)]);
    assert_eq!(c[1].orcid, "0000-0001-5109-3700");
    assert_eq!(c[0].openalex_id, "A3");
}

#[test]
#[ignore] // network: cargo test orcid_profile_online -- --ignored --nocapture
fn orcid_profile_online() {
    let p = metadata::orcid_profile("0000-0002-1825-0097").unwrap();
    println!("{} {} works={} cited={} recent={} top={} coauthors={:?}", p.name, p.openalex_id, p.works_count, p.cited_by_count,
             p.works.len(), p.most_cited.len(), p.coauthors.iter().map(|c| (&c.name, c.shared, &c.orcid)).collect::<Vec<_>>());
    for w in p.works.iter().take(5) { println!("  {} | {} | {}", w.meta.date, w.meta.doi, w.meta.title); }
    let by = metadata::watch_new(metadata::WatchKind::Author, "orcid:0000-0002-1825-0097", "2000-01-01", 5).unwrap();
    println!("author watch by ORCID: {}", by.len());
    let by = metadata::watch_new(metadata::WatchKind::Author, &p.openalex_id, "2000-01-01", 5).unwrap();
    println!("author watch by OpenAlex id: {}", by.len());
    if let Some(w) = p.most_cited.iter().find(|w| !w.meta.doi.is_empty()) {
        let c = metadata::citing_new(&w.meta.doi, &p.openalex_id, "2000-01-01", 5).unwrap();
        println!("citing {} (without self): {}", w.meta.doi, c.len());
    }
    assert!(!p.openalex_id.is_empty());
}

// ---- citations for Word and Google Docs ----

/// A small author–date CSL style, written into the style cache so tests stay offline.
const TEST_AUTHOR_DATE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
  <info><title>Test Author Date</title><id>http://www.zotero.org/styles/test-author-date</id><updated>2024-01-01T00:00:00+00:00</updated></info>
  <macro name="author"><names variable="author"><name form="short" and="symbol" et-al-min="3" et-al-use-first="1"/></names></macro>
  <macro name="year"><date variable="issued"><date-part name="year"/></date></macro>
  <citation disambiguate-add-year-suffix="true">
    <sort><key macro="author"/><key macro="year"/></sort>
    <layout prefix="(" suffix=")" delimiter="; "><text macro="author"/><text prefix=", " macro="year"/></layout>
  </citation>
  <bibliography>
    <sort><key macro="author"/><key macro="year"/></sort>
    <layout suffix="."><names variable="author"><name name-as-sort-order="all" initialize-with=". "/></names><text prefix=" (" suffix=")" macro="year"/><text prefix=". " variable="title"/><text prefix=". " variable="container-title" font-style="italic"/></layout>
  </bibliography>
</style>"#;

/// A small numbered CSL style (citation-number, superscript).
const TEST_NUMERIC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
  <info><title>Test Numeric</title><id>http://www.zotero.org/styles/test-numeric</id><updated>2024-01-01T00:00:00+00:00</updated></info>
  <citation collapse="citation-number">
    <sort><key variable="citation-number"/></sort>
    <layout vertical-align="sup" delimiter=","><text variable="citation-number"/></layout>
  </citation>
  <bibliography second-field-align="flush">
    <layout suffix="."><text variable="citation-number" suffix=". "/><names variable="author"><name initialize-with=""/></names><text prefix=" " variable="title"/></layout>
  </bibliography>
</style>"#;

fn csl_cache_with_test_styles() -> PathBuf {
    let dir = tmp();
    std::fs::create_dir_all(dir.join("csl")).unwrap();
    std::fs::write(dir.join("csl/test-author-date.csl"), TEST_AUTHOR_DATE).unwrap();
    std::fs::write(dir.join("csl/test-numeric.csl"), TEST_NUMERIC).unwrap();
    dir
}

fn paper_with(id: &str, title: &str, authors: &[&str], date: &str) -> Paper {
    let mut m = sample();
    m.title = title.into();
    m.authors = authors.iter().map(|a| a.to_string()).collect();
    m.date = date.into();
    m.doi = String::new();
    Paper {
        id: id.into(),
        meta: m,
        added: 0,
        modified: 0,
        has_pdf: false,
        has_epub: false,
        has_audio: false,
        read: false,
        starred: false,
        must_read: false,
        collections: vec![],
        tags: vec![],
        annotation_count: 0,
    }
}

#[test]
fn csl_titles_with_markup_characters_and_other_scripts() {
    let dir = csl_cache_with_test_styles();
    let cache = dir.to_str().unwrap();
    let titles = [
        "Signals <10 Hz & the \"quiet\" cortex: 'A' < B",
        "单细胞转录组 of the mouse brain",
        "Ωmega-3 and β-cells in Αθήνα",
        "Solving $x^2 + y^2 = z^2$ with A* search",
        "Tags like <i>not italic</i> stay text",
    ];
    let papers: Vec<Paper> = titles.iter().enumerate().map(|(i, t)| paper_with(&format!("p{i}"), t, &["Ada Lee"], &format!("20{:02}", 10 + i))).collect();
    let cites: Vec<Vec<String>> = papers.iter().map(|p| vec![p.id.clone()]).collect();
    let r = csl::render(cache, "test-author-date", &papers, &cites).unwrap();
    assert_eq!(r.style_title, "Test Author Date");
    assert_eq!(r.citations.len(), 5);
    assert_eq!(r.citations[0].text, "(Lee, 2010)");
    assert_eq!(r.bibliography.len(), 5);
    let all = r.bibliography.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("\n");
    for t in &titles {
        assert!(all.contains(t), "{t} missing in\n{all}");
    }
    // the journal in italics, the prefix's ". " outside them
    assert!(all.contains("brain. *Cell*."), "{all}");
    assert!(!all.contains("&amp;") && !all.contains("&lt;") && !all.contains("&quot;"), "entities left in\n{all}");
}

#[test]
fn csl_year_suffixes_numbers_and_empty_citations() {
    let dir = csl_cache_with_test_styles();
    let cache = dir.to_str().unwrap();
    let a = paper_with("a", "Alpha paper on cells", &["Jane Smith"], "2020");
    let b = paper_with("b", "Beta paper on cells", &["Jane Smith"], "2020");
    let c = paper_with("c", "Gamma paper", &["Ola Nordmann", "Kari Nordmann", "Per Hansen"], "2019");
    let papers = [a.clone(), b.clone(), c.clone()];
    let r = csl::render(cache, "test-author-date", &papers, &[vec!["a".into()], vec!["b".into()], vec!["c".into()]]).unwrap();
    assert_eq!(r.citations[0].text, "(Smith, 2020a)", "{:?}", r.citations);
    assert_eq!(r.citations[1].text, "(Smith, 2020b)");
    assert_eq!(r.citations[2].text, "(Nordmann et al., 2019)");
    // numbered: by first citation, repeated papers keep their number, a citation of nobody is empty
    let cites = vec![vec!["c".into(), "a".into()], vec!["b".into()], vec!["a".into()], vec!["gone".into()], vec!["a".into(), "b".into(), "c".into()]];
    let r = csl::render(cache, "test-numeric", &papers, &cites).unwrap();
    let texts: Vec<&str> = r.citations.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["1,2", "3", "2", "", "1–3"], "{:?}", r.citations);
    assert!(r.citations[0].superscript);
    assert_eq!(r.bibliography.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["c", "a", "b"]);
}

#[test]
fn documents_cite_papers_the_library_lacks() {
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let dir = csl_cache_with_test_styles();
    let mut m = sample();
    m.doi = "10.1000/Kept".into();
    let kept = lib.add_paper(m.clone(), None, None, None).unwrap();
    // a merged duplicate (old id), found by DOI or by title
    assert_eq!(lib.find_cited("https://doi.org/10.1000/kept".into(), String::new()).map(|p| p.id), Some(kept.id.clone()));
    assert_eq!(lib.find_cited(String::new(), m.title.to_uppercase()).map(|p| p.id), Some(kept.id.clone()));
    assert!(lib.find_cited(String::new(), "Short".into()).is_none());
    assert!(lib.find_cited("10.1/none".into(), "Some other long title that nobody has".into()).is_none());
    // a paper only the document knows: formatted from its details
    let mut gone = sample();
    gone.title = "Deleted & <gone>".into();
    gone.authors = vec!["Ludwig van Beethoven".into()];
    gone.date = "1808".into();
    let apa = format_citation(gone.clone(), CitationStyle::Apa);
    assert!(apa.starts_with("van Beethoven, L. (1808). Deleted & <gone>."), "{apa}");
    assert_eq!(family_name("Ludwig van Beethoven".into()), "van Beethoven");
    assert_eq!(family_name("Langseth, Christoffer M.".into()), "Langseth");
    assert_eq!(family_name("WHO".into()), "WHO");
    let r = lib
        .cite_csl_with(
            dir.to_string_lossy().into(),
            "test-numeric".into(),
            vec![vec!["old-id".into(), kept.id.clone()], vec!["unknown".into()]],
            vec![CitedPaper { id: "old-id".into(), meta: gone }],
        )
        .unwrap();
    assert_eq!(r.citations[0].text, "1,2");
    assert_eq!(r.citations[1].text, "");
    assert_eq!(r.bibliography.len(), 2);
    assert!(r.bibliography[0].text.contains("Deleted & <gone>"), "{:?}", r.bibliography);
}
