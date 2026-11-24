//! New for You: the Swift app's file format, the rules ported from apple/Shared/Watches.swift,
//! and whole runs on recorded OpenAlex and bioRxiv answers (testdata/; no network).

use super::run::{self, Finder, Writer};
use super::state::*;
use super::text::*;
use super::time::*;
use super::*;
use crate::metadata::{Coauthor, DiscoverHit, OrcidProfile, WatchKind};
use crate::model::{CoreError, PaperFilter, PaperMeta, SortKey};
use serde_json::Value;
use std::sync::Mutex;

const SWIFT_STATE: &str = include_str!("testdata/foryou-swift.json");
const SWIFT_WATCHES: &str = include_str!("testdata/watches-swift.json");
const OA_SPATIAL: &str = include_str!("testdata/openalex-spatial.json");
const OA_SOIL: &str = include_str!("testdata/openalex-soil.json");
const BX_P0: &str = include_str!("testdata/biorxiv-p0.json");
const BX_P3: &str = include_str!("testdata/biorxiv-p3.json");

/// 2026-11-23T08:00:00Z, the fixture's date.
const D0: i64 = 1_795_420_800_000;
const CET: i32 = 60;

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("lunta-foryou-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn library(root: &PathBuf) -> Arc<Library> {
    let cache = root.parent().unwrap().join(format!("{}-cache", root.file_name().unwrap().to_string_lossy()));
    Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), "test".into()).unwrap()
}

/// A recorded OpenAlex /works answer as found papers (the fields the feed uses).
fn works(json: &str) -> Vec<DiscoverHit> {
    let j: Value = serde_json::from_str(json).unwrap();
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    j["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            let mut words: Vec<(u64, String)> = vec![];
            for (word, pos) in w["abstract_inverted_index"].as_object().into_iter().flatten() {
                for p in pos.as_array().unwrap() {
                    words.push((p.as_u64().unwrap(), word.clone()));
                }
            }
            words.sort();
            DiscoverHit {
                meta: PaperMeta {
                    title: s(&w["title"]),
                    authors: w["authorships"].as_array().unwrap().iter().map(|a| s(&a["author"]["display_name"])).collect(),
                    date: s(&w["publication_date"]),
                    journal: s(&w["primary_location"]["source"]["display_name"]),
                    doi: s(&w["doi"]).trim_start_matches("https://doi.org/").to_string(),
                    abstract_text: words.into_iter().map(|x| x.1).collect::<Vec<_>>().join(" "),
                    ..Default::default()
                },
                cited_by: w["cited_by_count"].as_u64().unwrap_or(0) as u32,
                open_access: w["open_access"]["is_oa"].as_bool().unwrap_or(false),
                in_library: None,
            }
        })
        .collect()
}

/// OpenAlex and bioRxiv as recorded: searches with "spatial" find the spatial papers, others the
/// soil papers; an author watch finds one soil paper; your paper is cited by two papers, one
/// of them yours.
struct Fake {
    calls: Mutex<Vec<String>>,
    profile: Option<OrcidProfile>,
}

impl Fake {
    fn new() -> Fake {
        Fake { calls: Mutex::new(vec![]), profile: None }
    }
    fn calls(&self) -> Vec<String> {
        let mut c = self.calls.lock().unwrap().clone();
        c.sort();
        c
    }
}

fn hit(title: &str, doi: &str, authors: &[&str], date: &str) -> DiscoverHit {
    DiscoverHit {
        meta: PaperMeta {
            title: title.into(),
            authors: authors.iter().map(|a| a.to_string()).collect(),
            date: date.into(),
            journal: "Nature Methods".into(),
            doi: doi.into(),
            abstract_text: "Spatial transcriptomics of the brain with imaging-based cell segmentation. ".repeat(6),
            ..Default::default()
        },
        cited_by: 0,
        open_access: true,
        in_library: None,
    }
}

impl Finder for Fake {
    fn watch_new(&self, kind: WatchKind, value: &str, since: &str, limit: u32) -> crate::model::Result<Vec<DiscoverHit>> {
        self.calls.lock().unwrap().push(format!("{kind:?} {value} since {since} limit {limit}"));
        let mut out = match kind {
            WatchKind::Topic if value.contains("spatial") => works(OA_SPATIAL),
            WatchKind::Topic => works(OA_SOIL),
            WatchKind::Author => works(OA_SOIL).into_iter().take(1).collect(),
            _ => vec![],
        };
        out.truncate(limit as usize);
        Ok(out)
    }
    fn citing_new(&self, doi: &str, exclude: &str, since: &str, _limit: u32) -> crate::model::Result<Vec<DiscoverHit>> {
        self.calls.lock().unwrap().push(format!("citing {doi} not by {exclude} since {since}"));
        Ok(vec![
            hit("A short title: citing your segmentation work in mouse cortex", "10.9/cites-you", &["Someone Else"], "2026-11-20"),
            hit("Your own follow-up paper on the same atlas of the cortex", "10.9/by-you", &["Josiah Cárberry"], "2026-11-20"),
        ])
    }
    fn orcid_profile(&self, orcid: &str) -> crate::model::Result<OrcidProfile> {
        self.calls.lock().unwrap().push(format!("orcid {orcid}"));
        self.profile.clone().ok_or(CoreError::NotFound(orcid.into()))
    }
    fn get_json(&self, url: &str) -> crate::model::Result<Value> {
        self.calls.lock().unwrap().push(format!("get {url}"));
        let text = if url.contains("/biorxiv/") && url.ends_with("/0/json") {
            BX_P0
        } else if url.contains("/biorxiv/") && url.ends_with("/3/json") {
            BX_P3
        } else {
            r#"{"messages":[{"status":"no posts found","total":0}],"collection":[]}"#
        };
        Ok(serde_json::from_str(text).unwrap())
    }
}

/// A model that likes segmentation papers.
struct Judge {
    prompts: Mutex<Vec<String>>,
}

impl Writer for Judge {
    fn answer(&self, instructions: &str, prompt: &str, _max: u32) -> Option<String> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        if instructions == SEARCHES_INSTRUCTIONS {
            return Some("Q: spatial transcriptomics cell segmentation\nQ: spatial transcriptomics segmentation\nQ: soil microbiome nitrogen\nK: transcriptom, neurons, virus, ai".into());
        }
        if instructions == SCREEN_INSTRUCTIONS {
            let title = prompt.split("Title: ").nth(1).unwrap_or("").lines().next().unwrap_or("").to_lowercase();
            return Some(if title.contains("segmentation") {
                "SCORE: 8 — A segmentation method for your Xenium data.".into()
            } else if title.contains("prism") {
                "SCORE: 6 — Cell types in space, close to your maps.".into()
            } else {
                "Score 2/10 - unrelated.".into()
            });
        }
        if instructions == SUMMARY_INSTRUCTIONS {
            return Some("\"Reassigns transcripts to cells to segment them more accurately.\"\n".into());
        }
        if instructions == DIGEST_INSTRUCTIONS {
            return Some("Segmentation dominates today: [Unified](1) reassigns transcripts [2](2), and **SpaCut** fuses morphology.".into());
        }
        None
    }
}

fn judge() -> Judge {
    Judge { prompts: Mutex::new(vec![]) }
}

fn quiet(_: &str) {}

fn run_with(lib: &Library, files: &Files, finder: &dyn Finder, writer: Option<&dyn Writer>, now: i64) -> run::RunReport {
    run::run(&run::Run { lib, files, finder, writer, now, tz_min: CET, progress: &quiet, stop: None }).unwrap()
}

// ---- the files ----

#[test]
fn reads_what_the_swift_app_writes() {
    let s: ForYouState = serde_json::from_str(SWIFT_STATE).unwrap();
    assert!(s.interests.starts_with("I work on spatial transcriptomics"));
    assert_eq!((s.per_day, s.morning, s.preprints), (6, 390, Preprints::Few));
    assert_eq!(s.items.len(), 2);
    let r = &s.items[0];
    assert_eq!(r.key, "10.1002/advs.78423");
    assert_eq!(r.found, D0);
    assert_eq!(r.boosted, Some(true));
    assert_eq!(r.summary.as_deref(), Some("Fuses transcripts and morphology to segment cells."));
    assert!((r.score - 0.61).abs() < 1e-9);
    assert_eq!(s.items[1].in_library.as_deref(), Some("ABC-123"));
    assert_eq!(s.items[1].boosted, None);
    assert_eq!(s.last_run, Some(D0 + 60_000));
    assert_eq!(s.digest_for.as_deref(), Some("v2|10.1002/advs.78423"));
    let o = s.orcid.as_ref().unwrap();
    assert_eq!((o.name.as_str(), o.author.as_str(), o.works), ("Josiah Carberry", "A5000000001", 2));
    assert_eq!(o.papers[0].key(), "10.5555/12345678");
    assert_eq!(o.coauthors[0].watch_value(), "A5000000002");
    assert_eq!(o.updated, D0);
    let w: Vec<Watch> = serde_json::from_str(SWIFT_WATCHES).unwrap();
    assert_eq!(w[0].kind, WatchType::Author);
    assert_eq!(w[1].last_checked, Some(D0));
    assert_eq!(w[1].seen, vec!["10.1/a"]);
    assert_eq!(w[0].describe(), "Papers by Jane Doe");
}

#[test]
fn writes_what_the_swift_app_reads() {
    let s: ForYouState = serde_json::from_str(SWIFT_STATE).unwrap();
    let v: Value = serde_json::to_value(&s).unwrap();
    // Swift's synthesized Codable needs every non-optional key …
    for k in ["interests", "queries", "keywords", "queriesFor", "perDay", "items", "shown", "liked", "disliked", "boosted", "digest", "morning", "preprints"] {
        assert!(v.get(k).is_some(), "{k} missing");
    }
    for k in ["key", "title", "authors", "date", "journal", "doi", "url", "abstract", "openAccess", "citedBy", "reason", "source", "score", "found"] {
        assert!(v["items"][1].get(k).is_some(), "items.{k} missing");
    }
    // … and leaves out a nil optional; dates without fractions (Swift can't read them)
    assert!(v["items"][1].get("boosted").is_none() && v["items"][1].get("summary").is_none());
    assert_eq!(v["items"][0]["found"], "2026-11-23T08:00:00Z");
    assert_eq!(v["lastRun"], "2026-11-23T08:01:00Z");
    assert_eq!(v["preprints"], "few");
    // the same document back
    let back: ForYouState = serde_json::from_value(v).unwrap();
    assert_eq!(back, s);
    let empty: Value = serde_json::to_value(ForYouState::default()).unwrap();
    assert!(empty.get("lastRun").is_none() && empty.get("orcid").is_none() && empty.get("digestFor").is_none());
    assert_eq!((empty["perDay"].as_i64(), empty["morning"].as_i64()), (Some(8), Some(420)));
}

