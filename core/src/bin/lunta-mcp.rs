//! Lunta for agents: an MCP server (stdio) and a small command-line tool over the same library
//! the apps use.
//!
//!   lunta-mcp                     run as an MCP server (Claude Code, Claude Desktop, Cursor, …)
//!   lunta-mcp search <query>      search the library
//!   lunta-mcp show <id>           details of a paper
//!   lunta-mcp read <id> [query]   full text (or the passages matching a query)
//!   lunta-mcp cite <id>… [--style apa|harvard|chicago|cell|mla|vancouver|ieee|ama|nature|science|pnas|acs|bibtex|latex|ris]
//!   lunta-mcp path <id> [pdf|epub|text]
//!
//! The agent writes through its own change log (ops/agent-….jsonl), exactly like another device,
//! so it can work while the Mac and iPhone apps are open, and its changes sync everywhere.

use lunta_core::model::*;
use lunta_core::{export, placeholders, Library};
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
    if let Ok(p) = std::env::var("LUNTA_LIBRARY") {
        return PathBuf::from(p);
    }
    // where the Mac app has its library (iCloud, Google Drive or only on this Mac, chosen in the app)
    if let Ok(p) = std::fs::read_to_string(home().join("Library/Application Support/Lunta/library-root")) {
        let p = PathBuf::from(p.trim());
        if !p.as_os_str().is_empty() && p.is_dir() {
            return p;
        }
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
        return Err(format!("No Lunta library at {} (open the Lunta app once first)", root.display()));
    }
    let cache = home().join("Library/Caches/Lunta/agent");
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
    let lib = Library::open(root.display().to_string(), cache.display().to_string(), device).map_err(|e| e.to_string())?;
    apply_remote_files(&lib);
    Ok(lib)
}

/// A Mac whose library is in Google Drive and downloads papers when they're opened: the app lists
/// the paper files that are only in Drive (remote-files.json, for this library root). The agent
/// doesn't download them itself (it has no Drive sign-in, and two syncs of one folder would race):
/// they count as present (no find_pdf over a PDF that's in Drive, no "no reading view"), and what
/// needs the file itself says it isn't downloaded. Full texts are always on the Mac, so read_paper
/// and search see every paper. Re-read when the file changes.
fn apply_remote_files(lib: &Library) {
    static SEEN: std::sync::Mutex<Option<std::time::SystemTime>> = std::sync::Mutex::new(None);
    let f = home().join("Library/Application Support/Lunta/remote-files.json");
    let modified = std::fs::metadata(&f).and_then(|m| m.modified()).ok();
    let mut seen = SEEN.lock().unwrap();
    if seen.is_some() && *seen == modified {
        return;
    }
    *seen = modified;
    let files = std::fs::read_to_string(&f)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.get("root").and_then(|r| r.as_str()).map(|r| PathBuf::from(r) == library_root()).unwrap_or(false))
        .and_then(|v| v.get("files").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()))
        .unwrap_or_default();
    lib.set_remote_files(files);
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
        "cite_key": p.cite_key,
        "read": p.read,
        "starred": p.starred,
        "must_read": p.must_read,
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
        .map(|a| json!({"id": a.id, "kind": format!("{:?}", a.kind).to_lowercase(), "color": a.color, "quote": a.quote, "note": a.note}))
        .collect();
    let not_downloaded: Vec<&str> = [(FileKind::Pdf, "pdf"), (FileKind::Epub, "reading_view_epub"), (FileKind::Audio, "narration_audio")]
        .iter()
        .filter(|(k, _)| lib.file_status(p.id.clone(), *k) == FileStatus::InCloud)
        .map(|(_, n)| *n)
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
        "cite_key": p.cite_key,
        "tags": p.tags,
        "collections": p.collections.iter().filter_map(|c| cols.get(c).cloned()).collect::<Vec<_>>(),
        "read": p.read, "starred": p.starred, "must_read": p.must_read,
        "annotations": notes,
        "files": {
            "pdf": file(FileKind::Pdf),
            "reading_view_epub": file(FileKind::Epub),
            "full_text": file(FileKind::Fulltext),
            "narration_audio": file(FileKind::Audio),
        },
        // in iCloud or Google Drive, not downloaded to this Mac (opening the paper in Lunta downloads it)
        "not_downloaded": not_downloaded,
        "open_in_app": format!("lunta://open?id={}", p.id),
        "doi_link": if p.meta.doi.is_empty() { Value::Null } else { json!(format!("https://doi.org/{}", p.meta.doi)) },
    })
}

