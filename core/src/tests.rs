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
        cite_key: String::new(),
    };
    let bib = export::bibtex(&[p.clone(), p.clone()]);
    assert!(bib.contains("@article{Kukanja2024cellular,"), "{bib}");
    assert!(bib.contains("@article{Kukanja2024cellulara,"));
    assert!(bib.contains("author = {Kukanja, Petra and Langseth, Christoffer M. and Castelo-Branco, Gon{\\c{c}}alo}"));
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
fn search_by_identifier() {
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let a = lib
        .add_paper(
            PaperMeta { doi: "10.1038/NMETH.2563".into(), pmid: "23793237".into(), pmcid: "PMC3880121".into(), ..sample() },
            None,
            None,
            None,
        )
        .unwrap();
    let b = lib
        .add_paper(
            PaperMeta {
                title: "Attention is all you need".into(),
                doi: "10.48550/arXiv.1706.03762".into(),
                arxiv: "1706.03762v7".into(),
                ..sample()
            },
            None,
            None,
            Some("Earlier work (Ke et al., doi:10.1038/nmeth.2563) mapped sequences in situ.".into()),
        )
        .unwrap();
    let first = |q: &str| lib.search(q.into(), 10).unwrap().first().map(|h| h.paper.id.clone());
    for q in [
        "10.1038/nmeth.2563",
        "10.1038/NMETH.2563",
        "doi:10.1038/nmeth.2563",
        "DOI: 10.1038/nmeth.2563",
        "https://doi.org/10.1038/nmeth.2563",
        "https://dx.doi.org/10.1038/nmeth.2563.",
        "(10.1038/nmeth.2563),",
        "23793237",
        "pmid:23793237",
        "https://pubmed.ncbi.nlm.nih.gov/23793237/",
        "PMC3880121",
        "pmc3880121",
    ] {
        assert_eq!(first(q), Some(a.id.clone()), "{q}");
    }
    // the paper itself first, then a paper whose full text cites it
    let hits = lib.search("10.1038/nmeth.2563".into(), 10).unwrap();
    assert_eq!(hits.iter().map(|h| h.paper.id.clone()).collect::<Vec<_>>(), vec![a.id.clone(), b.id.clone()]);
    assert!(hits[0].snippet.contains("[10.1038/nmeth.2563]"), "{}", hits[0].snippet);
    for q in ["1706.03762", "arXiv:1706.03762", "https://arxiv.org/abs/1706.03762v5", "10.48550/arxiv.1706.03762"] {
        assert_eq!(first(q), Some(b.id.clone()), "{q}");
    }
    // a DOI among other words: the paper first, the words searched as usual
    let hits = lib.search("attention 10.1038/nmeth.2563".into(), 10).unwrap();
    assert_eq!(hits.iter().map(|h| h.paper.id.clone()).collect::<Vec<_>>(), vec![a.id.clone(), b.id.clone()]);
    // an unknown DOI with other words leaves the words' search alone
    assert_eq!(first("attention 10.9999/nothing"), Some(b.id.clone()));
    assert!(lib.search("10.9999/nothing".into(), 10).unwrap().is_empty());
    // a number that is no PubMed ID is searched as text
    assert!(lib.search("99999999".into(), 10).unwrap().is_empty());
}

