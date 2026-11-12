//! Lunta core: the shared library engine for the Mac and iPhone apps.
//!
//! The Swift apps own the UI and the PDF/JATS → EPUB conversion (PDFKit); this crate owns the
//! library: storage and sync layout, metadata lookup, annotations, full-text search and export.
//! `convert` (feature "convert") is a Rust port of that converter for future non-Apple apps;
//! it is not exported through UniFFI.

uniffi::setup_scaffolding!();

#[cfg(feature = "convert")]
pub mod convert;
pub mod epub;
pub mod export;
pub mod import;
pub mod index;
pub mod metadata;
pub mod model;
pub mod store;

use model::*;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use store::{OpKind, State, Store};

struct Inner {
    store: Store,
    state: State,
    index: index::Index,
    cache: PathBuf,
    /// paper id -> (has PDF, has reading view); refreshed by reindex(), avoids disk checks per listing
    files: std::collections::HashMap<String, (bool, bool, bool)>,
}

/// A paper library rooted at a folder (local or in iCloud Drive).
#[derive(uniffi::Object)]
pub struct Library {
    inner: Mutex<Inner>,
}

#[uniffi::export]
impl Library {
    /// `root`: the library folder (e.g. inside the iCloud container). `cache_dir`: device-local
    /// folder for the search index. `device_id`: stable, unique per device.
    #[uniffi::constructor]
    pub fn open(root: String, cache_dir: String, device_id: String) -> Result<Arc<Self>> {
        let mut store = Store::open(&PathBuf::from(&root), &device_id)?;
        let state = store.replay()?;
        let index = index::Index::open(&PathBuf::from(&cache_dir).join("index.sqlite"))?;
        let lib = Library { inner: Mutex::new(Inner { store, state, index, cache: PathBuf::from(&cache_dir), files: Default::default() }) };
        lib.reindex_quick()?;
        Ok(Arc::new(lib))
    }

    /// Picks up changes synced from other devices. Returns true if anything changed.
    pub fn refresh(&self) -> Result<bool> {
        {
            let mut g = self.inner.lock().unwrap();
            if !g.store.needs_replay()? {
                return Ok(false);
            }
            g.state = g.store.replay()?;
        }
        self.reindex_quick()?;
        Ok(true)
    }

    /// Indexes metadata and notes for new or changed papers without reading full texts (which
    /// may still be in iCloud). Full texts are added later by `index_fulltext`.
    pub fn reindex_quick(&self) -> Result<u32> {
        let mut g = self.inner.lock().unwrap();
        let ids: Vec<String> = g.state.papers.keys().cloned().collect();
        let mut n = 0;
        for id in &ids {
            let meta = g.state.papers[id].meta.clone();
            let notes = notes_text(&g.state, id);
            let quick = format!("quick-{}", index::signature(&meta, &notes, 0));
            let have = g.index.signature_of(id)?;
            // keep rows that already have full text for the same metadata
            let full_prefix = index::signature(&meta, &notes, 0).split('-').next().unwrap_or("").to_string();
            if have.as_deref() == Some(quick.as_str()) || have.as_deref().map_or(false, |h| h.starts_with(&full_prefix)) {
                continue;
            }
            g.index.upsert(id, &meta, &notes, "", &quick)?;
            n += 1;
        }
        let files = ids
            .iter()
            .map(|id| (id.clone(), (g.store.has_file(id, FileKind::Pdf), g.store.has_file(id, FileKind::Epub), g.store.has_file(id, FileKind::AudioIndex))))
            .collect();
        g.files = files;
        let keep: BTreeSet<String> = ids.into_iter().collect();
        g.index.remove_missing(&keep)?;
        Ok(n)
    }

