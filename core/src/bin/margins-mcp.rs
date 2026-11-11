//! Margins for agents: an MCP server (stdio) and a small command-line tool over the same library
//! the apps use.
//!
//!   margins-mcp                     run as an MCP server (Claude Code, Claude Desktop, Cursor, …)
//!   margins-mcp search <query>      search the library
//!   margins-mcp show <id>           details of a paper
//!   margins-mcp read <id> [query]   full text (or the passages matching a query)
//!   margins-mcp cite <id>… [--style apa|vancouver|nature|bibtex|ris]
//!   margins-mcp path <id> [pdf|epub|text]
//!
//! The agent writes through its own change log (ops/agent-….jsonl), exactly like another device,
//! so it can work while the Mac and iPhone apps are open, and its changes sync everywhere.

use margins_core::model::*;
use margins_core::{export, Library};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::result::Result;

const PROTOCOL: &str = "2025-06-18";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
}

fn library_root() -> PathBuf {
    if let Ok(p) = std::env::var("MARGINS_LIBRARY") {
        return PathBuf::from(p);
    }
    let cloud = home().join("Library/Mobile Documents/iCloud~com~cmlangseth~margins/Documents/Library");
    if cloud.exists() {
        return cloud;
    }
    home().join("Library/Application Support/Margins/Library")
}

fn open() -> Result<Arc<Library>, String> {
    let root = library_root();
    if !root.exists() {
        return Err(format!("No Margins library at {} (open the Margins app once first)", root.display()));
    }
    let cache = home().join("Library/Caches/Margins/agent");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    // one stable id per machine, so the agent's changes form one log
    let idf = cache.join("device-id");
    let device = match std::fs::read_to_string(&idf) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            let id = format!("agent-{}", &uuid_like()[..8]);
            let _ = std::fs::write(&idf, &id);
            id
        }
    };
    Library::open(root.display().to_string(), cache.display().to_string(), device).map_err(|e| e.to_string())
}

fn uuid_like() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{:x}{:x}", t, std::process::id())
}

// ---------------------------------------------------------------- helpers

fn summary(p: &Paper) -> Value {
    json!({
        "id": p.id,
        "title": p.meta.title,
        "authors": short_authors(&p.meta.authors),
        "year": p.meta.year(),
        "journal": p.meta.journal,
        "doi": p.meta.doi,
        "read": p.read,
        "starred": p.starred,
        "tags": p.tags,
        "has_pdf": p.has_pdf,
        "has_full_text": p.has_epub,
    })
}

fn short_authors(a: &[String]) -> String {
    match a.len() {
        0 => String::new(),
        1 | 2 => a.join(", "),
        _ => format!("{} et al.", a[0]),
    }
}

fn details(lib: &Library, p: &Paper) -> Value {
    let cols: HashMap<String, String> = lib.collections().into_iter().map(|c| (c.id, c.name)).collect();
    let file = |k: FileKind| lib.file_path(p.id.clone(), k);
    let notes: Vec<Value> = lib
        .annotations(p.id.clone())
        .into_iter()
        .map(|a| json!({"id": a.id, "kind": format!("{:?}", a.kind).to_lowercase(), "quote": a.quote, "note": a.note}))
        .collect();
    json!({
        "id": p.id,
        "title": p.meta.title,
        "authors": p.meta.authors,
        "date": p.meta.date,
        "journal": p.meta.journal,
        "volume": p.meta.volume, "issue": p.meta.issue, "pages": p.meta.pages,
        "doi": p.meta.doi, "pmid": p.meta.pmid, "pmcid": p.meta.pmcid, "arxiv": p.meta.arxiv, "url": p.meta.url,
        "abstract": p.meta.abstract_text,
        "keywords": p.meta.keywords,
        "tags": p.tags,
        "collections": p.collections.iter().filter_map(|c| cols.get(c).cloned()).collect::<Vec<_>>(),
        "read": p.read, "starred": p.starred,
        "annotations": notes,
        "files": {
            "pdf": file(FileKind::Pdf),
            "reading_view_epub": file(FileKind::Epub),
            "full_text": file(FileKind::Fulltext),
            "narration_audio": file(FileKind::Audio),
        },
        "open_in_app": format!("margins://open?id={}", p.id),
        "doi_link": if p.meta.doi.is_empty() { Value::Null } else { json!(format!("https://doi.org/{}", p.meta.doi)) },
    })
}

