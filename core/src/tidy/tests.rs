//! Tests for Tidy Library, on temporary libraries; DOI lookups on answers recorded from Crossref
//! (`testdata/tidy/`). `cargo test tidy -- --ignored record_tidy_fixtures` asks Crossref again
//! and re-records them (and is the live test).

use super::*;
use crate::checks::net::{Fixtures, LiveNet, Recorder};
use crate::checks::{PaperUpdates, PublishedVersion};
use crate::model::*;
use crate::store::OpKind;
use crate::Library;
use std::path::PathBuf;
use std::sync::Arc;

fn lib() -> (tempfile::TempDir, Arc<Library>) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("lib");
    let cache = d.path().join("cache");
    let l = Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), "test".into()).unwrap();
    (d, l)
}

fn meta(title: &str, first: &str, date: &str, journal: &str, doi: &str) -> PaperMeta {
    PaperMeta { title: title.into(), authors: vec![first.into(), "Someone Else".into()], date: date.into(), journal: journal.into(), doi: doi.into(), ..Default::default() }
}

/// Adds a paper, even when another has the same DOI (add_paper would attach to that one).
fn add(l: &Library, m: PaperMeta) -> String {
    let doi = m.doi.clone();
    let id = l.add_paper(PaperMeta { doi: String::new(), ..m.clone() }, None, None, None).unwrap().id;
    if !doi.is_empty() {
        l.update_meta(id.clone(), m).unwrap();
    }
    id
}

const KUKANJA: &str = "Cellular architecture of evolving neuroinflammatory lesions and multiple sclerosis pathology";

// ---------------------------------------------------------------- duplicates

#[test]
fn duplicates_by_doi_and_by_a_title_that_differs_a_little() {
    let (_d, l) = lib();
    let a = add(&l, meta(KUKANJA, "Petra Kukanja", "2024-04-11", "Cell", "10.1016/j.cell.2024.02.030"));
    // the same DOI written as a link, in capitals
    let b = add(&l, meta(KUKANJA, "Kukanja, Petra", "2024", "Cell", "https://doi.org/10.1016/J.CELL.2024.02.030"));
    // the title with a hyphen, a full stop and other capitals, no DOI, a year off by one
    let c = add(&l, meta("Cellular Architecture of Evolving Neuro-inflammatory Lesions and Multiple Sclerosis Pathology.", "P. Kukanja", "2023", "", ""));
    // a word left out of a long title, no DOI
    let e = add(&l, meta("Spatial transcriptomics of the adult mouse brain reveals regional cell type diversity", "Ann Ortiz", "2020", "Science Advances", ""));
    let f = add(&l, meta("Spatial transcriptomics of the adult mouse brain reveals regional cell-type diversity", "Ann Ortiz", "2020", "", ""));
    // not duplicates: another first author; short titles of different works; years far apart;
    // the same title with two different DOIs (a paper and its correction)
    let _ = add(&l, meta(KUKANJA, "John Smith", "2024", "Cell", ""));
    let _ = add(&l, meta("Deep learning", "Yann LeCun", "2015", "Nature", "10.1038/nature14539"));
    let _ = add(&l, meta("Deep learning", "Ian Goodfellow", "2016", "MIT Press", ""));
    let _ = add(&l, meta("Spatial transcriptomics of the adult mouse brain reveals regional cell type diversity", "Ann Ortiz", "2009", "", ""));
    let _ = add(&l, meta("Single-cell analysis of the developing human retina", "Lee Park", "2021", "Cell Reports", "10.1016/j.celrep.2021.01.001"));
    let _ = add(&l, meta("Single-cell analysis of the developing human retina", "Lee Park", "2021", "Cell Reports", "10.1016/j.celrep.2021.01.002"));

    let groups = l.tidy_duplicates();
    assert_eq!(groups.len(), 2, "{groups:#?}");
    let g = groups.iter().find(|g| g.paper_ids.contains(&a)).unwrap();
    assert_eq!(g.reason, DuplicateReason::SameDoi);
    let mut want = vec![a.clone(), b.clone(), c.clone()];
    want.sort();
    let mut got = g.paper_ids.clone();
    got.sort();
    assert_eq!(got, want);
    assert_eq!(g.key, duplicates::group_key(&want));
    let g2 = groups.iter().find(|g| g.paper_ids.contains(&e)).unwrap();
    assert_eq!(g2.reason, DuplicateReason::SimilarTitle);
    assert!(g2.paper_ids.contains(&f));
    // the one with the journal is kept (more complete)
    assert_eq!(g2.keep, e);

    // "Not Duplicates"
    l.tidy_dismiss(g2.key.clone()).unwrap();
    assert_eq!(l.tidy_duplicates().len(), 1);
    l.tidy_reset_dismissed().unwrap();
    assert_eq!(l.tidy_duplicates().len(), 2);
}

