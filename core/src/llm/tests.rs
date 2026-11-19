use super::ask::*;
use super::download::*;
use super::models::AiModel;
use super::prompt::*;
use super::summary::{self, PaperSummary};
use super::AiError;
use crate::model::{Paper, PaperMeta};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// ---- the Summary card's rules (SummaryWriter.swift) ----

#[test]
fn summary_prompt_has_title_source_and_ask() {
    assert_eq!(summary_prompt("  A title ", "Abstract:\nWe did."), "Title: A title\n\nAbstract:\nWe did.\n\nSummarize this paper in 3 or 4 sentences.");
    assert_eq!(summary_prompt("", "S"), "S\n\nSummarize this paper in 3 or 4 sentences.");
}

#[test]
fn source_hash_is_the_apple_apps() {
    // SHA256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e…; Swift keeps the first 12 bytes
    assert_eq!(source_hash("hello"), "2cf24dba5fb0a30e26e83b2a");
    assert_eq!(source_hash("hello").len(), 24);
}

#[test]
fn clean_takes_off_thinking_markdown_labels_and_quotes() {
    assert_eq!(clean_summary("<think>\nhmm\n</think>\n\n**Summary:** Mice   grew.\n"), "Mice grew.");
    assert_eq!(clean_summary("<think>still thinking"), "");
    assert_eq!(clean_summary("## TL;DR — Things grew."), "Things grew.");
    assert_eq!(clean_summary("- One finding.\n- Two findings.\n1. Three."), "One finding. Two findings. Three.");
    assert_eq!(clean_summary("“Quoted summary.”"), "Quoted summary.");
    assert_eq!(clean_summary("__Bold__ text"), "Bold text");
    // a label only at the very start
    assert_eq!(clean_summary("Growth was 40%. Summary: fine."), "Growth was 40%. Summary: fine.");
}

#[test]
fn acceptable_wants_a_real_summary() {
    let good = "Treated mice grew 40% more than controls over ten weeks, and the effect held in both sexes and at every dose tested.";
    assert!(summary_acceptable(good));
    assert!(!summary_acceptable("Too short."));
    assert!(!summary_acceptable("I'm sorry, but I cannot summarize this paper because it discusses a sensitive topic that I won't cover."));
    assert!(!summary_acceptable("Well, as an AI I am unable to read papers properly, so here is nothing much to say about this at all."));
}

#[test]
fn chatml_is_qwen3s_template_without_thinking() {
    let p = chatml("Be brief.", &[ChatTurn { role: "user".into(), text: "Hi".into() }], false);
    assert_eq!(p, "<|im_start|>system\nBe brief.<|im_end|>\n<|im_start|>user\nHi<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n");
    let p = chatml("", &[
        ChatTurn { role: "user".into(), text: "Q1".into() },
        ChatTurn { role: "assistant".into(), text: "<think>x</think>\n\nA1".into() },
        ChatTurn { role: "user".into(), text: "Q2".into() },
    ], true);
    assert_eq!(p, "<|im_start|>user\nQ1<|im_end|>\n<|im_start|>assistant\nA1<|im_end|>\n<|im_start|>user\nQ2<|im_end|>\n<|im_start|>assistant\n");
}

#[test]
fn digitize_spelled_out_numbers() {
    assert_eq!(digitize("One hundred eighteen subjects were included"), "118 subjects were included");
    assert_eq!(digitize("twenty-five mice"), "25 mice");
    assert_eq!(digitize("one and two"), "one and two");
    assert_eq!(digitize("three thousand two hundred"), "3200");
}

// ---- Ask ----

fn paper(id: &str, title: &str, abs: &str) -> Paper {
    Paper {
        id: id.into(),
        meta: PaperMeta { title: title.into(), authors: vec!["Smith, J".into(), "Doe, A".into()], date: "2021-02".into(), journal: "J Things".into(), abstract_text: abs.into(), ..Default::default() },
        added: 0, modified: 0, has_pdf: false, has_epub: true, has_audio: false, read: false, starred: true, must_read: false,
        collections: vec![], tags: vec!["growth".into()], annotation_count: 0, cite_key: String::new(),
    }
}