fn full_text(lib: &Library, id: &str) -> Option<String> {
    let path = lib.file_path(id.to_string(), FileKind::Fulltext)?;
    std::fs::read_to_string(path).ok()
}

fn terms(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.len() > 2)
        .map(String::from)
        .collect()
}

/// Paragraphs of the full text that best match a query (simple BM25).
fn passages(text: &str, query: &str, top: usize) -> Vec<(usize, String)> {
    // one paragraph per line; short lines (headings) join the paragraph after them; long ones are split
    let mut chunks: Vec<String> = vec![];
    let mut pending = String::new();
    for line in text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if line.len() < 80 {
            pending.push_str(line);
            pending.push_str(" — ");
            continue;
        }
        let mut cur = std::mem::take(&mut pending);
        for sentence in line.split_inclusive(". ") {
            if cur.len() + sentence.len() > 1400 && cur.len() > 300 {
                chunks.push(std::mem::take(&mut cur));
            }
            cur.push_str(sentence);
        }
        chunks.push(cur);
    }
    let paras: Vec<&str> = chunks.iter().map(|c| c.as_str()).filter(|p| p.len() > 40).collect();
    let q: Vec<String> = terms(query);
    if q.is_empty() {
        return vec![];
    }
    let docs: Vec<Vec<String>> = paras.iter().map(|p| terms(p)).collect();
    let n = docs.len() as f64;
    let avg = docs.iter().map(|d| d.len()).sum::<usize>() as f64 / n.max(1.0);
    let mut df: HashMap<&str, usize> = HashMap::new();
    for t in &q {
        df.insert(t, docs.iter().filter(|d| d.iter().any(|w| w == t)).count());
    }
    let mut scored: Vec<(usize, f64)> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let mut s = 0.0;
            for t in &q {
                let f = d.iter().filter(|w| *w == t).count() as f64;
                if f == 0.0 {
                    continue;
                }
                let dfi = *df.get(t.as_str()).unwrap_or(&0) as f64;
                let idf = (1.0 + (n - dfi + 0.5) / (dfi + 0.5)).ln();
                s += idf * f * 2.2 / (f + 1.2 * (0.25 + 0.75 * d.len() as f64 / avg.max(1.0)));
            }
            (i, s)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.into_iter().take(top).map(|(i, _)| (i + 1, paras[i].to_string())).collect()
}

fn style(s: &str) -> Option<CitationStyle> {
    match s.to_lowercase().as_str() {
        "apa" => Some(CitationStyle::Apa),
        "vancouver" => Some(CitationStyle::Vancouver),
        "nature" => Some(CitationStyle::Nature),
        _ => None,
    }
}

fn cite(lib: &Library, ids: &[String], st: &str) -> Result<String, String> {
    let papers: Vec<Paper> = ids.iter().filter_map(|i| lib.paper(i.clone())).collect();
    if papers.is_empty() {
        return Err("no such papers".into());
    }
    Ok(match st.to_lowercase().as_str() {
        "bibtex" | "bib" => export::bibtex(&papers),
        "ris" => export::ris(&papers),
        s => {
            let st = style(s).ok_or(format!("unknown style {s}: use apa, vancouver, nature, bibtex or ris"))?;
            papers.iter().map(|p| export::citation(p, st)).collect::<Vec<_>>().join("\n")
        }
    })
}