#[test]
fn a_preprint_and_its_published_version() {
    let (_d, l) = lib();
    let pre = add(
        &l,
        PaperMeta {
            work_type: "posted-content".into(),
            ..meta("A SARS-CoV-2-human protein-protein interaction map reveals drug targets and potential drug-repurposing", "David E. Gordon", "2020-03-22", "bioRxiv", "10.1101/2020.03.22.002386")
        },
    );
    let publ = add(
        &l,
        PaperMeta {
            work_type: "journal-article".into(),
            ..meta("A SARS-CoV-2 protein interaction map reveals targets for drug repurposing", "David E. Gordon", "2020-04-30", "Nature", "10.1038/s41586-020-2286-9")
        },
    );
    let groups = l.tidy_duplicates();
    assert_eq!(groups.len(), 1, "{groups:#?}");
    assert_eq!(groups[0].reason, DuplicateReason::PreprintAndPublished);
    assert_eq!(groups[0].keep, publ, "the published version is kept");
    assert_eq!(groups[0].paper_ids[0], publ);
    assert!(groups[0].note.contains("Switch to the Published Version"));

    // a preprint whose published version (found by the checks) has another title: paired by DOI
    let pre2 = add(&l, PaperMeta { work_type: "posted-content".into(), ..meta("Neural correlates of something in mice", "Kim Lee", "2022", "bioRxiv", "10.1101/2022.01.01.000001") });
    let pub2 = add(&l, meta("Hippocampal replay organizes memory in rodents", "Kim Lee", "2023", "Neuron", "10.1016/j.neuron.2023.1"));
    let v = PublishedVersion { doi: "10.1016/j.neuron.2023.1".into(), title: "Hippocampal replay organizes memory in rodents".into(), journal: "Neuron".into(), date: "2023".into(), found_by: "Crossref".into() };
    l.op(OpKind::SetUpdates { paper: pre2.clone(), updates: PaperUpdates { published: Some(v.clone()), ..Default::default() } }).unwrap();
    let groups = l.tidy_duplicates();
    let g = groups.iter().find(|g| g.paper_ids.contains(&pre2)).expect("paired by the published DOI");
    assert_eq!(g.keep, pub2);
    // it is then not offered "Switch to the Published Version" (that would make two of it)
    assert!(l.tidy_report().missing.published.is_empty());

    // a published version not in the library: offered the checks' switch
    let pre3 = add(&l, PaperMeta { work_type: "posted-content".into(), ..meta("Another preprint about zebrafish fins", "Ola Nordmann", "2021", "bioRxiv", "10.1101/2021.05.05.000002") });
    let v3 = PublishedVersion { doi: "10.1242/dev.199999".into(), title: "Zebrafish fins".into(), journal: "Development".into(), date: "2022".into(), found_by: "bioRxiv".into() };
    l.op(OpKind::SetUpdates { paper: pre3.clone(), updates: PaperUpdates { published: Some(v3), ..Default::default() } }).unwrap();
    let r = l.tidy_report();
    assert_eq!(r.missing.published.len(), 1);
    assert_eq!(r.missing.published[0].paper_id, pre3);
    let _ = pre;
}