const TEXT: &str = "Introduction\n\
We studied how things grow in mice, a question that has interested people for many years and still does today.\n\
Methods\n\
One hundred eighteen patients were enrolled at three hospitals between 2015 and 2019, and all of them gave consent.\n\
Statistical analysis used mixed models with a random intercept for each hospital, and p < 0.05 counted as significant.\n\
Results\n\
Treated mice grew 40% more than controls, and the difference held at every dose that we tested in the study.\n";

#[test]
fn passages_join_headings_and_rank_by_the_question() {
    let ps = passages_of(TEXT);
    assert_eq!(ps.len(), 4);
    assert!(ps[1].starts_with("Methods — One hundred"));
    // "patients" found from "patient", "enrolled" from "enrol"
    assert_eq!(rank(&ps, "how many patients were enrolled?", 2)[0], 1);
    assert_eq!(rank(&ps, "statistical analysis", 1), vec![2]);
    assert!(rank(&ps, "what is this about", 3).is_empty());
}

#[test]
fn paper_prompt_numbers_passages_digitizes_and_keeps_history_short() {
    let p = paper("p1", "Things grow", "We found growth.");
    let prev = vec![ChatTurn { role: "user".into(), text: "earlier".into() }, ChatTurn { role: "assistant".into(), text: "x".repeat(2000) }];
    let a = paper_prompt(&p, TEXT, &[("a quote".into(), "my note".into())], "How many patients?", &prev);
    assert!(a.system.contains("“Things grow” (Smith et al. 2021, J Things)"));
    assert!(a.system.contains("Abstract: We found growth."));
    assert!(a.system.contains("- “a quote” — note: my note"));
    assert_eq!(a.sources[0].n, 1);
    assert!(a.sources[0].text.contains("One hundred eighteen"));
    let last = &a.turns.last().unwrap().text;
    assert!(last.contains("[1] Methods — 118 patients"), "{last}");
    assert!(last.ends_with("Question: How many patients?"));
    assert_eq!(a.turns.len(), 3);
    assert!(a.turns[1].text.chars().count() <= 701);
    // nothing matches: the start of the paper
    let b = paper_prompt(&p, TEXT, &[], "summarize", &[]);
    assert_eq!(b.sources.len(), 4);
}

#[test]
fn library_prompt_and_search_fusion() {
    let papers = vec![paper("a", "Growth of things", "Things grow."), paper("b", "Mice and men", "About mice."), paper("c", "Unrelated", "")];
    let search = |q: &str, _n: u32| -> Vec<(Paper, String)> {
        let q = q.to_lowercase();
        papers.iter().filter(|p| q.split(' ').all(|w| format!("{} {}", p.meta.title, p.meta.abstract_text).to_lowercase().contains(w))).map(|p| (p.clone(), String::new())).collect()
    };
    // no paper has both words: each word on its own
    let found = search_for(&search, "Which papers are about growth in mice?", 8);
    let ids: Vec<&str> = found.iter().map(|(p, _)| p.id.as_str()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"a") && ids.contains(&"b"));
    let facts = LibraryFacts { all: 3, unread: 3, starred: 3, tags: vec!["growth".into()], collections: vec![] };
    let a = library_prompt(&facts, &found, "Which papers are about growth in mice?", &[]);
    assert!(a.system.contains("The library has 3 papers (3 unread, 3 starred). Tags: growth. Collections: none."));
    assert!(a.turns[0].text.contains("[1] "));
    assert!(a.turns[0].text.contains("(unread; starred; tags: growth)"));
    assert_eq!(a.sources.len(), 2);
    assert!(library_prompt(&facts, &[], "x", &[]).turns[0].text.contains("(the search found no papers)"));
}

#[test]
fn answers_lose_citations_that_point_nowhere() {
    assert_eq!(clean_answer("<think>a</think>\n\nIt grew [1][7], see [2].", 2), "It grew [1], see [2].");
}