    /// Adds full texts to the search index, at most `max` papers per call (reading a file may wait
    /// for iCloud to download it, so call this off the main thread, repeatedly). Returns how many
    /// papers it indexed; 0 means the index is complete.
    pub fn index_fulltext(&self, max: u32) -> Result<u32> {
        let todo: Vec<(String, PaperMeta, String, PathBuf, u64)> = {
            let g = self.inner.lock().unwrap();
            let mut v = vec![];
            for (id, p) in &g.state.papers {
                let notes = notes_text(&g.state, id);
                let path = g.store.file_path(id, FileKind::Fulltext);
                let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                if len == 0 {
                    continue;
                }
                let sig = index::signature(&p.meta, &notes, len);
                if g.index.signature_of(id)?.as_deref() != Some(sig.as_str()) {
                    v.push((id.clone(), p.meta.clone(), notes, path, len));
                    if v.len() as u32 >= max {
                        break;
                    }
                }
            }
            v
        };
        let mut n = 0;
        for (id, meta, notes, path, len) in todo {
            // read without holding the lock: this may block while iCloud downloads the file
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let mut g = self.inner.lock().unwrap();
            g.index.upsert(&id, &meta, &notes, &text, &index::signature(&meta, &notes, len))?;
            n += 1;
        }
        Ok(n)
    }

    /// Updates search-index rows whose metadata, notes or full text changed (incremental).
    pub fn reindex(&self) -> Result<u32> {
        let mut g = self.inner.lock().unwrap();
        let ids: Vec<String> = g.state.papers.keys().cloned().collect();
        let mut n = 0;
        for id in &ids {
            let meta = g.state.papers[id].meta.clone();
            let notes = notes_text(&g.state, id);
            let ft_path = g.store.file_path(id, FileKind::Fulltext);
            let ft_len = std::fs::metadata(&ft_path).map(|m| m.len()).unwrap_or(0);
            let sig = index::signature(&meta, &notes, ft_len);
            if g.index.signature_of(id)?.as_deref() == Some(sig.as_str()) {
                continue;
            }
            let fulltext = std::fs::read_to_string(&ft_path).unwrap_or_default();
            g.index.upsert(id, &meta, &notes, &fulltext, &sig)?;
            n += 1;
        }
        let files = ids
            .iter()
            .map(|id| (id.clone(), (g.store.has_file(id, FileKind::Pdf), g.store.has_file(id, FileKind::Epub), g.store.has_file(id, FileKind::AudioIndex))))
            .collect();
        g.files = files;
        let keep: BTreeSet<String> = ids.into_iter().collect();
        g.index.remove_missing(&keep)?;
        Ok(n)
    }

    // ---- papers ----