#[test]
fn merging_keeps_everything_on_the_paper_kept() {
    let (_d, l) = lib();
    let keep = add(&l, meta(KUKANJA, "Petra Kukanja", "2024-04-11", "Cell", "10.1016/j.cell.2024.02.030"));
    let other = l
        .add_paper(PaperMeta { abstract_text: "Multiple sclerosis lesions…".into(), ..meta(KUKANJA, "Petra Kukanja", "2024", "", "") }, Some(b"%PDF-1.7 x".to_vec()), None, Some("full text".into()))
        .unwrap()
        .id;
    l.set_tag(other.clone(), "MS".into(), true).unwrap();
    let f = l.create_collection("Thesis".into(), None).unwrap();
    l.set_membership(f.id.clone(), other.clone(), true).unwrap();
    l.set_starred(other.clone(), true).unwrap();
    l.set_cite_key(other.clone(), "kukanja2024cell".into()).unwrap();
    let a = l.add_highlight(other.clone(), "full text".into(), None, "green".into(), "check this".into()).unwrap();

    assert_eq!(l.tidy_duplicates().len(), 1);
    let p = l.merge_papers(keep.clone(), vec![other.clone()]).unwrap();
    assert!(l.paper(other.clone()).is_none());
    assert_eq!(p.meta.abstract_text, "Multiple sclerosis lesions…");
    assert_eq!(p.meta.doi, "10.1016/j.cell.2024.02.030");
    assert!(p.tags.contains(&"MS".to_string()));
    assert!(p.collections.contains(&f.id));
    assert!(p.starred);
    assert!(p.has_pdf);
    assert!(!p.cite_key.is_empty());
    let anns = l.annotations(keep.clone());
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].quote, a.quote);
    assert_eq!(anns[0].note, "check this");
    assert!(l.tidy_duplicates().is_empty());
}

// ---------------------------------------------------------------- missing details and DOIs

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata/tidy")
}

/// Papers without DOIs: two that Crossref has, one with the right title but the wrong year (the
/// guard leaves it), one that doesn't exist, an arXiv paper (no lookup), and one whose DOI is
/// another paper's in the library.
fn doi_library() -> (tempfile::TempDir, Arc<Library>, Vec<String>) {
    let (d, l) = lib();
    let add = |m: PaperMeta| l.add_paper(m, None, None, None).unwrap().id;
    let ids = vec![
        add(meta(KUKANJA, "Petra Kukanja", "2024", "Cell", "")),
        add(PaperMeta { title: "A new measure of rank correlation".into(), authors: vec!["M. G. Kendall".into()], date: "1938".into(), journal: "Biometrika".into(), ..Default::default() }),
        add(meta(KUKANJA, "Petra Kukanja", "2009", "Cell", "")),
        add(meta("Quantum coherence of mitochondrial lattices governs memory consolidation in hippocampal astrocytes", "Annabel Smithers", "2021", "Journal of Cellular Neurodynamics", "")),
        add(PaperMeta { title: "Attention is all you need".into(), authors: vec!["Ashish Vaswani".into()], date: "2017".into(), arxiv: "1706.03762v5".into(), ..Default::default() }),
    ];
    (d, l, ids)
}

fn check_dois(r: &DoiReport, ids: &[String]) {
    let by = |id: &String| r.suggestions.iter().find(|s| &s.paper_id == id);
    assert_eq!(by(&ids[0]).map(|s| s.doi.to_lowercase()), Some("10.1016/j.cell.2024.02.030".into()), "{r:#?}");
    assert_eq!(by(&ids[0]).unwrap().source, "Crossref");
    // Biometrika's paper has two DOIs (OUP's and JSTOR's): either is the work
    let kendall = by(&ids[1]).map(|s| s.doi.to_lowercase()).unwrap_or_default();
    assert!(kendall == "10.1093/biomet/30.1-2.81" || kendall == "10.2307/2332226", "{r:#?}");
    assert!(by(&ids[2]).is_none(), "fifteen years off: not this paper");
    assert!(by(&ids[3]).is_none(), "doesn't exist");
    assert_eq!(by(&ids[4]).map(|s| s.doi.as_str()), Some("10.48550/arXiv.1706.03762"));
    assert!(r.not_found.contains(&ids[2]) && r.not_found.contains(&ids[3]));
}