fn collection_id(lib: &Library, name: &str, create: bool) -> Result<String, String> {
    if let Some(c) = lib.collections().into_iter().find(|c| c.name.eq_ignore_ascii_case(name) || c.id == name) {
        return Ok(c.id);
    }
    if !create {
        return Err(format!("no collection called {name}"));
    }
    lib.create_collection(name.to_string(), None).map(|c| c.id).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- tools

fn tools() -> Value {
    fn t(name: &str, desc: &str, props: Value, required: &[&str], read_only: bool) -> Value {
        json!({
            "name": name,
            "description": desc,
            "inputSchema": {"type": "object", "properties": props, "required": required},
            "annotations": {"readOnlyHint": read_only, "openWorldHint": name == "find_new_papers" || name == "add_paper" || name == "find_pdf"},
        })
    }
    let ids = json!({"type": "array", "items": {"type": "string"}, "description": "Paper ids"});
    json!([
        t("search_papers", "Search the user's Margins library (titles, authors, abstracts, full text, notes). Returns paper summaries with ids.",
          json!({"query": {"type": "string"}, "limit": {"type": "integer", "default": 10}}), &["query"], true),
        t("list_papers", "List papers in the library, optionally filtered (unread, starred, tag, collection) and sorted.",
          json!({"unread": {"type": "boolean"}, "starred": {"type": "boolean"}, "tag": {"type": "string"},
                 "collection": {"type": "string", "description": "Collection name"},
                 "sort": {"type": "string", "enum": ["added", "year", "title", "author"], "default": "added"},
                 "limit": {"type": "integer", "default": 25}, "offset": {"type": "integer", "default": 0}}), &[], true),
        t("get_paper", "Full details of a paper: metadata, abstract, tags, collections, notes and highlights, and local file paths (PDF, EPUB reading view, full text).",
          json!({"id": {"type": "string"}}), &["id"], true),
        t("read_paper", "Read a paper's full text. With `query`, returns only the best-matching paragraphs; otherwise pages through the text with offset/max_chars.",
          json!({"id": {"type": "string"}, "query": {"type": "string"}, "offset": {"type": "integer", "default": 0},
                 "max_chars": {"type": "integer", "default": 12000}}), &["id"], true),
        t("cite", "Formatted citations or a bibliography for papers.",
          json!({"ids": ids, "style": {"type": "string", "enum": ["apa", "vancouver", "nature", "bibtex", "ris"], "default": "apa"}}), &["ids"], true),
        t("library_overview", "Size of the library, its tags and collections.", json!({}), &[], true),
        t("find_new_papers", "Search published literature beyond the library (OpenAlex). Marks results already in the library.",
          json!({"query": {"type": "string"}, "limit": {"type": "integer", "default": 10}}), &["query"], true),
        t("add_paper", "Add a paper to the library by DOI, PubMed ID, PMCID, arXiv id or URL. Fetches its metadata and, when legally available, its open-access PDF. The Margins app makes the reading view.",
          json!({"identifier": {"type": "string"}, "fetch_pdf": {"type": "boolean", "default": true}}), &["identifier"], false),
        t("find_pdf", "Find and attach a legal open-access PDF for a paper in the library.", json!({"id": {"type": "string"}}), &["id"], false),
        t("tag_papers", "Add (or remove) a tag on papers.",
          json!({"ids": ids, "tag": {"type": "string"}, "remove": {"type": "boolean", "default": false}}), &["ids", "tag"], false),
        t("add_to_collection", "Put papers into a collection (created if missing), or take them out.",
          json!({"ids": ids, "collection": {"type": "string"}, "remove": {"type": "boolean", "default": false}}), &["ids", "collection"], false),
        t("set_status", "Mark papers read/unread and/or starred/unstarred.",
          json!({"ids": ids, "read": {"type": "boolean"}, "starred": {"type": "boolean"}}), &["ids"], false),
        t("add_note", "Attach a note to a paper (shown in Margins on Mac and iPhone). Optionally quote the passage it refers to.",
          json!({"id": {"type": "string"}, "note": {"type": "string"}, "quote": {"type": "string"}}), &["id", "note"], false),
        t("update_paper", "Correct a paper's details. Only the fields given are changed.",
          json!({"id": {"type": "string"}, "title": {"type": "string"}, "authors": {"type": "array", "items": {"type": "string"}},
                 "date": {"type": "string"}, "journal": {"type": "string"}, "doi": {"type": "string"}, "abstract": {"type": "string"},
                 "keywords": {"type": "array", "items": {"type": "string"}}}), &["id"], false),
    ])
}

fn arg_str(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(|v| v.as_str()).map(|s| s.to_string()).filter(|s| !s.trim().is_empty())
}
fn arg_ids(a: &Value) -> Vec<String> {
    a.get("ids").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}
fn arg_u(a: &Value, k: &str, d: u64) -> u64 {
    a.get(k).and_then(|v| v.as_u64()).unwrap_or(d)
}

fn call(lib: &Library, name: &str, a: &Value) -> Result<Value, String> {
    let need = |id: &str| lib.paper(id.to_string()).ok_or(format!("no paper with id {id}"));
    match name {
        "search_papers" => {
            let q = arg_str(a, "query").ok_or("query is required")?;
            let hits = lib.search(q, arg_u(a, "limit", 10).min(50) as u32).map_err(|e| e.to_string())?;
            Ok(json!(hits.iter().map(|h| { let mut s = summary(&h.paper); s["match"] = json!(h.snippet); s }).collect::<Vec<_>>()))
        }
        "list_papers" => {
            let collection = match arg_str(a, "collection") { Some(c) => Some(collection_id(lib, &c, false)?), None => None };
            let f = PaperFilter {
                collection,
                tag: arg_str(a, "tag"),
                starred_only: a.get("starred").and_then(|v| v.as_bool()).unwrap_or(false),
                unread_only: a.get("unread").and_then(|v| v.as_bool()).unwrap_or(false),
            };
            let sort = match arg_str(a, "sort").as_deref() {
                Some("year") => SortKey::Year,
                Some("title") => SortKey::Title,
                Some("author") => SortKey::FirstAuthor,
                _ => SortKey::Added,
            };
            let all = lib.papers(f, sort);
            let off = arg_u(a, "offset", 0) as usize;
            let lim = arg_u(a, "limit", 25).min(200) as usize;
            Ok(json!({"total": all.len(), "papers": all.iter().skip(off).take(lim).map(summary).collect::<Vec<_>>()}))
        }
        "get_paper" => {
            let p = need(&arg_str(a, "id").ok_or("id is required")?)?;
            Ok(details(lib, &p))
        }
        "read_paper" => {
            let id = arg_str(a, "id").ok_or("id is required")?;
            let p = need(&id)?;
            let Some(text) = full_text(lib, &id) else {
                return Ok(json!({"id": id, "title": p.meta.title, "full_text": Value::Null,
                    "note": "No full text yet (no reading view). The abstract is below; find_pdf may help, then open Margins to convert it.",
                    "abstract": p.meta.abstract_text}));
            };
            if let Some(q) = arg_str(a, "query") {
                let hits = passages(&text, &q, 6);
                return Ok(json!({"id": id, "title": p.meta.title, "query": q,
                    "passages": hits.iter().map(|(n, t)| json!({"paragraph": n, "text": t})).collect::<Vec<_>>()}));
            }
            let chars: Vec<char> = text.chars().collect();
            let off = (arg_u(a, "offset", 0) as usize).min(chars.len());
            let max = arg_u(a, "max_chars", 12000).clamp(500, 60000) as usize;
            let end = (off + max).min(chars.len());
            Ok(json!({"id": id, "title": p.meta.title, "total_chars": chars.len(), "offset": off,
                "next_offset": if end < chars.len() { json!(end) } else { Value::Null },
                "text": chars[off..end].iter().collect::<String>()}))
        }
        "cite" => Ok(json!(cite(lib, &arg_ids(a), &arg_str(a, "style").unwrap_or("apa".into()))?)),
        "library_overview" => {
            let all = lib.papers(PaperFilter::default(), SortKey::Added);
            Ok(json!({
                "papers": all.len(),
                "unread": all.iter().filter(|p| !p.read).count(),
                "starred": all.iter().filter(|p| p.starred).count(),
                "with_full_text": all.iter().filter(|p| p.has_epub).count(),
                "tags": lib.tags(),
                "collections": lib.collections().iter().map(|c| json!({"name": c.name, "papers": c.paper_count})).collect::<Vec<_>>(),
                "library_folder": library_root().display().to_string(),
            }))
        }
        "find_new_papers" => {
            let q = arg_str(a, "query").ok_or("query is required")?;
            let hits = lib.discover(q, arg_u(a, "limit", 10).min(25) as u32).map_err(|e| e.to_string())?;
            Ok(json!(hits.iter().map(|h| json!({
                "title": h.meta.title, "authors": short_authors(&h.meta.authors), "year": h.meta.year(),
                "journal": h.meta.journal, "doi": h.meta.doi, "cited_by": h.cited_by, "open_access": h.open_access,
                "abstract": h.meta.abstract_text.chars().take(600).collect::<String>(),
                "in_library": h.in_library,
            })).collect::<Vec<_>>()))
        }
        "add_paper" => {
            let ident = arg_str(a, "identifier").ok_or("identifier is required")?;
            let meta = margins_core::resolve_metadata(ident).map_err(|e| e.to_string())?;
            if !meta.doi.is_empty() {
                if let Some(p) = lib.find_by_doi(meta.doi.clone()) {
                    return Ok(json!({"already_in_library": true, "paper": summary(&p)}));
                }
            }
            let p = lib.add_paper(meta, None, None, None).map_err(|e| e.to_string())?;
            let mut pdf = Value::Null;
            if a.get("fetch_pdf").and_then(|v| v.as_bool()).unwrap_or(true) {
                pdf = match lib.find_pdf(p.id.clone()) { Ok(Some(src)) => json!(src), _ => json!("no open-access PDF found") };
            }
            Ok(json!({"added": summary(&lib.paper(p.id.clone()).unwrap_or(p)), "pdf": pdf}))
        }
        "find_pdf" => {
            let id = arg_str(a, "id").ok_or("id is required")?;
            need(&id)?;
            Ok(match lib.find_pdf(id).map_err(|e| e.to_string())? {
                Some(src) => json!({"attached": true, "source": src}),
                None => json!({"attached": false, "note": "no legal open-access PDF found"}),
            })
        }
        "tag_papers" => {
            let tag = arg_str(a, "tag").ok_or("tag is required")?;
            let on = !a.get("remove").and_then(|v| v.as_bool()).unwrap_or(false);
            let ids = arg_ids(a);
            for id in &ids { need(id)?; lib.set_tag(id.clone(), tag.clone(), on).map_err(|e| e.to_string())?; }
            Ok(json!({"tag": tag, "papers": ids.len(), "removed": !on}))
        }
        "add_to_collection" => {
            let name = arg_str(a, "collection").ok_or("collection is required")?;
            let on = !a.get("remove").and_then(|v| v.as_bool()).unwrap_or(false);
            let cid = collection_id(lib, &name, on)?;
            let ids = arg_ids(a);
            for id in &ids { need(id)?; lib.set_membership(cid.clone(), id.clone(), on).map_err(|e| e.to_string())?; }
            Ok(json!({"collection": name, "papers": ids.len(), "removed": !on}))
        }
        "set_status" => {
            let ids = arg_ids(a);
            for id in &ids {
                need(id)?;
                if let Some(r) = a.get("read").and_then(|v| v.as_bool()) { lib.set_read(id.clone(), r).map_err(|e| e.to_string())?; }
                if let Some(s) = a.get("starred").and_then(|v| v.as_bool()) { lib.set_starred(id.clone(), s).map_err(|e| e.to_string())?; }
            }
            Ok(json!({"updated": ids.len()}))
        }
        "add_note" => {
            let id = arg_str(a, "id").ok_or("id is required")?;
            need(&id)?;
            let ann = Annotation {
                id: String::new(), paper_id: id, kind: AnnotationKind::Note, color: "agent".into(),
                location: "paper".into(), position: String::new(),
                quote: arg_str(a, "quote").unwrap_or_default(), prefix: String::new(), suffix: String::new(),
                note: arg_str(a, "note").ok_or("note is required")?, created: 0, modified: 0,
            };
            let saved = lib.save_annotation(ann).map_err(|e| e.to_string())?;
            Ok(json!({"note_id": saved.id}))
        }
        "update_paper" => {
            let id = arg_str(a, "id").ok_or("id is required")?;
            let mut m = need(&id)?.meta;
            if let Some(v) = arg_str(a, "title") { m.title = v; }
            if let Some(v) = a.get("authors").and_then(|v| v.as_array()) { m.authors = v.iter().filter_map(|x| x.as_str().map(String::from)).collect(); }
            if let Some(v) = arg_str(a, "date") { m.date = v; }
            if let Some(v) = arg_str(a, "journal") { m.journal = v; }
            if let Some(v) = arg_str(a, "doi") { m.doi = v; }
            if let Some(v) = arg_str(a, "abstract") { m.abstract_text = v; }
            if let Some(v) = a.get("keywords").and_then(|v| v.as_array()) { m.keywords = v.iter().filter_map(|x| x.as_str().map(String::from)).collect(); }
            lib.update_meta(id.clone(), m).map_err(|e| e.to_string())?;
            Ok(summary(&need(&id)?))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

// ---------------------------------------------------------------- MCP over stdio

fn serve() {
    let lib = match open() {
        Ok(l) => l,
        Err(e) => { eprintln!("margins-mcp: {e}"); std::process::exit(1); }
    };
    // full text joins the search index in the background
    { let l = lib.clone(); std::thread::spawn(move || while matches!(l.index_fulltext(25), Ok(n) if n > 0) {}); }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            send(&mut out, json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}));
            continue;
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // notifications need no answer
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOL),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "margins", "title": "Margins", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Margins is the user's reference manager (scientific papers). Search or list papers to get ids, \
                    read_paper to read full texts (use `query` for long papers), cite for references. Prefer reading the paper \
                    over answering from memory, and quote it. Changes you make sync to the user's Mac and iPhone.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "resources/list" => Ok(json!({"resources": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let _ = lib.refresh(); // see changes made in the apps since the last call
                Ok(match call(&lib, name, &args) {
                    Ok(v) => json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap()}], "isError": false}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                })
            }
            _ => Err(json!({"code": -32601, "message": format!("method not found: {method}")})),
        };
        send(&mut out, match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        });
    }
}

