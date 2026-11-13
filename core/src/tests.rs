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
    assert!(c.iter().any(|x| x.url.contains("biorxiv")), "expected the bioRxiv preprint");
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
    use crate::metadata::{watch_new, WatchKind};
    let since = "2024-01-01";
    let t = watch_new(WatchKind::Topic, "velocity-based training", since, 5).unwrap();
    assert!(!t.is_empty() && t.iter().all(|h| h.meta.date.as_str() >= since), "{:?}", t.iter().map(|h| &h.meta.date).collect::<Vec<_>>());
    let a = watch_new(WatchKind::Author, "Jonathon Weakley", since, 5).unwrap();
    assert!(!a.is_empty());
    let c = watch_new(WatchKind::Citing, "10.1519/SSC.0000000000000560", "2021-01-01", 5).unwrap();
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