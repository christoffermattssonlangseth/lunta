//! Lunta for Windows and Linux (and macOS, for testing): a Tauri shell around the Rust core.
//!
//! The core (`../../core`, crate lunta-core) is called directly, not through UniFFI. The web
//! frontend (../src) talks to it through the commands below. Reading views are served by the
//! `lunta` URI scheme: `lunta://localhost/epub/<paper>/<file>` (http://lunta.localhost/… on
//! Windows) serves the unpacked EPUB, with the apps' reader scripts injected into the continuous
//! document, and `/reader/<file>` serves those scripts.

mod add;
#[cfg(all(debug_assertions, target_os = "macos"))]
mod devsnap;
mod positions;
mod settings;
mod smart;

use lunta_core::model::{Annotation, AnnotationKind, CitationStyle, FileKind, Paper, PaperFilter, PaperMeta, SortKey};
use lunta_core::Library;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::http::{header, Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};

struct OpenLibrary {
    lib: Arc<Library>,
    root: PathBuf,
}

pub struct AppState {
    config_dir: PathBuf,
    cache_root: PathBuf,
    settings: Mutex<settings::Settings>,
    open: Mutex<Option<OpenLibrary>>,
    /// paper id -> folder its EPUB was unpacked into (what the lunta:// scheme may serve)
    books: Mutex<HashMap<String, PathBuf>>,
    open_error: Mutex<Option<String>>,
}

impl AppState {
    fn lib(&self) -> Result<Arc<Library>, String> {
        self.open.lock().unwrap().as_ref().map(|o| o.lib.clone()).ok_or_else(|| "No library is open.".to_string())
    }
    fn root(&self) -> Result<PathBuf, String> {
        self.open.lock().unwrap().as_ref().map(|o| o.root.clone()).ok_or_else(|| "No library is open.".to_string())
    }
    fn device(&self) -> String {
        self.settings.lock().unwrap().device_id.clone()
    }

    /// Opens a library folder; the search index lives in this install's cache, one per library.
    fn open_library(&self, root: &Path) -> Result<(), String> {
        if !root.is_dir() {
            return Err(format!("{} is not a folder.", root.display()));
        }
        let mut h: u64 = 1469598103934665603; // FNV-1a of the path: a stable cache folder name
        for b in root.to_string_lossy().bytes() {
            h = (h ^ b as u64).wrapping_mul(1099511628211);
        }
        let cache = self.cache_root.join("libraries").join(format!("{h:016x}"));
        std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let lib = Library::open(root.to_string_lossy().to_string(), cache.to_string_lossy().to_string(), self.device()).map_err(|e| e.to_string())?;
        // full texts into the search index, in the background (files may still be syncing)
        let bg = lib.clone();
        std::thread::spawn(move || while matches!(bg.index_fulltext(20), Ok(n) if n > 0) {});
        *self.open.lock().unwrap() = Some(OpenLibrary { lib, root: root.to_path_buf() });
        self.books.lock().unwrap().clear();
        Ok(())
    }
}

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---- what the frontend sees ----

#[derive(Serialize)]
struct PaperDto {
    id: String,
    meta: PaperMeta,
    year: String,
    added: i64,
    has_pdf: bool,
    has_epub: bool,
    read: bool,
    starred: bool,
    must_read: bool,
    collections: Vec<String>,
    tags: Vec<String>,
    annotation_count: u32,
}

impl From<Paper> for PaperDto {
    fn from(p: Paper) -> Self {
        PaperDto {
            year: p.meta.year(),
            id: p.id,
            meta: p.meta,
            added: p.added,
            has_pdf: p.has_pdf,
            has_epub: p.has_epub,
            read: p.read,
            starred: p.starred,
            must_read: p.must_read,
            collections: p.collections,
            tags: p.tags,
            annotation_count: p.annotation_count,
        }
    }
}

#[derive(Serialize)]
struct Row {
    paper: PaperDto,
    snippet: String,
}

#[derive(Serialize)]
struct Info {
    library: Option<String>,
    opened: bool,
    error: Option<String>,
    device_id: String,
    device_label: String,
}

#[derive(Serialize)]
struct Folder {
    id: String,
    name: String,
    parent: Option<String>,
    paper_count: u32,
}

