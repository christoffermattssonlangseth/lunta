//! New for You, stage by stage, on a copy of a library (for diagnosing an empty feed by hand):
//! cargo run --example foryou_diag -- <library copy with foryou.json, watches.json, ops/> [now as ISO 8601]
//! Every OpenAlex and bioRxiv request is made and logged (count or error), then each paper the
//! filters drop with the reason, then the candidates. Nothing is written: the library is opened
//! from a further temporary copy.
use lunta_core::foryou::run::{self, Finder, NetFinder};
use lunta_core::foryou::state::*;
use lunta_core::foryou::{text, time};
use lunta_core::metadata::{DiscoverHit, WatchKind};
use lunta_core::model::*;
use lunta_core::*;
use std::collections::HashSet;

struct Job {
    kind: WatchKind,
    value: String,
    source: String,
    explicit: bool,
    limit: u32,
    not_by: Option<String>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let src = std::path::PathBuf::from(args.next().expect("library copy"));
    let now = args.next().and_then(|s| time::parse_iso8601(&s)).unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64);
    let tmp = std::env::temp_dir().join(format!("lunta-foryou-diag-{}", std::process::id()));
    let lib_dir = tmp.join("lib");
    std::fs::create_dir_all(lib_dir.join("ops")).unwrap();
    for f in ["foryou.json", "watches.json"] {
        std::fs::copy(src.join(f), lib_dir.join(f)).unwrap();
    }
    for e in std::fs::read_dir(src.join("ops")).unwrap().flatten() {
        std::fs::copy(e.path(), lib_dir.join("ops").join(e.file_name())).unwrap();
    }
    let lib = Library::open(lib_dir.to_string_lossy().into(), tmp.join("cache").to_string_lossy().into(), "diag".into()).unwrap();
    let files = Files::new(&lib_dir, tmp.join("local"));
    let state = files.state();
    let watches = files.watches();
    let every = lib.papers(PaperFilter::default(), SortKey::Added);
    println!("now {} | lastRun {:?} | perDay {} | preprints {:?} | library {} papers | shown {} | items {}",
        time::iso8601(now), state.last_run.map(time::iso8601), state.per_day, state.preprints, every.len(), state.shown.len(), state.items.len());
    let first = state.last_run.is_none();
    let since = time::utc_date(now - if first { 30 } else { 9 } * time::DAY_MS);
    println!("since {since}\nqueries {:?}\nkeywords {:?}", state.queries, state.keywords);

    let me = state.orcid.clone();
    let mut jobs: Vec<Job> = vec![];
    let job = |kind, value: &str, source: String, explicit, limit| Job { kind, value: value.to_string(), source, explicit, limit, not_by: None };
    for q in &state.queries {
        jobs.push(job(WatchKind::Topic, q, format!("search “{q}”"), false, 40));
    }
    for w in &watches {
        jobs.push(match w.kind {
            WatchType::Topic => job(WatchKind::Topic, &w.value, format!("your topic “{}”", w.label), false, 25),
            WatchType::Author => job(WatchKind::Author, &w.value, format!("New paper by {}, whom you follow.", w.label), true, 15),
            WatchType::Citing => job(WatchKind::Citing, &w.value, format!("Cites “{}”, which you watch.", w.label), true, 15),
            WatchType::Journal => job(WatchKind::Journal, &w.value, text::journal_source(&w.label), false, 30),
        });
    }
    if let Some(me) = &me {
        for p in me.citable(15) {
            jobs.push(Job { not_by: Some(me.author.clone()), ..job(WatchKind::Citing, &p.doi, text::cites_your_paper(&p.title), true, 10) });
        }
    }
    let own_keys: HashSet<String> = me.as_ref().map(|m| m.papers.iter().map(OwnPaper::key).collect()).unwrap_or_default();
    for p in every.iter().filter(|p| !p.meta.doi.is_empty() && !own_keys.contains(&text::doi_key(&p.meta.doi))).take(8) {
        let title: String = p.meta.title.chars().take(80).collect();
        jobs.push(job(WatchKind::Citing, &p.meta.doi, format!("Cites “{title}” in your library."), false, 10));
    }

    println!("\n== fetching ({} OpenAlex jobs)", jobs.len());
    let f = NetFinder;
    let mut pool: Vec<(DiscoverHit, String, bool)> = vec![];
    for j in &jobs {
        let t = std::time::Instant::now();
        let r = match &j.not_by {
            Some(a) => f.citing_new(&j.value, a, &since, j.limit),
            None => f.watch_new(j.kind, &j.value, &since, j.limit),
        };
        match r {
            Ok(h) => {
                println!("  {:>3} hits  {:>5} ms  {:?} {} | {}", h.len(), t.elapsed().as_millis(), j.kind, j.value, j.source);
                pool.extend(h.into_iter().map(|x| (x, j.source.clone(), j.explicit)));
            }
            Err(e) => println!("  ERROR     {:>5} ms  {:?} {} | {}: {e}", t.elapsed().as_millis(), j.kind, j.value, j.source),
        }
    }
    if !state.keywords.is_empty() && state.preprints != Preprints::None {
        let from = time::utc_date(now - if first { 4 } else { 2 } * time::DAY_MS);
        let to = time::utc_date(now);
        for (server, label) in [("biorxiv", "bioRxiv"), ("medrxiv", "medRxiv")] {
            match f.get_json(&format!("https://api.biorxiv.org/details/{server}/{from}/{to}/0/json")) {
                Ok(j) => println!("  {server} {from}..{to}: total {} (first page {})", j["messages"][0]["total"], j["collection"].as_array().map_or(0, |a| a.len())),
                Err(e) => println!("  {server} {from}..{to}: ERROR {e}"),
            }
            let hits = run::preprints(&f, server, &from, &to, &state.keywords).unwrap_or_else(|e| { println!("  {label}: ERROR {e}"); vec![] });
            println!("  {:>3} keyword matches from {label}", hits.len());
            pool.extend(hits.into_iter().map(|x| (x, label.to_string(), false)));
        }
    }

    println!("\n== filters (pool {})", pool.len());
    let shown_pos: std::collections::HashMap<&str, usize> = state.shown.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();
    let items: HashSet<&str> = state.items.iter().map(|i| i.key.as_str()).collect();
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let titles: HashSet<String> = every.iter().map(|p| norm(&p.meta.title)).filter(|t| t.len() >= 12).collect();
    let mut seen = HashSet::new();
    let mut reasons: std::collections::BTreeMap<String, usize> = Default::default();
    let mut cands = vec![];
    // the strongest reason first, as the run orders them
    pool.sort_by_key(|(_, s, e)| if text::cites_yours(s) { 0 } else if *e { 1 } else { 2 });
    for (h, source, explicit) in &pool {
        let key = text::hit_key(h);
        let in_lib = (!h.meta.doi.is_empty() && lib.find_by_doi(h.meta.doi.clone()).is_some()) || titles.contains(&norm(&h.meta.title));
        let why = if key.is_empty() {
            "no key".to_string()
        } else if seen.contains(&key) {
            "found twice".to_string()
        } else if in_lib {
            "in library".to_string()
        } else if let Some(p) = shown_pos.get(key.as_str()) {
            format!("already offered (shown, {} from the end)", state.shown.len() - p)
        } else if items.contains(key.as_str()) {
            "in the feed".to_string()
        } else if h.meta.title.chars().count() <= 10 {
            "short title".to_string()
        } else if own_keys.contains(&key) || me.as_ref().is_some_and(|m| m.is_mine(&h.meta.authors)) {
            "yours".to_string()
        } else if ["zenodo", "figshare", "dryad", "osf", "repository", "publikationsserver", "book series", "dissertation", "thesis"].iter().any(|x| h.meta.journal.to_lowercase().contains(x)) {
            "not a paper".to_string()
        } else if !explicit && h.meta.abstract_text.chars().count() < 300 {
            "no abstract".to_string()
        } else {
            String::new()
        };
        seen.insert(key.clone());
        if why.is_empty() {
            cands.push((h, source));
        } else {
            let bucket = if why.starts_with("already offered") { "already offered" } else { why.as_str() };
            *reasons.entry(bucket.to_string()).or_default() += 1;
            if why.starts_with("already offered") {
                println!("  dropped [{why}] {} | {} | {} | {}", h.meta.date, key, h.meta.title.chars().take(70).collect::<String>(), source);
            }
        }
    }
    println!("  dropped: {reasons:?}");
    println!("\n== candidates {}", cands.len());
    for (h, s) in &cands {
        println!("  {} | {} | {} | {}", h.meta.date, h.meta.journal, h.meta.title.chars().take(80).collect::<String>(), s);
    }

    println!("\n== the core's run (no model) on a copy");
    let report = run::run(&run::Run { lib: &lib, files: &files, finder: &f, writer: None, now, tz_min: 60, progress: &|_| {}, stop: None });
    println!("  {report:?}");
    let _ = std::fs::remove_dir_all(&tmp);
}