// ---- summary.json ----

#[test]
fn summary_json_reads_what_swift_writes_and_writes_what_swift_reads() {
    let dir = tempfile::tempdir().unwrap();
    // as JSONEncoder (.prettyPrinted, .sortedKeys, .iso8601) writes it on the Mac
    std::fs::write(dir.path().join("summary.json"), "{\n  \"created\" : \"2026-11-19T09:30:00Z\",\n  \"model\" : \"apple-on-device\",\n  \"source\" : \"0123456789abcdef01234567\",\n  \"text\" : \"Mice \\/ rats grew.\"\n}").unwrap();
    let s = summary::read(dir.path()).unwrap();
    assert_eq!(s.text, "Mice / rats grew.");
    assert_eq!(s.model, "apple-on-device");
    assert_eq!(summary::iso8601(s.created), "2026-11-19T09:30:00Z");
    let mine = PaperSummary { text: "A “quoted” line.\nTwo".into(), model: "qwen3-4b".into(), created: 1_763_544_600_123, source: "abc".into() };
    summary::write(dir.path(), &mine).unwrap();
    let raw = std::fs::read_to_string(dir.path().join("summary.json")).unwrap();
    // whole seconds, as Swift's .iso8601 decoding wants; keys sorted
    assert!(raw.starts_with("{\n  \"created\" : \"2025-11-19T09:30:00Z\",\n  \"model\" : \"qwen3-4b\""), "{raw}");
    let back = summary::read(dir.path()).unwrap();
    assert_eq!(back.text, mine.text);
    assert_eq!(back.created, 1_763_544_600_000);
    summary::remove(dir.path()).unwrap();
    summary::remove(dir.path()).unwrap();
    assert!(summary::read(dir.path()).is_none());
}

#[test]
fn iso8601_both_ways() {
    assert_eq!(summary::iso8601(0), "1970-01-01T00:00:00Z");
    assert_eq!(summary::parse_iso8601("2000-02-29T23:59:59Z"), Some(951_868_799_000));
    assert_eq!(summary::parse_iso8601("2000-03-01T00:59:59+01:00"), Some(951_868_799_000));
    assert_eq!(summary::parse_iso8601("2000-02-29T23:59:59.5Z"), Some(951_868_799_500));
    assert_eq!(summary::parse_iso8601("yesterday"), None);
}

#[test]
fn library_keeps_summaries_in_the_paper_folder() {
    let dir = tempfile::tempdir().unwrap();
    let lib = crate::Library::open(dir.path().join("lib").to_string_lossy().into(), dir.path().join("cache").to_string_lossy().into(), "test".into()).unwrap();
    let p = lib.add_paper(PaperMeta { title: "T".into(), ..Default::default() }, None, None, Some("Some text of the paper that is long enough to be a passage of its own here.".into())).unwrap();
    assert!(lib.summary(p.id.clone()).is_none());
    let s = PaperSummary { text: "x".into(), model: "qwen3-1.7b".into(), created: 1000, source: "h".into() };
    lib.save_summary(p.id.clone(), s.clone()).unwrap();
    assert_eq!(lib.summary(p.id.clone()), Some(s));
    assert!(dir.path().join("lib/papers").join(&p.id).join("summary.json").exists());
    let a = lib.ask_paper_prompt(p.id.clone(), "passage text".into(), vec![], None).unwrap();
    assert_eq!(a.sources.len(), 1);
    lib.remove_summary(p.id.clone()).unwrap();
    assert!(lib.summary(p.id).is_none());
}

// ---- downloading ----

fn model_of(data: &[u8]) -> AiModel {
    AiModel {
        id: "test".into(),
        name: "Test".into(),
        file_name: "test.gguf".into(),
        url: "https://example.org/test.gguf".into(),
        size: data.len() as u64,
        sha256: Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect(),
        context: 2048,
    }
}