#[test]
fn dois_found_with_the_title_year_and_author_guard() {
    let (_d, l, ids) = doi_library();
    let net = Fixtures::open(fixtures_dir());
    let missing = l.tidy_report().missing;
    for id in &ids {
        assert!(missing.no_doi.contains(id));
    }
    let r = l.find_dois_with(&net, &[], &|_| {});
    check_dois(&r, &ids);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // the arXiv paper needed no lookup: four searches
    assert_eq!(net.asked.lock().unwrap().len(), 4);

    // applied: the DOI and the details the paper lacked (Kendall's volume and pages); undone
    let kendall: Vec<DoiSuggestion> = r.suggestions.iter().filter(|s| s.paper_id == ids[1]).cloned().collect();
    let ch = l.apply_doi_suggestions(kendall.clone()).unwrap();
    let p = l.paper(ids[1].clone()).unwrap();
    assert_eq!(p.meta.doi, kendall[0].doi);
    assert_eq!(p.meta.title, "A new measure of rank correlation", "what the paper had stays");
    assert!(!p.meta.volume.is_empty() && !p.meta.pages.is_empty());
    assert!(!l.tidy_report().missing.no_doi.contains(&ids[1]));
    l.undo_tidy_change(ch).unwrap();
    let p = l.paper(ids[1].clone()).unwrap();
    assert!(p.meta.doi.is_empty() && p.meta.volume.is_empty());

    // a DOI another paper has is marked as a duplicate
    let _ = l.add_paper(meta(KUKANJA, "Petra Kukanja", "2024", "Cell", "10.1016/j.cell.2024.02.030"), None, None, None).unwrap();
    let r = l.find_dois_with(&net, &[ids[0].clone()], &|_| {});
    assert!(r.suggestions[0].duplicate_of.is_some());
}

#[test]
fn the_guard_takes_only_the_same_work() {
    let m = meta(KUKANJA, "Petra Kukanja", "2024", "Cell", "");
    let mut f = meta(KUKANJA, "P. Kukanja", "2024-04-11", "Cell", "10.1016/j.cell.2024.02.030");
    assert!(doi::is_this(&m, &f));
    f.date = "2019".into();
    assert!(!doi::is_this(&m, &f), "years apart");
    f.date = "2024".into();
    f.authors = vec!["John Smith".into()];
    assert!(!doi::is_this(&m, &f), "another first author");
    f.authors = vec!["Petra Kukanja".into()];
    f.title = "Cellular architecture of evolving neuroinflammatory lesions".into();
    assert!(doi::is_this(&m, &f), "a subtitle left out");
    f.title = "Molecular architecture of chronic lesions in multiple sclerosis".into();
    assert!(!doi::is_this(&m, &f), "another title");
    // a published paper never gets its preprint's DOI
    let mut pre = meta(KUKANJA, "Petra Kukanja", "2023", "bioRxiv", "10.1101/2023.01.01.1");
    pre.work_type = "posted-content".into();
    assert!(!doi::is_this(&m, &pre));
}

// ---------------------------------------------------------------- tags and folders

fn topic_library() -> (tempfile::TempDir, Arc<Library>, Vec<String>, Vec<String>, Vec<String>) {
    let (d, l) = lib();
    let add = |t: &str, kw: &[&str], j: &str| {
        l.add_paper(PaperMeta { title: t.into(), authors: vec!["A. Author".into()], date: "2022".into(), journal: j.into(), keywords: kw.iter().map(|s| s.to_string()).collect(), ..Default::default() }, None, None, None).unwrap().id
    };
    let spatial = vec![
        add("Spatial transcriptomics of the mouse hippocampus", &[], "Nature Neuroscience"),
        add("High-resolution spatial transcriptomics with in situ sequencing", &["in situ sequencing"], "Nature Methods"),
        add("Spatial transcriptomics reveals tumour niches in breast cancer", &[], "Cell"),
        add("Benchmarking spatial transcriptomics platforms", &["benchmark"], "Nature Methods"),
        add("Cell segmentation for spatial transcriptomics data", &[], "Bioinformatics"),
        add("Spatial transcriptomics atlas of the developing human heart", &[], "Cell"),
        add("Deconvolution of spot-based spatial transcriptomics", &[], "Nature Biotechnology"),
        add("Integrating single-cell and spatial transcriptomics in the cortex", &[], "Neuron"),
    ];
    let ms = vec![
        add("Remyelination failure in multiple sclerosis lesions", &["multiple sclerosis"], "Brain"),
        add("Oligodendrocyte heterogeneity in multiple sclerosis", &[], "Nature"),
        add("B cells in progressive multiple sclerosis", &["multiple sclerosis"], "Lancet Neurology"),
        add("Microglia at the rim of chronic active multiple sclerosis lesions", &[], "Brain"),
        add("Epstein-Barr virus and the risk of multiple sclerosis", &[], "Science"),
        add("Smoldering inflammation in multiple sclerosis", &[], "Annals of Neurology"),
    ];
    let protein = vec![
        add("Highly accurate protein structure prediction with AlphaFold", &["protein structure prediction"], "Nature"),
        add("Protein structure prediction from single sequences with language models", &[], "Science"),
        add("Accurate protein structure prediction of complexes", &[], "Bioinformatics"),
        add("Evaluating protein structure prediction in CASP15", &[], "Proteins"),
    ];
    for t in ["A history of the microscope", "Statistics for field ecologists", "Urban heat islands and bird song", "Teaching programming to children"] {
        add(t, &[], "");
    }
    (d, l, spatial, ms, protein)
}

