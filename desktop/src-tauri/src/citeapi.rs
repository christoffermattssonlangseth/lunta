//! Citing in Google Docs from the browser extension, on Windows and Linux: the extension endpoint
//! (extension.rs, extension requests only) answers `/cite/…` as Lunta for Mac's citation API does
//! (`apple/LuntaCore/Sources/LuntaCore/CitationAPI.swift`; the Mac's extension endpoint forwards
//! `/cite/…` there), so `extension/gdocs` works the same with either app:
//!
//!   GET  /cite/status                       → {ok, papers, api}
//!   GET  /cite/search?q=…[&limit=n]         → [item]  (item: id, title, authors, families, year, journal, doi, meta)
//!   POST /cite/bibliography {style, ids, papers?}
//!        → {entries: [item + entry]}; an id the library lacks is formatted from `papers[id]` (the
//!          details the document keeps) as {fromDocument: true}, or found again by DOI or title
//!          ({libraryId}), or else {missing: true}
//!   GET  /cite/styles?q=…                   → journal styles in the CSL catalogue [{id, title, format}]
//!   POST /cite/render {style, citations, papers?}
//!        → a journal style: {title, citations: [{text, superscript}], bibliography: [{id, text}],
//!          items: [item], missing: [id]}

use lunta_core::model::{CitationStyle, CitedPaper, Paper, PaperFilter, PaperMeta, SortKey};
use lunta_core::Library;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// The API's version, as /cite/status says it (CitationAPI.version on the Mac).
pub const VERSION: u32 = 2;