/// A server for one file: honours Range (unless `no_range`), and can drop the connection after
/// `drop_after` bytes of a response (the first `drops` responses).
struct Mock {
    data: Vec<u8>,
    no_range: bool,
    drop_after: usize,
    drops: AtomicUsize,
    calls: Mutex<Vec<u64>>,
}

impl Mock {
    fn new(data: Vec<u8>) -> Mock {
        Mock { data, no_range: false, drop_after: usize::MAX, drops: AtomicUsize::new(0), calls: Mutex::new(vec![]) }
    }
}

struct Dropping {
    inner: std::io::Cursor<Vec<u8>>,
    left: usize,
}

impl Read for Dropping {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset"));
        }
        let n = buf.len().min(self.left).min(1000);
        let n = self.inner.read(&mut buf[..n])?;
        self.left -= n;
        Ok(n)
    }
}

impl Fetch for Mock {
    fn get(&self, _url: &str, from: u64) -> Result<Fetched, AiError> {
        self.calls.lock().unwrap().push(from);
        let from = if self.no_range { 0 } else { from as usize };
        if from >= self.data.len() && from > 0 {
            return Ok(Fetched { status: 416, body: Box::new(std::io::empty()) });
        }
        let body = self.data[from..].to_vec();
        let status = if from > 0 { 206 } else { 200 };
        if self.drops.load(Ordering::SeqCst) > 0 {
            self.drops.fetch_sub(1, Ordering::SeqCst);
            return Ok(Fetched { status, body: Box::new(Dropping { inner: std::io::Cursor::new(body), left: self.drop_after }) });
        }
        Ok(Fetched { status, body: Box::new(std::io::Cursor::new(body)) })
    }
}

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

#[test]
fn download_verifies_and_names_the_file_only_when_complete() {
    let dir = tempfile::tempdir().unwrap();
    let d = data(700_000);
    let m = model_of(&d);
    assert!(installed(&m, dir.path()).is_none());
    let mut seen = vec![];
    let p = download(&m, dir.path(), &Mock::new(d.clone()), &mut |a, b| { seen.push((a, b)); true }).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), d);
    assert_eq!(installed(&m, dir.path()), Some(p));
    assert_eq!(partial_bytes(&m, dir.path()), 0);
    assert_eq!(seen.first(), Some(&(0, 700_000)));
    assert_eq!(seen.last(), Some(&(700_000, 700_000)));
    // there already: nothing fetched
    let again = Mock::new(d.clone());
    download(&m, dir.path(), &again, &mut |_, _| true).unwrap();
    assert!(again.calls.lock().unwrap().is_empty());
    remove(&m, dir.path()).unwrap();
    assert!(installed(&m, dir.path()).is_none());
}

#[test]
fn download_cancelled_then_resumed_with_a_range_request() {
    let dir = tempfile::tempdir().unwrap();
    let d = data(3_000_000);
    let m = model_of(&d);
    let server = Mock::new(d.clone());
    let r = download(&m, dir.path(), &server, &mut |done, _| done < 1_000_000);
    assert!(matches!(r, Err(AiError::Cancelled)));
    let part = partial_bytes(&m, dir.path());
    assert!(part >= 1_000_000 && part < 3_000_000, "{part}");
    assert!(installed(&m, dir.path()).is_none());
    let p = download(&m, dir.path(), &server, &mut |_, _| true).unwrap();
    assert_eq!(std::fs::read(p).unwrap(), d);
    assert_eq!(*server.calls.lock().unwrap(), vec![0, part]);
}

#[test]
fn download_survives_dropped_connections() {
    let dir = tempfile::tempdir().unwrap();
    let d = data(50_000);
    let m = model_of(&d);
    let mut server = Mock::new(d.clone());
    server.drop_after = 20_000;
    server.drops = AtomicUsize::new(2);
    let p = download(&m, dir.path(), &server, &mut |_, _| true).unwrap();
    assert_eq!(std::fs::read(p).unwrap(), d);
    assert_eq!(*server.calls.lock().unwrap(), vec![0, 20_000, 40_000]);
}