    pub fn papers(&self, filter: PaperFilter, sort: SortKey) -> Vec<Paper> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<Paper> = g
            .state
            .papers
            .keys()
            .map(|id| paper_of(&g, id))
            .filter(|p| {
                filter.collection.as_ref().map_or(true, |c| p.collections.contains(c))
                    && filter.tag.as_ref().map_or(true, |t| p.tags.contains(t))
                    && (!filter.starred_only || p.starred)
                    && (!filter.unread_only || !p.read)
            })
            .collect();
        match sort {
            SortKey::Added => v.sort_by(|a, b| b.added.cmp(&a.added)),
            SortKey::Year => v.sort_by(|a, b| b.meta.date.cmp(&a.meta.date)),
            SortKey::Title => v.sort_by_key(|p| p.meta.title.to_lowercase()),
            SortKey::FirstAuthor => v.sort_by_key(|p| {
                p.meta.authors.first().map(|a| export::split_name(a).0.to_lowercase()).unwrap_or_default()
            }),
        }
        v
    }

    pub fn paper(&self, id: String) -> Option<Paper> {
        let g = self.inner.lock().unwrap();
        g.state.papers.contains_key(&id).then(|| paper_of(&g, &id))
    }

    pub fn find_by_doi(&self, doi: String) -> Option<Paper> {
        let want = metadata::normalize_doi(&doi);
        if want.is_empty() {
            return None;
        }
        let g = self.inner.lock().unwrap();
        let id = g.state.papers.iter().find(|(_, p)| metadata::normalize_doi(&p.meta.doi) == want)?.0.clone();
        Some(paper_of(&g, &id))
    }

    /// Adds a paper (or, if one with the same DOI exists, attaches the new files to it).
    pub fn add_paper(&self, meta: PaperMeta, pdf: Option<Vec<u8>>, epub: Option<Vec<u8>>, fulltext: Option<String>) -> Result<Paper> {
        let existing = self.find_by_doi(meta.doi.clone()).map(|p| p.id);
        let id = existing.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        {
            let mut g = self.inner.lock().unwrap();
            if let Some(d) = &pdf {
                g.store.write_file(&id, FileKind::Pdf, d)?;
            }
            if let Some(d) = &epub {
                g.store.write_file(&id, FileKind::Epub, d)?;
            }
            if let Some(t) = &fulltext {
                g.store.write_file(&id, FileKind::Fulltext, t.as_bytes())?;
            }
            let op = if existing.is_some() {
                // keep the richer metadata
                let old = g.state.papers[&id].meta.clone();
                let mut m = old.clone();
                merge_meta(&mut m, &meta);
                g.store.append(OpKind::SetMeta { paper: id.clone(), meta: m })?
            } else {
                g.store.append(OpKind::AddPaper { paper: id.clone(), meta })?
            };
            apply_local(&mut g, &op);
        }
        self.reindex()?;
        Ok(self.paper(id.clone()).ok_or(CoreError::NotFound(id))?)
    }

    pub fn attach_file(&self, id: String, kind: FileKind, data: Vec<u8>) -> Result<()> {
        {
            let g = self.inner.lock().unwrap();
            if !g.state.papers.contains_key(&id) {
                return Err(CoreError::NotFound(id));
            }
            g.store.write_file(&id, kind, &data)?;
        }
        self.reindex()?;
        Ok(())
    }

    /// Where a paper's file is: on this device, only in iCloud (not downloaded yet), or nowhere.
    pub fn file_status(&self, id: String, kind: FileKind) -> FileStatus {
        let g = self.inner.lock().unwrap();
        if g.store.file_path(&id, kind).exists() {
            FileStatus::Local
        } else if g.store.has_file(&id, kind) {
            FileStatus::InCloud
        } else {
            FileStatus::Missing
        }
    }

    /// Absolute path the file has (or will have once downloaded) — for asking iCloud to download it.
    pub fn expected_file_path(&self, id: String, kind: FileKind) -> String {
        self.inner.lock().unwrap().store.file_path(&id, kind).to_string_lossy().to_string()
    }

    /// Absolute path of a paper's file, if it is on this device.
    pub fn file_path(&self, id: String, kind: FileKind) -> Option<String> {
        let g = self.inner.lock().unwrap();
        let p = g.store.file_path(&id, kind);
        p.exists().then(|| p.to_string_lossy().to_string())
    }

    pub fn update_meta(&self, id: String, meta: PaperMeta) -> Result<()> {
        self.op(OpKind::SetMeta { paper: id, meta })?;
        self.reindex()?;
        Ok(())
    }

    pub fn set_read(&self, id: String, read: bool) -> Result<()> {
        self.op(OpKind::SetRead { paper: id, read })
    }

    pub fn set_starred(&self, id: String, starred: bool) -> Result<()> {
        self.op(OpKind::SetStarred { paper: id, starred })
    }

    /// Removes a paper and its files.
    pub fn delete_paper(&self, id: String) -> Result<()> {
        self.op(OpKind::DeletePaper { paper: id.clone() })?;
        let g = self.inner.lock().unwrap();
        let _ = std::fs::remove_dir_all(g.store.paper_dir(&id));
        Ok(())
    }

    // ---- tags & collections ----

    pub fn tags(&self) -> Vec<String> {
        let g = self.inner.lock().unwrap();
        let all: BTreeSet<String> = g.state.papers.values().flat_map(|p| p.tags.iter().cloned()).collect();
        all.into_iter().collect()
    }

    pub fn set_tag(&self, id: String, tag: String, present: bool) -> Result<()> {
        let tag = tag.trim().to_string();
        if tag.is_empty() {
            return Err(CoreError::Invalid("empty tag".into()));
        }
        self.op(OpKind::Tag { paper: id, tag, present })
    }

    pub fn collections(&self) -> Vec<PaperCollection> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<PaperCollection> = g
            .state
            .collections
            .iter()
            .map(|(id, (name, parent))| PaperCollection {
                id: id.clone(),
                name: name.clone(),
                parent: parent.clone(),
                paper_count: g.state.papers.values().filter(|p| p.collections.contains(id)).count() as u32,
            })
            .collect();
        v.sort_by_key(|c| c.name.to_lowercase());
        v
    }

    pub fn create_collection(&self, name: String, parent: Option<String>) -> Result<PaperCollection> {
        let id = uuid::Uuid::new_v4().to_string();
        self.op(OpKind::CollectionUpsert { id: id.clone(), name: name.clone(), parent: parent.clone() })?;
        Ok(PaperCollection { id, name, parent, paper_count: 0 })
    }

    pub fn rename_collection(&self, id: String, name: String) -> Result<()> {
        let parent = {
            let g = self.inner.lock().unwrap();
            g.state.collections.get(&id).ok_or(CoreError::NotFound(id.clone()))?.1.clone()
        };
        self.op(OpKind::CollectionUpsert { id, name, parent })
    }

    /// Moves a collection under another one (None = top level). Refuses to move it into itself
    /// or one of its own subcollections.
    pub fn move_collection(&self, id: String, parent: Option<String>) -> Result<()> {
        let name = {
            let g = self.inner.lock().unwrap();
            let name = g.state.collections.get(&id).ok_or(CoreError::NotFound(id.clone()))?.0.clone();
            let mut cur = parent.clone();
            while let Some(c) = cur {
                if c == id {
                    return Err(CoreError::Invalid("a folder can't go inside itself".into()));
                }
                cur = g.state.collections.get(&c).and_then(|x| x.1.clone());
            }
            name
        };
        self.op(OpKind::CollectionUpsert { id, name, parent })
    }

    pub fn delete_collection(&self, id: String) -> Result<()> {
        self.op(OpKind::CollectionDelete { id })
    }

    pub fn set_membership(&self, collection: String, paper: String, present: bool) -> Result<()> {
        self.op(OpKind::Member { collection, paper, present })
    }

    // ---- annotations ----

    pub fn annotations(&self, paper: String) -> Vec<Annotation> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<Annotation> = g.state.annotations.get(&paper).map(|m| m.values().cloned().collect()).unwrap_or_default();
        v.sort_by(|a, b| (&a.location, a.created).cmp(&(&b.location, b.created)));
        v
    }

    /// Creates or updates an annotation. Leave `id` empty to create; timestamps are set here.
    pub fn save_annotation(&self, mut a: Annotation) -> Result<Annotation> {
        let now = now_ms();
        if a.id.is_empty() {
            a.id = uuid::Uuid::new_v4().to_string();
            a.created = now;
        }
        a.modified = now;
        self.op(OpKind::AnnotationUpsert { annotation: a.clone() })?;
        self.reindex()?;
        Ok(a)
    }

    pub fn delete_annotation(&self, paper: String, id: String) -> Result<()> {
        self.op(OpKind::AnnotationDelete { paper, id })?;
        self.reindex()?;
        Ok(())
    }

    // ---- search & export ----

    pub fn search(&self, query: String, limit: u32) -> Result<Vec<SearchHit>> {
        let g = self.inner.lock().unwrap();
        let hits = g.index.search(&query, limit)?;
        Ok(hits
            .into_iter()
            .filter(|(id, _, _)| g.state.papers.contains_key(id))
            .map(|(id, snippet, score)| SearchHit { paper: paper_of(&g, &id), snippet, score })
            .collect())
    }

    pub fn export_bibtex(&self, ids: Vec<String>) -> String {
        export::bibtex(&self.collect(ids))
    }

    pub fn export_ris(&self, ids: Vec<String>) -> String {
        export::ris(&self.collect(ids))
    }

    pub fn citation(&self, id: String, style: CitationStyle) -> Option<String> {
        self.paper(id).map(|p| export::citation(&p, style))
    }

    // ---- discovering new papers ----

    /// Searches beyond the library (OpenAlex) and marks hits that are already in it (blocking).
    pub fn discover(&self, query: String, limit: u32) -> Result<Vec<metadata::DiscoverHit>> {
        let mut hits = metadata::discover(&query, limit)?;
        for h in hits.iter_mut() {
            h.in_library = self.find_by_doi(h.meta.doi.clone()).or_else(|| self.find_by_title(&h.meta.title)).map(|p| p.id);
        }
        Ok(hits)
    }

    /// New papers for a watch (author, topic, or citing a paper) published since `since` (YYYY-MM-DD).
    pub fn watch_new(&self, kind: metadata::WatchKind, value: String, since: String, limit: u32) -> Result<Vec<metadata::DiscoverHit>> {
        let mut hits = metadata::watch_new(kind, &value, &since, limit)?;
        for h in hits.iter_mut() {
            h.in_library = self.find_by_doi(h.meta.doi.clone()).or_else(|| self.find_by_title(&h.meta.title)).map(|p| p.id);
        }
        Ok(hits)
    }

    // ---- finding PDFs ----

    /// Searches open-access sources for the paper's PDF, downloads the best one and attaches it.
    /// Returns where it came from (e.g. "PubMed Central"), or None if no open copy was found.
    pub fn find_pdf(&self, id: String) -> Result<Option<String>> {
        let p = self.paper(id.clone()).ok_or(CoreError::NotFound(id.clone()))?;
        for c in metadata::pdf_candidates(&p.meta) {
            if let Ok(data) = metadata::download_pdf(&c.url) {
                self.attach_file(id, FileKind::Pdf, data)?;
                return Ok(Some(c.label));
            }
        }
        Ok(None)
    }

    // ---- reading ----

    /// Unpacks the paper's EPUB into the device cache and returns its chapters and contents.
    pub fn open_epub(&self, id: String) -> Result<epub::EpubBook> {
        let (path, cache) = {
            let g = self.inner.lock().unwrap();
            (g.store.file_path(&id, FileKind::Epub), g.cache.join("epub").join(&id))
        };
        if !path.exists() {
            return Err(CoreError::NotFound(format!("no reading view for {id}")));
        }
        epub::open(&path, &cache)
    }

    // ---- import ----

    /// Adds entries from another reference manager. Duplicates (same DOI, or same title when there
    /// is no DOI) are merged into the existing paper. PDFs referenced by the entries are copied in.
    pub fn import_entries(&self, entries: Vec<import::ImportEntry>) -> Result<import::ImportReport> {
        let mut rep = import::ImportReport::default();
        for e in entries {
            if e.meta.title.trim().is_empty() && e.meta.doi.is_empty() {
                rep.skipped += 1;
                continue;
            }
            let existing = self.find_by_doi(e.meta.doi.clone()).or_else(|| self.find_by_title(&e.meta.title));
            let pdf = e.files.iter().map(PathBuf::from).find(|p| p.exists()).and_then(|p| std::fs::read(p).ok());
            if pdf.is_some() {
                rep.pdfs_copied += 1;
            }
            let paper = match &existing {
                Some(p) => {
                    let mut m = p.meta.clone();
                    merge_meta(&mut m, &e.meta);
                    if m != p.meta {
                        self.update_meta(p.id.clone(), m)?;
                    }
                    if let Some(d) = pdf {
                        if !p.has_pdf {
                            self.attach_file(p.id.clone(), FileKind::Pdf, d)?;
                        }
                    }
                    rep.merged += 1;
                    p.clone()
                }
                None => {
                    rep.added += 1;
                    self.add_paper(e.meta.clone(), pdf, None, None)?
                }
            };
            for t in &e.tags {
                let _ = self.set_tag(paper.id.clone(), t.clone(), true);
            }
            rep.paper_ids.push(paper.id);
        }
        Ok(rep)
    }

    /// iCloud placeholders the app should ask the system to download.
    pub fn pending_downloads(&self) -> Vec<String> {
        self.inner.lock().unwrap().store.placeholders()
    }
}