#[test]
fn keeps_what_a_newer_app_adds_and_survives_bad_values() {
    let mut v: Value = serde_json::from_str(SWIFT_STATE).unwrap();
    v["someNewSetting"] = Value::from("kept");
    v["items"][0]["badge"] = Value::from(3);
    v["preprints"] = Value::from("sometimes");
    v["morning"] = Value::from("seven");
    v["items"].as_array_mut().unwrap().push(serde_json::json!({"title": "no key"}));
    let s: ForYouState = serde_json::from_value(v).unwrap();
    assert_eq!(s.preprints, Preprints::Any);
    assert_eq!(s.morning, 420);
    assert_eq!(s.items.len(), 2, "the unreadable paper is left out");
    let out = serde_json::to_value(&s).unwrap();
    assert_eq!(out["someNewSetting"], "kept");
    assert_eq!(out["items"][0]["badge"], 3);
    // an older or broken file: defaults
    let s: ForYouState = serde_json::from_str("{}").unwrap();
    assert_eq!(s, ForYouState::default());
}

#[test]
fn dates() {
    assert_eq!(iso8601(D0), "2026-11-23T08:00:00Z");
    assert_eq!(parse_iso8601("2026-11-23T09:00:00+01:00"), Some(D0));
    assert_eq!(parse_iso8601("2026-11-23T08:00:00.250Z"), Some(D0 + 250));
    assert_eq!(utc_date(D0), "2026-11-23");
    // 07:00 in Stockholm (UTC+1) is 06:00Z
    let s = ForYouState::default();
    assert_eq!(iso8601(s.last_morning(D0, CET)), "2026-11-23T06:00:00Z");
    assert_eq!(iso8601(s.next_morning(D0, CET)), "2026-11-24T06:00:00Z");
    // before this morning's time: yesterday's
    assert_eq!(iso8601(s.last_morning(D0 - 3 * HOUR_MS, CET)), "2026-11-22T06:00:00Z");
    assert_eq!(published_label("2026-11-16", D0, CET).unwrap(), "Published 16 Nov (7 days ago)");
    assert_eq!(published_label("2026-11-22", D0, CET).unwrap(), "Published 22 Nov (yesterday)");
    assert_eq!(published_label("2026-11", D0, CET).unwrap(), "Published 2026-11");
    assert_eq!(published_label("", D0, CET), None);
}

#[test]
fn when_a_run_is_due() {
    let mut s = ForYouState::default();
    let morning = s.last_morning(D0, CET);
    assert!(!run::is_due(&s, &[], D0, CET, false), "nothing to look for");
    s.interests = "spatial transcriptomics".into();
    assert!(run::is_due(&s, &[], D0, CET, false));
    assert!(run::is_due(&s, &[], D0, CET, true), "a phone runs when there never was a run");
    s.last_run = Some(morning - HOUR_MS);
    assert!(run::is_due(&s, &[], D0, CET, false));
    // a phone leaves two hours after the morning time to a computer
    assert!(!run::is_due(&s, &[], morning + HOUR_MS, CET, true));
    assert!(run::is_due(&s, &[], morning + 2 * HOUR_MS, CET, true));
    s.last_run = Some(morning + 60_000);
    assert!(!run::is_due(&s, &[], D0, CET, false), "already made this morning");
    // a watch alone is enough
    let mut s = ForYouState::default();
    let w = vec![Watch::new(WatchType::Topic, "x", "x", D0)];
    assert!(run::is_due(&s, &w, D0, CET, false));
    s.orcid = None;
    assert!(s.has_setup(&w));
}

// ---- words ----

#[test]
fn keys_and_sources() {
    assert_eq!(doi_key(" https://doi.org/10.1002/ADVS.78423"), "10.1002/advs.78423");
    assert_eq!(doi_key("doi:10.1/X"), "10.1/x");
    assert_eq!(title_key("SpaCut: Transcript-Aware Fusion!"), "spacuttranscriptawarefusion");
    assert_eq!(title_key(&"a".repeat(100)).len(), 70);
    assert_eq!(cites_your_paper("Cellular architecture: evolving lesions"), "Cites your paper “Cellular architecture”.");
    assert_eq!(cites_your_paper("Short: a much longer subtitle"), "Cites your paper “Short: a much longer subtitle”.");
    assert!(cites_your_paper(&"Long title ".repeat(10)).ends_with("…”."));
    assert!(cites_yours(&cites_your_paper("Some paper title here")));
    assert!(from_journal(&journal_source("Nature Methods")));
    let mut h = hit("A paper", "10.64898/2026.11.03.764126", &[], "");
    assert!(is_preprint(&h, "search “x”"));
    h.meta.doi = "10.1038/x".into();
    assert!(!is_preprint(&h, "search “x”"));
    assert!(is_preprint(&h, "bioRxiv"));
    h.meta.journal = "Research Square".into();
    assert!(is_preprint(&h, "x"));
    assert_eq!(snip("T", &"a".repeat(500)).len(), 3 + 400);
}

#[test]
fn reads_the_models_searches() {
    let (q, k) = parse_searches(
        "Here you go:\nQ: \"spatial transcriptomics cell segmentation\"\nq: 2. spatial transcriptomics segmentation\nQ: Xenium brain atlas spatial\nQ: ab\nK: Xenium, MERFISH, cell segmentation, AI, Baysor",
    )
    .unwrap();
    // the near-repeat (its words a subset of the first) is dropped, too-short ones too
    assert_eq!(q, vec!["spatial transcriptomics cell segmentation", "Xenium brain atlas spatial"]);
    assert_eq!(k, vec!["xenium", "merfish", "cell segmentation", "baysor"]);
    assert!(parse_searches("Q: only one query here").is_none());
    let (q, k) = fallback_searches("I study spatial transcriptomics, cell segmentation in mouse brain (Xenium). Also: AI");
    assert_eq!(q, vec!["I study spatial transcriptomics", "cell segmentation in mouse brain"]);
    assert!(k.contains(&"transcriptomics".to_string()) && k.contains(&"segmentation".to_string()) && !k.contains(&"study".to_string()));
}

#[test]
fn reads_the_models_screening_and_summaries() {
    assert_eq!(parse_screen("SCORE: 8 — A segmentation method for your Xenium data."), Some((8.0, "A segmentation method for your Xenium data.".into())));
    assert_eq!(parse_screen("Score 7/10 - Maps cell types in your tissue."), Some((7.0, "Maps cell types in your tissue.".into())));
    assert_eq!(parse_screen("SCORE: 3"), Some((3.0, "Matches your interests.".into())));
    assert_eq!(parse_screen("no idea"), None);
    assert_eq!(clean_summary("  \"**Reassigns transcripts to cells.**\"\nmore").as_deref(), Some("Reassigns transcripts to cells.**\"\nmore".split('\n').next().unwrap()));
    assert_eq!(clean_summary("short"), None);
    assert_eq!(clean_summary("\"A plain sentence about the method and finding.\"").as_deref(), Some("A plain sentence about the method and finding."));
}

fn rec(key: &str, title: &str) -> FeedRecord {
    FeedRecord { key: key.into(), title: title.into(), doi: key.into(), ..Default::default() }
}

#[test]
fn links_the_digest_to_its_papers() {
    let batch = vec![rec("10.1/a", "HistoTx: Early fusion of histology"), rec("10.1/b", "scPolyA reveals polyadenylation"), rec("10.1/c", "Third paper")];
    let link = |k: &str| feed_link(k);
    assert_eq!(link("10.1/a"), "foryou://paper?key=10%2E1%2Fa");
    assert_eq!(feed_key(&link("10.1/a")).as_deref(), Some("10.1/a"));
    assert_eq!(feed_key("https://example.org"), None);
    let out = link_digest("Fusion leads: [scPolyA](2) maps sites, cell-type inference [1](1), and HistoTx too (3).", &batch);
    // HistoTx has its ↗ link already, so its name stays plain
    assert_eq!(out, format!("Fusion leads: [scPolyA]({}) maps sites, cell-type inference [↗]({}), and HistoTx too.", link("10.1/b"), link("10.1/a")));
    let out = link_digest("Mostly about HistoTx and [scPolyA](2).", &batch);
    assert_eq!(out, format!("Mostly about [HistoTx]({}) and [scPolyA]({}).", link("10.1/a"), link("10.1/b")));
    // a number out of range keeps the name; a bracketed title becomes a link, other brackets text
    let out = link_digest("See [Gone](9) and [Third paper] and [something else].", &batch);
    assert_eq!(out, format!("See Gone and [Third paper]({}) and something else.", link("10.1/c")));
    // a bare link (an older digest) becomes ↗
    assert_eq!(repair_digest(&format!("Finds this ({}).", link("10.1/a"))), format!("Finds this [↗]({}).", link("10.1/a")));
    assert_eq!(repair_digest(&format!("[x]({})", link("10.1/a"))), format!("[x]({})", link("10.1/a")));
    // pieces to draw, and text to share
    let pieces = digest_pieces(&format!("Read [scPolyA]({}) first.", link("10.1/b")), &batch);
    assert_eq!(pieces.len(), 3);
    assert_eq!(pieces[1], DigestPiece { text: "scPolyA".into(), key: Some("10.1/b".into()) });
    let share = share_text(&format!("Read [scPolyA]({}) first [↗]({}).", link("10.1/b"), link("10.1/b")), &batch[..1]);
    assert_eq!(share, "New for You (Lunta)\n\nRead scPolyA first.\n\nPapers:\n1. HistoTx: Early fusion of histology\n   https://doi.org/10.1/a");
}

#[test]
fn first_sentences_stand_in_for_a_summary() {
    let a = "We built a map. It has many cells and much detail about them. A third sentence goes on and on.";
    assert_eq!(first_sentences(a, 200), a);
    assert_eq!(first_sentences(a, 70), "We built a map. It has many cells and much detail about them.");
    assert_eq!(first_sentences(a, 40), "We built a map.");
    assert!(first_sentences(&"word ".repeat(200), 100).ends_with('…'));
}

// ---- ranking ----

#[test]
fn ranks_papers_by_your_interests_library_and_feedback() {
    let spatial = works(OA_SPATIAL);
    let soil = works(OA_SOIL);
    let texts: Vec<String> = [&spatial[0], &spatial[2], &soil[0], &soil[3]].iter().map(|h| snip(&h.meta.title, &h.meta.abstract_text)).collect();
    let interests = "I work on spatial transcriptomics: imaging-based cell segmentation and cell-type maps of tissue.";
    let s = rank::score(&texts, interests, &[], &[], &[], &[]);
    assert!(s[0] > s[2] && s[0] > s[3] && s[1] > s[2] && s[1] > s[3], "{s:?}");
    // the library alone (no paragraph)
    let lib = vec![texts[2].clone()];
    let s = rank::score(&texts[..2].iter().chain(&texts[3..]).cloned().collect::<Vec<_>>(), "", &lib, &[], &[], &[]);
    assert!(s[2] > s[0], "the soil paper is closest to a soil library: {s:?}");
    // turned down: lower; more like this: higher
    let base = rank::score(&texts, interests, &[], &[], &[], &[]);
    let down = rank::score(&texts, interests, &[], &[], &[texts[3].clone()], &[]);
    let up = rank::score(&texts, interests, &[], &[], &[], &[texts[3].clone()]);
    assert!(down[3] < base[3] && up[3] > base[3]);
    assert!(rank::recency("2026-11-23", D0, CET) > rank::recency("2026-11-16", D0, CET));
    assert_eq!(rank::recency("2026-11-01", D0, CET), 0.0);
}

// ---- runs ----

fn setup() -> (PathBuf, Arc<Library>, Files) {
    let root = tmp().join("lib");
    std::fs::create_dir_all(&root).unwrap();
    let lib = library(&root);
    let files = Files::new(&root, root.parent().unwrap().join("local"));
    (root, lib, files)
}