fn send(out: &mut std::io::Stdout, v: Value) {
    let _ = writeln!(out, "{}", v);
    let _ = out.flush();
}

// ---------------------------------------------------------------- command line

fn cli(args: &[String]) -> Result<(), String> {
    let lib = open()?;
    let rest = &args[1..];
    let print = |v: Value| match v { Value::String(s) => println!("{s}"), v => println!("{}", serde_json::to_string_pretty(&v).unwrap()) };
    match args[0].as_str() {
        "search" => {
            let _ = lib.index_fulltext(2000);
            print(call(&lib, "search_papers", &json!({"query": rest.join(" "), "limit": 20}))?)
        }
        "show" => print(call(&lib, "get_paper", &json!({"id": rest.first().ok_or("usage: show <id>")?}))?),
        "read" => {
            let id = rest.first().ok_or("usage: read <id> [query]")?;
            let q = rest[1..].join(" ");
            let v = call(&lib, "read_paper", &json!({"id": id, "query": q, "max_chars": 60000}))?;
            if let Some(t) = v.get("text").and_then(|t| t.as_str()) { println!("{t}") } else { print(v) }
        }
        "cite" => {
            let mut st = "apa".to_string();
            let mut ids = vec![];
            let mut it = rest.iter();
            while let Some(x) = it.next() {
                if x == "--style" { st = it.next().cloned().unwrap_or(st) } else { ids.push(x.clone()) }
            }
            println!("{}", cite(&lib, &ids, &st)?)
        }
        "path" => {
            let id = rest.first().ok_or("usage: path <id> [pdf|epub|text]")?;
            let k = match rest.get(1).map(|s| s.as_str()) { Some("epub") => FileKind::Epub, Some("text") => FileKind::Fulltext, _ => FileKind::Pdf };
            println!("{}", lib.file_path(id.clone(), k).ok_or("no such file")?)
        }
        "help" | "--help" | "-h" => println!("{}", include_str!("margins-mcp.rs").lines().take(10).map(|l| l.trim_start_matches("//!").trim_start_matches(' ')).collect::<Vec<_>>().join("\n")),
        other => return Err(format!("unknown command {other} (try: help)")),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "serve" {
        serve();
    } else if let Err(e) = cli(&args) {
        eprintln!("margins-mcp: {e}");
        std::process::exit(1);
    }
}