/// Answers one `/cite/…` request. `cache_dir`: where journal styles are kept.
pub(crate) fn route(lib: &Library, cache_dir: &str, method: &str, path: &str, query: &HashMap<String, String>, body: &[u8]) -> (u16, Value) {
    let q = |k: &str| query.get(k).cloned().unwrap_or_default();
    match (method, path) {
        ("GET", "/cite/status") => {
            let n = lib.papers(PaperFilter::default(), SortKey::Added).len();
            (200, json!({"ok": true, "papers": n, "api": VERSION}))
        }
        ("GET", "/cite/search") => {
            let text = q("q").trim().to_string();
            let limit = q("limit").parse::<u32>().unwrap_or(30).clamp(1, 50);
            if text.is_empty() {
                return (200, json!([]));
            }
            let mut papers: Vec<Paper> = Vec::new();
            if let Some(p) = lib.find_by_doi(text.clone()) {
                papers.push(p); // a pasted DOI
            }
            for h in lib.search(text, limit).unwrap_or_default() {
                if !papers.iter().any(|p| p.id == h.paper.id) {
                    papers.push(h.paper);
                }
            }
            (200, Value::Array(papers.iter().take(limit as usize).map(item).collect()))
        }
        ("POST", "/cite/bibliography") => {
            let Some(j) = object(body) else { return error(400, "Expected {style, ids}.") };
            let Some(ids) = strings(j.get("ids")).filter(|v| v.len() <= 10_000) else { return error(400, "Expected {style, ids}.") };
            let key = j.get("style").and_then(Value::as_str).unwrap_or("apa").trim().to_lowercase();
            let Some(style) = CitationStyle::from_key(&key) else { return error(400, &format!("Unknown citation style “{key}”.")) };
            let given = j.get("papers").and_then(Value::as_object).cloned().unwrap_or_default();
            let mut entries = Vec::new();
            for id in unique(&ids) {
                if let Some(p) = lib.paper(id.clone()) {
                    let mut d = item(&p);
                    d["entry"] = json!(lib.citation(id.clone(), style).unwrap_or_default());
                    entries.push(d);
                } else if let Some(m) = given.get(&id).and_then(Value::as_object).and_then(meta) {
                    if let Some(p) = lib.find_cited(m.doi.clone(), m.title.clone()) {
                        let mut d = item(&p);
                        d["id"] = json!(id);
                        d["libraryId"] = json!(p.id);
                        d["entry"] = json!(lib.citation(p.id.clone(), style).unwrap_or_default());
                        entries.push(d);
                    } else {
                        let mut d = item_of(&id, &m);
                        d["fromDocument"] = json!(true);
                        d["entry"] = json!(lunta_core::format_citation(m, style));
                        entries.push(d);
                    }
                } else {
                    entries.push(json!({"id": id, "missing": true, "entry": ""}));
                }
            }
            (200, json!({"entries": entries}))
        }
        ("GET", "/cite/styles") => match lunta_core::csl_search(cache_dir.to_string(), q("q"), 40) {
            Ok(hits) => (200, Value::Array(hits.into_iter().map(|h| json!({"id": h.id, "title": h.title, "format": h.format})).collect())),
            Err(e) => error(502, &format!("Couldn’t load the list of journal styles ({e}). Check the internet connection; styles used before still work.")),
        },
        ("POST", "/cite/render") => {
            let Some(j) = object(body) else { return error(400, "Expected {style, citations}.") };
            let style = j.get("style").and_then(Value::as_str).map(str::to_string);
            let cites: Option<Vec<Vec<String>>> = j.get("citations").and_then(Value::as_array).map(|a| a.iter().filter_map(|c| strings(Some(c))).collect());
            let (Some(style), Some(cites)) = (style, cites) else { return error(400, "Expected {style, citations}.") };
            let given = j.get("papers").and_then(Value::as_object).cloned().unwrap_or_default();
            let mut swap: HashMap<String, String> = HashMap::new(); // document id → the library's id
            let (mut extra, mut missing, mut items) = (Vec::new(), Vec::new(), Vec::new());
            for id in unique(&cites.iter().flatten().cloned().collect::<Vec<_>>()) {
                if let Some(p) = lib.paper(id.clone()) {
                    items.push(item(&p));
                } else if let Some(m) = given.get(&id).and_then(Value::as_object).and_then(meta) {
                    if let Some(p) = lib.find_cited(m.doi.clone(), m.title.clone()) {
                        let mut d = item(&p);
                        d["id"] = json!(id);
                        d["libraryId"] = json!(p.id);
                        swap.insert(id, p.id);
                        items.push(d);
                    } else {
                        let mut d = item_of(&id, &m);
                        d["fromDocument"] = json!(true);
                        items.push(d);
                        extra.push(CitedPaper { id, meta: m });
                    }
                } else {
                    missing.push(id);
                }
            }
            let mapped: Vec<Vec<String>> = cites.iter().map(|l| l.iter().map(|id| swap.get(id).cloned().unwrap_or_else(|| id.clone())).collect()).collect();
            let back: HashMap<&String, &String> = swap.iter().map(|(doc, lib_id)| (lib_id, doc)).collect();
            match lib.cite_csl_with(cache_dir.to_string(), style, mapped, extra) {
                Ok(out) => (
                    200,
                    json!({
                        "title": out.style_title,
                        "citations": out.citations.iter().map(|c| json!({"text": c.text, "superscript": c.superscript})).collect::<Vec<_>>(),
                        "bibliography": out.bibliography.iter().map(|e| json!({"id": back.get(&e.id).map(|s| s.as_str()).unwrap_or(&e.id), "text": e.text})).collect::<Vec<_>>(),
                        "items": items,
                        "missing": missing,
                    }),
                ),
                Err(e) => error(502, &format!("Couldn’t format in this journal style ({e}). The first time a style is used it is downloaded: check the internet connection.")),
            }
        }
        _ => error(404, "Not found."),
    }
}

fn error(status: u16, message: &str) -> (u16, Value) {
    (status, json!({"error": message}))
}

fn object(body: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => Some(m),
        _ => None,
    }
}

fn strings(v: Option<&Value>) -> Option<Vec<String>> {
    v?.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}

fn unique(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && seen.insert(s.clone())).collect()
}