#[derive(Serialize)]
struct Sidebar {
    all: usize,
    starred: usize,
    must_read: usize,
    unread: usize,
    folders: Vec<Folder>,
    tags: Vec<String>,
    smart: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct Source {
    kind: String,
    #[serde(default)]
    id: Option<String>,
}

fn sort_key(s: &str) -> SortKey {
    match s {
        "year" => SortKey::Year,
        "title" => SortKey::Title,
        "author" => SortKey::FirstAuthor,
        _ => SortKey::Added,
    }
}

// ---- library ----

#[tauri::command]
fn app_info(state: State<AppState>) -> Info {
    let s = state.settings.lock().unwrap().clone();
    Info {
        library: s.library,
        opened: state.open.lock().unwrap().is_some(),
        error: state.open_error.lock().unwrap().clone(),
        device_id: s.device_id,
        device_label: settings::device_label().into(),
    }
}

/// Uses a folder as the library (made if it doesn't exist) and remembers it.
#[tauri::command]
async fn choose_library(state: State<'_, AppState>, path: String) -> R<()> {
    let root = PathBuf::from(path.trim());
    if root.as_os_str().is_empty() {
        return Err("Choose a folder first.".into());
    }
    if !root.exists() {
        std::fs::create_dir_all(&root).map_err(err)?;
    }
    state.open_library(&root)?;
    *state.open_error.lock().unwrap() = None;
    let mut s = state.settings.lock().unwrap();
    s.library = Some(root.to_string_lossy().to_string());
    settings::save(&state.config_dir, &s).map_err(err)
}

/// Picks up changes synced from other devices. True if anything changed.
#[tauri::command]
async fn refresh(state: State<'_, AppState>) -> R<bool> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.refresh().map_err(err)).await.map_err(err)?
}

#[tauri::command]
async fn sidebar(state: State<'_, AppState>) -> R<Sidebar> {
    let lib = state.lib()?;
    let every = lib.papers(PaperFilter::default(), SortKey::Added);
    let folders = lib
        .collections()
        .into_iter()
        .map(|c| Folder { id: c.id, name: c.name, parent: c.parent, paper_count: c.paper_count })
        .collect();
    let smart = smart::load(&state.root()?).into_iter().map(|f| (f.id, f.name)).collect();
    Ok(Sidebar {
        all: every.len(),
        starred: every.iter().filter(|p| p.starred).count(),
        must_read: every.iter().filter(|p| p.must_read).count(),
        unread: every.iter().filter(|p| !p.read).count(),
        folders,
        tags: lib.tags(),
        smart,
    })
}

/// The paper list for a sidebar place, optionally narrowed by a full-text search.
#[tauri::command]
async fn list(state: State<'_, AppState>, source: Source, query: String, sort: String) -> R<Vec<Row>> {
    let lib = state.lib()?;
    let root = state.root()?;
    tauri::async_runtime::spawn_blocking(move || {
        let sort = sort_key(&sort);
        let mut filter = PaperFilter::default();
        let id = source.id.clone().unwrap_or_default();
        let base: Vec<Paper> = match source.kind.as_str() {
            "smart" => match smart::load(&root).into_iter().find(|f| f.id == id) {
                Some(f) => smart::papers(&f, &lib, sort),
                None => vec![],
            },
            kind => {
                match kind {
                    "starred" => filter.starred_only = true,
                    "must_read" => filter.must_read_only = true,
                    "unread" => filter.unread_only = true,
                    "folder" => filter.collection = Some(id),
                    "tag" => filter.tag = Some(id),
                    _ => {}
                }
                lib.papers(filter, sort)
            }
        };
        let q = query.trim();
        if q.is_empty() {
            return Ok(base.into_iter().map(|p| Row { paper: p.into(), snippet: String::new() }).collect());
        }
        let allowed: HashSet<String> = base.iter().map(|p| p.id.clone()).collect();
        let hits = lib.search(q.to_string(), 300).map_err(err)?;
        Ok(hits
            .into_iter()
            .filter(|h| allowed.contains(&h.paper.id))
            .map(|h| Row { paper: h.paper.into(), snippet: h.snippet })
            .collect())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn smart_folders(state: State<'_, AppState>) -> R<Vec<smart::SmartFolder>> {
    Ok(smart::load(&state.root()?))
}

#[tauri::command]
async fn save_smart_folder(state: State<'_, AppState>, folder: smart::SmartFolder) -> R<String> {
    let root = state.root()?;
    let mut folder = folder;
    if folder.id.is_empty() {
        folder.id = uuid::Uuid::new_v4().to_string().to_uppercase();
    }
    smart::save(&root, folder.clone())?;
    Ok(folder.id)
}

#[tauri::command]
async fn delete_smart_folder(state: State<'_, AppState>, id: String) -> R<()> {
    smart::delete(&state.root()?, &id)
}

/// How many papers a smart folder's rules match right now (the editor's live count).
#[tauri::command]
async fn smart_count(state: State<'_, AppState>, folder: smart::SmartFolder) -> R<usize> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || smart::papers(&folder, &lib, SortKey::Added).len()).await.map_err(err)
}

#[tauri::command]
async fn update_meta(state: State<'_, AppState>, id: String, meta: PaperMeta) -> R<()> {
    if meta.title.trim().is_empty() {
        return Err("A paper needs a title.".into());
    }
    state.lib()?.update_meta(id, meta).map_err(err)
}

/// Looks a DOI (or any identifier the adder takes) up online, for "Fill in from the DOI".
#[tauri::command]
async fn lookup_meta(input: String) -> R<PaperMeta> {
    tauri::async_runtime::spawn_blocking(move || lunta_core::resolve_metadata(input).map_err(err)).await.map_err(err)?
}

#[tauri::command]
async fn paper(state: State<'_, AppState>, id: String) -> R<Option<PaperDto>> {
    Ok(state.lib()?.paper(id).map(Into::into))
}

#[tauri::command]
async fn set_read(state: State<'_, AppState>, id: String, read: bool) -> R<()> {
    state.lib()?.set_read(id, read).map_err(err)
}

#[tauri::command]
async fn set_starred(state: State<'_, AppState>, id: String, starred: bool) -> R<()> {
    state.lib()?.set_starred(id, starred).map_err(err)
}

/// On (marking it unread) or off the Must Read list.
#[tauri::command]
async fn set_must_read(state: State<'_, AppState>, id: String, must_read: bool) -> R<()> {
    state.lib()?.set_must_read(id, must_read).map_err(err)
}

/// Opening a paper marks it read but keeps it on the Must Read list.
#[tauri::command]
async fn mark_opened(state: State<'_, AppState>, id: String) -> R<()> {
    state.lib()?.mark_opened(id).map_err(err)
}

#[tauri::command]
async fn set_tag(state: State<'_, AppState>, id: String, tag: String, present: bool) -> R<()> {
    state.lib()?.set_tag(id, tag, present).map_err(err)
}

#[tauri::command]
async fn set_membership(state: State<'_, AppState>, folder: String, id: String, present: bool) -> R<()> {
    state.lib()?.set_membership(folder, id, present).map_err(err)
}

#[tauri::command]
async fn create_folder(state: State<'_, AppState>, name: String, parent: Option<String>) -> R<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Give the folder a name.".into());
    }
    Ok(state.lib()?.create_collection(name, parent).map_err(err)?.id)
}

#[tauri::command]
async fn rename_folder(state: State<'_, AppState>, id: String, name: String) -> R<()> {
    state.lib()?.rename_collection(id, name.trim().to_string()).map_err(err)
}

#[tauri::command]
async fn move_folder(state: State<'_, AppState>, id: String, parent: Option<String>) -> R<()> {
    state.lib()?.move_collection(id, parent).map_err(err)
}

#[tauri::command]
async fn delete_folder(state: State<'_, AppState>, id: String) -> R<()> {
    state.lib()?.delete_collection(id).map_err(err)
}

#[tauri::command]
async fn delete_paper(state: State<'_, AppState>, id: String) -> R<()> {
    state.books.lock().unwrap().remove(&id);
    state.lib()?.delete_paper(id).map_err(err)
}

/// Reference text in a citation style (CitationStyle keys: apa, harvard, …), or BibTeX / RIS.
#[tauri::command]
async fn cite(state: State<'_, AppState>, ids: Vec<String>, style: String) -> R<String> {
    let lib = state.lib()?;
    let one = |s: CitationStyle| ids.iter().filter_map(|i| lib.citation(i.clone(), s)).collect::<Vec<_>>().join("\n\n");
    Ok(match style.as_str() {
        "bibtex" => lib.export_bibtex(ids),
        "ris" => lib.export_ris(ids),
        other => one(CitationStyle::from_key(other).ok_or(format!("unknown style {other}"))?),
    })
}

// ---- adding papers and files ----

fn logger(app: &AppHandle) -> add::Logger {
    let app = app.clone();
    add::Logger(Arc::new(move |m: &str| {
        let _ = app.emit("status", m.to_string());
    }))
}

#[derive(Serialize)]
struct AddResult {
    paper: PaperDto,
    note: Option<String>,
}

#[tauri::command]
async fn add_paper(app: AppHandle, state: State<'_, AppState>, input: String) -> R<AddResult> {
    let lib = state.lib()?;
    let log = logger(&app);
    tauri::async_runtime::spawn_blocking(move || add::add(&lib, &input, &log).map(|a| AddResult { paper: a.paper.into(), note: a.note }))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn attach_pdf(app: AppHandle, state: State<'_, AppState>, id: String, path: String) -> R<Option<String>> {
    let lib = state.lib()?;
    let log = logger(&app);
    state.books.lock().unwrap().remove(&id);
    tauri::async_runtime::spawn_blocking(move || add::attach_pdf(&lib, &id, Path::new(&path), &log)).await.map_err(err)?
}

#[tauri::command]
async fn make_reading_view(app: AppHandle, state: State<'_, AppState>, id: String) -> R<()> {
    let lib = state.lib()?;
    let log = logger(&app);
    state.books.lock().unwrap().remove(&id);
    tauri::async_runtime::spawn_blocking(move || add::make_reading_view(&lib, &id, &log)).await.map_err(err)?
}

/// Looks for an open-access PDF and attaches it. Returns where it came from, if found.
#[tauri::command]
async fn find_pdf(state: State<'_, AppState>, id: String) -> R<Option<String>> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.find_pdf(id).map_err(err)).await.map_err(err)?
}

/// The PDF's bytes, for PDF.js.
#[tauri::command]
async fn pdf_bytes(state: State<'_, AppState>, id: String) -> R<tauri::ipc::Response> {
    let path = state.lib()?.file_path(id, FileKind::Pdf).ok_or("This paper has no PDF on this computer.")?;
    let data = tauri::async_runtime::spawn_blocking(move || std::fs::read(path)).await.map_err(err)?.map_err(err)?;
    Ok(tauri::ipc::Response::new(data))
}

// ---- reading ----

#[derive(Serialize)]
struct TocItem {
    title: String,
    level: u32,
    anchor: String,
}

#[derive(Serialize)]
struct Book {
    /// path under the lunta:// scheme of the continuous document
    path: String,
    toc: Vec<TocItem>,
}

/// Unpacks the paper's EPUB and returns where its continuous document is served, plus contents.
#[tauri::command]
async fn open_book(state: State<'_, AppState>, id: String) -> R<Book> {
    let lib = state.lib()?;
    let pid = id.clone();
    let book = tauri::async_runtime::spawn_blocking(move || lib.open_epub(pid).map_err(err)).await.map_err(err)??;
    let dir = PathBuf::from(&book.dir);
    let combined = PathBuf::from(&book.combined);
    let rel = combined.strip_prefix(&dir).map_err(err)?;
    let rel: Vec<String> = rel.components().map(|c| enc(&c.as_os_str().to_string_lossy())).collect();
    // contents entries point at "section.xhtml#id" or a whole section; in the continuous
    // document these are "#id" and "#section-<n>" (as in ReaderWindow.anchor)
    let toc = book
        .toc
        .iter()
        .map(|t| {
            let anchor = match t.target.split_once('#') {
                Some((_, frag)) => frag.to_string(),
                None => format!("section-{}", book.chapters.iter().position(|c| c.path == t.target).unwrap_or(0)),
            };
            TocItem { title: t.title.clone(), level: t.level, anchor }
        })
        .collect();
    state.books.lock().unwrap().insert(id.clone(), dir);
    Ok(Book { path: format!("epub/{}/{}", enc(&id), rel.join("/")), toc })
}

fn enc(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[tauri::command]
async fn annotations(state: State<'_, AppState>, id: String) -> R<Vec<Annotation>> {
    Ok(state.lib()?.annotations(id))
}

#[derive(Deserialize)]
struct Selection {
    quote: String,
    prefix: String,
    suffix: String,
    start: i64,
}

/// A new highlight from a selection in the reading view (LibraryModel.addHighlight).
#[tauri::command]
async fn add_highlight(state: State<'_, AppState>, id: String, selection: Selection, color: String, note: String) -> R<Annotation> {
    let a = Annotation {
        id: String::new(),
        paper_id: id,
        kind: if note.trim().is_empty() { AnnotationKind::Highlight } else { AnnotationKind::Note },
        color,
        location: "epub".into(),
        position: selection.start.to_string(),
        quote: selection.quote,
        prefix: selection.prefix,
        suffix: selection.suffix,
        note,
        created: 0,
        modified: 0,
    };
    state.lib()?.save_annotation(a).map_err(err)
}

/// Saves an edited highlight (colour, note); a note makes it a Note.
#[tauri::command]
async fn save_annotation(state: State<'_, AppState>, mut annotation: Annotation) -> R<Annotation> {
    annotation.kind = if annotation.note.trim().is_empty() { AnnotationKind::Highlight } else { AnnotationKind::Note };
    state.lib()?.save_annotation(annotation).map_err(err)
}

#[tauri::command]
async fn delete_annotation(state: State<'_, AppState>, paper: String, id: String) -> R<()> {
    state.lib()?.delete_annotation(paper, id).map_err(err)
}

#[tauri::command]
async fn position_latest(state: State<'_, AppState>, id: String) -> R<Option<positions::Latest>> {
    Ok(positions::latest(&state.root()?, &state.device(), &id))
}

#[tauri::command]
async fn position_save(state: State<'_, AppState>, id: String, offset: i64) -> R<()> {
    positions::save(&state.root()?, &state.device(), settings::device_label(), &id, offset).map_err(err)
}

// ---- the lunta:// scheme ----

const BRIDGE: &str = include_str!("../../src/reader/bridge.js");
const READER: [(&str, &str); 7] = [
    ("style.js", include_str!("../../src/reader/generated/style.js")),
    ("tracking.js", include_str!("../../src/reader/generated/tracking.js")),
    ("narration.js", include_str!("../../src/reader/generated/narration.js")),
    ("annotation.js", include_str!("../../src/reader/generated/annotation.js")),
    ("position.js", include_str!("../../src/reader/generated/position.js")),
    ("paged.js", include_str!("../../src/reader/generated/paged.js")),
    ("reader.css", include_str!("../../src/reader/generated/reader.css")),
];

/// Script tags added at the end of the continuous document, in the order the apps add their
/// user scripts (all at document end): the bridge first, so `window.webkit.messageHandlers` exists.
fn injected() -> String {
    let mut s = String::from("<script src=\"/reader/bridge.js\"></script>\n");
    for (name, _) in READER.iter().filter(|(n, _)| n.ends_with(".js")) {
        s += &format!("<script src=\"/reader/{name}\"></script>\n");
    }
    s
}

fn mime(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "xhtml" | "xht" => "application/xhtml+xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "otf" => "font/otf",
        "ttf" => "font/ttf",
        "mml" => "application/mathml+xml",
        _ => "application/octet-stream",
    }
}

fn respond(status: StatusCode, ctype: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::CACHE_CONTROL, "no-store")
        .body(body)
        .unwrap()
}

fn serve(app: &AppHandle, path: &str) -> Response<Vec<u8>> {
    let not_found = || respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec());
    let parts: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .map(|p| percent_encoding::percent_decode_str(p).decode_utf8_lossy().to_string())
        .collect();
    match parts.first().map(String::as_str) {
        Some("reader") if parts.len() == 2 => {
            if parts[1] == "bridge.js" {
                return respond(StatusCode::OK, "text/javascript", BRIDGE.as_bytes().to_vec());
            }
            match READER.iter().find(|(n, _)| *n == parts[1]) {
                Some((n, body)) => respond(StatusCode::OK, mime(n), body.as_bytes().to_vec()),
                None => not_found(),
            }
        }
        Some("epub") if parts.len() >= 3 => {
            // only files inside a book the app opened, never anything above it
            if parts[2..].iter().any(|p| p.is_empty() || p == ".." || p == "." || p.contains('\\')) {
                return not_found();
            }
            let Some(dir) = app.state::<AppState>().books.lock().unwrap().get(&parts[1]).cloned() else { return not_found() };
            let file = parts[2..].iter().fold(dir, |acc, p| acc.join(p));
            let Ok(mut data) = std::fs::read(&file) else { return not_found() };
            let name = parts.last().unwrap();
            if name == "all.xhtml" {
                let text = String::from_utf8_lossy(&data).to_string();
                if let Some(i) = text.rfind("</body>") {
                    data = format!("{}{}{}", &text[..i], injected(), &text[i..]).into_bytes();
                }
            }
            respond(StatusCode::OK, mime(name), data)
        }
        _ => not_found(),
    }
}

/// Debug builds only: with LUNTA_DEV_EVAL_DIR set, every `*.js` file put in that folder is run
/// in the main window (then deleted), and `dev_log` appends to `log.txt` there. Lets the app be
/// driven by scripts in development; compiled out of release builds.
#[cfg(debug_assertions)]
fn dev_hook(app: &AppHandle) {
    let Ok(dir) = std::env::var("LUNTA_DEV_EVAL_DIR") else { return };
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "js")).collect();
        files.sort();
        for f in files {
            if let Ok(code) = std::fs::read_to_string(&f) {
                let _ = std::fs::remove_file(&f);
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.eval(&code);
                }
            }
        }
    });
}