#[test]
fn query_and_doi_parsing() {
    assert_eq!(index::fts_query("spatial omics"), "\"spatial\"* AND \"omics\"*");
    assert_eq!(index::fts_query("\"cell niches\" moscot"), "\"cell niches\" AND \"moscot\"*");
    assert_eq!(index::fts_query("10.1038/nmeth.2563, Hebb's (x)"), "\"10.1038/nmeth.2563\"* AND \"Hebb's\"* AND \"x\"");
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
        cite_key: String::new(),
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
        cite_key: String::new(),
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

// ---- citation keys ----

fn sample_doi(doi: &str) -> PaperMeta {
    PaperMeta { doi: doi.into(), ..sample() }
}

fn keys_of(lib: &Library) -> Vec<(String, String)> {
    lib.papers(PaperFilter::default(), SortKey::Added).into_iter().map(|p| (p.id, p.cite_key)).collect::<std::collections::BTreeMap<_, _>>().into_iter().collect()
}

#[test]
fn cite_keys_are_fixed_and_unique() {
    let root = tmp().join("lib");
    let mac = open(&root, "mac");
    let a = mac.add_paper(sample_doi("10.1/a"), None, None, None).unwrap();
    let b = mac.add_paper(sample_doi("10.1/b"), None, None, None).unwrap();
    assert_eq!(a.cite_key, "Kukanja2024cellular");
    assert_eq!(b.cite_key, "Kukanja2024cellulara");

    // the same key in every export, whatever else is in it and in which order
    assert!(mac.export_bibtex(vec![b.id.clone()]).contains("@article{Kukanja2024cellulara,"));
    let both = mac.export_bibtex(vec![b.id.clone(), a.id.clone()]);
    assert!(both.contains("@article{Kukanja2024cellular,") && both.contains("@article{Kukanja2024cellulara,"), "{both}");
    assert_eq!(mac.latex_cite(vec![a.id.clone(), b.id.clone()]), "\\cite{Kukanja2024cellular,Kukanja2024cellulara}");
    assert_eq!(mac.latex_cite(vec!["nope".into()]), "");

    // editing the details doesn't change it
    let mut m = sample_doi("10.1/a");
    m.title = "A different title".into();
    m.authors = vec!["Someone Else".into()];
    mac.update_meta(a.id.clone(), m).unwrap();
    assert_eq!(mac.paper(a.id.clone()).unwrap().cite_key, "Kukanja2024cellular");
    // a new paper with that base still gets a free key
    let c = mac.add_paper(sample_doi("10.1/c"), None, None, None).unwrap();
    assert_eq!(c.cite_key, "Kukanja2024cellularb");
    // deleting one doesn't move the others
    mac.delete_paper(a.id.clone()).unwrap();
    assert_eq!(mac.paper(b.id.clone()).unwrap().cite_key, "Kukanja2024cellulara");
    assert_eq!(mac.paper(c.id.clone()).unwrap().cite_key, "Kukanja2024cellularb");

    // the user's own key: checked, unique without case, kept
    assert!(mac.set_cite_key(b.id.clone(), "has space".into()).is_err());
    assert!(mac.set_cite_key(b.id.clone(), "x{y}".into()).is_err());
    assert!(mac.set_cite_key(b.id.clone(), "Müller".into()).is_err());
    assert!(mac.set_cite_key(b.id.clone(), "".into()).is_err());
    assert!(mac.set_cite_key(b.id.clone(), "kukanja2024CELLULARB".into()).is_err());
    assert!(mac.set_cite_key("nope".into(), "free".into()).is_err());
    assert_eq!(mac.set_cite_key(b.id.clone(), " kukanja:ms-2024 ".into()).unwrap(), "kukanja:ms-2024");
    assert_eq!(mac.set_cite_key(b.id.clone(), "kukanja:ms-2024".into()).unwrap(), "kukanja:ms-2024");
    // freeing a key doesn't hand it to another paper
    assert_eq!(mac.paper(c.id.clone()).unwrap().cite_key, "Kukanja2024cellularb");
    let again = open(&root, "mac");
    assert_eq!(again.paper(b.id.clone()).unwrap().cite_key, "kukanja:ms-2024");
    assert!(again.export_bibtex(vec![b.id.clone()]).contains("@article{kukanja:ms-2024,"));

    // accents are folded, other scripts left out
    let d = mac
        .add_paper(PaperMeta { title: "Über die Natur".into(), authors: vec!["José Müller-Lüdenscheidt".into()], date: "1999".into(), ..Default::default() }, None, None, None)
        .unwrap();
    assert_eq!(d.cite_key, "MullerLudenscheidt1999uber");
    let e = mac.add_paper(PaperMeta { title: "神经 networks".into(), authors: vec!["王 伟".into()], date: "2020".into(), ..Default::default() }, None, None, None).unwrap();
    assert_eq!(e.cite_key, "anon2020networks");
    for k in [&d.cite_key, &e.cite_key] {
        assert!(export::check_cite_key(k).is_ok());
    }
    assert_eq!((export::key_suffix(1), export::key_suffix(26), export::key_suffix(27)), ("a".into(), "z".into(), "aa".into()));
}

#[test]
fn thin_details_get_their_key_once_filled_in() {
    let root = tmp().join("lib");
    let mac = open(&root, "mac");
    let p = mac.add_paper(PaperMeta { title: "Draft from a PDF".into(), ..Default::default() }, None, None, None).unwrap();
    assert_eq!(p.cite_key, "anondraft");
    mac.update_meta(p.id.clone(), sample()).unwrap();
    assert_eq!(mac.paper(p.id.clone()).unwrap().cite_key, "Kukanja2024cellular");
    // once used, it stays
    assert_eq!(mac.cite_keys(vec![p.id.clone()]), vec!["Kukanja2024cellular".to_string()]);
    mac.update_meta(p.id.clone(), PaperMeta { title: "Other".into(), ..Default::default() }).unwrap();
    assert_eq!(mac.paper(p.id.clone()).unwrap().cite_key, "Kukanja2024cellular");
}

/// Papers added by a version without citation keys (no `SetCiteKey` in the logs).
fn legacy_library(root: &PathBuf) -> (String, String) {
    std::fs::create_dir_all(root.join("ops")).unwrap();
    let ops: Vec<store::Op> = [("p-old", 1000, "10.1/old"), ("p-new", 2000, "10.1/new")]
        .into_iter()
        .map(|(id, at, doi)| store::Op { at, device: "old".into(), kind: OpKind::AddPaper { paper: id.into(), meta: sample_doi(doi) } })
        .collect();
    let text: String = ops.iter().map(|o| serde_json::to_string(o).unwrap() + "\n").collect();
    std::fs::write(root.join("ops/old.jsonl"), text).unwrap();
    ("p-old".into(), "p-new".into())
}

fn copy_log(from: &PathBuf, to: &PathBuf, device: &str) {
    std::fs::copy(from.join(format!("ops/{device}.jsonl")), to.join(format!("ops/{device}.jsonl"))).unwrap();
}

#[test]
fn legacy_keys_agree_across_devices() {
    let root = tmp().join("lib");
    let (old, new) = legacy_library(&root);
    // a second device with the same synced logs, kept apart to act at the same time
    let root2 = tmp().join("lib");
    std::fs::create_dir_all(root2.join("ops")).unwrap();
    copy_log(&root, &root2, "old");
    let mac = open(&root, "mac");
    let phone = open(&root2, "phone");
    let want = vec![(new.clone(), "Kukanja2024cellulara".to_string()), (old.clone(), "Kukanja2024cellular".to_string())];
    assert_eq!(keys_of(&mac), want);
    assert_eq!(keys_of(&phone), want);

    // both use the keys at the same time (fixing them): they write the same keys
    mac.export_bibtex(vec![new.clone()]);
    phone.latex_cite(vec![old.clone(), new.clone()]);
    copy_log(&root, &root2, "mac");
    copy_log(&root2, &root, "phone");
    mac.refresh().unwrap();
    phone.refresh().unwrap();
    assert_eq!(keys_of(&mac), want);
    assert_eq!(keys_of(&phone), want);
}

#[test]
fn legacy_keys_stay_through_edits_and_deletes() {
    let root = tmp().join("lib");
    let (old, new) = legacy_library(&root);
    let mac = open(&root, "mac");
    let phone = open(&root, "phone");
    // an edit on the phone keeps the old paper's key (and so the newer one's)
    let mut m = sample_doi("10.1/old");
    m.authors = vec!["Ada Lovelace".into()];
    phone.update_meta(old.clone(), m).unwrap();
    assert_eq!(phone.paper(old.clone()).unwrap().cite_key, "Kukanja2024cellular");
    mac.refresh().unwrap();
    assert_eq!(mac.paper(old.clone()).unwrap().cite_key, "Kukanja2024cellular");
    assert_eq!(mac.paper(new.clone()).unwrap().cite_key, "Kukanja2024cellulara");
    // deleting the old paper doesn't hand its key to the newer one
    mac.delete_paper(old.clone()).unwrap();
    assert_eq!(mac.paper(new.clone()).unwrap().cite_key, "Kukanja2024cellulara");
    phone.refresh().unwrap();
    assert_eq!(phone.paper(new.clone()).unwrap().cite_key, "Kukanja2024cellulara");

    // the same when nothing was fixed before the delete
    let root = tmp().join("lib");
    let (old, new) = legacy_library(&root);
    let mac = open(&root, "mac");
    mac.delete_paper(old).unwrap();
    assert_eq!(mac.paper(new.clone()).unwrap().cite_key, "Kukanja2024cellulara");
    assert_eq!(open(&root, "phone").paper(new).unwrap().cite_key, "Kukanja2024cellulara");
}

#[test]
fn concurrent_keys_resolve_the_same_everywhere() {
    // two devices offline add different papers with the same base key
    let (r1, r2) = (tmp().join("lib"), tmp().join("lib"));
    let mac = open(&r1, "mac");
    let phone = open(&r2, "phone");
    let p = mac.add_paper(sample_doi("10.1/mac"), None, None, None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let q = phone.add_paper(sample_doi("10.1/phone"), None, None, None).unwrap();
    assert_eq!((p.cite_key.as_str(), q.cite_key.as_str()), ("Kukanja2024cellular", "Kukanja2024cellular"));
    copy_log(&r1, &r2, "mac");
    copy_log(&r2, &r1, "phone");
    mac.refresh().unwrap();
    phone.refresh().unwrap();
    // the earlier claim keeps the key, on both
    let want = vec![(p.id.clone(), "Kukanja2024cellular".to_string()), (q.id.clone(), "Kukanja2024cellulara".to_string())];
    let sorted = |mut v: Vec<(String, String)>| { v.sort(); v };
    assert_eq!(sorted(keys_of(&mac)), sorted(want.clone()));
    assert_eq!(sorted(keys_of(&phone)), sorted(want));

    // a key the user sets syncs
    phone.set_cite_key(q.id.clone(), "castelo2024".into()).unwrap();
    copy_log(&r2, &r1, "phone");
    mac.refresh().unwrap();
    assert_eq!(mac.paper(q.id.clone()).unwrap().cite_key, "castelo2024");
    assert!(mac.set_cite_key(p.id.clone(), "Castelo2024".into()).is_err());
}

fn keyed_paper(meta: PaperMeta, key: &str) -> Paper {
    Paper {
        id: key.into(),
        meta,
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
        cite_key: key.into(),
    }
}

#[test]
fn exports_by_work_type() {
    let grobid = PaperMeta {
        title: "GROBID: Combining Automatic Bibliographic Data Recognition and Term Extraction for Scholarship Publications".into(),
        authors: vec!["Patrice Lopez".into()],
        date: "2009".into(),
        journal: "Research and Advanced Technology for Digital Libraries".into(),
        series: "Lecture Notes in Computer Science".into(),
        pages: "473-474".into(),
        publisher: "Springer Berlin Heidelberg".into(),
        doi: "10.1007/978-3-642-04346-8_62".into(),
        work_type: "book-chapter".into(),
        ..Default::default()
    };
    let figures = PaperMeta {
        title: "PDFFigures 2.0: Mining Figures from Research Papers".into(),
        authors: vec!["Christopher Clark".into(), "Santosh Divvala".into()],
        date: "2016-06-19".into(),
        journal: "Proceedings of the 16th ACM/IEEE-CS on Joint Conference on Digital Libraries".into(),
        pages: "143-152".into(),
        publisher: "ACM".into(),
        doi: "10.1145/2910896.2910904".into(),
        work_type: "proceedings-article".into(),
        ..Default::default()
    };
    // as an older version stored it: no type, in capitals
    let kendall = PaperMeta {
        title: "A NEW MEASURE OF RANK CORRELATION".into(),
        authors: vec!["M. G. KENDALL".into()],
        date: "1938-06-01".into(),
        journal: "Biometrika".into(),
        volume: "30".into(),
        issue: "1-2".into(),
        pages: "81-93".into(),
        doi: "10.1093/biomet/30.1-2.81".into(),
        ..Default::default()
    };
    let nougat = PaperMeta {
        title: "Nougat: Neural Optical Understanding for Academic Documents".into(),
        authors: vec!["Lukas Blecher".into(), "Robert Stojnic".into()],
        date: "2023-08-25".into(),
        journal: "arXiv".into(),
        doi: "10.48550/arXiv.2308.13418".into(),
        arxiv: "2308.13418".into(),
        url: "https://arxiv.org/abs/2308.13418".into(),
        work_type: "posted-content".into(),
        ..Default::default()
    };
    // an older LNCS paper: the series as its journal, no type
    let crdt = PaperMeta {
        title: "Conflict-Free Replicated Data Types".into(),
        authors: vec!["Marc Shapiro".into()],
        date: "2011".into(),
        journal: "Lecture Notes in Computer Science".into(),
        ..Default::default()
    };
    let report = PaperMeta {
        title: "Web Annotation Data Model".into(),
        authors: vec!["World Wide Web Consortium".into()],
        date: "2017".into(),
        publisher: "W3C".into(),
        work_type: "report".into(),
        ..Default::default()
    };
    let papers: Vec<Paper> = [(grobid, "g"), (figures, "f"), (kendall, "k"), (nougat, "n"), (crdt, "c"), (report, "r")]
        .into_iter()
        .map(|(m, k)| keyed_paper(m, k))
        .collect();
    let bib = export::bibtex(&papers);
    let entry = |key: &str| bib.split("\n\n").find(|e| e.contains(&format!("{{{key},"))).unwrap().to_string();
    assert_eq!(
        entry("g"),
        "@inproceedings{g,\n  title = {{GROBID}: Combining Automatic Bibliographic Data Recognition and Term Extraction for Scholarship Publications},\n  author = {Lopez, Patrice},\n  booktitle = {Research and Advanced Technology for Digital Libraries},\n  series = {Lecture Notes in Computer Science},\n  year = {2009},\n  pages = {473--474},\n  publisher = {Springer Berlin Heidelberg},\n  doi = {10.1007/978-3-642-04346-8_62},\n}"
    );
    assert!(entry("f").starts_with("@inproceedings{f,\n  title = {{PDFFigures} 2.0: Mining Figures from Research Papers},"), "{}", entry("f"));
    assert!(entry("f").contains("booktitle = {Proceedings of the 16th ACM/IEEE-CS Joint Conference on Digital Libraries}"), "{}", entry("f"));
    assert_eq!(
        entry("k"),
        "@article{k,\n  title = {A New Measure of Rank Correlation},\n  author = {Kendall, M. G.},\n  journal = {Biometrika},\n  year = {1938},\n  volume = {30},\n  number = {1-2},\n  pages = {81--93},\n  doi = {10.1093/biomet/30.1-2.81},\n}"
    );
    assert_eq!(
        entry("n"),
        "@misc{n,\n  title = {Nougat: Neural Optical Understanding for Academic Documents},\n  author = {Blecher, Lukas and Stojnic, Robert},\n  howpublished = {arXiv preprint arXiv:2308.13418},\n  year = {2023},\n  doi = {10.48550/arXiv.2308.13418},\n  eprint = {2308.13418},\n  archivePrefix = {arXiv},\n  url = {https://arxiv.org/abs/2308.13418},\n}"
    );
    assert!(entry("c").starts_with("@inproceedings{c,") && entry("c").contains("booktitle = {Lecture Notes in Computer Science}"), "{}", entry("c"));
    assert!(entry("r").starts_with("@techreport{r,") && entry("r").contains("author = {{World Wide Web Consortium}}") && entry("r").contains("institution = {W3C}"), "{}", entry("r"));

    let ris = export::ris(&papers);
    assert!(ris.contains("TY  - CHAP\r\nAU  - Lopez, Patrice\r\nTI  - GROBID"), "{ris}");
    assert!(ris.contains("T2  - Research and Advanced Technology for Digital Libraries\r\nT3  - Lecture Notes in Computer Science\r\n"));
    assert!(ris.contains("TY  - CPAPER\r\n") && ris.contains("TY  - RPRT\r\nAU  - World Wide Web Consortium\r\n"));
    assert!(ris.contains("TI  - A New Measure of Rank Correlation\r\n"));
    // RIS and BibTeX read back with their types
    let back = import::parse_any(&ris);
    assert_eq!(back.iter().map(|e| e.meta.work_type.as_str()).collect::<Vec<_>>(), vec!["book-chapter", "proceedings-article", "journal-article", "journal-article", "proceedings-article", "report"]);
    assert_eq!(back[0].meta.series, "Lecture Notes in Computer Science");
    let back = import::parse_bibtex(&bib);
    assert_eq!(back.iter().map(|e| e.meta.work_type.as_str()).collect::<Vec<_>>(), vec!["proceedings-article", "proceedings-article", "journal-article", "posted-content", "proceedings-article", "report"]);
    assert_eq!((back[3].meta.arxiv.as_str(), back[3].meta.journal.as_str()), ("2308.13418", "arXiv"));

    // citations: conference papers "In" their proceedings, capitals fixed
    let apa = export::citation(&papers[1], CitationStyle::Apa);
    assert_eq!(apa, "Clark, C., & Divvala, S. (2016). PDFFigures 2.0: Mining Figures from Research Papers. In *Proceedings of the 16th ACM/IEEE-CS Joint Conference on Digital Libraries* (pp. 143–152). ACM. https://doi.org/10.1145/2910896.2910904");
    let van = export::citation(&papers[0], CitationStyle::Vancouver);
    assert_eq!(van, "Lopez P. GROBID: Combining Automatic Bibliographic Data Recognition and Term Extraction for Scholarship Publications. In: Research and Advanced Technology for Digital Libraries. Springer Berlin Heidelberg; 2009. p. 473-474. doi:10.1007/978-3-642-04346-8_62");
    let ieee = export::citation(&papers[1], CitationStyle::Ieee);
    assert!(ieee.contains("“PDFFigures 2.0: Mining Figures from Research Papers,” in *Proceedings of the 16th"), "{ieee}");
    assert!(!ieee.contains("vol."), "{ieee}");
    let k = export::citation(&papers[2], CitationStyle::Apa);
    assert!(k.starts_with("Kendall, M. G. (1938). A New Measure of Rank Correlation. *Biometrika*, *30*(1-2), 81–93."), "{k}");
    for style in CitationStyle::ALL {
        let c = export::citation(&papers[0], style);
        assert!(c.contains("Research and Advanced Technology for Digital Libraries"), "{style:?}: {c}");
    }
    // CSL item types
    let types: Vec<&str> = papers.iter().map(|p| csl::csl_type(&p.meta)).collect();
    assert_eq!(types, vec!["chapter", "paper-conference", "article-journal", "article", "paper-conference", "report"]);
}

#[test]
fn work_type_in_the_log_is_compatible() {
    // a log line from an older version (no work_type or series) reads with them empty
    let old = r#"{"at":1,"device":"old","op":"add_paper","paper":"p1","meta":{"title":"T","authors":["A B"],"date":"2020","journal":"Proceedings of the 2020 CHI Conference"}}"#;
    let op: store::Op = serde_json::from_str(old).unwrap();
    let store::OpKind::AddPaper { meta, .. } = op.kind else { panic!() };
    assert_eq!((meta.work_type.as_str(), meta.series.as_str()), ("", ""));
    assert_eq!(worktype::kind(&meta), worktype::Kind::Conference);
    // and a newer version's fields are skipped by a reader that doesn't know them (serde
    // ignores unknown fields, as older versions do with work_type and series)
    let newer = r#"{"title":"T","work_type":"report","series":"S","some_future_field":"x"}"#;
    let m: PaperMeta = serde_json::from_str(newer).unwrap();
    assert_eq!(m.work_type, "report");
    // a log with the new op and fields replays; the folder's extra entries are kept
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let c = lib.create_collection("F".into(), None).unwrap();
    lib.set_collection_bib(c.id.clone(), "@misc{x, title={X}}".into()).unwrap();
    let again = open(&root, "phone");
    assert_eq!(again.collection_bib(c.id.clone()), "@misc{x, title={X}}");
    assert!(again.kept_bibtex(Some(c.id.clone())).unwrap().contains("@misc{x, title={X}}"));
    // a deleted folder has no .bib (its file is left as it was)
    again.delete_collection(c.id.clone()).unwrap();
    assert!(again.kept_bibtex(Some(c.id)).is_none());
}

#[test]
fn bibtex_letters_as_latex() {
    let names = "Paweł Szostek, Łukasz Bolikowski, Nuno Preguiça, Ångström Øre, Müller Straße, Dvořák Čapek, Íñigo Ğür, Œuvre Đorđe";
    let tex = export::latex_letters(names);
    assert_eq!(tex, "Pawe{\\l} Szostek, {\\L}ukasz Bolikowski, Nuno Pregui{\\c{c}}a, {\\r{A}}ngstr{\\\"o}m {\\O}re, M{\\\"u}ller Stra{\\ss}e, Dvo{\\v{r}}{\\'a}k {\\v{C}}apek, {\\'I}{\\~n}igo {\\u{G}}{\\\"u}r, {\\OE}uvre {\\DJ}or{\\dj}e");
    assert!(tex.is_ascii());
    // read back as they were
    assert_eq!(import::delatex(&tex), names);
    assert_eq!(import::delatex(&export::latex_letters("Rodríguez")), "Rodríguez");
}

#[test]
fn bibtex_titles_protect_only_what_must_stay() {
    // sentence case (Crossref's): no braces but for acronyms and proper nouns
    assert_eq!(export::bib_title("Robust annotation positioning in digital documents"), "Robust annotation positioning in digital documents");
    assert_eq!(
        export::bib_title("HTML papers on arXiv -- why it is important, and how we made it happen"),
        "{HTML} papers on {arXiv} -- why it is important, and how we made it happen"
    );
    assert_eq!(export::bib_title("Kendall's tau for Bayesian models of RNA-seq data in Sweden"), "Kendall's tau for {Bayesian} models of {RNA}-seq data in {Sweden}");
    assert_eq!(export::bib_title("CERMINE: automatic extraction of structured metadata"), "{CERMINE}: automatic extraction of structured metadata");
    // title case: only acronyms, words with capitals inside or with digits
    assert_eq!(export::bib_title("PDFFigures 2.0: Mining Figures from Research Papers"), "{PDFFigures} 2.0: Mining Figures from Research Papers");
    assert_eq!(
        export::bib_title("The Semantic Reader Project: Augmenting Scholarly Documents through AI-Powered Interactive Reading Interfaces"),
        "The Semantic Reader Project: Augmenting Scholarly Documents through {AI}-Powered Interactive Reading Interfaces"
    );
    assert_eq!(export::bib_title("Mutations in BRCA1 (CRISPR) on the iPhone, Qwen3 and S2ORC"), "Mutations in {BRCA1} ({CRISPR}) on the {iPhone}, {Qwen3} and {S2ORC}");
    assert_eq!(export::bib_title("Local-First Software: You Own Your Data"), "Local-First Software: You Own Your Data");
    // escaped and with LaTeX letters, inside and outside the braces
    assert_eq!(export::bib_title("R&D at Universität Zürich: the ÖBB_data set"), "{R\\&D} at Universit{\\\"a}t Z{\\\"u}rich: the {{\\\"O}BB\\_data} set");
    // read back as written
    assert_eq!(import::delatex(&export::bib_title("HTML papers on arXiv")), "HTML papers on arXiv");
}

#[test]
fn bibtex_long_author_lists() {
    let authors: Vec<String> = (1..=60).map(|i| format!("Given{i} Family{i}")).collect();
    let meta = PaperMeta { title: "Qwen3 Technical Report".into(), authors: authors.clone(), date: "2025".into(), arxiv: "2505.09388".into(), ..Default::default() };
    let p = keyed_paper(meta.clone(), "qwen3");
    // default: more than 15 authors, the first 10 and "and others"
    let bib = export::bibtex(&[p.clone()]);
    assert!(bib.contains("author = {Family1, Given1 and Family2, Given2 and Family3, Given3 and Family4, Given4 and Family5, Given5 and Family6, Given6 and Family7, Given7 and Family8, Given8 and Family9, Given9 and Family10, Given10 and others},"), "{bib}");
    assert!(!bib.contains("Family11"));
    // 15 authors are all written
    let fifteen = keyed_paper(PaperMeta { authors: authors[..15].to_vec(), ..meta.clone() }, "f");
    assert!(export::bibtex(&[fifteen]).contains("Family15, Given15},"));
    // the options: 0 never shortens; the shown count stays within the limit
    let all = export::bibtex_with(&[p.clone()], export::BibtexOptions { max_authors: 0, shown_authors: 3 }, &Default::default());
    assert!(all.contains("Family60, Given60}") && !all.contains("others"));
    let three = export::bibtex_with(&[p.clone()], export::BibtexOptions { max_authors: 3, shown_authors: 9 }, &Default::default());
    assert!(three.contains("author = {Family1, Given1 and Family2, Given2 and Family3, Given3 and others},"), "{three}");
    // read back: "others" isn't an author
    assert_eq!(import::parse_bibtex(&bib)[0].meta.authors.len(), 10);

    // the library's setting, synced through the log and kept on reopening
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let added = lib.add_paper(meta, None, None, None).unwrap();
    assert_eq!(lib.bibtex_options(), export::BibtexOptions { max_authors: 15, shown_authors: 10 });
    assert!(lib.export_bibtex(vec![added.id.clone()]).contains("Family10, Given10 and others}"));
    lib.set_bibtex_options(export::BibtexOptions { max_authors: 5, shown_authors: 1 }).unwrap();
    assert!(lib.export_bibtex(vec![added.id.clone()]).contains("author = {Family1, Given1 and others},"));
    let other = open(&root, "iphone");
    assert_eq!(other.bibtex_options(), export::BibtexOptions { max_authors: 5, shown_authors: 1 });
    // a paper's own fields (from an imported .bib) win over the list
    other.set_bib_fields(added.id.clone(), vec![BibField { name: "Author".into(), value: "{Qwen Team}".into() }, BibField { name: "type".into(), value: "Technical Report".into() }]).unwrap();
    let own = other.export_bibtex(vec![added.id.clone()]);
    assert!(own.contains("  author = {{Qwen Team}},\n") && own.contains("  type = {Technical Report},\n}"), "{own}");
    other.set_bib_fields(added.id.clone(), vec![]).unwrap();
    assert!(other.bib_fields(added.id.clone()).is_empty());
}

// ---- big reading views, chapter by chapter (epub::reading_parts) ----

/// A book's unpacked sections in a folder, as `epub::open` reads them: `secs` are (file, body).
fn sample_book(secs: &[(&str, &str)]) -> epub::EpubBook {
    let dir = tmp();
    let text = dir.join("EPUB/text");
    std::fs::create_dir_all(&text).unwrap();
    std::fs::create_dir_all(dir.join("EPUB/images")).unwrap();
    // a PNG's header (its size is all that is read): 1082 × 821
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&[0, 0, 4, 0x3a, 0, 0, 3, 0x35]);
    std::fs::write(dir.join("EPUB/images/p1-eq1.png"), &png).unwrap();
    let chapters: Vec<epub::EpubChapter> = secs
        .iter()
        .map(|(name, body)| {
            let p = text.join(name);
            let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><meta charset=\"UTF-8\"/><title>{name}</title><link rel=\"stylesheet\" href=\"../css/style.css\"/></head>\n<body>{body}</body>\n</html>\n");
            std::fs::write(&p, doc).unwrap();
            epub::EpubChapter { title: String::new(), path: p.to_string_lossy().into() }
        })
        .collect();
    let combined = epub::combine(&chapters).unwrap();
    epub::EpubBook { dir: dir.to_string_lossy().into(), combined, chapters, toc: vec![] }
}