fn cap(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The first 20 authors and the last: all any style shows (a consortium paper has thousands).
fn short_list(a: &[String]) -> Vec<String> {
    if a.len() > 21 {
        a[..20].iter().chain(a.last()).cloned().collect()
    } else {
        a.to_vec()
    }
}

/// What the extension gets for a paper. `meta` is what a document keeps of the papers it cites.
fn item(p: &Paper) -> Value {
    item_of(&p.id, &p.meta)
}

fn item_of(id: &str, m: &PaperMeta) -> Value {
    let authors = short_list(&m.authors);
    let families: Vec<String> = authors.iter().map(|a| lunta_core::family_name(a.clone())).collect();
    json!({"id": id, "title": m.title, "authors": authors, "families": families, "year": m.year(),
           "journal": m.journal, "doi": m.doi, "meta": meta_json(m)})
}

fn meta_json(m: &PaperMeta) -> Value {
    let mut d = Map::new();
    d.insert("title".into(), json!(cap(&m.title, 2000)));
    d.insert("authors".into(), json!(short_list(&m.authors).iter().map(|a| cap(a, 2000)).collect::<Vec<_>>()));
    for (k, v) in [("date", &m.date), ("journal", &m.journal), ("volume", &m.volume), ("issue", &m.issue), ("pages", &m.pages),
                   ("publisher", &m.publisher), ("doi", &m.doi), ("pmid", &m.pmid), ("pmcid", &m.pmcid), ("arxiv", &m.arxiv), ("url", &m.url)] {
        if !v.is_empty() {
            d.insert(k.into(), json!(cap(v, 2000)));
        }
    }
    Value::Object(d)
}

/// A paper's details as a document sent them (None without a title or authors to cite by).
fn meta(d: &Map<String, Value>) -> Option<PaperMeta> {
    let s = |k: &str| match d.get(k) {
        Some(Value::String(v)) => cap(v, 2000),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let authors: Vec<String> = d.get("authors").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).take(200).map(|x| cap(x, 500)).collect()).unwrap_or_default();
    let date = if s("date").is_empty() { s("year") } else { s("date") };
    let m = PaperMeta {
        title: s("title"), authors, date, journal: s("journal"), volume: s("volume"), issue: s("issue"), pages: s("pages"),
        publisher: s("publisher"), doi: s("doi"), pmid: s("pmid"), pmcid: s("pmcid"), arxiv: s("arxiv"), url: s("url"),
        ..Default::default()
    };
    if m.title.is_empty() && m.authors.is_empty() {
        None
    } else {
        Some(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (std::sync::Arc<Library>, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("lunta-cite-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let lib = Library::open(d.join("lib").to_string_lossy().into(), d.join("cache").to_string_lossy().into(), "test".into()).unwrap();
        (lib, d)
    }

    #[test]
    fn answers_as_the_mac_does() {
        let (lib, dir) = lib();
        let meta = PaperMeta { title: "Alpha cells in the islet".into(), authors: vec!["Ada Lee".into(), "Per Wu".into()], date: "2020-03".into(),
                               journal: "Cell".into(), doi: "10.1234/alpha".into(), ..Default::default() };
        let p = lib.add_paper(meta, None, None, None).unwrap();
        let cache = dir.join("csl").to_string_lossy().to_string();
        let none = HashMap::new();
        let call = |m: &str, path: &str, q: &HashMap<String, String>, body: Value| route(&lib, &cache, m, path, q, &serde_json::to_vec(&body).unwrap());

        let (st, j) = call("GET", "/cite/status", &none, json!(null));
        assert_eq!((st, j["papers"].clone(), j["api"].clone()), (200, json!(1), json!(VERSION)));
        // a pasted DOI finds its paper
        let (st, j) = call("GET", "/cite/search", &HashMap::from([("q".to_string(), "10.1234/alpha".to_string())]), json!(null));
        assert_eq!(st, 200);
        assert_eq!(j[0]["id"], json!(p.id));
        assert_eq!(j[0]["families"], json!(["Lee", "Wu"]));
        assert_eq!(j[0]["year"], json!("2020"));
        assert_eq!(j[0]["meta"]["journal"], json!("Cell"));
        // the library's paper; one only the document knows; one with nothing to go on; the
        // document's copy of a paper the library has under another id (found by DOI)
        let body = json!({"style": "apa", "ids": [p.id, "doc-only", "gone", "merged", p.id],
            "papers": {"doc-only": {"title": "Beta waves", "authors": ["Kim Ng"], "year": 2021, "journal": "Neuron"},
                       "merged": {"title": "whatever", "doi": "10.1234/alpha"}}});
        let (st, j) = call("POST", "/cite/bibliography", &none, body);
        assert_eq!(st, 200, "{j}");
        let e = j["entries"].as_array().unwrap();
        assert_eq!(e.len(), 4);
        assert!(e[0]["entry"].as_str().unwrap().contains("Alpha cells"), "{j}");
        assert_eq!(e[1]["fromDocument"], json!(true));
        assert!(e[1]["entry"].as_str().unwrap().contains("Beta waves"), "{j}");
        assert_eq!(e[1]["year"], json!("2021"));
        assert_eq!(e[2]["missing"], json!(true));
        assert_eq!((e[3]["id"].clone(), e[3]["libraryId"].clone()), (json!("merged"), json!(p.id)));
        // errors that say what's wrong
        assert_eq!(call("POST", "/cite/bibliography", &none, json!({"style": "nope", "ids": []})).0, 400);
        assert_eq!(call("POST", "/cite/render", &none, json!({"style": 1})).0, 400);
        assert_eq!(call("GET", "/cite/elsewhere", &none, json!(null)).0, 404);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