#[test]
fn a_run_without_a_model_trusts_the_ranking() {
    let (_root, lib, files) = setup();
    // SpaCut is in the library already, one paper was offered before
    lib.add_paper(PaperMeta { title: "SpaCut, saved".into(), doi: "10.1002/advs.78423".into(), ..Default::default() }, None, None, None).unwrap();
    files
        .change(|s| {
            s.interests = "I work on spatial transcriptomics: imaging-based cell segmentation and cell-type maps of tissue.".into();
            s.shown = vec!["10.64898/2026.11.07.765042".into()]; // PRISM
            s.keywords = vec!["transcriptom".into(), "neurons".into(), "virus".into()];
            s.queries = vec!["spatial transcriptomics cell segmentation".into(), "soil microbiome nitrogen".into()];
            s.queries_for = s.interests.clone();
            s.per_day = 4;
        })
        .unwrap();
    let fake = Fake::new();
    let report = run_with(&lib, &files, &fake, None, D0);
    let s = files.state();
    let keys: Vec<&str> = s.items.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(report.found as usize, s.items.len());
    assert_eq!(s.items.len(), 4, "{keys:?}");
    for gone in ["10.1002/advs.78423", "10.64898/2026.11.07.765042", "10.5281/zenodo.24287454", "10.1007/978-981-95-5283-2_30-1"] {
        assert!(!keys.contains(&gone), "{gone} should be left out: {keys:?}");
    }
    // the best first, and the spatial papers before the soil ones
    assert!(s.items.windows(2).all(|w| w[0].score >= w[1].score));
    assert!(s.items[0].source.starts_with("search “spatial"), "{:?}", s.items[0]);
    assert!(s.items[0].reason.starts_with("Close to your interests (from search “spatial"));
    assert_eq!(s.last_run, Some(D0));
    assert!(s.items.iter().all(|r| r.found == D0 && r.summary.is_none()));
    assert_eq!(s.shown.len(), 1 + 4);
    // the first run looks back 30 days; bioRxiv's last 4, both pages
    let calls = fake.calls();
    assert!(calls.iter().any(|c| c == "Topic spatial transcriptomics cell segmentation since 2026-10-24 limit 40"), "{calls:?}");
    assert!(calls.iter().any(|c| c.ends_with("/biorxiv/2026-11-19/2026-11-23/3/json")), "{calls:?}");
    assert!(calls.iter().any(|c| c.contains("citing 10.1002/advs.78423")) == false, "library papers are watched with watch_new");
    assert!(calls.iter().any(|c| c.starts_with("Citing 10.1002/advs.78423 since")), "{calls:?}");

    // the next day: nothing offered twice; yesterday's papers are "Earlier"
    let next = D0 + DAY_MS;
    let report = run_with(&lib, &files, &fake, None, next);
    let s = files.state();
    let again: Vec<&FeedRecord> = s.items.iter().filter(|r| r.found == next).collect();
    assert_eq!(report.found as usize, again.len());
    assert!(again.iter().all(|r| !keys.contains(&r.key.as_str())));
    assert!(fake.calls().iter().any(|c| c.contains("since 2026-11-15")), "later runs look back 9 days");
    let view = view::feed_view(&s, &[], &LocalState::default(), next, CET);
    assert_eq!(view.latest.len(), again.len());
    assert_eq!(view.earlier.len(), 4);
}

/// OpenAlex refusing (its daily allowance used up) for every request, or only for authors; bioRxiv
/// down too when `biorxiv_down`.
struct Refusing {
    fake: Fake,
    all: bool,
    biorxiv_down: bool,
}

const USED_UP: &str = "OpenAlex's free daily allowance for your network is used up (it renews in about 17 hours, at midnight UTC). A free OpenAlex API key (in New for You's settings) gives Lunta its own.";

impl Finder for Refusing {
    fn watch_new(&self, kind: WatchKind, value: &str, since: &str, limit: u32) -> crate::model::Result<Vec<DiscoverHit>> {
        if self.all || kind == WatchKind::Author {
            return Err(CoreError::Network(USED_UP.into()));
        }
        self.fake.watch_new(kind, value, since, limit)
    }
    fn citing_new(&self, doi: &str, exclude: &str, since: &str, limit: u32) -> crate::model::Result<Vec<DiscoverHit>> {
        if self.all {
            return Err(CoreError::Network(USED_UP.into()));
        }
        self.fake.citing_new(doi, exclude, since, limit)
    }
    fn orcid_profile(&self, orcid: &str) -> crate::model::Result<OrcidProfile> {
        self.fake.orcid_profile(orcid)
    }
    fn get_json(&self, url: &str) -> crate::model::Result<Value> {
        if self.biorxiv_down {
            return Err(CoreError::Network("api.biorxiv.org answered HTTP 502".into()));
        }
        self.fake.get_json(url)
    }
}

#[test]
fn a_source_that_cant_be_reached_is_said_not_taken_for_a_quiet_day() {
    let (_root, lib, files) = setup();
    let yesterday = D0 - DAY_MS;
    files
        .change(|s| {
            s.interests = "I work on spatial transcriptomics: imaging-based cell segmentation and cell-type maps of tissue.".into();
            s.queries = vec!["spatial transcriptomics cell segmentation".into()];
            s.keywords = vec!["transcriptom".into()];
            s.queries_for = s.interests.clone();
            s.last_run = Some(yesterday);
        })
        .unwrap();
    files.save_watches(&[Watch::new(WatchType::Author, "Jane Doe", "Jane Doe", yesterday)]).unwrap();
    assert!(run::is_due(&files.state(), &files.watches(), D0, CET, false));

    // OpenAlex refuses everything: no feed, nothing marked as offered, the run still due, and
    // the feed says why (not "Nothing new today")
    let down = Refusing { fake: Fake::new(), all: true, biorxiv_down: false };
    let report = run_with(&lib, &files, &down, None, D0);
    assert!(report.failed);
    assert_eq!(report.problem.as_deref(), Some(USED_UP));
    assert_eq!(report.found, 0);
    let s = files.state();
    assert_eq!(s.last_run, Some(yesterday), "a failed run doesn't count as the day's run");
    assert!(s.shown.is_empty() && s.items.is_empty());
    assert!(run::is_due(&s, &files.watches(), D0, CET, false));
    let view = view::feed_view(&s, &files.watches(), &files.local(), D0, CET);
    assert_eq!(view.problem.as_deref(), Some(USED_UP));
    assert!(view.fetch_failed);

    // only the author searches fail, and bioRxiv is down: the feed is made, and what failed is said
    let some = Refusing { fake: Fake::new(), all: false, biorxiv_down: true };
    let report = run_with(&lib, &files, &some, None, D0 + 60_000);
    assert!(!report.failed);
    assert!(report.found > 0);
    let p = report.problem.clone().unwrap();
    assert!(p.starts_with("1 of ") && p.contains("OpenAlex requests failed: OpenAlex's free daily allowance"), "{p}");
    assert!(p.contains("Couldn't reach bioRxiv: api.biorxiv.org answered HTTP 502"), "{p}");
    let s = files.state();
    assert_eq!(s.last_run, Some(D0 + 60_000));
    let view = view::feed_view(&s, &files.watches(), &files.local(), D0 + 60_000, CET);
    assert_eq!(view.problem, Some(p));
    assert!(!view.fetch_failed);

    // everything answers again: the problem is gone
    let report = run_with(&lib, &files, &Fake::new(), None, D0 + 120_000);
    assert_eq!(report.problem, None);
    let view = view::feed_view(&files.state(), &files.watches(), &files.local(), D0 + 120_000, CET);
    assert_eq!(view.problem, None);

    // a failure here hides once another device has made a feed since
    let local = LocalState { problem: "x".into(), failed: true, problem_at: D0, ..Default::default() };
    let later = ForYouState { last_run: Some(D0 + 1), ..Default::default() };
    assert_eq!(local.problem_for(&later), None);
    assert_eq!(local.problem_for(&ForYouState { last_run: Some(D0 - 1), ..Default::default() }), Some(("x", true)));
}

#[test]
fn openalex_allowance_used_up_is_said_and_not_waited_for() {
    use crate::metadata::{refusal_with, with_key};
    let body = r#"{"error":"Rate limit exceeded","message":"Insufficient budget. This request has no API key, so it counts against the free daily budget shared by everyone on your network's IP address","reason":"daily_budget","retryAfter":59525}"#;
    let why = refusal_with("api.openalex.org", 429, Some(59525), body, false).unwrap();
    assert_eq!(why, USED_UP);
    let with = refusal_with("api.openalex.org", 429, None, body, true).unwrap();
    assert_eq!(with, "OpenAlex's free daily allowance for your API key is used up (it renews at midnight UTC).");
    // a short wait is waited for; other services' long waits are said
    assert_eq!(refusal_with("api.openalex.org", 429, Some(2), "Too many requests", false), None);
    assert_eq!(refusal_with("api.crossref.org", 429, Some(3600), "", false).as_deref(), Some("api.crossref.org asks to wait 60 minutes before asking again"));
    // the key goes on OpenAlex's addresses only
    assert_eq!(with_key("https://api.openalex.org/works?filter=x", "k1"), "https://api.openalex.org/works?filter=x&api_key=k1");
    assert_eq!(with_key("https://api.openalex.org/works/W1", "k1"), "https://api.openalex.org/works/W1?api_key=k1");
    assert_eq!(with_key("https://api.crossref.org/works?x=1", "k1"), "https://api.crossref.org/works?x=1");
    assert_eq!(with_key("https://api.openalex.org/works?x=1", ""), "https://api.openalex.org/works?x=1");
}

#[test]
fn the_openalex_key_is_kept_with_the_settings() {
    let (root, lib, _files) = setup();
    let fy = ForYou::new(lib, &root, root.parent().unwrap().join("local2"));
    assert_eq!(fy.settings().openalex_key, "");
    let s0 = serde_json::to_value(fy.state()).unwrap();
    assert!(s0.get("openalexKey").is_none(), "left out when there is none, as Swift does");
    // set_openalex_key also sets the process-wide key: put it back after
    fy.set_openalex_key(" abc123 ").unwrap();
    assert_eq!(fy.settings().openalex_key, "abc123");
    assert_eq!(serde_json::to_value(fy.state()).unwrap()["openalexKey"], "abc123");
    assert_eq!(crate::metadata::openalex_key(), "abc123");
    fy.set_openalex_key("").unwrap();
    assert_eq!(crate::metadata::openalex_key(), "");
}

#[test]
fn preprints_from_biorxiv_by_keyword() {
    let fake = Fake::new();
    let kws = vec!["transcriptom".into(), "neurons".into(), "virus".into(), "ai".into()];
    let hits = run::preprints(&fake, "biorxiv", "2026-11-16", "2026-11-17", &kws).unwrap();
    let dois: Vec<&str> = hits.iter().map(|h| h.meta.doi.as_str()).collect();
    // in the title, or two keywords in the abstract ("ai" is too short to count)
    assert_eq!(dois, vec!["10.64898/2026.11.13.766189", "10.64898/2026.11.11.765928"]);
    let h = &hits[0];
    assert_eq!(h.meta.journal, "bioRxiv");
    assert_eq!(h.meta.authors[0], "E. Stronati");
    assert_eq!(h.meta.url, "https://www.biorxiv.org/content/10.64898/2026.11.13.766189");
    assert!(run::preprints(&fake, "biorxiv", "a", "b", &["ai".into()]).unwrap().is_empty());
}