fn utf16(s: &str) -> Vec<u16> { s.encode_utf16().collect() }

#[test]
fn reading_parts_are_slices_of_the_continuous_view() {
    let words = |n: usize, w: &str| vec![w; n].join(" ");
    let long = words(2600, "word");
    let book = sample_book(&[
        ("cover.xhtml", "<img src=\"../images/cover.png\"/>"),
        ("title.xhtml", "<h1 class=\"title\">A Book</h1><p>By someone &amp; another</p>"),
        ("ch01.xhtml", &format!("<section><h1 id=\"s1\">1 Start</h1><p>{long} see <a href=\"ch03.xhtml#s3\">chapter 3</a> and <a href=\"ch01.xhtml#s1\">here</a>.</p></section>")),
        ("ch02.xhtml", &format!("<section><h1 id=\"s2\">2 Math 𝑥</h1><p>Non-BMP 𝑥 and é &#8212; {long}</p><img class=\"equation\" src=\"../images/p1-eq1.png\" style=\"width: 3em\"/></section>")),
        ("ch03.xhtml", &format!("<section><h1 id=\"s3\">3 More</h1><p>{long} (Smith, 2019)</p><!-- a comment --><p>x<em>y</em>z</p></section>")),
        ("ch04.xhtml", "<section><h1 id=\"s4\">Bibliography</h1><ol><li>Smith, J. (2019). A paper. 12</li></ol></section>"),
    ]);

    // small: not chaptered (but parts can be made)
    assert!(!epub::reading_parts(&book, false).unwrap().chaptered);
    let r = epub::reading_parts(&book, true).unwrap();
    // the title page shares a part with what follows (it's short); after a long chapter a new part
    let secs: Vec<Vec<u32>> = r.parts.iter().map(|p| p.sections.clone()).collect();
    assert_eq!(secs, vec![vec![1, 2], vec![3], vec![4], vec![5]], "{secs:?}");
    assert_eq!(r.bibliography, Some(3));
    assert_eq!(r.parts[0].title, "A Book");

    // every part's text is the continuous view's text from `start`, as the page's DOM has it
    let combined = utf16(&highlight::dom_text(&std::fs::read_to_string(&book.combined).unwrap()));
    for p in &r.parts {
        let file = std::fs::read_to_string(&p.path).unwrap();
        let t = utf16(&highlight::dom_text(&file));
        assert_eq!(t, utf16(&p.text));
        assert_eq!(t.len() as u64, p.length);
        let s = p.start as usize;
        assert_eq!(&combined[s..s + t.len()], &t[..], "part {}", p.path);
        assert!(file.contains(&format!("<meta name=\"lunta-text-base\" content=\"{}\"/>", p.start)));
    }
    let last = r.parts.last().unwrap();
    assert_eq!((last.start + last.length) as usize, combined.len());
    // a passage found in a part is at start + its offset in the whole view
    let q = utf16("Non-BMP 𝑥 and é");
    let local = r.parts[1].text.encode_utf16().collect::<Vec<_>>().windows(q.len()).position(|w| w == &q[..]).unwrap();
    let global = combined.windows(q.len()).position(|w| w == &q[..]).unwrap();
    assert_eq!(r.parts[1].start as usize + local, global);

    // links: into another part go to its file, within the part stay in the page
    let p0 = std::fs::read_to_string(&r.parts[0].path).unwrap();
    assert!(p0.contains("href=\"lunta-part-2.xhtml#s3\"") && p0.contains("href=\"#s1\""), "{p0}");
    // the way on: an empty link (no text) to the next part, labelled by CSS; none before the first
    assert!(p0.contains("<a class=\"lunta-part-nav lunta-next\" href=\"lunta-part-1.xhtml\" data-t=\"Next: 2 Math 𝑥\"></a>"));
    assert!(!p0.contains("lunta-prev"));
    let p1 = std::fs::read_to_string(&r.parts[1].path).unwrap();
    assert!(p1.contains("lunta-prev\" href=\"lunta-part-0.xhtml\""));
    // images: lazy, decoded off the main thread, with their size so the page keeps its layout
    assert!(p1.contains("src=\"../images/p1-eq1.png\" style=\"width: 3em\" loading=\"lazy\" decoding=\"async\" width=\"1082\" height=\"821\"/>"), "{p1}");
    let all = std::fs::read_to_string(&book.combined).unwrap();
    assert!(all.contains("loading=\"lazy\" decoding=\"async\" width=\"1082\" height=\"821\""));
    assert!(!all.contains("cover.png"), "the cover isn't in the reading view");
}