#[test]
fn topics_without_a_model() {
    let (_d, l, spatial, ms, protein) = topic_library();
    let topics = l.tidy_topics();
    assert!(topics.len() <= groups::MAX_TOPICS);
    let names: Vec<&str> = topics.iter().map(|t| t.name.as_str()).collect();
    let find = |n: &str| topics.iter().find(|t| t.name.eq_ignore_ascii_case(n)).unwrap_or_else(|| panic!("{n} not in {names:?}"));
    let st = find("Spatial transcriptomics");
    assert_eq!(st.paper_ids.len(), spatial.len());
    assert!(spatial.iter().all(|id| st.paper_ids.contains(id)));
    assert!(!st.by_model);
    let m = find("Multiple sclerosis");
    assert!(ms.iter().all(|id| m.paper_ids.contains(id)));
    let ps = find("Protein structure prediction");
    assert!(protein.iter().all(|id| ps.paper_ids.contains(id)));
    // no topic that is only a word of a phrase chosen, nor generic words
    assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("transcriptomics") || n.eq_ignore_ascii_case("sclerosis") || n.eq_ignore_ascii_case("study")), "{names:?}");

    // dismissed by the name as shown
    l.tidy_dismiss("topic:Protein Structure Prediction".into()).unwrap();
    assert!(!l.tidy_report().topics.iter().any(|t| t.name.eq_ignore_ascii_case("Protein structure prediction")));
    l.tidy_reset_dismissed().unwrap();

    // an existing tag isn't proposed again; created as tags, then undone
    l.set_tag(ms[0].clone(), "Multiple Sclerosis".into(), true).unwrap();
    let topics = l.tidy_topics();
    assert!(!topics.iter().any(|t| t.name.eq_ignore_ascii_case("Multiple sclerosis")));
    let st = topics.iter().find(|t| t.name == "Spatial transcriptomics").unwrap().clone();
    let ch = l.create_groups(vec![st.clone()], false).unwrap();
    assert_eq!(ch.tags.len(), spatial.len());
    assert!(ch.summary.contains("Spatial transcriptomics"));
    assert!(l.paper(spatial[0].clone()).unwrap().tags.contains(&"Spatial transcriptomics".to_string()));
    l.undo_tidy_change(ch).unwrap();
    assert!(l.paper(spatial[0].clone()).unwrap().tags.is_empty());
    // as a folder, then undone (the folder goes too)
    let ch = l.create_groups(vec![st], true).unwrap();
    assert_eq!(ch.created_folders.len(), 1);
    assert_eq!(l.collections().iter().find(|c| c.name == "Spatial transcriptomics").unwrap().paper_count as usize, spatial.len());
    l.undo_tidy_change(ch).unwrap();
    assert!(l.collections().is_empty());
}

