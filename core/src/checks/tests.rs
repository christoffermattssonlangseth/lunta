//! Tests for the checks, on answers recorded from the services (`testdata/checks/`, no network).
//! `cargo test checks -- --ignored record_check_fixtures` asks the services again and re-records
//! them (and is the live test).

use super::net::*;
use super::refcheck::*;
use super::*;
use crate::model::*;
use crate::Library;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata/checks")
}

fn lib() -> (tempfile::TempDir, Arc<Library>) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("lib");
    let cache = d.path().join("cache");
    let l = Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), "test".into()).unwrap();
    (d, l)
}

/// A manuscript's .bib: a reference that is right, one with a wrong year and a missing author,
/// one that doesn't exist, one whose DOI is another paper's, a retracted paper, and software.
const BIB: &str = r#"
@article{lecun2015,
  author  = {LeCun, Yann and Bengio, Yoshua and Hinton, Geoffrey},
  title   = {Deep learning},
  journal = {Nature},
  volume  = {521},
  number  = {7553},
  pages   = {436--444},
  year    = {2015},
  doi     = {10.1038/nature14539}
}
@article{lecun2016,
  author  = {LeCun, Yann and Bengio, Yoshua},
  title   = {Deep Learning},
  journal = {Nat.},
  volume  = {521},
  pages   = {436--444},
  year    = {2016},
  doi     = {10.1038/nature14539}
}
@article{smithers2021,
  author  = {Smithers, Annabel and Okonkwo, Daniel},
  title   = {Quantum coherence of mitochondrial lattices governs memory consolidation in hippocampal astrocytes},
  journal = {Journal of Cellular Neurodynamics},
  volume  = {14},
  pages   = {201--219},
  year    = {2021}
}
@inproceedings{vaswani2017,
  author    = {Vaswani, Ashish and Shazeer, Noam and Parmar, Niki and Uszkoreit, Jakob and Jones, Llion and Gomez, Aidan N. and Kaiser, Lukasz and Polosukhin, Illia},
  title     = {Attention Is All You Need},
  booktitle = {Advances in Neural Information Processing Systems},
  year      = {2017},
  doi       = {10.1038/nature14539}
}
@article{wakefield1998,
  author  = {Wakefield, A. J. and Murch, S. H. and Anthony, A. and others},
  title   = {Ileal-lymphoid-nodular hyperplasia, non-specific colitis, and pervasive developmental disorder in children},
  journal = {The Lancet},
  volume  = {351},
  pages   = {637--641},
  year    = {1998},
  doi     = {10.1016/S0140-6736(97)11096-0}
}
@misc{zotero,
  author       = {{Corporation for Digital Scholarship}},
  title        = {Zotero},
  howpublished = {\url{https://www.zotero.org}},
  year         = {2025}
}
"#;

/// The same kind of list pasted from a manuscript, numbered, without DOIs.
const PASTED: &str = "1. LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521(7553):436-44.
2. Kendall MG. A new measure of rank correlation. Biometrika. 1938;30(1/2):81-93.
3. Smithers A, Okonkwo D. Quantum coherence of mitochondrial lattices governs memory
   consolidation in hippocampal astrocytes. J Cell Neurodyn. 2021;14:201-19.";

fn check_bib(net: &dyn Net) -> RefCheckReport {
    check_all(net, &parse_input(BIB), &|_| {})
}

fn check_pasted(net: &dyn Net) -> RefCheckReport {
    check_all(net, &parse_input(PASTED), &|_| {})
}

const RETRACTED: &str = "10.1016/S0140-6736(97)11096-0";
const BIORXIV: &str = "10.1101/2020.03.22.002386";
const NATURE_VERSION: &str = "10.1038/s41586-020-2286-9";

/// A library: the retracted paper, a bioRxiv preprint since published in Nature, an arXiv
/// preprint (never published in a journal), and a published paper that has an arXiv id too.
fn updates_library(net: &dyn Net) -> (tempfile::TempDir, Arc<Library>, [String; 4], super::updates::UpdatesSummary) {
    let (d, l) = lib();
    let add = |m: PaperMeta| l.add_paper(m, None, None, None).unwrap().id;
    let retracted = add(PaperMeta {
        title: "Ileal-lymphoid-nodular hyperplasia, non-specific colitis, and pervasive developmental disorder in children".into(),
        authors: vec!["A.J Wakefield".into()],
        date: "1998-02".into(),
        journal: "The Lancet".into(),
        doi: RETRACTED.into(),
        work_type: "journal-article".into(),
        ..Default::default()
    });
    let preprint = add(PaperMeta {
        title: "A SARS-CoV-2-Human Protein-Protein Interaction Map Reveals Drug Targets and Potential Drug-Repurposing".into(),
        authors: vec!["David E. Gordon".into(), "Gwendolyn M. Jang".into()],
        date: "2020-03-22".into(),
        journal: "bioRxiv".into(),
        doi: BIORXIV.into(),
        work_type: "posted-content".into(),
        ..Default::default()
    });
    let arxiv = add(PaperMeta {
        title: "Attention Is All You Need".into(),
        authors: vec!["Ashish Vaswani".into(), "Noam Shazeer".into()],
        date: "2017-06-12".into(),
        journal: "arXiv".into(),
        doi: "10.48550/arXiv.1706.03762".into(),
        arxiv: "1706.03762".into(),
        work_type: "posted-content".into(),
        ..Default::default()
    });
    let published = add(PaperMeta {
        title: "Deep learning".into(),
        authors: vec!["Yann LeCun".into(), "Yoshua Bengio".into(), "Geoffrey Hinton".into()],
        date: "2015-05-27".into(),
        journal: "Nature".into(),
        doi: "10.1038/nature14539".into(),
        arxiv: "1206.5538".into(),
        work_type: "journal-article".into(),
        ..Default::default()
    });
    let r = l.check_updates_with(net, &[], &|_| {}).unwrap();
    (d, l, [retracted, preprint, arxiv, published], r)
}

// ---------------------------------------------------------------- recorded answers

fn verdicts(r: &RefCheckReport) -> Vec<(String, RefVerdict)> {
    r.items.iter().map(|i| (i.key.clone(), i.verdict)).collect()
}

#[test]
fn checks_bib_verified_minor_not_found_wrong_doi() {
    let net = Fixtures::open(fixtures_dir());
    let r = check_bib(&net);
    assert_eq!(
        verdicts(&r),
        vec![
            ("lecun2015".into(), RefVerdict::Verified),
            ("lecun2016".into(), RefVerdict::MinorDifferences),
            ("smithers2021".into(), RefVerdict::NotFound),
            ("vaswani2017".into(), RefVerdict::WrongDoi),
            ("wakefield1998".into(), RefVerdict::Verified),
            ("zotero".into(), RefVerdict::NotChecked),
        ],
        "{}",
        r.markdown
    );
    // the minor differences: the year and the number of authors (the venue's abbreviation is fine)
    let minor = &r.items[1];
    let fields: Vec<&str> = minor.differences.iter().map(|d| d.field.as_str()).collect();
    assert_eq!(fields, vec!["number of authors", "year"], "{:?}", minor.differences);
    assert_eq!(minor.differences[1].given, "2016");
    assert_eq!(minor.differences[1].found, "2015");
    // not found: said as such, never "fake"
    assert_eq!(r.items[2].note, "Could not be found in Crossref, OpenAlex or PubMed.");
    assert!(!r.markdown.to_lowercase().contains("fake"));
    // the wrong DOI: what it points to, and the reference's own record (OpenAlex: NeurIPS isn't in Crossref)
    let wrong = &r.items[3];
    assert_eq!(wrong.found.as_ref().unwrap().doi.to_lowercase(), "10.1038/nature14539");
    assert!(wrong.note.starts_with("The DOI 10.1038/nature14539 points to a different work: “Deep learning”"), "{}", wrong.note);
    assert!(wrong.note.contains("OpenAlex") || !wrong.suggested_doi.is_empty(), "{}", wrong.note);
    // the retracted paper: verified, and flagged
    let w = &r.items[4];
    assert_eq!(level(&w.notices), NoticeLevel::Retracted);
    assert_eq!(notice_sentence(&w.notices, "The work"), "The work was retracted on 6 February 2010 (and 1 more notice).");
    assert_eq!(r.retracted, 1);
    assert_eq!((r.verified, r.minor, r.not_found, r.wrong_doi, r.not_checked, r.failed), (2, 1, 1, 1, 1, 0));
    assert!(r.summary.starts_with("6 references: 2 verified, 1 with minor differences"), "{}", r.summary);
    // the exports
    assert!(r.markdown.contains("## Could not be found in Crossref, OpenAlex or PubMed (1)"));
    assert!(r.markdown.contains("<https://doi.org/10.1038/nature14539>"));
    assert!(r.csv.lines().next().unwrap().starts_with("number,key,result,reference"));
    assert_eq!(r.csv.lines().count(), 7);
    assert!(r.csv.contains("wakefield1998"));
}

#[test]
fn checks_a_pasted_numbered_list() {
    let net = Fixtures::open(fixtures_dir());
    let r = check_pasted(&net);
    assert_eq!(r.items.len(), 3);
    assert_eq!(r.items[0].verdict, RefVerdict::Verified, "{:?}", r.items[0]);
    assert_eq!(r.items[0].found.as_ref().unwrap().doi.to_lowercase(), "10.1038/nature14539");
    assert_eq!(r.items[1].verdict, RefVerdict::Verified, "{:?}", r.items[1]);
    assert_eq!(r.items[2].verdict, RefVerdict::NotFound, "{:?}", r.items[2]);
    assert_eq!(r.items[2].key, "3");
}

#[test]
fn library_retractions_and_published_preprints() {
    let net = Fixtures::open(fixtures_dir());
    let (_d, l, [retracted, preprint, arxiv, published], r) = updates_library(&net);
    // the retraction (publisher's and Retraction Watch's records), and an older correction
    let u = l.paper_updates(retracted.clone()).unwrap();
    assert_eq!(updates_level(u.clone()), NoticeLevel::Retracted);
    assert_eq!(updates_headline(u.clone()), "This paper was retracted on 6 February 2010 (and 1 more notice).");
    let main = main_notice(&u.notices).unwrap();
    assert_eq!(main.doi, "10.1016/s0140-6736(10)60175-4");
    assert!(main.source.contains("Retraction Watch"));
    // the bioRxiv preprint's Nature version
    let v = l.paper_updates(preprint.clone()).unwrap().published.unwrap();
    assert_eq!(v.doi.to_lowercase(), NATURE_VERSION);
    assert_eq!(v.found_by, "Crossref (has-preprint)");
    assert_eq!(v.journal, "Nature");
    assert!(v.title.starts_with("A SARS-CoV-2 protein interaction map"), "{}", v.title);
    // nothing for the others: a published paper is never offered its preprint
    assert!(l.paper_updates(published.clone()).is_none_or(|u| u.published.is_none()));
    assert!(l.paper_updates(arxiv.clone()).is_none_or(|u| u.published.is_none()));
    assert_eq!(r.retracted, vec![retracted.clone()]);
    assert_eq!(r.published, vec![preprint.clone()]);
    assert_eq!(r.changed.len(), 2);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.text, "Checked 4 papers: 1 retracted; 1 preprint has been published.");

    // checked again: nothing new, nothing written
    let ops = |l: &Library| l.inner.lock().unwrap().store.replay().unwrap();
    let _ = ops(&l);
    let again = l.check_updates_with(&net, &[], &|_| {}).unwrap();
    assert!(again.changed.is_empty());

    // another device sees the same (through the log)
    let root = l.inner.lock().unwrap().store.root.clone();
    let other = Library::open(root.to_string_lossy().into(), _d.path().join("cache2").to_string_lossy().into(), "other".into()).unwrap();
    assert_eq!(other.all_paper_updates(), l.all_paper_updates());
}

#[test]
fn switching_to_the_published_version_keeps_highlights_and_undoes() {
    let net = Fixtures::open(fixtures_dir());
    let (_d, l, [retracted, preprint, _, _], _) = updates_library(&net);
    let before = l.paper(preprint.clone()).unwrap();
    let h = l
        .save_annotation(Annotation {
            id: String::new(),
            paper_id: preprint.clone(),
            kind: AnnotationKind::Highlight,
            color: "yellow".into(),
            location: "epub".into(),
            position: "0".into(),
            quote: "332 high confidence".into(),
            prefix: String::new(),
            suffix: String::new(),
            note: "key result".into(),
            created: 0,
            modified: 0,
        })
        .unwrap();
    l.set_tag(preprint.clone(), "covid".into(), true).unwrap();
    // the published record (as `metadata::resolve` would give it), from the recorded Crossref answer
    let published = crate::checks::refcheck::crossref_doi(&net, NATURE_VERSION).unwrap().unwrap().meta;
    let after = l.switch_to(&preprint, published).unwrap();
    assert_eq!(after.meta.doi.to_lowercase(), NATURE_VERSION);
    assert_eq!(after.meta.journal, "Nature");
    assert_eq!(after.meta.work_type, "journal-article");
    assert_eq!(after.cite_key, before.cite_key, "the citation key stays");
    assert_eq!(l.annotations(preprint.clone()), vec![h.clone()], "highlights stay");
    assert!(after.tags.contains(&"covid".to_string()));
    let u = l.paper_updates(preprint.clone()).unwrap();
    assert_eq!(u.preprint.as_ref().unwrap().doi, BIORXIV);
    assert!(u.published.is_none());
    assert!(u.switched_at > 0);
    // a published paper is never switched (to anything)
    let pub_meta = l.paper(retracted.clone()).unwrap().meta;
    assert!(l.switch_to(&retracted, pub_meta).is_err());
    // undo: the preprint again, the published version suggested again
    let back = l.undo_published_version(preprint.clone()).unwrap();
    assert_eq!(back.meta.doi, BIORXIV);
    assert_eq!(back.cite_key, before.cite_key);
    assert_eq!(l.paper_updates(preprint.clone()).unwrap().published.unwrap().doi.to_lowercase(), NATURE_VERSION);
    // link both, then keep the preprint: not suggested again
    l.link_published_version(preprint.clone()).unwrap();
    let u = l.paper_updates(preprint.clone()).unwrap();
    assert!(u.published.is_none() && u.linked.is_some());
    l.keep_preprint(preprint.clone()).unwrap();
    let u = l.paper_updates(preprint.clone()).unwrap();
    assert!(u.published.is_none() && u.linked.is_none());
    assert_eq!(u.declined_doi.to_lowercase(), NATURE_VERSION);
    let r = l.check_updates_with(&net, &[preprint.clone()], &|_| {}).unwrap();
    assert!(r.published.is_empty(), "a declined version isn't suggested again");
}

// ---------------------------------------------------------------- caching and pacing

/// Answers every URL with its length, and counts the requests.
struct Counting {
    n: AtomicUsize,
}

impl Net for Counting {
    fn get(&self, url: &str) -> crate::model::Result<Option<String>> {
        self.n.fetch_add(1, Ordering::SeqCst);
        if url.contains("missing") {
            return Ok(None);
        }
        if url.contains("broken") {
            return Err(CoreError::Network("offline".into()));
        }
        Ok(Some(url.len().to_string()))
    }
}

#[test]
fn answers_are_cached_and_kept_on_disk() {
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("checks/answers.json");
    let inner = Counting { n: AtomicUsize::new(0) };
    {
        let c = Cached::new(&inner, Duration::from_secs(3600), Some(file.clone()));
        assert_eq!(c.get("https://a.org/x").unwrap(), Some("15".into()));
        assert_eq!(c.get("https://a.org/x").unwrap(), Some("15".into()));
        assert_eq!(c.get("https://a.org/missing").unwrap(), None);
        assert_eq!(c.get("https://a.org/missing").unwrap(), None);
        assert!(c.get("https://a.org/broken").is_err());
        assert!(c.get("https://a.org/broken").is_err(), "errors aren't kept");
        assert_eq!(inner.n.load(Ordering::SeqCst), 4);
    }
    // the file outlives the cache: a new one asks nothing
    let c = Cached::new(&inner, Duration::from_secs(3600), Some(file.clone()));
    assert_eq!(c.get("https://a.org/x").unwrap(), Some("15".into()));
    assert_eq!(c.get("https://a.org/missing").unwrap(), None);
    assert_eq!(inner.n.load(Ordering::SeqCst), 4);
    // too old: asked again
    let stale = Cached::new(&inner, Duration::from_secs(0), Some(file));
    stale.get("https://a.org/x").unwrap();
    assert_eq!(inner.n.load(Ordering::SeqCst), 5);
}

#[test]
fn requests_to_a_service_are_paced() {
    fn every_40ms(h: &str) -> Duration {
        if h == "slow.org" { Duration::from_millis(40) } else { Duration::ZERO }
    }
    let p = Pacer::new(every_40ms);
    let t = Instant::now();
    for _ in 0..4 {
        p.wait("https://slow.org/a");
    }
    assert!(t.elapsed() >= Duration::from_millis(120), "{:?}", t.elapsed());
    // other services aren't held up
    let t = Instant::now();
    p.wait("https://fast.org/a");
    p.wait("https://fast.org/b");
    assert!(t.elapsed() < Duration::from_millis(30));
    // threads share the pace
    let p = Arc::new(Pacer::new(every_40ms));
    let t = Instant::now();
    let hs: Vec<_> = (0..3).map(|_| { let p = p.clone(); std::thread::spawn(move || p.wait("https://slow.org/x")) }).collect();
    hs.into_iter().for_each(|h| h.join().unwrap());
    assert!(t.elapsed() >= Duration::from_millis(80));
    // polite intervals for the real services
    assert_eq!(interval("export.arxiv.org"), Duration::from_secs(3));
    assert!(interval("api.crossref.org") >= Duration::from_millis(200));
    assert!(USER_AGENT.contains("github.com/christoffermattssonlangseth/lunta") && !USER_AGENT.contains('@'));
}

// ---------------------------------------------------------------- reading references

#[test]
fn reads_pasted_lists() {
    let numbered = split_list(PASTED);
    assert_eq!(numbered.len(), 3);
    assert_eq!(numbered[2].0, "3");
    assert!(numbered[2].1.ends_with("memory consolidation in hippocampal astrocytes. J Cell Neurodyn. 2021;14:201-19."));
    let author_year = "Kendall, M. G. (1938). A new measure of rank correlation. Biometrika, 30, 81–93.\nLeCun, Y., Bengio, Y., & Hinton, G. (2015). Deep learning.\n   Nature, 521, 436–444. https://doi.org/10.1038/\nnature14539\n";
    let e = split_list(author_year);
    assert_eq!(e.len(), 2, "{e:?}");
    assert!(e[1].1.ends_with("https://doi.org/10.1038/nature14539"), "{}", e[1].1);
    let paragraphs = "Smith J. One title here. J Biol. 2001;1:1-2.\n\nJones K. Another\ntitle there. Cell. 2002;2:3-4.";
    assert_eq!(split_list(paragraphs).len(), 2);
    assert_eq!(guess_title("Kendall, M. G. (1938). A new measure of rank correlation. Biometrika, 30, 81–93."), "A new measure of rank correlation");
    assert_eq!(guess_title("LeCun Y, Bengio Y, Hinton G. Deep learning in the brain and beyond. Nature. 2015;521(7553):436-44."), "Deep learning in the brain and beyond");
    assert_eq!(count_references(BIB.into()), 6);
}

#[test]
fn compares_venues_titles_and_pages() {
    assert!(same_venue("Nat. Neurosci.", "Nature Neuroscience"));
    assert!(same_venue("J Biol Chem", "Journal of Biological Chemistry"));
    assert!(same_venue("PNAS", "Proceedings of the National Academy of Sciences"));
    assert!(same_venue("The Lancet", "Lancet"));
    assert!(same_venue("Proceedings of the 36th Annual ACM Symposium on User Interface Software and Technology (UIST)", "Proceedings of the 36th Annual ACM Symposium on User Interface Software and Technology"));
    assert!(!same_venue("Cell", "Nature"));
    assert!(same_title("Deep Learning", "Deep learning."));
    assert!(same_title("PDFFigures 2.0", "PDFFigures 2.0: Mining Figures from Research Papers"));
    assert!(!same_title("Deep learning", "Attention is all you need"));
    assert_eq!(first_page("436–44"), "436");
    assert_eq!(first_page("e1002"), "e1002");
    assert_eq!(long_date("2010-02-06"), "6 February 2010");
}

#[test]
fn notices_from_crossref() {
    let m = serde_json::json!({"updated-by": [
        {"DOI": "10.1/n", "type": "retraction", "label": "Retraction", "source": "publisher", "updated": {"date-parts": [[2021, 12, 15]]}},
        {"DOI": "10.1/N", "type": "retraction", "label": "Retraction", "source": "retraction-watch", "updated": {"date-parts": [[2021, 12, 15]]}},
        {"DOI": "10.1/c", "type": "expression-of-concern", "label": "Expression of concern", "source": "retraction-watch", "updated": {"date-parts": [[2020, 1]]}},
        {"DOI": "10.1/v", "type": "new_version", "source": "publisher", "updated": {"date-parts": [[2019]]}}
    ]});
    let n = crossref_notices(&m);
    assert_eq!(n.len(), 2);
    assert_eq!(n[1].source, "publisher, Retraction Watch");
    assert_eq!(n[0].kind, "expression_of_concern");
    assert_eq!(level(&n), NoticeLevel::Retracted);
    assert_eq!(notice_sentence(&n[..1], "This paper"), "This paper has an expression of concern in January 2020.");
}

#[test]
fn reading_view_reference_lists() {
    let html = r#"<h2>References</h2><ol class="refs"><li id="r1">LeCun Y, Bengio Y, Hinton G. <i>Deep learning</i>. Nature 2015;521:436&#8211;444.</li><li>x</li><li>Kendall MG. A new measure of rank correlation. Biometrika. 1938;30:81&amp;93.</li></ol>"#;
    let refs = html_references(html);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0], "LeCun Y, Bengio Y, Hinton G. Deep learning . Nature 2015;521:436–444.");
    assert!(refs[1].contains("81&93"));
}

// ---------------------------------------------------------------- online

/// Asks the services (and records their answers as the fixtures above use them). Run with
/// `cargo test checks -- --ignored record_check_fixtures --nocapture`.
#[test]
#[ignore]
fn record_check_fixtures() {
    let dir = fixtures_dir();
    let _ = std::fs::remove_dir_all(&dir);
    let live = LiveNet;
    let rec = Recorder { inner: &live, dir: dir.clone() };
    let r = check_bib(&rec);
    println!("{}", r.markdown);
    let p = check_pasted(&rec);
    println!("{}", p.markdown);
    let (_d, l, ids, u) = updates_library(&rec);
    println!("{}", u.text);
    let _ = crate::checks::refcheck::crossref_doi(&rec, NATURE_VERSION);
    l.keep_preprint(ids[1].clone()).unwrap();
    let _ = l.check_updates_with(&rec, &[ids[1].clone()], &|_| {});
    assert!(Fixtures::open(dir).get("https://api.crossref.org/works/10.1038/nature14539").unwrap().is_some());
}