/// A highlight or note as the tools show it.
fn highlight_json(a: &Annotation, title: Option<&str>) -> Value {
    let mut v = json!({
        "highlight_id": a.id,
        "paper_id": a.paper_id,
        "kind": format!("{:?}", a.kind).to_lowercase(),
        "color": a.color,
        "quote": a.quote.split_whitespace().collect::<Vec<_>>().join(" "),
        "note": a.note,
        "made_in": match a.location.split(':').next().unwrap_or("") { "epub" => "reading view", "pdf" => "pdf", _ => "note on the paper" },
        "created": date(a.created),
    });
    if let Some(t) = title {
        v["paper_title"] = json!(t);
    }
    v
}

/// YYYY-MM-DD for a time in ms since 1970 (UTC).
fn date(ms: i64) -> String {
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
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
    CitationStyle::from_key(s)
}

/// "apa, harvard, …, bibtex or ris", for help and errors.
fn style_keys() -> String {
    CitationStyle::ALL.iter().map(|s| s.key()).collect::<Vec<_>>().join(", ") + ", bibtex, latex or ris"
}

fn cite(lib: &Library, ids: &[String], st: &str) -> Result<String, String> {
    let papers: Vec<Paper> = ids.iter().filter_map(|i| lib.paper(i.clone())).collect();
    if papers.is_empty() {
        return Err("no such papers".into());
    }
    Ok(match st.to_lowercase().as_str() {
        // the papers' fixed citation keys, as in the apps
        "bibtex" | "bib" => lib.export_bibtex(papers.iter().map(|p| p.id.clone()).collect()),
        "latex" | "cite" => lib.latex_cite(papers.iter().map(|p| p.id.clone()).collect()),
        "ris" => export::ris(&papers),
        s => {
            let st = style(s).ok_or(format!("unknown style {s}: use {}", style_keys()))?;
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
            "annotations": {"readOnlyHint": read_only, "openWorldHint": name == "find_new_papers" || name == "add_paper" || name == "find_pdf" || name == "resolve_citations" || name == "check_references"},
        })
    }
    let ids = json!({"type": "array", "items": {"type": "string"}, "description": "Paper ids"});
    json!([
        t("search_papers", "Search the user's Lunta library (titles, authors, abstracts, full text, and the user's highlights and notes). Returns paper summaries with ids.",
          json!({"query": {"type": "string"}, "limit": {"type": "integer", "default": 10}}), &["query"], true),
        t("list_papers", "List papers in the library, optionally filtered (unread, starred, must_read: the user's reading queue, tag, collection) and sorted.",
          json!({"unread": {"type": "boolean"}, "starred": {"type": "boolean"}, "must_read": {"type": "boolean"}, "tag": {"type": "string"},
                 "collection": {"type": "string", "description": "Collection name"},
                 "sort": {"type": "string", "enum": ["added", "year", "title", "author"], "default": "added"},
                 "limit": {"type": "integer", "default": 25}, "offset": {"type": "integer", "default": 0}}), &[], true),
        t("get_paper", "Full details of a paper: metadata, abstract, tags, collections, notes and highlights, and local file paths (PDF, EPUB reading view, full text).",
          json!({"id": {"type": "string"}}), &["id"], true),
        t("read_paper", "Read a paper's full text. With `query`, returns only the best-matching paragraphs; otherwise pages through the text with offset/max_chars.",
          json!({"id": {"type": "string"}, "query": {"type": "string"}, "offset": {"type": "integer", "default": 0},
                 "max_chars": {"type": "integer", "default": 12000}}), &["id"], true),
        t("cite", "Formatted citations or a bibliography for papers. `bibtex` uses each paper's citation key (fixed, the same as in the Lunta apps); `latex` gives \\cite{key1,key2} to paste into a LaTeX document.",
          json!({"ids": ids, "style": {"type": "string", "enum": CitationStyle::ALL.iter().map(|s| s.key()).chain(["bibtex", "latex", "ris"]).collect::<Vec<_>>(), "default": "apa"}}), &["ids"], true),
        t("resolve_citations", "Find citation placeholders in a text the user is writing ((Langseth et al. 2025), (Smith 2019; Lee 2020), [Smith 2019 hint words], {Smith 2019}; with latex: \\cite{?smith2019}) and propose, for each, up to 5 papers it may mean: the library's first (family names, year, hint words), then Crossref/OpenAlex. Read-only: nothing is added or changed; show the candidates and let the user confirm each one (then cite them, adding an outside paper with add_paper).",
          json!({"text": {"type": "string"}, "latex": {"type": "boolean", "default": false, "description": "The text is LaTeX: \\cite{?key} placeholders, comments skipped"},
                 "external": {"type": "boolean", "default": true, "description": "Also search Crossref/OpenAlex when the library has no exact match"}}), &["text"], true),
        t("library_overview", "Size of the library, its tags and collections.", json!({}), &[], true),
        t("find_new_papers", "Search published literature beyond the library (OpenAlex). Marks results already in the library.",
          json!({"query": {"type": "string"}, "limit": {"type": "integer", "default": 10}}), &["query"], true),
        t("add_paper", "Add a paper to the library by DOI, PubMed ID, PMCID, arXiv id or URL. Fetches its metadata and, when legally available, its open-access PDF. The Lunta app makes the reading view.",
          json!({"identifier": {"type": "string"}, "fetch_pdf": {"type": "boolean", "default": true}}), &["identifier"], false),
        t("find_pdf", "Find and attach a legal open-access PDF for a paper in the library.", json!({"id": {"type": "string"}}), &["id"], false),
        t("tag_papers", "Add (or remove) a tag on papers.",
          json!({"ids": ids, "tag": {"type": "string"}, "remove": {"type": "boolean", "default": false}}), &["ids", "tag"], false),
        t("add_to_collection", "Put papers into a collection (created if missing), or take them out.",
          json!({"ids": ids, "collection": {"type": "string"}, "remove": {"type": "boolean", "default": false}}), &["ids", "collection"], false),
        t("set_status", "Mark papers read/unread, starred/unstarred, and/or on/off the Must Read list (the user's reading queue; marking a paper read takes it off).",
          json!({"ids": ids, "read": {"type": "boolean"}, "starred": {"type": "boolean"}, "must_read": {"type": "boolean"}}), &["ids"], false),
        t("add_note", "Attach a note to a paper (shown in Lunta on Mac and iPhone). Optionally quote the passage it refers to.",
          json!({"id": {"type": "string"}, "note": {"type": "string"}, "quote": {"type": "string"}}), &["id", "note"], false),
        t("list_highlights", "The user's highlights and notes across the library, newest first, with the paper each is in. Filter by paper, colour, tag, collection, or words in the highlighted text or note (e.g. to find papers where the user highlighted something).",
          json!({"paper_id": {"type": "string"}, "color": {"type": "string", "enum": ["yellow", "green", "blue", "pink"]},
                 "tag": {"type": "string"}, "collection": {"type": "string", "description": "Collection name"},
                 "query": {"type": "string", "description": "Words that must all appear in the highlighted text or the note"},
                 "limit": {"type": "integer", "default": 25}, "offset": {"type": "integer", "default": 0}}), &[], true),
        t("add_highlight", "Highlight a passage in a paper's reading view by quoting it exactly (copy the text from read_paper; a sentence or part of one). Optionally attach a note. Shows in Lunta on Mac and iPhone, in the reading view and the PDF. If the passage occurs more than once, pass `before`: a few words just before it.",
          json!({"id": {"type": "string", "description": "Paper id"}, "quote": {"type": "string", "description": "The passage, word for word"},
                 "before": {"type": "string", "description": "A few words just before the passage, to tell repeated passages apart"},
                 "note": {"type": "string"}, "color": {"type": "string", "enum": ["yellow", "green", "blue", "pink"], "default": "yellow"}}), &["id", "quote"], false),
        t("update_highlight", "Change a highlight's or note's text note or colour, or delete it (ids from list_highlights or get_paper).",
          json!({"highlight_id": {"type": "string"}, "note": {"type": "string", "description": "New note; empty string removes the note"},
                 "color": {"type": "string", "enum": ["yellow", "green", "blue", "pink"]}, "delete": {"type": "boolean", "default": false}}), &["highlight_id"], false),
        t("update_paper", "Correct a paper's details. Only the fields given are changed.",
          json!({"id": {"type": "string"}, "title": {"type": "string"}, "authors": {"type": "array", "items": {"type": "string"}},
                 "date": {"type": "string"}, "journal": {"type": "string"}, "doi": {"type": "string"}, "abstract": {"type": "string"},
                 "keywords": {"type": "array", "items": {"type": "string"}}}), &["id"], false),
        t("check_references", "Check a reference list (a manuscript's, or one being reviewed) against the published record: each reference looked up by DOI (Crossref, DataCite), else by its details (Crossref, OpenAlex, PubMed), and its title, first author, number of authors, year, venue, volume, pages and DOI compared. Results per reference: verified, minor differences (which fields), not found (\"could not be found in Crossref, OpenAlex or PubMed\": report it as that, not as invented), DOI points to a different work, or not a paper. Retracted or corrected works are flagged. Pass `text` (a pasted list, numbered or author-year, or RIS) or `bibtex`. Read-only; about a second per reference.",
          json!({"text": {"type": "string", "description": "The references: a pasted list (one per line or numbered) or RIS"},
                 "bibtex": {"type": "string", "description": "A .bib file's contents"},
                 "markdown": {"type": "boolean", "default": false, "description": "Also return the report as Markdown with links"}}), &[], true),
        t("outline_from_highlights", "An outline of the user's highlights and notes in some papers (`ids`, or a `collection`/folder name, or a `tag`), optionally about a `topic` (then only the highlights that mention it, unless none does): Markdown with headings per paper and section, each point a highlight word for word or the user's own note (marked Note), cited author-year with a lunta:// link and the citation key ([@key]), and a Sources list. Read-only. You may regroup the points into themes for the user, but keep every quote as it is and never present your own words as a highlight.",
          json!({"ids": ids, "collection": {"type": "string", "description": "Collection (folder) name"}, "tag": {"type": "string"},
                 "topic": {"type": "string", "description": "What the outline is about (keeps the highlights that mention it)"}}), &[], true),
        t("library_status", "Papers in the library flagged by Lunta's checks: retracted, withdrawn, corrected or with an expression of concern (Crossref and Retraction Watch notices), and preprints that have been published (with the published version), linked to it, or switched to it. Read-only; the apps check weekly.",
          json!({"id": {"type": "string", "description": "Only this paper"}}), &[], true),
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
                must_read_only: a.get("must_read").and_then(|v| v.as_bool()).unwrap_or(false),
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
                    "note": "No full text yet (no reading view). The abstract is below; find_pdf may help, then open Lunta to convert it.",
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
        "resolve_citations" => {
            let text = arg_str(a, "text").ok_or("text is required")?;
            let latex = a.get("latex").and_then(Value::as_bool).unwrap_or(false);
            let external = a.get("external").and_then(Value::as_bool).unwrap_or(true);
            let found = if latex { placeholders::find_tex_placeholders(text) } else { placeholders::find_placeholders(text) };
            Ok(json!(found.iter().take(200).map(|p| json!({
                "placeholder": p.text, "start": p.start, "end": p.end,
                "cites": p.cites.iter().map(|c| {
                    let s = lib.suggest_citation(c.clone(), external);
                    json!({
                        "cite": c.text, "surnames": c.surnames, "year": c.year, "hints": c.hints,
                        "exact": s.exact, "note": s.note,
                        "candidates": s.candidates.iter().map(|k| json!({
                            "id": (!k.paper_id.is_empty()).then(|| k.paper_id.clone()), "in_library": !k.paper_id.is_empty(),
                            "source": k.source, "confidence": (k.confidence * 100.0).round() / 100.0, "exact": k.exact,
                            "title": k.meta.title, "authors": short_authors(&k.meta.authors), "year": k.meta.year(),
                            "journal": k.meta.journal, "doi": k.meta.doi, "cite_key": (!k.cite_key.is_empty()).then(|| k.cite_key.clone()),
                        })).collect::<Vec<_>>(),
                    })
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>()))
        }
        "library_overview" => {
            let all = lib.papers(PaperFilter::default(), SortKey::Added);
            Ok(json!({
                "papers": all.len(),
                "unread": all.iter().filter(|p| !p.read).count(),
                "starred": all.iter().filter(|p| p.starred).count(),
                "must_read": all.iter().filter(|p| p.must_read).count(),
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
            let meta = lunta_core::resolve_metadata(ident).map_err(|e| e.to_string())?;
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
                if let Some(m) = a.get("must_read").and_then(|v| v.as_bool()) { lib.set_must_read(id.clone(), m).map_err(|e| e.to_string())?; }
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
        "list_highlights" => {
            let collection = match arg_str(a, "collection") { Some(c) => Some(collection_id(lib, &c, false)?), None => None };
            let paper = arg_str(a, "paper_id");
            if let Some(p) = &paper { need(p)?; }
            let all = lib.find_annotations(lunta_core::highlight::AnnotationFilter {
                paper, color: arg_str(a, "color"), tag: arg_str(a, "tag"), collection, query: arg_str(a, "query"),
            });
            let off = arg_u(a, "offset", 0) as usize;
            let lim = arg_u(a, "limit", 25).min(200) as usize;
            let mut titles: HashMap<String, String> = HashMap::new();
            let page: Vec<Value> = all.iter().skip(off).take(lim).map(|x| {
                let t = titles.entry(x.paper_id.clone()).or_insert_with(|| lib.paper(x.paper_id.clone()).map(|p| p.meta.title).unwrap_or_default());
                highlight_json(x, Some(t))
            }).collect();
            let next = if off + page.len() < all.len() { json!(off + page.len()) } else { Value::Null };
            Ok(json!({"total": all.len(), "next_offset": next, "highlights": page}))
        }
        "add_highlight" => {
            let id = arg_str(a, "id").ok_or("id is required")?;
            let p = need(&id)?;
            let quote = arg_str(a, "quote").ok_or("quote is required")?;
            let saved = lib
                .add_highlight(id, quote, arg_str(a, "before"), arg_str(a, "color").unwrap_or_default(), arg_str(a, "note").unwrap_or_default())
                .map_err(|e| e.to_string())?;
            Ok(highlight_json(&saved, Some(&p.meta.title)))
        }
        "update_highlight" => {
            let hid = arg_str(a, "highlight_id").ok_or("highlight_id is required")?;
            let mut h = lib.annotation(hid.clone()).ok_or(format!("no highlight with id {hid}"))?;
            if a.get("delete").and_then(|v| v.as_bool()).unwrap_or(false) {
                lib.delete_annotation(h.paper_id.clone(), hid.clone()).map_err(|e| e.to_string())?;
                return Ok(json!({"deleted": hid}));
            }
            if let Some(c) = arg_str(a, "color") {
                let c = c.trim().to_lowercase();
                if !lunta_core::highlight::COLORS.contains(&c.as_str()) { return Err(format!("unknown colour {c}: use yellow, green, blue or pink")); }
                h.color = c;
            }
            if let Some(n) = a.get("note").and_then(|v| v.as_str()) {
                h.note = n.trim().to_string();
                // a quoted passage without a note is a plain highlight; a note on the paper stays a note
                if !h.quote.trim().is_empty() {
                    h.kind = if h.note.is_empty() { AnnotationKind::Highlight } else { AnnotationKind::Note };
                }
            }
            let saved = lib.save_annotation(h).map_err(|e| e.to_string())?;
            let title = lib.paper(saved.paper_id.clone()).map(|p| p.meta.title);
            Ok(highlight_json(&saved, title.as_deref()))
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
        "check_references" => {
            let text = arg_str(a, "bibtex").or_else(|| arg_str(a, "text")).ok_or("text or bibtex is required")?;
            let cache = home().join("Library/Caches/Lunta/agent");
            let r = lunta_core::checks::refcheck::check_references(text, cache.display().to_string(), None);
            let r = lib.mark_references_in_library(r);
            let items: Vec<Value> = r
                .items
                .iter()
                .map(|i| {
                    let mut v = json!({
                        "number": i.number, "key": i.key, "reference": i.reference,
                        "result": lunta_core::checks::refcheck::verdict_name(i.verdict),
                        "source": i.source, "link": i.link, "note": i.note,
                        "differences": i.differences.iter().map(|d| json!({"field": d.field, "in_reference": d.given, "in_record": d.found})).collect::<Vec<_>>(),
                        "notices": i.notices.iter().map(|n| json!({"kind": n.kind, "date": n.date, "notice": if n.doi.is_empty() { Value::Null } else { json!(format!("https://doi.org/{}", n.doi)) }, "source": n.source})).collect::<Vec<_>>(),
                    });
                    if let Some(f) = &i.found {
                        v["found"] = json!({"title": f.title, "authors": short_authors(&f.authors), "year": f.year(), "venue": f.journal, "doi": f.doi});
                    }
                    if !i.suggested_doi.is_empty() {
                        v["suggested_doi"] = json!(i.suggested_doi);
                    }
                    if let Some(id) = &i.in_library {
                        v["in_library"] = json!(id);
                    }
                    v
                })
                .collect();
            let mut out = json!({"summary": r.summary, "verified": r.verified, "minor_differences": r.minor, "not_found": r.not_found,
                                 "wrong_doi": r.wrong_doi, "not_papers": r.not_checked, "could_not_check": r.failed, "retracted": r.retracted, "references": items});
            if a.get("markdown").and_then(|v| v.as_bool()).unwrap_or(false) {
                out["markdown"] = json!(r.markdown);
            }
            Ok(out)
        }
        "outline_from_highlights" => {
            let mut ids = arg_ids(a);
            if ids.is_empty() {
                let collection = match arg_str(a, "collection") {
                    Some(c) => Some(collection_id(lib, &c, false)?),
                    None => None,
                };
                let tag = arg_str(a, "tag");
                if collection.is_none() && tag.is_none() {
                    return Err("give ids, a collection or a tag".into());
                }
                ids = lib.papers(PaperFilter { collection, tag, ..Default::default() }, SortKey::FirstAuthor).into_iter().map(|p| p.id).collect();
            }
            let o = lib.outline_from_highlights(ids, arg_str(a, "topic").unwrap_or_default(), None, None);
            Ok(json!({"markdown": o.markdown, "highlights": o.items, "papers": o.papers, "note": o.note}))
        }
        "library_status" => {
            let only = arg_str(a, "id");
            let mut list: Vec<Value> = vec![];
            for (id, u) in lib.all_paper_updates() {
                if only.as_ref().is_some_and(|o| o != &id) {
                    continue;
                }
                let Some(p) = lib.paper(id.clone()) else { continue };
                let mut v = summary(&p);
                let level = format!("{:?}", lunta_core::checks::updates_level(u.clone())).to_lowercase();
                if level != "none" {
                    v["status"] = json!(level);
                    v["headline"] = json!(lunta_core::checks::updates_headline(u.clone()));
                    v["notices"] = json!(u.notices.iter().map(|n| json!({"kind": n.kind, "label": n.label, "date": n.date, "notice": if n.doi.is_empty() { Value::Null } else { json!(format!("https://doi.org/{}", n.doi)) }, "source": n.source})).collect::<Vec<_>>());
                }
                let version = |pv: &lunta_core::checks::PublishedVersion| json!({"doi": pv.doi, "title": pv.title, "journal": pv.journal, "date": pv.date, "found_by": pv.found_by, "link": format!("https://doi.org/{}", pv.doi)});
                if let Some(pv) = &u.published {
                    v["published_version"] = version(pv);
                    v["published_note"] = json!("A preprint with a published version; the user can switch to it in Lunta (not done automatically).");
                }
                if let Some(pv) = &u.linked {
                    v["linked_published_version"] = version(pv);
                }
                if let Some(pre) = &u.preprint {
                    v["switched_from_preprint"] = json!({"doi": pre.doi, "journal": pre.journal});
                }
                if v.get("status").is_none() && v.get("published_version").is_none() && v.get("linked_published_version").is_none() && v.get("switched_from_preprint").is_none() {
                    continue;
                }
                list.push(v);
            }
            list.sort_by_key(|v| v["title"].as_str().unwrap_or("").to_lowercase());
            Ok(json!({"papers": list, "note": "From Lunta's checks of Crossref (with Retraction Watch), bioRxiv, arXiv and OpenAlex, run weekly by the apps."}))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

// ---------------------------------------------------------------- MCP over stdio

fn serve() {
    let lib = match open() {
        Ok(l) => l,
        Err(e) => { eprintln!("lunta-mcp: {e}"); std::process::exit(1); }
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
                "serverInfo": {"name": "lunta", "title": "Lunta", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Lunta is the user's reference manager (scientific papers). Search or list papers to get ids, \
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
                apply_remote_files(&lib);
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
        "help" | "--help" | "-h" => println!("{}", include_str!("lunta-mcp.rs").lines().take(10).map(|l| l.trim_start_matches("//!").trim_start_matches(' ')).collect::<Vec<_>>().join("\n")),
        other => return Err(format!("unknown command {other} (try: help)")),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "serve" {
        serve();
    } else if let Err(e) = cli(&args) {
        eprintln!("lunta-mcp: {e}");
        std::process::exit(1);
    }
}