#[test]
fn existing_tags_for_untagged_papers() {
    let (_d, l, spatial, ms, _protein) = topic_library();
    // a tag whose name isn't in the titles: suggested by the words its papers share
    for id in &spatial[..4] {
        l.set_tag(id.clone(), "Lab methods".into(), true).unwrap();
    }
    // a tag whose name is in the titles
    l.set_tag(ms[0].clone(), "Multiple sclerosis".into(), true).unwrap();
    let s = l.tidy_tag_suggestions();
    let tags_for = |id: &String| s.iter().filter(|x| &x.paper_id == id).map(|x| x.tag.clone()).collect::<Vec<_>>();
    for id in &spatial[4..] {
        assert_eq!(tags_for(id), vec!["Lab methods".to_string()], "{s:#?}");
    }
    for id in &ms[1..] {
        assert!(tags_for(id).contains(&"Multiple sclerosis".to_string()), "{s:#?}");
    }
    let because = &s.iter().find(|x| x.paper_id == spatial[4]).unwrap().because;
    assert!(because.contains("spatial transcriptomics") && because.contains("4 papers tagged Lab methods"), "{because}");
    // tagged papers get nothing; unrelated papers get nothing
    assert!(tags_for(&spatial[0]).is_empty() && tags_for(&ms[0]).is_empty());
    let all = l.papers(PaperFilter::default(), SortKey::Added);
    let unrelated = all.iter().find(|p| p.meta.title.starts_with("A history")).unwrap();
    assert!(tags_for(&unrelated.id).is_empty(), "{s:#?}");

    // dismissed, then applied and undone
    let one = s.iter().find(|x| x.paper_id == spatial[4]).unwrap().clone();
    l.tidy_dismiss(format!("tag:{}:{}", one.paper_id, one.tag)).unwrap();
    assert!(!l.tidy_tag_suggestions().iter().any(|x| x.paper_id == spatial[4]));
    let ch = l.apply_tag_suggestions(s.clone()).unwrap();
    assert!(l.paper(spatial[5].clone()).unwrap().tags.contains(&"Lab methods".to_string()));
    l.undo_tidy_change(ch).unwrap();
    assert!(l.paper(spatial[5].clone()).unwrap().tags.is_empty());
}

#[test]
fn topics_from_the_models_answer() {
    let (_d, l, _spatial, ms, _protein) = topic_library();
    let p = l.tidy_topic_prompt(10);
    assert_eq!(p.ids.len(), 10);
    assert!(p.prompt.contains("1. ") && p.prompt.contains("10. "));
    assert!(!p.prompt.contains("11. "));
    let n_of = |id: &String| p.ids.iter().position(|x| x == id).map(|i| i + 1);
    let mut l2 = String::from("<think>\nhmm\n</think>\nHere are the topics:\n");
    l2 += &format!("1. **Multiple sclerosis**: {}\n", ms.iter().filter_map(n_of).map(|n| n.to_string()).collect::<Vec<_>>().join(", "));
    l2 += "- Protein folding: 1, 2, 99, 2\n";
    l2 += "Other: 3, 4, 5\n";
    l2 += "Lonely topic: 7\n";
    let topics = l.tidy_groups_from_answer(p.clone(), l2);
    let names: Vec<&str> = topics.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Protein folding"), "{names:?}");
    assert!(!names.contains(&"Other") && !names.contains(&"Lonely topic"), "{names:?}");
    let pf = topics.iter().find(|t| t.name == "Protein folding").unwrap();
    assert_eq!(pf.paper_ids, vec![p.ids[0].clone(), p.ids[1].clone()], "only numbers from the prompt, once each");
    assert!(pf.by_model);
    for t in &topics {
        assert!(t.paper_ids.iter().all(|id| p.ids.contains(id)));
    }
}

// ---------------------------------------------------------------- online

/// Asks Crossref (and records its answers for the tests above). Run with
/// `cargo test tidy -- --ignored record_tidy_fixtures --nocapture`.
#[test]
#[ignore]
fn record_tidy_fixtures() {
    let dir = fixtures_dir();
    let _ = std::fs::remove_dir_all(&dir);
    let live = LiveNet;
    let rec = Recorder { inner: &live, dir: dir.clone() };
    let (_d, l, ids) = doi_library();
    let r = l.find_dois_with(&rec, &[], &|m| println!("{m}"));
    println!("{r:#?}");
    check_dois(&r, &ids);
}