#[test]
fn download_starts_over_when_the_server_ignores_range() {
    let dir = tempfile::tempdir().unwrap();
    let d = data(400_000);
    let m = model_of(&d);
    std::fs::write(dir.path().join("test.gguf.part"), &d[..1000]).unwrap();
    let mut server = Mock::new(d.clone());
    server.no_range = true;
    let p = download(&m, dir.path(), &server, &mut |_, _| true).unwrap();
    assert_eq!(std::fs::read(p).unwrap(), d);
}

#[test]
fn download_with_a_wrong_checksum_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let d = data(10_000);
    let mut m = model_of(&d);
    m.sha256 = "00".repeat(32);
    let r = download(&m, dir.path(), &Mock::new(d), &mut |_, _| true);
    assert!(matches!(r, Err(AiError::Checksum(_))), "{r:?}");
    assert_eq!(partial_bytes(&m, dir.path()), 0);
    assert!(installed(&m, dir.path()).is_none());
}

#[test]
fn a_part_that_is_already_whole_is_checked_without_downloading_again() {
    let dir = tempfile::tempdir().unwrap();
    let d = data(10_000);
    let m = model_of(&d);
    std::fs::write(dir.path().join("test.gguf.part"), &d).unwrap();
    let server = Mock::new(d.clone());
    let p = download(&m, dir.path(), &server, &mut |_, _| true).unwrap();
    assert_eq!(std::fs::read(p).unwrap(), d);
    assert!(server.calls.lock().unwrap().is_empty());
}

#[test]
fn the_catalogue_is_pinned() {
    for m in [super::models::desktop_model(), super::models::phone_model()] {
        assert!(m.url.starts_with("https://huggingface.co/"));
        assert!(m.url.contains("/resolve/") && !m.url.contains("/main/"), "pinned to a revision: {}", m.url);
        assert!(m.url.ends_with(&m.file_name));
        assert_eq!(m.sha256.len(), 64);
        assert!(m.size > 1_000_000_000);
    }
    assert_eq!(super::models::size_label(2_497_280_256), "2.5 GB");
}

// ---- the real model (needs a GGUF file) ----

/// LUNTA_TEST_GGUF=<a small Qwen3 GGUF, e.g. Qwen3-0.6B-Q8_0.gguf> cargo test --features llm llm::tests::real_model -- --ignored --nocapture
#[test]
#[ignore]
fn real_model_writes_a_summary_and_an_answer() {
    let Some(path) = std::env::var_os("LUNTA_TEST_GGUF") else {
        eprintln!("LUNTA_TEST_GGUF not set: skipped");
        return;
    };
    let e = super::AiEngine::load_model(std::path::Path::new(&path), "test", 4096, None).unwrap();
    assert!(e.engine.uses_chatml());
    let source = "Abstract:\nWe treated 118 mice with a growth factor for ten weeks and compared them with 120 untreated controls. Treated mice grew 40% more (p < 0.001), in both sexes and at every dose. The effect was gone four weeks after treatment stopped.\n\nFrom the discussion:\nThe growth factor speeds growth only while it is given; its effect does not last.";
    let started = std::time::Instant::now();
    let mut partials = 0;
    let s = e.write_summary("A growth factor in mice", source, &mut |_| { partials += 1; true }).unwrap();
    eprintln!("summary in {:?} ({partials} updates): {s:?}", started.elapsed());
    assert!(partials > 3);
    let s = s.expect("a summary");
    assert!(!s.contains("<think>"));
    let p = paper("x", "A growth factor in mice", source);
    let ask = paper_prompt(&p, &source.replace("\n\n", "\n"), &[], "How many mice were treated?", &[]);
    let a = e.write_answer(&ask, &mut |_| true).unwrap();
    eprintln!("answer: {a}");
    assert!(a.contains("118"), "{a}");
    // cancelling
    let r = e.write_summary("T", source, &mut |_| false);
    assert!(matches!(r, Ok(_)));
}