#[tauri::command]
fn dev_log(msg: String) {
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("LUNTA_DEV_EVAL_DIR") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(Path::new(&dir).join("log.txt")) {
            let _ = writeln!(f, "{msg}");
        }
    }
    let _ = msg;
}

#[tauri::command]
fn dev_snapshot(app: AppHandle, path: String) -> R<()> {
    #[cfg(all(debug_assertions, target_os = "macos"))]
    if std::env::var("LUNTA_DEV_EVAL_DIR").is_ok() {
        return devsnap::snapshot(&app, path);
    }
    let _ = (app, path);
    Err("snapshots are for debug builds on macOS".into())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("lunta", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(serve(&app, &path)));
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let cache_root = app.path().app_cache_dir()?;
            let s = settings::load(&config_dir);
            let state = AppState {
                config_dir,
                cache_root,
                settings: Mutex::new(s.clone()),
                open: Mutex::new(None),
                books: Mutex::new(HashMap::new()),
                open_error: Mutex::new(None),
            };
            if let Some(lib) = &s.library {
                if let Err(e) = state.open_library(Path::new(lib)) {
                    *state.open_error.lock().unwrap() = Some(e);
                }
            }
            app.manage(state);
            // the window starts hidden (no white flash while the page loads); scripted debug
            // runs keep it hidden so they don't pop up over other work (LUNTA_DEV_SHOW=1 shows it)
            let scripted = cfg!(debug_assertions) && std::env::var("LUNTA_DEV_EVAL_DIR").is_ok() && std::env::var("LUNTA_DEV_SHOW").is_err();
            if !scripted {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                }
            }
            #[cfg(debug_assertions)]
            dev_hook(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            choose_library,
            refresh,
            sidebar,
            list,
            paper,
            smart_folders,
            save_smart_folder,
            delete_smart_folder,
            smart_count,
            update_meta,
            lookup_meta,
            set_read,
            set_starred,
            set_must_read,
            mark_opened,
            set_tag,
            set_membership,
            create_folder,
            rename_folder,
            move_folder,
            delete_folder,
            delete_paper,
            cite,
            add_paper,
            attach_pdf,
            make_reading_view,
            find_pdf,
            pdf_bytes,
            open_book,
            annotations,
            add_highlight,
            save_annotation,
            delete_annotation,
            position_latest,
            position_save,
            dev_log,
            dev_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lunta");
}