#[test]
fn big_reading_views_are_chaptered() {
    // a textbook (204,000 words, 677 images, 29 chapters): chapter by chapter
    assert!(epub::chaptered(29, 204_000, 677, 50_000_000));
    // many images or many megabytes of them, even with fewer words
    assert!(epub::chaptered(12, 30_000, 160, 8_000_000));
    assert!(epub::chaptered(12, 30_000, 40, 35_000_000));
    // papers, also long reviews with many figures: one page as before
    assert!(!epub::chaptered(9, 15_000, 40, 12_000_000));
    assert!(!epub::chaptered(20, 45_000, 120, 25_000_000));
    // a long text in two or three sections can't be split
    assert!(!epub::chaptered(3, 200_000, 0, 0));
}

#[test]
fn old_continuous_views_are_made_again_with_lazy_images() {
    let book = sample_book(&[("ch01.xhtml", "<p>One <img src=\"../images/a.png\"/></p>")]);
    // a view made before lazy images (no mark): made again
    std::fs::write(&book.combined, "<html><body class=\"continuous\">\n<p>old</p></body></html>").unwrap();
    let again = epub::combine(&book.chapters).unwrap();
    let t = std::fs::read_to_string(again).unwrap();
    assert!(t.contains("data-lunta-view=\"2\"") && t.contains("loading=\"lazy\"") && !t.contains("old"), "{t}");
}