#[test]
fn a_run_with_the_model_screens_summarizes_and_digests() {
    let (_root, lib, files) = setup();
    files
        .change(|s| {
            s.interests = "I work on spatial transcriptomics: imaging-based cell segmentation.".into();
            s.per_day = 8;
            s.preprints = Preprints::Few; // two preprints in 8
        })
        .unwrap();
    files.save_watches(&[Watch::new(WatchType::Author, "0000-0001-2345-6789", "Jane Doe", D0)]).unwrap();
    let fake = Fake::new();
    let model = judge();
    run_with(&lib, &files, &fake, Some(&model), D0);
    let s = files.state();
    // the model wrote the searches and keywords
    assert_eq!(s.queries, vec!["spatial transcriptomics cell segmentation", "soil microbiome nitrogen"]);
    assert_eq!(s.queries_for, s.interests);
    assert_eq!(s.keywords, vec!["transcriptom", "neurons", "virus"]);
    // the author you follow goes in whatever the model thinks
    let by_jane: Vec<&FeedRecord> = s.items.iter().filter(|r| r.source == "New paper by Jane Doe, whom you follow.").collect();
    assert_eq!(by_jane.len(), 1);
    assert_eq!(by_jane[0].reason, by_jane[0].source);
    // the rest: the four segmentation papers the model kept (8 ≥ 7), two of them preprints
    let screened: Vec<&FeedRecord> = s.items.iter().filter(|r| r.source.starts_with("search")).collect();
    assert_eq!(screened.len(), 4, "{screened:?}");
    assert!(screened.iter().all(|r| r.title.to_lowercase().contains("segmentation") && r.reason == "A segmentation method for your Xenium data."));
    let preprints = s.items.iter().filter(|r| is_preprint(&record_hit(r), &r.source)).count();
    assert_eq!(preprints, 2, "{:?}", s.items);
    // turned down (soil) were offered: never again; a third preprint (PRISM) was skipped, not
    // screened, so it may come another day; every kept paper has its sentence
    let prism = "10.64898/2026.11.07.765042".to_string();
    assert!(!s.shown.contains(&prism) && !s.items.iter().any(|r| r.key == prism));
    assert!(s.shown.contains(&"10.7717/peerj.21912".to_string()));
    assert!(s.shown.len() > s.items.len());
    assert!(s.items.iter().filter(|r| !r.r#abstract.is_empty()).all(|r| r.summary.as_deref() == Some("Reassigns transcripts to cells to segment them more accurately.")));
    // the digest, linked; the screening prompt showed the model your interests
    assert!(s.digest.starts_with("Segmentation dominates today: [Unified]("), "{}", s.digest);
    assert_eq!(s.digest_for.as_deref(), Some(s.digest_key(CET).as_str()));
    let prompts = model.prompts.lock().unwrap();
    assert!(prompts.iter().any(|p| p.contains("The researcher's interests:\nI work on spatial transcriptomics")));
    // nothing more to summarize
    let fy = ForYou::new(lib.clone(), files.library.clone(), files.local.clone());
    assert!(!fy.needs_summaries(CET));
}

#[test]
fn your_orcid_id() {
    let (_root, lib, files) = setup();
    let mut fake = Fake::new();
    let own = works(OA_SPATIAL);
    fake.profile = Some(OrcidProfile {
        orcid: "0000-0002-1825-0097".into(),
        name: "Josiah Carberry".into(),
        openalex_id: "A5000000001".into(),
        works_count: 40,
        cited_by_count: 100,
        works: own[..3].to_vec(),
        // one of the most cited is also among the latest: once
        most_cited: vec![own[2].clone(), own[3].clone()],
        coauthors: vec![Coauthor { name: "Jane Doe".into(), openalex_id: "A5000000002".into(), orcid: "0000-0001-2345-6789".into(), shared: 3 }],
    });
    let fy = ForYou::new(lib.clone(), files.library.clone(), files.local.clone());
    assert_eq!(fy.connect_orcid(&fake, "not an id", D0).unwrap_err(), "That isn’t an ORCID iD. It looks like 0000-0002-1825-0097.");
    fy.connect_orcid(&fake, "https://orcid.org/0000-0002-1825-0097", D0).unwrap();
    let link = files.state().orcid.unwrap();
    assert_eq!(link.papers.len(), 4);
    assert_eq!(link.papers[0].authors.len().min(12), link.papers[0].authors.len());
    assert!(link.is_mine(&["someone".into(), "JOSIAH CÁRBERRY".into()]));
    assert!(!link.is_mine(&["Josiah Carberry-Smith".into()]));
    // the most cited half first, then the latest, all with DOIs
    let mut cited = link.papers.clone();
    cited.sort_by(|a, b| b.cited_by.cmp(&a.cited_by));
    assert_eq!(link.citable(2)[0].key(), cited[0].key());
    assert_eq!(link.citable(15).len(), 4);
    // co-authors: follow, once
    let settings = fy.settings();
    assert_eq!(settings.orcid.as_ref().unwrap().coauthors[0].following, false);
    assert!(fy.follow("A5000000002", D0).unwrap());
    assert!(!fy.follow("A5000000002", D0).unwrap());
    let w = fy.watches();
    assert_eq!((w[0].kind, w[0].value.as_str(), w[0].label.as_str()), (WatchType::Author, "0000-0001-2345-6789", "Jane Doe"));
    assert!(fy.settings().orcid.unwrap().coauthors[0].following);

    // a run: papers citing yours come first and always go in; your own never
    let report = fy.run(&fake, None, D0 + HOUR_MS, CET, &quiet).unwrap().unwrap();
    assert!(report.found > 0);
    let s = files.state();
    assert!(s.items.iter().any(|r| r.key == "10.9/cites-you" && r.reason.starts_with("Cites your paper “")));
    assert!(!s.items.iter().any(|r| r.key == "10.9/by-you"), "your own paper (by name)");
    for p in &link.papers {
        assert!(!s.items.iter().any(|r| r.key == p.key()), "your own paper (by DOI)");
    }
    let calls = fake.calls();
    assert!(calls.iter().any(|c| c.starts_with("citing ") && c.contains("not by A5000000001")), "{calls:?}");
    // looked up again only after 20 hours
    assert_eq!(calls.iter().filter(|c| c.starts_with("orcid")).count(), 1);
    fy.run(&fake, None, D0 + 21 * HOUR_MS, CET, &quiet).unwrap();
    assert_eq!(fake.calls().iter().filter(|c| c.starts_with("orcid")).count(), 2);
    fy.disconnect_orcid().unwrap();
    assert!(fy.state().orcid.is_none());
}

#[test]
fn feedback_add_more_like_this_not_for_me() {
    let (_root, lib, files) = setup();
    let fy = ForYou::new(lib, files.library.clone(), files.local.clone());
    files
        .change(|s| {
            s.items = (0..5).map(|i| FeedRecord { r#abstract: "abstract ".repeat(100), found: D0, ..rec(&format!("10.1/{i}"), &format!("Paper {i}")) }).collect();
            s.disliked = (0..150).map(|i| FeedbackSnip { key: format!("old{i}"), text: String::new() }).collect();
        })
        .unwrap();
    fy.dismiss("10.1/0").unwrap();
    let s = fy.state();
    assert_eq!(s.items.len(), 4);
    assert_eq!(s.disliked.len(), 150, "the oldest goes");
    assert_eq!(s.disliked.last().unwrap().key, "10.1/0");
    assert!(s.disliked.last().unwrap().text.starts_with("Paper 0. abstract"));
    assert_eq!(s.disliked.last().unwrap().text.chars().count(), "Paper 0. ".len() + 400);
    assert!(fy.boost("10.1/1").unwrap());
    assert_eq!(fy.state().boosted.len(), 1);
    assert!(!fy.boost("10.1/1").unwrap(), "again: taken back");
    let s = fy.state();
    assert!(s.boosted.is_empty() && s.items[0].boosted == Some(false));
    fy.added("10.1/2", "PAPER-ID").unwrap();
    fy.added("10.1/2", "PAPER-ID").unwrap();
    let s = fy.state();
    assert_eq!(s.items.iter().find(|r| r.key == "10.1/2").unwrap().in_library.as_deref(), Some("PAPER-ID"));
    assert_eq!(s.liked.len(), 1);
    assert_eq!(fy.find_meta("10.1/3").unwrap().doi, "10.1/3");
    // the badge counts what is new since the feed was opened here, not added
    assert_eq!(fy.new_count(), 3);
    fy.mark_opened(D0).unwrap();
    assert_eq!(fy.new_count(), 0);
}

#[test]
fn two_devices_share_one_new_for_you() {
    let (root, lib, _) = setup();
    // the same library folder (synced), each device its own folder
    let pc = ForYou::new(lib.clone(), &root, root.parent().unwrap().join("pc"));
    let phone = ForYou::new(lib.clone(), &root, root.parent().unwrap().join("phone"));
    assert!(pc.set_settings("  spatial transcriptomics  ", 6, 6 * 60, "few").unwrap());
    assert!(!pc.set_settings("spatial transcriptomics", 6, 6 * 60, "few").unwrap(), "same paragraph: no run needed");
    pc.files().change(|s| s.items = vec![FeedRecord { found: D0, ..rec("10.1/a", "A") }, FeedRecord { found: D0, ..rec("10.1/b", "B") }]).unwrap();
    // each change re-reads the file: the phone's and the computer's changes all stay
    phone.dismiss("10.1/a").unwrap();
    pc.boost("10.1/b").unwrap();
    assert!(phone.add_watch("journal", "Nature Methods", "", D0).unwrap());
    assert!(!pc.add_watch("journal", "nature methods", "", D0).unwrap(), "already followed (on the phone)");
    assert!(pc.add_watch("topic", "xenium", "Xenium", D0).unwrap());
    for d in [&pc, &phone] {
        let s = d.state();
        assert_eq!(s.interests, "spatial transcriptomics");
        assert_eq!((s.per_day, s.morning, s.preprints), (6, 360, Preprints::Few));
        assert_eq!(s.items.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), vec!["10.1/b"]);
        assert_eq!(s.items[0].boosted, Some(true));
        assert_eq!(s.disliked[0].key, "10.1/a");
        assert_eq!(d.watches().len(), 2);
    }
    let id = phone.watches()[1].id.clone();
    pc.remove_watch(&id).unwrap();
    assert_eq!(phone.watches().len(), 1);
    // what was opened and announced is each device's own
    phone.mark_opened(D0 + 1).unwrap();
    assert_eq!(phone.new_count(), 0);
    assert_eq!(pc.new_count(), 1);
    // the files are where the Apple apps look
    assert!(root.join("foryou.json").exists() && root.join("watches.json").exists());
    assert!(!std::fs::read_dir(&root).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
}

#[test]
fn the_morning_notification() {
    let mut s = ForYouState { interests: "x".into(), ..Default::default() };
    let morning = s.last_morning(D0, CET);
    s.last_run = Some(morning + 60_000);
    s.items = vec![FeedRecord { found: morning + 60_000, ..rec("10.1/a", "First paper") }, FeedRecord { found: morning + 60_000, ..rec("10.1/b", "Second paper") }];
    let local = LocalState::default();
    let (title, body) = run::morning_notice(&s, &local, D0, CET).unwrap();
    assert_eq!(title, "New for You · 2 papers");
    assert_eq!(body, "First paper · Second paper");
    s.digest = format!("Segmentation dominates: [First]({}) maps cells. Then more.", feed_link("10.1/a"));
    assert_eq!(run::morning_notice(&s, &local, D0, CET).unwrap().1, "Segmentation dominates: First maps cells.");
    // once per feed, not after opening it, not for an old feed, not for added papers
    assert!(run::morning_notice(&s, &LocalState { notified: s.last_run.unwrap(), ..local.clone() }, D0, CET).is_none());
    assert!(run::morning_notice(&s, &LocalState { opened: D0, ..local.clone() }, D0, CET).is_none());
    assert!(run::morning_notice(&s, &local, D0 + 2 * DAY_MS, CET).is_none());
    s.items.iter_mut().for_each(|r| r.in_library = Some("x".into()));
    assert!(run::morning_notice(&s, &local, D0, CET).is_none());
}

#[test]
fn library_papers_are_not_offered() {
    let (_root, lib, files) = setup();
    // a paper in the library by title only (no DOI) and one by DOI
    let spatial = works(OA_SPATIAL);
    lib.add_paper(PaperMeta { title: spatial[0].meta.title.clone(), ..Default::default() }, None, None, None).unwrap();
    lib.add_paper(PaperMeta { title: "Other".into(), doi: spatial[3].meta.doi.to_uppercase(), ..Default::default() }, None, None, None).unwrap();
    assert_eq!(lib.papers(PaperFilter::default(), SortKey::Added).len(), 2);
    files
        .change(|s| {
            s.interests = "spatial transcriptomics segmentation".into();
            s.queries = vec!["spatial transcriptomics".into()];
            s.queries_for = s.interests.clone();
            s.preprints = Preprints::None;
        })
        .unwrap();
    run_with(&lib, &files, &Fake::new(), None, D0);
    let s = files.state();
    assert!(!s.items.iter().any(|r| r.title == spatial[0].meta.title || r.doi == spatial[3].meta.doi), "{:?}", s.items);
    assert!(!s.items.is_empty());
}

/// Writes a full foryou.json and watches.json made by the core into $LUNTA_FORYOU_DUMP, for the
/// Apple apps' decoder to read (checked by hand with a Swift program holding Watches.swift's types).
#[test]
#[ignore]
fn dump_for_swift() {
    let Ok(dir) = std::env::var("LUNTA_FORYOU_DUMP") else { return };
    let (_root, lib, files) = setup();
    let mut fake = Fake::new();
    let own = works(OA_SPATIAL);
    fake.profile = Some(OrcidProfile {
        orcid: "0000-0002-1825-0097".into(),
        name: "Josiah Carberry".into(),
        openalex_id: "A5000000001".into(),
        works_count: 40,
        cited_by_count: 100,
        works: own[..3].to_vec(),
        most_cited: vec![own[3].clone()],
        coauthors: vec![Coauthor { name: "Jane Doe".into(), openalex_id: "A5000000002".into(), orcid: String::new(), shared: 3 }],
    });
    let fy = ForYou::new(lib.clone(), files.library.clone(), files.local.clone());
    fy.set_settings("I work on spatial transcriptomics: imaging-based cell segmentation.", 8, 390, "few").unwrap();
    fy.connect_orcid(&fake, "0000-0002-1825-0097", D0).unwrap();
    fy.follow("A5000000002", D0).unwrap();
    fy.add_watch("journal", "Nature Methods", "Nature Methods", D0).unwrap();
    fy.run(&fake, Some(&judge()), D0, CET, &quiet).unwrap();
    let first = fy.state().items[0].key.clone();
    fy.boost(&first).unwrap();
    let second = fy.state().items[1].key.clone();
    fy.added(&second, "PAPER-1").unwrap();
    let third = fy.state().items[2].key.clone();
    fy.dismiss(&third).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(files.state_path(), std::path::Path::new(&dir).join("foryou.json")).unwrap();
    std::fs::copy(files.watches_path(), std::path::Path::new(&dir).join("watches.json")).unwrap();
}

/// A real run on OpenAlex and bioRxiv (network), without a model, on a throwaway library:
/// LUNTA_FORYOU_LIVE=1 cargo test foryou::tests::live_run -- --ignored --nocapture
#[test]
#[ignore]
fn live_run() {
    if std::env::var("LUNTA_FORYOU_LIVE").is_err() {
        return;
    }
    let (_root, lib, files) = setup();
    let fy = ForYou::new(lib, files.library.clone(), files.local.clone());
    fy.set_settings("I work on spatial transcriptomics: imaging-based cell segmentation and cell-type maps of brain tissue.", 8, 420, "few").unwrap();
    fy.add_watch("journal", "Nature Methods", "Nature Methods", now_ms()).unwrap();
    let started = std::time::Instant::now();
    let r = fy.run(&run::NetFinder, None, now_ms(), CET, &|s| eprintln!("{s}")).unwrap().unwrap();
    eprintln!("{} found in {:.1} s; {}", r.found, started.elapsed().as_secs_f64(), r.stats);
    for f in fy.feed(now_ms(), CET).latest {
        eprintln!("- {:.3} {} [{}]\n  {}\n  {}", fy.state().items.iter().find(|i| i.key == f.key).unwrap().score, f.title, f.byline, f.reason, f.preview);
    }
    assert!(r.candidates > 0);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

// ---- journal highlights (journals.rs), on recorded Crossref and Europe PMC answers ----

mod journal_highlights {
    use super::super::journals::{self, *};
    use super::*;

    const CR_NATURE: &str = include_str!("testdata/journals/crossref-nature.json");
    const CR_SCIENCE: &str = include_str!("testdata/journals/crossref-science.json");
    const CR_CELL: &str = include_str!("testdata/journals/crossref-cell.json");
    const EPMC: &str = include_str!("testdata/journals/europepmc.json");

    /// Crossref and Europe PMC as recorded on 23 Nov 2026 (the week from 16 Nov); `down`: Crossref
    /// doesn't answer.
    struct Recorded {
        calls: Mutex<Vec<String>>,
        down: bool,
    }

    impl Recorded {
        fn new() -> Recorded {
            Recorded { calls: Mutex::new(vec![]), down: false }
        }
    }

    impl Finder for Recorded {
        fn watch_new(&self, _: WatchKind, _: &str, _: &str, _: u32) -> crate::model::Result<Vec<DiscoverHit>> {
            Ok(vec![])
        }
        fn citing_new(&self, _: &str, _: &str, _: &str, _: u32) -> crate::model::Result<Vec<DiscoverHit>> {
            Ok(vec![])
        }
        fn orcid_profile(&self, o: &str) -> crate::model::Result<OrcidProfile> {
            Err(CoreError::NotFound(o.into()))
        }
        fn get_json(&self, url: &str) -> crate::model::Result<Value> {
            self.calls.lock().unwrap().push(url.to_string());
            if self.down && url.contains("api.crossref.org") {
                return Err(CoreError::Network("api.crossref.org answered HTTP 503".into()));
            }
            let text = if url.contains("/journals/0028-0836/works") {
                CR_NATURE
            } else if url.contains("/journals/0036-8075/works") {
                CR_SCIENCE
            } else if url.contains("/journals/0092-8674/works") {
                CR_CELL
            } else if url.contains("europepmc") {
                EPMC
            } else if url.contains("api.crossref.org/journals?query=") {
                r#"{"message":{"items":[{"title":"Cell Reports Methods","ISSN":["2667-2375"]},{"title":"Cell Systems","ISSN":["2405-4712","2405-4720"]}]}}"#
            } else {
                return Err(CoreError::NotFound(url.into()));
            };
            Ok(serde_json::from_str(text).unwrap())
        }
    }

    fn handle() -> (Arc<JournalHighlights>, Files) {
        let (root, lib, files) = setup();
        files
            .change(|s| s.interests = "I map the brain's circuits and cell types with spatial transcriptomics (MERFISH, in situ sequencing) and study neurons and glia in neuroinflammation.".into())
            .unwrap();
        let h = JournalHighlights::new(lib, root.to_string_lossy().into(), files.local.to_string_lossy().into());
        (h, files)
    }

    fn meta(title: &str, doi: &str, authors: usize, abs: usize, pages: &str) -> PaperMeta {
        PaperMeta {
            title: title.into(),
            doi: doi.into(),
            authors: (0..authors).map(|i| format!("A{i}")).collect(),
            abstract_text: "word ".repeat(abs / 5),
            pages: pages.into(),
            work_type: "journal-article".into(),
            ..Default::default()
        }
    }

    #[test]
    fn research_papers_only() {
        assert!(is_research(&meta("Bogong moths navigate with a light-dependent geomagnetic inclination compass", "10.1038/s41586-026-11322-9", 7, 1800, "")));
        // no abstract on Crossref (Cell, some of Nature's), enough authors
        assert!(is_research(&meta("Myelin pathology is a key feature of X-linked dystonia-parkinsonism", "10.1016/j.cell.2026.10.046", 23, 0, "5012-5030.e21")));
        for (m, why) in [
            (meta("Author Correction: SARS-CoV-2 mRNA vaccines sensitize tumours", "10.1038/s41586-026-10974-x", 140, 0, ""), "correction"),
            (meta("Retraction Note: Detection of anisotropic cosmic structures", "10.1038/s41586-026-11388-5", 2, 0, ""), "retraction"),
            (meta("How measuring the state of nature can reverse biodiversity loss", "10.1038/d41586-026-03714-8", 13, 0, "594-597"), "Nature's news and comment"),
            (meta("In Science Journals", "10.1126/science.aeo2218", 11, 183, "850-852"), "In this issue"),
            (meta("Measles comeback in U.S. adds to global resurgence", "10.1126/science.aeo2672", 1, 121, "822-823"), "a teaser for an abstract"),
            (meta("Rising risk of deforestation in Indonesia’s pulp sector", "10.1126/science.aeb9995", 11, 73, "847-849"), "a teaser for an abstract"),
            (meta("The scientific record is never finished", "10.1126/science.aen9176", 3, 1135, "817-817"), "one page"),
            (meta("Making ecological programs work at scale", "10.1126/science.aen0206", 1, 0, ""), "no abstract, one author"),
            (meta("SnapShot: Microglia", "10.1016/j.cell.2026.10.001", 2, 0, "1-1.e1"), "SnapShot"),
        ] {
            assert!(!is_research(&m), "{} should be left out ({why})", m.title);
        }
        let mut posted = meta("A preprint", "10.1101/x", 5, 1000, "");
        posted.work_type = "posted-content".into();
        assert!(!is_research(&posted));
    }

    #[test]
    fn a_week_of_nature_science_and_cell() {
        let (h, files) = handle();
        let rec = Recorded::new();
        for j in ["Nature", "Science", "Cell"] {
            h.set_journal_with(&rec, j, true).unwrap();
        }
        assert!(h.is_due(D0, CET, false), "due the first time");
        let report = h.run_with(&rec, D0, CET).unwrap();
        assert!(report.ran && !report.failed, "{report:?}");
        assert_eq!(report.problem, None);
        let s = h.state();
        assert_eq!(report.found as usize, s.items.len());
        let calls = rec.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c.starts_with("https://api.crossref.org/journals/0028-0836/works?filter=from-created-date:2026-11-16,type:journal-article&sort=created")), "{calls:?}");
        assert!(calls.iter().any(|c| c.contains("europepmc") && c.contains("DOI:%2210.1016%2Fj.cell.2026.10.046%22")), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains("openalex")), "no OpenAlex allowance used");
        assert_eq!(s.since, "2026-11-16");
        assert_eq!(s.last_run, Some(D0));

        let keys: Vec<&str> = s.items.iter().map(|r| r.key.as_str()).collect();
        let by = |j: &str| s.items.iter().filter(|r| r.journal == j).count();
        assert!(by("Nature") >= 15 && by("Nature") <= 30, "{}", by("Nature"));
        assert!(by("Science") >= 8, "{}", by("Science"));
        assert_eq!(by("Cell"), 8);
        for k in &keys {
            assert!(!k.starts_with("10.1038/d41586"), "Nature's news: {k}");
        }
        for gone in ["10.1038/s41586-026-10974-x", "10.1038/s41586-026-11388-5", "10.1126/science.aeo2218", "10.1126/science.aeo2672", "10.1126/science.aen9176"] {
            assert!(!keys.contains(&gone), "{gone} isn't a research paper");
        }
        for kept in ["10.1038/s41586-026-11235-7", "10.1126/science.aej3009", "10.1016/j.cell.2026.10.046", "10.1038/s41586-026-11322-9"] {
            assert!(keys.contains(&kept), "{kept} should be in: {keys:?}");
        }
        // Europe PMC filled in what Crossref lacks (Cell has no abstracts there)
        let myelin = s.items.iter().find(|r| r.key == "10.1016/j.cell.2026.10.046").unwrap();
        assert!(myelin.r#abstract.len() > 300, "{myelin:?}");
        assert_eq!(myelin.date, "2026-11-19", "the day it went online, not the issue's month");
        // closest to the paragraph first
        let pos = |k: &str| keys.iter().position(|x| *x == k).unwrap();
        assert!(pos("10.1038/s41586-026-11235-7") < pos("10.1038/s41586-026-11299-5"), "MERFISH circuits before perovskite circuits");

        // the list as the apps show it
        let v = h.view(D0, CET);
        assert!(v.on);
        assert_eq!(v.subtitle, "Nature, Science and Cell · 16–23 Nov");
        assert_eq!(v.papers.len(), s.items.len());
        assert_eq!(v.papers[0].number, 1);
        assert!(v.papers.iter().all(|p| !p.line.is_empty() || p.abstract_text.is_empty()));
        let merfish = v.papers.iter().find(|p| p.key == "10.1038/s41586-026-11235-7").unwrap();
        assert!(merfish.byline.ends_with("· Nature"), "{}", merfish.byline);
        assert_eq!(merfish.link, "https://doi.org/10.1038/s41586-026-11235-7");
        assert!(v.digest.is_empty() && v.problem.is_none());
        assert_eq!(v.next_run, D0 - 2 * HOUR_MS + 7 * DAY_MS, "next Monday at 07:00 CET");

        // done for the week; due next Monday at the morning time (a phone leaves two hours)
        assert!(!h.is_due(D0 + 3 * DAY_MS, CET, false));
        let next_monday = D0 - 2 * HOUR_MS + 7 * DAY_MS;
        assert!(!h.is_due(next_monday - 60_000, CET, false));
        assert!(h.is_due(next_monday + 60_000, CET, false));
        assert!(!h.is_due(next_monday + 60_000, CET, true));
        assert!(h.is_due(next_monday + 2 * HOUR_MS + 60_000, CET, true));
        // every day: due each morning
        h.set_cadence("daily".into()).unwrap();
        assert!(h.is_due(D0, CET, false), "a new cadence starts a new period");

        // the next run offers nothing twice
        let again = h.run_with(&rec, D0 + DAY_MS, CET).unwrap();
        assert_eq!(again.found, 0, "{:?}", h.state().items.iter().map(|r| &r.key).collect::<Vec<_>>());
        assert!(files.state().items.is_empty(), "the feed itself is untouched");

        // Not for Me, Add
        h.set_cadence("weekly".into()).unwrap();
        let fresh = handle().0;
        for j in ["Nature", "Cell"] {
            fresh.set_journal_with(&rec, j, true).unwrap();
        }
        fresh.run_with(&rec, D0, CET).unwrap();
        let first = fresh.state().items[0].key.clone();
        fresh.dismiss(first.clone()).unwrap();
        assert!(!fresh.state().items.iter().any(|r| r.key == first));
        let second = fresh.state().items[0].key.clone();
        assert_eq!(fresh.find_meta(second.clone()).unwrap().doi, second);
        fresh.added(second.clone(), "paper-1".into()).unwrap();
        assert_eq!(fresh.view(D0, CET).papers[0].in_library.as_deref(), Some("paper-1"));
    }

    #[test]
    fn the_digest_cites_its_papers_and_says_only_what_they_say() {
        let (h, _files) = handle();
        let rec = Recorded::new();
        for j in ["Nature", "Science", "Cell"] {
            h.set_journal_with(&rec, j, true).unwrap();
        }
        h.run_with(&rec, D0, CET).unwrap();
        let s = h.state();
        let batch = s.digest_batch();
        assert_eq!(batch.len(), 12);
        assert!(["Nature", "Science", "Cell"].iter().all(|j| batch.iter().filter(|r| r.journal == *j).count() <= 4));
        let req = h.digest_request().unwrap();
        assert_eq!(req.instructions, journals::DIGEST_INSTRUCTIONS);
        assert!(req.prompt.contains(&format!("[1] {} ({})", batch[0].title, batch[0].journal)), "{}", req.prompt);
        assert!(req.prompt.contains("New research papers this week in Nature, Science and Cell:"));
        assert!(req.prompt.contains("The scientist's interests"));
        let t = |i: usize| batch[i].title.clone();
        let answer = format!(
            "Here is the digest:\n\
             {} [1]. {} and {} [2, 3]. \
             Everything in biology changed this week. \
             {} in 97% of zebrafish [4]. \
             {} [42].",
            t(0), t(1), t(2), t(3), t(4)
        );
        let kept = h.set_digest(req.key.clone(), answer).unwrap();
        assert_eq!(kept, 2, "{:?}", h.state().digest);
        let s = h.state();
        assert_eq!(s.digest[0].keys, vec![batch[0].key.clone()]);
        assert_eq!(s.digest[1].keys, vec![batch[1].key.clone(), batch[2].key.clone()]);
        assert!(h.digest_request().is_none(), "written for these papers");
        let v = h.view(D0, CET);
        assert_eq!(v.digest.len(), 2);
        assert_eq!(v.digest[1].cites.iter().map(|c| c.number).collect::<Vec<_>>(), vec![2, 3]);
        let link = highlight_link(&batch[0].key);
        assert!(v.digest_markdown.contains(&format!("[1]({link})")), "{}", v.digest_markdown);
        assert_eq!(highlight_key(&link).as_deref(), Some(batch[0].key.as_str()));
        assert!(v.digest_share.starts_with("From Nature, Science and Cell (16–23 Nov), Lunta"));
        // an answer for papers since replaced is dropped
        assert_eq!(h.set_digest("v1|other".into(), "Anything [1].".into()).unwrap(), 0);

        // turning a cited paper down drops the sentence citing it
        h.dismiss(batch[0].key.clone()).unwrap();
        let v = h.view(D0, CET);
        assert_eq!(v.digest.len(), 1);

        // the core's model (desktop, Android)
        struct Model;
        impl Writer for Model {
            fn answer(&self, instructions: &str, prompt: &str, _: u32) -> Option<String> {
                assert_eq!(instructions, journals::DIGEST_INSTRUCTIONS);
                let first = prompt.split("\n[1] ").nth(1)?.split(" (").next()?.to_string();
                Some(format!("{first} [1]."))
            }
        }
        assert_eq!(write_digest(h.files(), &Model), 1);
    }

    #[test]
    fn choosing_journals() {
        let (h, files) = handle();
        let rec = Recorded::new();
        // journals you follow come with a Highlights toggle (OpenAlex's name for PNAS too)
        files
            .save_watches(&[
                Watch::new(WatchType::Journal, "Nature", "Nature", D0),
                Watch::new(WatchType::Journal, "Proceedings of the National Academy of Sciences", "Proceedings of the National Academy of Sciences", D0),
                Watch::new(WatchType::Author, "Jane Doe", "Jane Doe", D0),
            ])
            .unwrap();
        let c = h.choices();
        assert_eq!(c[0], JournalChoice { name: "Nature".into(), on: false, followed: true });
        assert_eq!(c[1], JournalChoice { name: "PNAS".into(), on: false, followed: true });
        assert!(c.iter().any(|x| x.name == "Cell" && !x.followed && !x.on));
        assert_eq!(c.iter().filter(|x| x.name == "Nature").count(), 1);
        h.set_journal_with(&rec, "PNAS", true).unwrap();
        h.set_journal_with(&rec, "nature neuroscience", true).unwrap();
        let s = h.state();
        assert_eq!(s.journals[0].issn, vec!["0027-8424", "1091-6490"]);
        assert_eq!(s.journals[1].name, "Nature Neuroscience");
        assert_eq!(h.choices()[0], JournalChoice { name: "PNAS".into(), on: true, followed: true });
        // a journal not in the table: its ISSN from Crossref (an exact name, else the first)
        h.set_journal_with(&rec, "Cell Systems", true).unwrap();
        assert_eq!(h.state().journals[2].issn, vec!["2405-4712", "2405-4720"]);
        h.set_journal_with(&rec, "PNAS", false).unwrap();
        assert_eq!(h.state().journals.len(), 2);
        assert!(suggested("The New England Journal of Medicine").is_some() && suggested("NEJM").is_some());
        // the file keeps what a newer app adds
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(files.library.join(journals::FILE)).unwrap()).unwrap();
        v["futureKey"] = Value::from(1);
        std::fs::write(files.library.join(journals::FILE), v.to_string()).unwrap();
        h.set_cadence("daily".into()).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(files.library.join(journals::FILE)).unwrap()).unwrap();
        assert_eq!(v["futureKey"], 1);
        assert_eq!(v["cadence"], "daily");
    }

    #[test]
    fn crossref_down_is_said_and_the_week_stays_due() {
        let (h, _files) = handle();
        let rec = Recorded { calls: Mutex::new(vec![]), down: true };
        h.set_journal_with(&rec, "Nature", true).unwrap();
        let r = h.run_with(&rec, D0, CET).unwrap();
        assert!(r.failed);
        assert_eq!(r.problem.as_deref(), Some("Couldn't reach Crossref. Nature: api.crossref.org answered HTTP 503"));
        assert_eq!(h.state().last_run, None);
        assert!(h.is_due(D0, CET, false));
        assert_eq!(h.view(D0, CET).problem, r.problem);
        // and gone once it answers
        h.run_with(&Recorded::new(), D0, CET).unwrap();
        assert_eq!(h.view(D0, CET).problem, None);
    }

    #[test]
    fn one_line_and_periods() {
        assert_eq!(
            key_sentence("Direction selectivity is a classic computation. Here we develop TransA-MERFISH to map circuits. It works."),
            "We develop TransA-MERFISH to map circuits."
        );
        assert_eq!(key_sentence("Only one sentence here"), "Only one sentence here");
        assert_eq!(key_sentence(""), "");
        let s = HighlightsState::default();
        // Wednesday 25 Nov 2026, 10:00 CET: the week's papers were due Monday 07:00
        let wed = D0 + 2 * DAY_MS + HOUR_MS;
        assert_eq!(iso8601(s.last_due(7 * 60, wed, CET)), "2026-11-23T06:00:00Z");
        // Monday before 07:00: last Monday's
        assert_eq!(iso8601(s.last_due(7 * 60, D0 - 3 * HOUR_MS, CET)), "2026-11-16T06:00:00Z");
        let daily = HighlightsState { cadence: "daily".into(), ..Default::default() };
        assert_eq!(iso8601(daily.last_due(7 * 60, wed, CET)), "2026-11-25T06:00:00Z");
    }
}

// ---- Across science (across.rs) and the impact prior (impact.rs), on a recorded OpenAlex answer ----

mod across_science {
    use super::super::across::{self, *};
    use super::super::impact;
    use super::*;

    /// OpenAlex as recorded on 24 Nov 2026: the table's journals' articles since 17 Nov (91 of the
    /// 897 works, cut down: every field's best and some of the broad journals', Nature's news,
    /// PNAS's "In This Issue", C&EN's news, and papers on a neuroscientist's own topics).
    const OA_DAY: &str = include_str!("testdata/across/openalex-2026-11-24.json");
    /// 2026-11-24T08:00:00Z (09:00 CET): the morning after the recording.
    const NOW: i64 = D0 + DAY_MS;
    const NEURO: &str = "I map the brain's circuits and cell types with spatial transcriptomics (MERFISH, in situ sequencing) and study neurons and glia: microglia in neuroinflammation and myelin.";

    /// OpenAlex: the recorded day as page 1; `pages`: that many full pages of 200 instead (to
    /// count requests); `down`: refused.
    struct Recorded {
        calls: Mutex<Vec<String>>,
        down: bool,
        pages: Option<(usize, u64)>,
    }

    impl Recorded {
        fn new() -> Recorded {
            Recorded { calls: Mutex::new(vec![]), down: false, pages: None }
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Finder for Recorded {
        fn watch_new(&self, _: WatchKind, _: &str, _: &str, _: u32) -> crate::model::Result<Vec<DiscoverHit>> {
            Ok(vec![])
        }
        fn citing_new(&self, _: &str, _: &str, _: &str, _: u32) -> crate::model::Result<Vec<DiscoverHit>> {
            Ok(vec![])
        }
        fn orcid_profile(&self, o: &str) -> crate::model::Result<OrcidProfile> {
            Err(CoreError::NotFound(o.into()))
        }
        fn get_json(&self, url: &str) -> crate::model::Result<Value> {
            self.calls.lock().unwrap().push(url.to_string());
            assert!(url.starts_with("https://api.openalex.org/works?"), "only OpenAlex's works: {url}");
            if self.down {
                return Err(CoreError::Network("OpenAlex's free daily allowance for your network is used up (it renews in about 17 hours, at midnight UTC).".into()));
            }
            if let Some((_, count)) = self.pages {
                // a page of 200 trivial works (research, Nature, one field)
                let page: usize = url.split("&page=").nth(1).and_then(|p| p.split('&').next()).and_then(|p| p.parse().ok()).unwrap();
                let results: Vec<Value> = (0..200)
                    .map(|i| {
                        serde_json::json!({"id": format!("W{page}-{i}"), "doi": format!("https://doi.org/10.9/p{page}-{i}"), "title": format!("A long enough research title {page} {i}"),
                        "authorships": [{"author": {"display_name": "A"}}, {"author": {"display_name": "B"}}, {"author": {"display_name": "C"}}],
                        "publication_date": "2026-11-21", "primary_location": {"source": {"display_name": "Nature", "issn": ["0028-0836"]}},
                        "primary_topic": {"field": {"id": "https://openalex.org/fields/31"}, "subfield": {"id": "https://openalex.org/subfields/3100"}, "score": 0.9}})
                    })
                    .collect();
                return Ok(serde_json::json!({"meta": {"count": count, "page": page, "per_page": 200}, "results": results}));
            }
            assert!(url.contains("&page=1&"), "one page answers the recorded day: {url}");
            Ok(serde_json::from_str(OA_DAY).unwrap())
        }
    }

    fn handle(interests: &str) -> (Arc<AcrossScience>, Arc<Library>, Files) {
        let (root, lib, files) = setup();
        files.change(|s| s.interests = interests.into()).unwrap();
        let a = AcrossScience::new(lib.clone(), root.to_string_lossy().into(), files.local.to_string_lossy().into());
        a.set_on(true).unwrap();
        (a, lib, files)
    }

    /// The recorded day's research papers as candidates.
    fn candidates() -> Vec<Candidate> {
        let j: Value = serde_json::from_str(OA_DAY).unwrap();
        j["results"].as_array().unwrap().iter().filter_map(across::candidate).collect()
    }

    fn titles(a: &AcrossScience) -> Vec<String> {
        a.state().items.iter().map(|r| r.title.clone()).collect()
    }

    #[test]
    fn the_table_and_the_fields() {
        assert_eq!(impact::by_name("Proceedings of the National Academy of Sciences").unwrap().name, "PNAS");
        assert_eq!(impact::by_name("Nature Structural & Molecular Biology").unwrap().tier, 0.8);
        assert_eq!(impact::by_name("the lancet").unwrap().name, "The Lancet");
        assert_eq!(impact::by_issn("1476-4687").unwrap().name, "Nature");
        assert!(impact::by_name("Journal of Something Else").is_none() && impact::by_name("").is_none());
        // journals.rs's suggestions are the table's, ISSNs and all
        for n in super::super::journals::SUGGESTED {
            assert!(impact::by_name(n).is_some(), "{n}");
        }
        assert_eq!(super::super::journals::suggested("NEJM").unwrap().1, &["0028-4793", "1533-4406"]);
        // one ISSN each in the one request, OpenAlex's limit is 100
        assert!(impact::JOURNALS.len() <= 100);
        let url = request_url("2026-11-20", 1);
        assert_eq!(url.matches('|').count(), impact::JOURNALS.len() - 1);
        assert!(!url.contains("group_by") && !url.contains("search=") && url.contains("type:article") && url.contains("per_page=200"));
        // every journal's field is a field, every OpenAlex field has one
        for j in impact::JOURNALS {
            assert!(j.field.is_empty() || field(j.field).is_some(), "{}", j.name);
        }
        for f in 11..=36 {
            assert!(field_of(f, 0).is_some(), "OpenAlex field {f}");
        }
        assert_eq!(field_of(13, 1311).unwrap().id, "genetics");
        assert_eq!(field_of(13, 1312).unwrap().id, "cellbio");
        assert_eq!(field_of(27, 2716).unwrap().id, "genetics");
        assert_eq!(FIELDS.len(), 13);
    }

    #[test]
    fn research_papers_only_one_a_field_by_impact() {
        let (a, _lib, _files) = handle("");
        let rec = Recorded::new();
        assert!(a.is_due(NOW, CET, false), "due the first time");
        let r = a.run_with(&rec, NOW, CET).unwrap();
        assert!(r.ran && !r.failed && r.problem.is_none(), "{r:?}");
        // one request: the first run looks back 7 days
        let calls = rec.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].contains("from_publication_date:2026-11-17,type:article") && calls[0].contains("primary_location.source.issn:0028-0836|0036-8075|0092-8674|"), "{}", calls[0]);
        assert_eq!(r.requests, 1);
        let s = a.state();
        assert_eq!((r.found, r.fields), (13, 13), "{:?}", titles(&a));
        // in the fields' order, one each
        let order: Vec<&str> = s.items.iter().map(|x| x.field.as_str()).collect();
        assert_eq!(order, FIELDS.iter().map(|f| f.id).collect::<Vec<_>>());
        // no news, comment, "In This Issue" or C&EN's news
        for x in &s.items {
            assert!(!x.doi.contains("d41586") && !x.title.starts_with("In This Issue") && !x.title.starts_with("News"), "{}", x.title);
        }
        // each field's paper is its most impactful research paper (impact first, then the rest)
        let pool = candidates();
        for x in &s.items {
            let best = pool
                .iter()
                .filter(|c| c.field.id == x.field && !super::super::journals::not_research(&c.meta) && super::super::journals::is_research(&c.meta))
                .map(|c| c.impact())
                .fold(0.0, f64::max);
            assert!(x.impact >= best - 0.08, "{}: {} picked at {}, best {best}", x.field, x.title, x.impact);
        }
        let neuro = s.items.iter().find(|x| x.field == "neuro").unwrap();
        assert!(["Nature", "Science", "Cell"].contains(&neuro.journal.as_str()), "{neuro:?}");
        // shown: never offered again
        assert_eq!(s.shown.len(), 13);
        // the view
        let v = a.view(NOW, CET);
        assert_eq!(v.subtitle, "13 fields · 24 Nov");
        assert_eq!(v.papers[0].number, 1);
        assert_eq!(v.papers[2].field_label, "Neuroscience");
        assert!(v.quiet_fields.is_empty() && v.problem.is_none());
        assert!(v.papers.iter().all(|p| p.link.starts_with("https://doi.org/10.")));
        // the same day: not due, no request
        assert!(!a.is_due(NOW + HOUR_MS, CET, false));
    }

    #[test]
    fn your_own_topics_stay_out() {
        let pool = candidates();
        let merfish = pool.iter().find(|c| c.meta.title.starts_with("TransA-MERFISH")).unwrap();
        assert_eq!(merfish.field.id, "neuro");
        let (a, lib, files) = handle(NEURO);
        // a paper in the library, and one the feed offered
        let science = pool.iter().find(|c| c.field.id == "neuro" && c.meta.journal == "Science").unwrap();
        lib.add_paper(PaperMeta { title: "Saved".into(), doi: science.meta.doi.clone(), ..Default::default() }, None, None, None).unwrap();
        let nature_neuro = pool.iter().find(|c| c.meta.title.starts_with("Microglia mediate motor memory")).unwrap();
        files.change(|s| s.shown.push(text::doi_key(&nature_neuro.meta.doi))).unwrap();
        let r = a.run_with(&Recorded::new(), NOW, CET).unwrap();
        assert!(r.yours >= 3, "{r:?}");
        let t = titles(&a);
        for gone in ["TransA-MERFISH", "Microglia mediate", "Galectin-3 links microglial", "Space-map"] {
            assert!(!t.iter().any(|x| x.starts_with(gone)), "{gone}: {t:?}");
        }
        assert!(!a.state().items.iter().any(|x| x.doi == science.meta.doi), "in the library");
        // the other fields as without a paragraph
        assert!(r.fields >= 12, "{r:?} {t:?}");
    }

    #[test]
    fn two_a_field_and_choosing_fields_pick_again_without_asking() {
        let (a, _lib, _files) = handle("");
        let rec = Recorded::new();
        a.run_with(&rec, NOW, CET).unwrap();
        let first: Vec<String> = a.state().items.iter().map(|r| r.key.clone()).collect();
        a.set_per_field(2).unwrap();
        assert!(a.is_due(NOW + HOUR_MS, CET, false));
        let r = a.run_with(&rec, NOW + HOUR_MS, CET).unwrap();
        assert_eq!(r.requests, 0, "picked again from the day's answer");
        assert_eq!(rec.calls().len(), 1);
        let s = a.state();
        // the morning's picks are still there (offered today, not before), with a second
        for k in &first {
            assert!(s.items.iter().any(|r| &r.key == k), "{k}");
        }
        for f in FIELDS {
            let mine: Vec<&AcrossRecord> = s.items.iter().filter(|r| r.field == f.id).collect();
            assert!(mine.len() <= 2);
            if mine.len() == 2 && mine[0].journal == mine[1].journal {
                // only when the field has nothing from another journal
                let others = candidates().into_iter().filter(|c| c.field.id == f.id && c.meta.journal != mine[0].journal && super::super::journals::is_research(&c.meta)).count();
                assert_eq!(others, 0, "{}: {:?}", f.id, mine);
            }
        }
        assert!(s.items.len() > 20, "{}", s.items.len());
        // a field off
        a.set_field("math".into(), false).unwrap();
        a.run_with(&rec, NOW + 2 * HOUR_MS, CET).unwrap();
        assert!(!a.state().items.iter().any(|r| r.field == "math"));
        assert!(!a.fields().iter().find(|f| f.id == "math").unwrap().on);
        assert_eq!(rec.calls().len(), 1);
        let v = a.view(NOW + 2 * HOUR_MS, CET);
        assert!(!v.subtitle.contains("13 fields") && v.subtitle.contains("Psychology & social sciences"), "{}", v.subtitle);
    }

    #[test]
    fn frugal_with_openalex() {
        let (a, _lib, _files) = handle("");
        let rec = Recorded::new();
        a.run_with(&rec, NOW, CET).unwrap();
        let first: Vec<String> = a.state().items.iter().map(|r| r.key.clone()).collect();
        // the next morning: one request, 4 days back, nothing offered twice
        let next = NOW + DAY_MS;
        assert!(a.is_due(next - HOUR_MS, CET, false) && !a.is_due(next - HOUR_MS, CET, true), "a phone leaves the first two hours to a computer");
        let r = a.run_with(&rec, next, CET).unwrap();
        assert_eq!(r.requests, 1);
        assert!(rec.calls()[1].contains("from_publication_date:2026-11-21,"), "{}", rec.calls()[1]);
        assert!(a.state().items.iter().all(|x| !first.contains(&x.key)));
        // a busy week: page by page, at most five
        let (a, _lib, _files) = handle("");
        let busy = Recorded { calls: Mutex::new(vec![]), down: false, pages: Some((0, 450)) };
        let r = a.run_with(&busy, NOW, CET).unwrap();
        assert_eq!((r.requests, busy.calls().len()), (3, 3));
        assert!(busy.calls()[2].contains("&page=3&"));
        let (a, _lib, _files) = handle("");
        let huge = Recorded { calls: Mutex::new(vec![]), down: false, pages: Some((0, 50_000)) };
        assert_eq!(a.run_with(&huge, NOW, CET).unwrap().requests, MAX_PAGES as u32);
        // off: nothing
        let (a, _lib, _files) = handle("");
        a.set_on(false).unwrap();
        assert!(!a.is_due(NOW, CET, false));
        let rec = Recorded::new();
        assert!(!a.run_with(&rec, NOW, CET).unwrap().ran);
        assert!(rec.calls().is_empty());
    }

    #[test]
    fn openalex_down_is_said_and_the_day_stays_due() {
        let (a, _lib, _files) = handle("");
        let down = Recorded { calls: Mutex::new(vec![]), down: true, pages: None };
        let r = a.run_with(&down, NOW, CET).unwrap();
        assert!(r.failed && r.ran);
        assert_eq!(r.problem.as_deref(), Some("Couldn't reach OpenAlex. OpenAlex's free daily allowance for your network is used up (it renews in about 17 hours, at midnight UTC)."));
        assert_eq!(down.calls().len(), 1, "not asked again and again");
        assert_eq!(a.state().last_run, None);
        assert!(a.is_due(NOW, CET, false));
        assert_eq!(a.view(NOW, CET).problem, r.problem);
        // and gone once it answers
        a.run_with(&Recorded::new(), NOW, CET).unwrap();
        assert_eq!(a.view(NOW, CET).problem, None);
        assert_eq!(a.state().items.len(), 13);
    }

    #[test]
    fn not_for_me_stays_here() {
        let (a, _lib, files) = handle(NEURO);
        let rec = Recorded::new();
        a.run_with(&rec, NOW, CET).unwrap();
        let before = files.state();
        let gone = a.state().items[3].clone();
        a.dismiss(gone.key.clone()).unwrap();
        assert!(!a.state().items.iter().any(|r| r.key == gone.key));
        assert_eq!(a.state().dismissed.len(), 1);
        // the feed's own feedback isn't touched
        let after = files.state();
        assert_eq!((after.disliked.len(), after.liked.len(), after.boosted.len()), (before.disliked.len(), before.liked.len(), before.boosted.len()));
        // picking again the same day doesn't bring it back
        a.set_per_field(2).unwrap();
        a.run_with(&rec, NOW + HOUR_MS, CET).unwrap();
        assert!(!a.state().items.iter().any(|r| r.key == gone.key));
        // added
        let k = a.state().items[0].key.clone();
        a.added(k.clone(), "p1".into()).unwrap();
        assert_eq!(a.view(NOW, CET).papers[0].in_library.as_deref(), Some("p1"));
        assert_eq!(a.find_meta(k).unwrap().work_type, "journal-article");
    }

    #[test]
    fn the_digest_one_sentence_a_field() {
        let (a, _lib, files) = handle("");
        a.run_with(&Recorded::new(), NOW, CET).unwrap();
        let s = a.state();
        let req = a.digest_request().unwrap();
        assert_eq!(req.instructions, across::DIGEST_INSTRUCTIONS);
        assert!(req.prompt.contains("[3] Neuroscience: "), "{}", req.prompt);
        assert!(req.prompt.len() < 6000, "fits the on-device model: {}", req.prompt.len());
        let t = |i: usize| s.items[i].title.clone();
        let answer = format!(
            "{} [1]. {} [2]. Also, {} [1]. This cures 99 diseases in 3 weeks [3]. {} [40]. Nothing cited here. {} [4].",
            t(0),
            t(1),
            t(0),
            t(2),
            t(3)
        );
        let n = a.set_digest(req.key.clone(), answer).unwrap();
        let v = a.view(NOW, CET);
        assert_eq!(n as usize, v.digest.len());
        let cited: Vec<u32> = v.digest.iter().map(|d| d.cites[0].number).collect();
        // one a field, said only what the papers say; a field whose sentence went (3: a claim its paper
        // doesn't make) or never came is there with its paper's title, in the list's order
        let fields = s.digest_batch().len() as u32;
        assert_eq!(cited, (1..=fields).collect::<Vec<u32>>(), "every field, in order: {:?}", v.digest);
        assert_eq!(v.digest[2].text, t(2).trim().trim_end_matches('.'), "the title stands in for the claim");
        assert!(!v.digest[2].text.contains("99 diseases"));
        assert_eq!(v.digest[0].field_label, "Cell & molecular biology");
        assert!(v.digest_share.starts_with("Across science (24 Nov), Lunta\n\nCell & molecular biology: "), "{}", v.digest_share);
        assert!(a.digest_request().is_none(), "written");
        assert_eq!(across_key(&across_link(&v.digest[1].cites[0].key)).as_deref(), Some(v.digest[1].cites[0].key.as_str()));
        // a stale answer is ignored; a sentence about a paper turned down since goes
        assert_eq!(set_digest(&files, "v1|other", "x [1].").unwrap(), 0);
        a.dismiss(s.items[1].key.clone()).unwrap();
        assert_eq!(a.view(NOW, CET).digest.len() as u32, fields - 1);
        // a model's digest (desktop and Android)
        struct Model;
        impl Writer for Model {
            fn answer(&self, i: &str, p: &str, _: u32) -> Option<String> {
                assert_eq!(i, across::DIGEST_INSTRUCTIONS);
                let title = p.lines().find(|l| l.starts_with("[1] ")).unwrap().split(": ").nth(1).unwrap().rsplit_once(" (").unwrap().0.to_string();
                Some(format!("{title} [1]."))
            }
        }
        // the model wrote one sentence; the other fields (one turned down above) have their titles
        assert_eq!(write_digest(&files, &Model), fields - 1);
    }

    #[test]
    fn impact_weighting_changes_order_only_modestly() {
        // at most 15 % more closeness: a closer paper stays first unless they were within 15 %
        let nature = impact::impact("Nature", 0, 0.0);
        assert_eq!(nature, 0.75);
        assert!(impact::weigh(0.30, nature) < impact::weigh(0.36, impact::impact("Some Journal", 0, 0.0)));
        assert!(impact::weigh(0.30, nature) > impact::weigh(0.32, 0.0));
        assert_eq!(impact::weigh(-0.1, 1.0), -0.1);
        assert_eq!(impact::weigh(0.2, 0.0), 0.2);
        assert!((impact::weigh(1.0, 1.0) - (1.0 + impact::IMPACT_WEIGHT)).abs() < 1e-12);
        assert_eq!(impact::foryou_weigh(0.3, "Nature".into(), 0), impact::weigh(0.3, 0.75));
        // attention: citations and FWCI, capped
        assert_eq!(impact::attention(0, 0.0), 0.0);
        assert!(impact::attention(50, 0.0) > 0.99 && impact::attention(5000, 0.0) <= 1.0);
        assert!(impact::attention(0, 5.0) > impact::attention(2, 0.0));
        assert!(impact::impact("Nature", 12, 0.0) > impact::impact("Nature", 0, 0.0));
        // the recorded feed: the order by closeness alone and weighted barely differ
        let hits: Vec<DiscoverHit> = works(OA_SPATIAL).into_iter().chain(works(OA_SOIL)).collect();
        let texts: Vec<String> = hits.iter().map(|h| snip(&h.meta.title, &h.meta.abstract_text)).collect();
        let interests = "I work on spatial transcriptomics: imaging-based cell segmentation and cell-type maps of tissue.";
        let close = rank::score(&texts, interests, &[], &[], &[], &[]);
        let weighted: Vec<f64> = close.iter().zip(&hits).map(|(c, h)| impact::weigh(*c, impact::impact(&h.meta.journal, h.cited_by, 0.0))).collect();
        let order = |s: &[f64]| {
            let mut i: Vec<usize> = (0..s.len()).collect();
            i.sort_by(|a, b| s[*b].partial_cmp(&s[*a]).unwrap());
            i
        };
        let (o1, o2) = (order(&close), order(&weighted));
        assert_eq!(o1[0], o2[0], "the closest stays first");
        let moved = o1.iter().enumerate().map(|(r, i)| (o2.iter().position(|x| x == i).unwrap() as i64 - r as i64).abs()).max().unwrap();
        assert!(moved <= 2, "a paper moves at most two places: {o1:?} {o2:?}");
        for x in 0..hits.len() {
            for y in 0..hits.len() {
                if close[x] > close[y] && weighted[x] < weighted[y] {
                    assert!(close[x] / close[y] < 1.0 + impact::IMPACT_WEIGHT, "swapped only within 15 %");
                }
            }
        }
    }
}
