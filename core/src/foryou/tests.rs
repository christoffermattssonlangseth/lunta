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

#[test]
fn preprints_from_biorxiv_by_keyword() {
    let fake = Fake::new();
    let kws = vec!["transcriptom".into(), "neurons".into(), "virus".into(), "ai".into()];
    let hits = run::preprints(&fake, "biorxiv", "2026-11-16", "2026-11-17", &kws);
    let dois: Vec<&str> = hits.iter().map(|h| h.meta.doi.as_str()).collect();
    // in the title, or two keywords in the abstract ("ai" is too short to count)
    assert_eq!(dois, vec!["10.64898/2026.11.13.766189", "10.64898/2026.11.11.765928"]);
    let h = &hits[0];
    assert_eq!(h.meta.journal, "bioRxiv");
    assert_eq!(h.meta.authors[0], "E. Stronati");
    assert_eq!(h.meta.url, "https://www.biorxiv.org/content/10.64898/2026.11.13.766189");
    assert!(run::preprints(&fake, "biorxiv", "a", "b", &["ai".into()]).is_empty());
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