#[test]
fn image_sizes_from_headers() {
    let d = tmp();
    let gif = d.join("a.gif");
    std::fs::write(&gif, b"GIF89a\x10\x00\x20\x00rest").unwrap();
    assert_eq!(epub::image_size(&gif), Some((16, 32)));
    // JPEG: an APP0 segment, then the frame header
    let jpg = d.join("a.jpg");
    let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
    j.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0x2C, 0x02, 0x58, 0x03]);
    j.extend_from_slice(&[0; 20]);
    std::fs::write(&jpg, &j).unwrap();
    assert_eq!(epub::image_size(&jpg), Some((600, 300)));
    let txt = d.join("a.txt");
    std::fs::write(&txt, b"not an image").unwrap();
    assert_eq!(epub::image_size(&txt), None);
}

#[test]
fn a_real_book_reads_chapter_by_chapter() {
    // LUNTA_TEST_BOOK_EPUB=/path/to/understanding-deep-learning.epub cargo test a_real_book
    let Ok(path) = std::env::var("LUNTA_TEST_BOOK_EPUB") else { return };
    let root = tmp().join("lib");
    let lib = open(&root, "mac");
    let p = lib.add_paper(PaperMeta { title: "Book".into(), ..Default::default() }, None, Some(std::fs::read(&path).unwrap()), None).unwrap();
    let book = lib.open_epub(p.id.clone()).unwrap();
    let t0 = std::time::Instant::now();
    let r = lib.reading_parts(p.id.clone()).unwrap();
    eprintln!("{} parts, {} words, {} images, {} MB of images, {:?} (bibliography {:?})", r.parts.len(), r.words, r.images, r.image_bytes / 1_000_000, t0.elapsed(), r.bibliography);
    for x in &r.parts { eprintln!("  {:>7} {:>6} words {:>4} images  {}", x.start, x.words, x.images, x.title); }
    assert!(r.chaptered);
    let combined = utf16(&highlight::dom_text(&std::fs::read_to_string(&book.combined).unwrap()));
    for x in &r.parts {
        let t = utf16(&x.text);
        assert_eq!(&combined[x.start as usize..x.start as usize + t.len()], &t[..]);
    }
}