impl Library {
    fn find_by_title(&self, title: &str) -> Option<Paper> {
        let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        let want = norm(title);
        if want.len() < 12 {
            return None;
        }
        let g = self.inner.lock().unwrap();
        let id = g.state.papers.iter().find(|(_, p)| norm(&p.meta.title) == want)?.0.clone();
        Some(paper_of(&g, &id))
    }

    fn op(&self, kind: OpKind) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        let op = g.store.append(kind)?;
        apply_local(&mut g, &op);
        Ok(())
    }

    fn collect(&self, ids: Vec<String>) -> Vec<Paper> {
        ids.into_iter().filter_map(|i| self.paper(i)).collect()
    }
}

/// Brings the in-memory state up to date after a local write. Replaying every log keeps a single
/// code path for local and synced changes (and picks up anything synced meanwhile); it is fast
/// for libraries of thousands of papers. Switch to incremental apply if that ever changes.
fn apply_local(g: &mut Inner, _op: &store::Op) {
    if let Ok(st) = g.store.replay() {
        g.state = st;
    }
}

fn notes_text(st: &State, id: &str) -> String {
    st.annotations
        .get(id)
        .map(|m| m.values().map(|a| format!("{} {}", a.quote, a.note)).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn paper_of(g: &Inner, id: &str) -> Paper {
    let p = &g.state.papers[id];
    Paper {
        id: id.to_string(),
        meta: p.meta.clone(),
        added: p.added,
        modified: p.modified,
        has_pdf: g.files.get(id).map(|f| f.0).unwrap_or(false),
        has_epub: g.files.get(id).map(|f| f.1).unwrap_or(false),
        has_audio: g.files.get(id).map(|f| f.2).unwrap_or(false),
        read: p.read,
        starred: p.starred,
        collections: p.collections.iter().cloned().collect(),
        tags: p.tags.iter().cloned().collect(),
        annotation_count: g.state.annotations.get(id).map(|m| m.len() as u32).unwrap_or(0),
    }
}

fn merge_meta(m: &mut PaperMeta, o: &PaperMeta) {
    macro_rules! f { ($($x:ident),*) => { $( if m.$x.is_empty() { m.$x = o.$x.clone(); } )* } }
    f!(title, authors, date, journal, volume, issue, pages, publisher, doi, pmid, pmcid, arxiv, url, abstract_text, keywords, license);
}

// ---- free functions exported to Swift ----

/// Looks up metadata for a DOI, PMID, PMCID, arXiv ID or a link containing one (blocking; call off the main thread).
#[uniffi::export]
pub fn resolve_metadata(input: String) -> Result<PaperMeta> {
    metadata::resolve(&input)
}

/// Open-access PDF locations for a paper, best first (blocking).
#[uniffi::export]
pub fn find_pdf_candidates(meta: PaperMeta) -> Vec<metadata::PdfCandidate> {
    metadata::pdf_candidates(&meta)
}

/// Extracts a normalised DOI from any text, or "" if there is none.
#[uniffi::export]
pub fn extract_doi(text: String) -> String {
    metadata::doi_in(&text).unwrap_or_default()
}

/// Parses a BibTeX or RIS export (format detected from the content).
#[uniffi::export]
pub fn parse_import(text: String) -> Vec<import::ImportEntry> {
    import::parse_any(&text)
}

#[cfg(test)]
mod tests;
