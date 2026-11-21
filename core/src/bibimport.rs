//! Importing a LaTeX project's .bib with its citation keys: each entry is found (by DOI, arXiv id,
//! or Crossref's bibliographic search, with a title and year check) and added to a folder with the
//! entry's key as the paper's citation key, so `\cite{oldkey}` keeps working in the .bib Lunta
//! keeps for the folder. Entries that aren't papers (software, web pages) are kept as they are
//! with the folder (`OpKind::CollectionBib`) and written into its .bib after the papers. A report
//! lists how each entry was found and where the .bib and the published record differ.

use crate::import::{entry_from_bib, parse_bibtex_raw, RawBibEntry};
use crate::model::*;
use crate::store::OpKind;
use crate::{export, refcite, Library, ProgressListener};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// How a .bib entry was found.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BibFound {
    /// By the DOI in the entry.
    Doi,
    /// By the arXiv id in the entry (eprint, or "arXiv:…" in the journal or note).
    Arxiv,
    /// By Crossref's search for the reference, its title and year checked.
    Search,
    /// Not found online (or offline): added from the entry's own fields.
    AsIs,
    /// Not a paper (software, a web page): kept as written in the folder's .bib.
    Extra,
    /// Neither a title nor a DOI: left out.
    Skipped,
}

/// One entry of the .bib and what became of it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct BibImportItem {
    /// The entry's key in the .bib.
    pub key: String,
    pub found: BibFound,
    /// The paper it is now (empty for extra and skipped entries).
    pub paper_id: String,
    pub title: String,
    /// The paper's citation key now: the entry's, unless it couldn't be kept (see `notes`).
    pub cite_key: String,
    /// Where the .bib and the record found differ ("year: 2023 in the .bib, 2024 in Crossref"),
    /// and problems with the key.
    pub notes: Vec<String>,
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct BibImportReport {
    pub items: Vec<BibImportItem>,
    pub by_doi: u32,
    pub by_arxiv: u32,
    pub by_search: u32,
    pub as_is: u32,
    pub extra: u32,
    pub skipped: u32,
    /// Entries whose key the paper now has.
    pub keys_kept: u32,
    /// Papers added or updated (to make reading views for afterwards).
    pub paper_ids: Vec<String>,
    /// The report as text, for the apps to show or save.
    pub text: String,
}

/// An entry that is cited but isn't a scholarly work: software, a web page, a blog post (a
/// `@misc`/`@online`/`@software` without a DOI, an arXiv id or a journal).
pub fn is_extra(e: &RawBibEntry, meta: &PaperMeta) -> bool {
    // a web page (@online, or a @misc with only an address) stays an extra entry, as written
    if !meta.doi.is_empty() || !meta.arxiv.is_empty() || !(meta.work_type.is_empty() || crate::worktype::is_web(&meta.work_type)) {
        return false;
    }
    let kind = e.kind.as_str();
    matches!(kind, "misc" | "online" | "electronic" | "www" | "webpage" | "software" | "manual" | "codefragment")
        && e.get("journal").is_empty()
        && e.get("booktitle").is_empty()
}

/// The entry as a reference, for Crossref's bibliographic search.
fn reference_text(m: &PaperMeta) -> String {
    let authors: Vec<String> = m.authors.iter().take(6).map(|a| export::split_name(a).0).collect();
    [authors.join(", "), m.title.clone(), m.journal.clone(), m.year()].iter().filter(|x| !x.trim().is_empty()).cloned().collect::<Vec<_>>().join(". ")
}

/// Looks an entry up online: the record and how it was found.
fn lookup_online(bib: &PaperMeta) -> Option<(PaperMeta, BibFound)> {
    let got = |id: &str| crate::resolve_metadata(id.to_string()).ok().filter(|m| !m.title.is_empty());
    if !bib.doi.is_empty() {
        if let Some(m) = got(&bib.doi) {
            return Some((m, BibFound::Doi));
        }
    }
    if !bib.arxiv.is_empty() {
        if let Some(m) = got(&format!("arXiv:{}", bib.arxiv)) {
            return Some((m, BibFound::Arxiv));
        }
    }
    if bib.title.split_whitespace().count() < 2 {
        return None;
    }
    let ids = refcite::resolve_reference(&reference_text(bib)).ok().flatten()?;
    if !ids.doi.is_empty() {
        got(&ids.doi).map(|m| (m, BibFound::Search))
    } else if !ids.arxiv.is_empty() {
        got(&format!("arXiv:{}", ids.arxiv)).map(|m| (m, BibFound::Arxiv))
    } else {
        None
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

fn family(a: &str) -> String {
    norm(&crate::export::split_name(a).0)
}

/// Where the .bib entry and the record found differ: year, title, pages, authors.
fn differences(e: &RawBibEntry, bib: &PaperMeta, found: &PaperMeta) -> Vec<String> {
    let source = if crate::worktype::kind(found) == crate::worktype::Kind::Preprint && !found.arxiv.is_empty() { "arXiv" } else { "Crossref" };
    let mut out = vec![];
    if !bib.year().is_empty() && !found.year().is_empty() && bib.year() != found.year() {
        out.push(format!("year: {} in the .bib, {} in {source}", bib.year(), found.year()));
    }
    if !bib.title.is_empty() && norm(&bib.title) != norm(&found.title) {
        out.push(format!("title: “{}” in the .bib, “{}” in {source}", bib.title, found.title));
    }
    let pages = |p: &str| p.replace(['–', '—'], "-").replace("--", "-").replace(' ', "");
    if !bib.pages.is_empty() && !found.pages.is_empty() && pages(&bib.pages) != pages(&found.pages) {
        out.push(format!("pages: {} in the .bib, {} in {source}", bib.pages, found.pages));
    }
    if !bib.doi.is_empty() && !found.doi.is_empty() && !bib.doi.eq_ignore_ascii_case(&found.doi) {
        out.push(format!("DOI: {} in the .bib (kept), {} in {source}", bib.doi, found.doi));
    }
    let others = has_others(&e.get("author"));
    let (b, f) = (bib.authors.len(), found.authors.len());
    if b > 0 && f > 0 {
        if (!others && b != f) || (others && f < b) {
            let more = if others { " and others" } else { "" };
            out.push(format!("authors: {b}{more} in the .bib, {f} in {source}"));
        } else if family(&bib.authors[0]) != family(&found.authors[0]) {
            out.push(format!("first author: {} in the .bib, {} in {source}", bib.authors[0], found.authors[0]));
        }
    }
    out
}

/// Fields the library has no place for that are kept from the .bib as written (`type = {W3C
/// Recommendation}`, a `note`, an `edition`…).
const KEPT_FIELDS: [&str; 12] =
    ["type", "note", "addendum", "edition", "editor", "address", "organization", "month", "isbn", "issn", "chapter", "language"];

/// An author field with "and others" at the end.
fn has_others(raw_authors: &str) -> bool {
    raw_authors.to_lowercase().split_whitespace().collect::<Vec<_>>().windows(2).any(|w| w == ["and", "others"])
}

/// An author field that is one organization or a name written whole: `{Qwen Team}`, `{Apple}`,
/// "World Wide Web Consortium".
fn corporate(raw_authors: &str, authors: &[String]) -> bool {
    let r = raw_authors.trim();
    let whole = r.starts_with('{') && r.ends_with('}') && {
        // one group: the brace opened first closes last
        let mut depth = 0;
        r.char_indices().all(|(i, c)| {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            depth > 0 || i == r.len() - 1
        })
    };
    authors.len() == 1 && (whole || crate::worktype::is_organization(&authors[0]))
}

/// The fields of the entry the researcher's .bib keeps for BibTeX outputs (see
/// `OpKind::SetBibFields`): an author field that is an organization or ends in "and others"
/// (the record's full list, 60 or 156 names, stays in the library), the booktitle of a conference
/// paper or chapter (with its "(CHI)" or "(ECDL 2009)", which Crossref's lacks), and fields the
/// library has no place for (`KEPT_FIELDS`). As written, LaTeX and all.
pub fn own_fields(e: &RawBibEntry, bib: &PaperMeta) -> Vec<(String, String)> {
    let mut out = vec![];
    if let Some(a) = e.get_raw("author") {
        if has_others(&a) || corporate(&a, &bib.authors) {
            out.push(("author".to_string(), a));
        }
    }
    if matches!(e.kind.as_str(), "inproceedings" | "incollection" | "conference" | "inbook") {
        if let Some(b) = e.get_raw("booktitle").filter(|b| !b.trim().is_empty()) {
            out.push(("booktitle".to_string(), b));
        }
    }
    for f in KEPT_FIELDS {
        if let Some(v) = e.get_raw(f).filter(|v| !v.trim().is_empty()) {
            out.push((f.to_string(), v));
        }
    }
    out
}

/// Given names as letters: "Maurice G." → ["maurice", "g"].
fn given_parts(g: &str) -> Vec<String> {
    g.split(|c: char| c.is_whitespace() || c == '-' || c == '.').map(norm).filter(|p| !p.is_empty()).collect()
}

/// The record's authors with the .bib's given names where those are fuller ("Maurice G. Kendall"
/// for Crossref's "M. G. Kendall", "Daniel S. Weld" for "Daniel Weld"): same family name in the
/// same place, and every given name of the record's an initial or the same name in the .bib's.
fn fuller_names(found: &[String], bib: &[String]) -> Vec<String> {
    found
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let Some(b) = bib.get(i) else { return a.clone() };
            if crate::worktype::is_organization(a) {
                return a.clone();
            }
            let ((ff, fg), (bf, bg)) = (crate::export::split_name(a), crate::export::split_name(b));
            if norm(&ff) != norm(&bf) || fg.is_empty() {
                return a.clone();
            }
            let (fp, bp) = (given_parts(&fg), given_parts(&bg));
            let fits = fp.len() <= bp.len() && fp.iter().zip(&bp).all(|(f, b)| if f.chars().count() == 1 { b.starts_with(f.as_str()) } else { f == b });
            let fuller = bp.iter().map(|p| p.chars().count()).sum::<usize>() > fp.iter().map(|p| p.chars().count()).sum::<usize>();
            if fits && fuller { format!("{} {}", bg.trim(), ff) } else { a.clone() }
        })
        .collect()
}

/// Adds the extra entries to the ones the folder keeps: an entry with the same key (ignoring
/// case) is replaced, others are added after.
fn merge_extras(old: &str, new: &[&RawBibEntry]) -> String {
    let new_keys: HashSet<String> = new.iter().map(|e| e.key.to_lowercase()).collect();
    let mut out: Vec<String> = parse_bibtex_raw(old)
        .into_iter()
        .filter(|e| !new_keys.contains(&e.key.to_lowercase()))
        .map(|e| e.raw.trim().to_string())
        .collect();
    out.extend(new.iter().map(|e| e.raw.trim().to_string()));
    out.join("\n\n")
}

const FOUND_NAMES: [(BibFound, &str); 6] = [
    (BibFound::Doi, "DOI"),
    (BibFound::Arxiv, "arXiv"),
    (BibFound::Search, "Crossref"),
    (BibFound::AsIs, "as is"),
    (BibFound::Extra, "extra"),
    (BibFound::Skipped, "skipped"),
];

fn found_name(f: BibFound) -> &'static str {
    FOUND_NAMES.iter().find(|(x, _)| *x == f).map(|(_, n)| *n).unwrap_or("")
}

/// The report as text: totals, what to check, then every entry.
pub fn report_text(r: &BibImportReport, folder: &str) -> String {
    let found = r.by_doi + r.by_arxiv + r.by_search;
    let papers = found + r.as_is;
    let mut s = format!("Imported {} entries into “{folder}”.\n", r.items.len());
    s += &format!("  {papers} papers: {} found by DOI, {} by arXiv id, {} by searching Crossref; {} added as they are (not found online)\n", r.by_doi, r.by_arxiv, r.by_search, r.as_is);
    if r.extra > 0 {
        s += &format!("  {} entries that aren't papers (software, web pages) kept as they are in the folder's .bib\n", r.extra);
    }
    if r.skipped > 0 {
        s += &format!("  {} entries skipped (no title or DOI)\n", r.skipped);
    }
    let key_problems: Vec<&BibImportItem> = r.items.iter().filter(|i| !i.paper_id.is_empty() && i.cite_key != i.key).collect();
    if key_problems.is_empty() {
        s += &format!("  Citation keys: all {} kept\n", r.keys_kept);
    } else {
        s += &format!("  Citation keys: {} kept, {} not (below)\n", r.keys_kept, key_problems.len());
    }
    let checks: Vec<&BibImportItem> = r.items.iter().filter(|i| !i.notes.is_empty()).collect();
    if !checks.is_empty() {
        s += "\nTo check (the .bib and the record found differ, or the key couldn't be kept):\n";
        for i in checks {
            for n in &i.notes {
                s += &format!("  {}: {n}\n", i.key);
            }
        }
    }
    s += "\nEntries:\n";
    let w = r.items.iter().map(|i| i.key.chars().count()).max().unwrap_or(0);
    for i in &r.items {
        s += &format!("  {:w$}  {:8}  {}\n", i.key, found_name(i.found), i.title);
    }
    s
}

#[uniffi::export]
impl Library {
    /// Imports a LaTeX project's .bib into a folder, keeping its citation keys (see the module).
    /// `online`: look entries up (DOI, arXiv, Crossref search); without it every entry is added
    /// from its own fields. Blocking: call off the main thread.
    pub fn import_bibtex_keep_keys(
        &self,
        text: String,
        collection: String,
        online: bool,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<BibImportReport> {
        let say = |m: String| {
            if let Some(p) = &progress {
                p.on_progress(m);
            }
        };
        if online {
            self.import_bib_with(&text, &collection, &|m: &PaperMeta, n: usize, of: usize| {
                say(format!("Looking up {n} of {of}: {}", m.title));
                lookup_online(m)
            })
        } else {
            self.import_bib_with(&text, &collection, &|_: &PaperMeta, _: usize, _: usize| None)
        }
    }

    /// The text of the .bib Lunta keeps for a folder (or the whole library, `None`): its papers
    /// sorted by citation key, then the extra entries kept from an imported .bib. None when there
    /// is nothing to write yet (an empty library, a folder that isn't there): leave the file as is.
    pub fn kept_bibtex(&self, collection: Option<String>) -> Option<String> {
        let (extras, exists) = {
            let g = self.inner.lock().unwrap();
            let extras = match &collection {
                Some(c) => g.state.collection_bib.get(c).cloned().unwrap_or_default(),
                // the whole library: every folder's, once per key
                None => {
                    let mut seen = HashSet::new();
                    let mut all = vec![];
                    for bib in g.state.collection_bib.values() {
                        for e in parse_bibtex_raw(bib) {
                            if seen.insert(e.key.to_lowercase()) {
                                all.push(e.raw.trim().to_string());
                            }
                        }
                    }
                    all.join("\n\n")
                }
            };
            (extras, collection.as_ref().is_none_or(|c| g.state.collections.contains_key(c)))
        };
        if !exists {
            return None;
        }
        let filter = PaperFilter { collection: collection.clone(), ..Default::default() };
        let mut papers = self.papers(filter, SortKey::Added);
        if papers.is_empty() && collection.is_none() {
            return None;
        }
        papers.sort_by_key(|p| p.cite_key.to_lowercase());
        let mut s = format!("% Kept up to date by Lunta: changes made here are overwritten.\n\n{}", self.export_bibtex(papers.into_iter().map(|p| p.id).collect()));
        if !extras.trim().is_empty() {
            s += "% Entries kept from an imported .bib (not papers in the library):\n\n";
            s += extras.trim();
            s += "\n";
        }
        Some(s)
    }

    /// The extra BibTeX entries kept with a folder (from an imported .bib), as written.
    pub fn collection_bib(&self, collection: String) -> String {
        self.inner.lock().unwrap().state.collection_bib.get(&collection).cloned().unwrap_or_default()
    }

    /// Replaces a folder's extra BibTeX entries (empty: none).
    pub fn set_collection_bib(&self, collection: String, bib: String) -> Result<()> {
        self.op(OpKind::CollectionBib { collection, bib })
    }
}

impl Library {
    /// The import, with `lookup` finding an entry's record (online, or a stand-in in tests).
    pub(crate) fn import_bib_with(
        &self,
        text: &str,
        collection: &str,
        lookup: &dyn Fn(&PaperMeta, usize, usize) -> Option<(PaperMeta, BibFound)>,
    ) -> Result<BibImportReport> {
        let folder = {
            let g = self.inner.lock().unwrap();
            g.state.collections.get(collection).map(|c| c.0.clone()).ok_or_else(|| CoreError::NotFound(format!("folder {collection}")))?
        };
        let entries = parse_bibtex_raw(text);
        let mut r = BibImportReport::default();
        let mut extras: Vec<&RawBibEntry> = vec![];
        // paper id -> the first key it got in this import
        let mut keyed: HashMap<String, String> = HashMap::new();
        let n = entries.len();
        for (i, e) in entries.iter().enumerate() {
            let bib = entry_from_bib(&e.kind, &e.fields).meta;
            let mut item = BibImportItem {
                key: e.key.clone(),
                found: BibFound::AsIs,
                paper_id: String::new(),
                title: bib.title.clone(),
                cite_key: String::new(),
                notes: vec![],
            };
            if is_extra(e, &bib) {
                item.found = BibFound::Extra;
                extras.push(e);
                r.extra += 1;
                r.items.push(item);
                continue;
            }
            if bib.title.trim().is_empty() && bib.doi.is_empty() {
                item.found = BibFound::Skipped;
                r.skipped += 1;
                r.items.push(item);
                continue;
            }
            let (meta, found) = match lookup(&bib, i + 1, n) {
                Some((mut m, how)) => {
                    // a record with the first page only (JSTOR's "81"): the .bib's range ("81-93")
                    if !m.pages.is_empty() && !m.pages.contains('-') && bib.pages.starts_with(&format!("{}-", m.pages)) {
                        m.pages = bib.pages.clone();
                    }
                    item.notes = differences(e, &bib, &m);
                    // a DOI in the .bib is the researcher's: never replaced by what a search found
                    if !bib.doi.is_empty() && !bib.doi.eq_ignore_ascii_case(&m.doi) {
                        if m.url.contains(&m.doi) && !m.doi.is_empty() {
                            m.url = format!("https://doi.org/{}", bib.doi);
                        }
                        m.doi = bib.doi.clone();
                    }
                    m.authors = fuller_names(&m.authors, &bib.authors);
                    // what the .bib has and the record lacks (an LNCS volume, a series)
                    crate::merge_meta(&mut m, &bib);
                    (m, how)
                }
                None => (bib.clone(), BibFound::AsIs),
            };
            item.found = found;
            item.title = meta.title.clone();
            match found {
                BibFound::Doi => r.by_doi += 1,
                BibFound::Arxiv => r.by_arxiv += 1,
                BibFound::Search => r.by_search += 1,
                _ => r.as_is += 1,
            }
            let own = own_fields(e, &bib);
            if own.iter().any(|(k, _)| k == "author") {
                for n in item.notes.iter_mut().filter(|n| n.starts_with("authors:")) {
                    n.push_str(" (BibTeX keeps the .bib's)");
                }
            }
            let (paper, new) = self.add_or_merge(meta, &e.key)?;
            self.set_membership(collection.to_string(), paper.clone(), true)?;
            // the paper's kept fields are added to, never cleared or replaced: they may have come
            // from another folder's .bib, whose export would change
            let mut fields = self.bib_fields(paper.clone());
            for (name, value) in own {
                if !fields.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
                    fields.push(BibField { name, value });
                }
            }
            self.set_bib_fields(paper.clone(), fields)?;
            item.paper_id = paper.clone();
            item.cite_key = self.keep_key(&paper, new, &e.key, &keyed, &mut item.notes);
            if item.cite_key == e.key {
                r.keys_kept += 1;
            }
            keyed.entry(paper.clone()).or_insert_with(|| item.cite_key.clone());
            if !r.paper_ids.contains(&paper) {
                r.paper_ids.push(paper);
            }
            r.items.push(item);
        }
        if !extras.is_empty() {
            // an extra entry's key taken by a paper would be in the .bib twice
            let keys: HashSet<String> = self.inner.lock().unwrap().state.cite_keys.values().map(|k| k.to_lowercase()).collect();
            for item in r.items.iter_mut().filter(|i| i.found == BibFound::Extra) {
                item.cite_key = item.key.clone();
                if keys.contains(&item.key.to_lowercase()) {
                    item.notes.push(format!("a paper in the library also has the key {}: the .bib would have it twice", item.key));
                }
            }
            let old = self.collection_bib(collection.to_string());
            self.set_collection_bib(collection.to_string(), merge_extras(&old, &extras))?;
        }
        r.text = report_text(&r, &folder);
        Ok(r)
    }

    /// The paper with these details: one already in the library (same DOI, or the same title when
    /// there is no DOI, or the paper with the entry's key and title: a short title such as "EPUB
    /// 3.3" imported again) with its details filled in, or a new one. Returns its id, and whether
    /// it is new.
    fn add_or_merge(&self, meta: PaperMeta, key: &str) -> Result<(String, bool)> {
        let keyed = || {
            let id = {
                let g = self.inner.lock().unwrap();
                g.state.cite_keys.iter().find(|(_, k)| k.eq_ignore_ascii_case(key)).map(|(id, _)| id.clone())
            }?;
            self.paper(id).filter(|p| !norm(&meta.title).is_empty() && norm(&p.meta.title) == norm(&meta.title))
        };
        let existing = self
            .find_by_doi(meta.doi.clone())
            .or_else(|| if meta.doi.is_empty() { self.find_by_title(&meta.title).or_else(keyed) } else { None });
        match existing {
            Some(p) => {
                let mut m = p.meta.clone();
                crate::merge_meta(&mut m, &meta);
                if m != p.meta {
                    self.update_meta(p.id.clone(), m)?;
                }
                Ok((p.id, false))
            }
            None => Ok((self.add_paper(meta, None, None, None)?.id, true)),
        }
    }

    /// Gives the paper the .bib's key if it can; returns the key it has. `keyed`: papers that got
    /// a key earlier in this import (two entries for one paper).
    fn keep_key(&self, paper: &str, new: bool, key: &str, keyed: &HashMap<String, String>, notes: &mut Vec<String>) -> String {
        let current = self.paper(paper.to_string()).map(|p| p.cite_key).unwrap_or_default();
        if let Some(first) = keyed.get(paper) {
            notes.push(format!("the same paper as {first} (a paper has one key): cite it as {first}, or edit the document"));
            return current;
        }
        if current == key {
            return current;
        }
        let k = match export::check_cite_key(key) {
            Ok(k) => k,
            Err(e) => {
                notes.push(format!("the key can't be used ({e}); the paper's key is {current}"));
                return current;
            }
        };
        let holder = {
            let g = self.inner.lock().unwrap();
            g.state.cite_keys.iter().find(|(id, other)| id.as_str() != paper && other.to_lowercase() == k.to_lowercase()).map(|(id, _)| id.clone())
        };
        if let Some(other) = holder {
            let t = self.paper(other).map(|p| p.meta.title).unwrap_or_default();
            notes.push(format!("another paper in the library has the key {k} (“{t}”); this one's key is {current}"));
            return current;
        }
        // a paper that was in the library before, with a key that may be in use elsewhere
        let was_fixed = !new && self.inner.lock().unwrap().state.fixed_keys.contains(paper);
        match self.set_cite_key(paper.to_string(), k.clone()) {
            Ok(k) => {
                if was_fixed && !current.is_empty() {
                    notes.push(format!("the paper was already in the library as {current}; its key is now {k}"));
                }
                k
            }
            Err(e) => {
                notes.push(format!("the key couldn't be kept ({e})"));
                current
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (tempfile::TempDir, Arc<Library>) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("lib");
        let cache = d.path().join("cache");
        let l = Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), "test".into()).unwrap();
        (d, l)
    }

    const BIB: &str = r#"
@string{acm = "ACM"}
@misc{zotero,
  author = {{Corporation for Digital Scholarship}},
  title = {Zotero},
  howpublished = {\url{https://www.zotero.org}},
  year = {2025}
}
@article{kendall1938,
  author  = {Kendall, Maurice G.},
  title   = {A New Measure of Rank Correlation},
  journal = {Biometrika},
  volume  = {30},
  number  = {1/2},
  pages   = {81--93},
  year    = {1938},
  doi     = {10.1093/biomet/30.1-2.81}
}
@article{blecher2023nougat,
  author  = {Blecher, Lukas and Cucurull, Guillem and others},
  title   = {Nougat: Neural Optical Understanding for Academic Documents},
  journal = {arXiv preprint arXiv:2308.13418},
  year    = {2023}
}
@inproceedings{lo2020s2orc,
  author    = {Lo, Kyle and Wang, Lucy Lu},
  title     = {{S2ORC}: The Semantic Scholar Open Research Corpus},
  booktitle = {Proceedings of the 58th Annual Meeting of the Association for Computational Linguistics},
  pages     = {4969--4983},
  year      = {2020}
}
@article{qwen3,
  author  = {{Qwen Team}},
  title   = {Qwen3 Technical Report},
  journal = {arXiv preprint arXiv:2505.09388},
  year    = {2025}
}
@techreport{epub33,
  author      = {{World Wide Web Consortium}},
  title       = {{EPUB} 3.3},
  institution = {W3C},
  type        = {W3C Recommendation},
  year        = {2023},
  url         = {https://www.w3.org/TR/epub-33/},
  note        = {Accessed \todo{date}}
}
@techreport{jats,
  author      = {{National Information Standards Organization}},
  title       = {{JATS}: Journal Article Tag Suite, version 1.2},
  institution = {NISO},
  number      = {ANSI/NISO Z39.96-2019},
  year        = {2019}
}
@inproceedings{lopez2009grobid,
  author    = {Lopez, Patrice},
  title     = {{GROBID}: Combining Automatic Bibliographic Data Recognition and Term Extraction for Scholarship Publications},
  booktitle = {Research and Advanced Technology for Digital Libraries (ECDL 2009)},
  series    = {Lecture Notes in Computer Science},
  volume    = {5714},
  pages     = {473--474},
  year      = {2009}
}
"#;

    /// Stands in for the online lookups: Crossref's Kendall (in capitals), arXiv's Nougat, a
    /// Crossref search hit for S2ORC, GROBID as Crossref has it (no volume); JATS not found.
    fn fake(m: &PaperMeta, _: usize, _: usize) -> Option<(PaperMeta, BibFound)> {
        if m.doi == "10.1093/biomet/30.1-2.81" {
            let mut k = PaperMeta {
                title: "A NEW MEASURE OF RANK CORRELATION".into(),
                authors: vec!["M. G. KENDALL".into()],
                date: "1938-06-01".into(),
                journal: "Biometrika".into(),
                volume: "30".into(),
                issue: "1-2".into(),
                pages: "81-93".into(),
                // JSTOR's record of the same paper, as Crossref's search found it
                doi: "10.2307/2332226".into(),
                url: "https://doi.org/10.2307/2332226".into(),
                work_type: "journal-article".into(),
                ..Default::default()
            };
            crate::worktype::tidy(&mut k);
            return Some((k, BibFound::Doi));
        }
        if m.arxiv == "2308.13418" {
            return Some((
                PaperMeta {
                    title: "Nougat: Neural Optical Understanding for Academic Documents".into(),
                    authors: vec!["Lukas Blecher".into(), "Guillem Cucurull".into(), "Thomas Scialom".into(), "Robert Stojnic".into()],
                    date: "2023-08-25".into(),
                    journal: "arXiv".into(),
                    arxiv: "2308.13418".into(),
                    doi: "10.48550/arXiv.2308.13418".into(),
                    work_type: "posted-content".into(),
                    ..Default::default()
                },
                BibFound::Arxiv,
            ));
        }
        if m.arxiv == "2505.09388" {
            return Some((
                PaperMeta {
                    title: "Qwen3 Technical Report".into(),
                    authors: (0..60).map(|i| format!("Author{i} Name{i}")).collect(),
                    date: "2025-05-14".into(),
                    journal: "arXiv".into(),
                    arxiv: "2505.09388".into(),
                    doi: "10.48550/arXiv.2505.09388".into(),
                    work_type: "posted-content".into(),
                    ..Default::default()
                },
                BibFound::Arxiv,
            ));
        }
        if m.title.starts_with("S2ORC") {
            return Some((
                PaperMeta {
                    title: "S2ORC: The Semantic Scholar Open Research Corpus".into(),
                    authors: vec!["Kyle Lo".into(), "Lucy Lu Wang".into(), "Mark Neumann".into(), "Rodney Kinney".into(), "Daniel Weld".into()],
                    date: "2020".into(),
                    journal: "Proceedings of the 58th Annual Meeting of the Association for Computational Linguistics".into(),
                    pages: "4969-4983".into(),
                    doi: "10.18653/v1/2020.acl-main.447".into(),
                    work_type: "proceedings-article".into(),
                    publisher: "Association for Computational Linguistics".into(),
                    ..Default::default()
                },
                BibFound::Search,
            ));
        }
        if m.title.starts_with("GROBID") {
            return Some((
                PaperMeta {
                    title: "GROBID: Combining Automatic Bibliographic Data Recognition and Term Extraction for Scholarship Publications".into(),
                    authors: vec!["Patrice Lopez".into()],
                    date: "2009".into(),
                    journal: "Research and Advanced Technology for Digital Libraries".into(),
                    series: "Lecture Notes in Computer Science".into(),
                    pages: "473-474".into(),
                    publisher: "Springer Berlin Heidelberg".into(),
                    doi: "10.1007/978-3-642-04346-8_62".into(),
                    work_type: "book-chapter".into(),
                    ..Default::default()
                },
                BibFound::Search,
            ));
        }
        None
    }

    #[test]
    fn bib_import_keeps_keys_and_reports() {
        let (_d, l) = lib();
        let c = l.create_collection("Lunta paper".into(), None).unwrap();
        let r = l.import_bib_with(BIB, &c.id, &fake).unwrap();
        assert_eq!((r.by_doi, r.by_arxiv, r.by_search, r.as_is, r.extra, r.skipped), (1, 2, 2, 2, 1, 0), "{}", r.text);
        assert_eq!(r.keys_kept, 7, "{}", r.text);
        let by_key = |k: &str| r.items.iter().find(|i| i.key == k).unwrap().clone();
        // the paper keeps the .bib's key, also for \cite
        let kendall = by_key("kendall1938");
        assert_eq!(l.paper(kendall.paper_id.clone()).unwrap().cite_key, "kendall1938");
        assert_eq!(l.latex_cite(vec![kendall.paper_id.clone()]), "\\cite{kendall1938}");
        // Crossref's capitals fixed on the way in; issue 1-2 vs 1/2 isn't a difference we report
        assert_eq!(l.paper(kendall.paper_id.clone()).unwrap().meta.title, "A New Measure of Rank Correlation");
        // the .bib's DOI is kept, not the one the lookup found (reported); its fuller given names too
        assert_eq!(kendall.notes, vec!["DOI: 10.1093/biomet/30.1-2.81 in the .bib (kept), 10.2307/2332226 in Crossref".to_string()]);
        let km = l.paper(kendall.paper_id.clone()).unwrap().meta;
        assert_eq!((km.doi.as_str(), km.url.as_str()), ("10.1093/biomet/30.1-2.81", "https://doi.org/10.1093/biomet/30.1-2.81"));
        assert_eq!(km.authors, vec!["Maurice G. Kendall".to_string()]);
        // Qwen: the library has arXiv's 60 authors, BibTeX the .bib's {Qwen Team}
        let qwen = by_key("qwen3");
        assert_eq!(l.paper(qwen.paper_id.clone()).unwrap().meta.authors.len(), 60);
        assert_eq!(qwen.notes, vec!["authors: 1 in the .bib, 60 in arXiv (BibTeX keeps the .bib's)".to_string()]);
        // "and others" in the .bib: 4 authors in arXiv is not a mismatch; the year is the same
        assert!(by_key("blecher2023nougat").notes.is_empty(), "{:?}", by_key("blecher2023nougat").notes);
        // S2ORC: 2 authors in the .bib, 5 found
        assert_eq!(by_key("lo2020s2orc").notes, vec!["authors: 2 in the .bib, 5 in Crossref".to_string()]);
        // GROBID keeps the .bib's LNCS volume, which Crossref lacks
        let grobid = l.paper(by_key("lopez2009grobid").paper_id).unwrap();
        assert_eq!(grobid.meta.volume, "5714");
        assert_eq!(grobid.meta.series, "Lecture Notes in Computer Science");
        // JATS: not found, added from the entry
        let jats = l.paper(by_key("jats").paper_id).unwrap();
        assert_eq!((jats.cite_key.as_str(), jats.meta.work_type.as_str(), jats.meta.publisher.as_str()), ("jats", "report", "NISO"));
        // all the papers are in the folder
        assert_eq!(l.papers(PaperFilter { collection: Some(c.id.clone()), ..Default::default() }, SortKey::Added).len(), 7);

        // the folder's .bib: papers by key with the fixed exporter, then the extra entry as written
        let bib = l.kept_bibtex(Some(c.id.clone())).unwrap();
        assert!(bib.contains("@inproceedings{lopez2009grobid,"), "{bib}");
        // the .bib's booktitle, with its "(ECDL 2009)"
        assert!(bib.contains("  booktitle = {Research and Advanced Technology for Digital Libraries (ECDL 2009)},"), "{bib}");
        assert!(bib.contains("  series = {Lecture Notes in Computer Science},"), "{bib}");
        assert!(bib.contains("@misc{blecher2023nougat,") && bib.contains("howpublished = {arXiv preprint arXiv:2308.13418}"), "{bib}");
        assert!(bib.contains("@article{kendall1938,\n  title = {A New Measure of Rank Correlation},\n  author = {Kendall, Maurice G.},"), "{bib}");
        assert!(bib.contains("doi = {10.1093/biomet/30.1-2.81}") && !bib.contains("2332226"), "{bib}");
        assert!(bib.contains("@misc{qwen3,\n  title = {{Qwen3} Technical Report},\n  author = {{Qwen Team}},"), "{bib}");
        // fields the library has no place for, as written
        assert!(bib.contains("@techreport{epub33,\n  title = {{EPUB} 3.3},\n  author = {{World Wide Web Consortium}},"), "{bib}");
        assert!(bib.contains("  type = {W3C Recommendation},\n  note = {Accessed \\todo{date}},\n}"), "{bib}");
        assert!(bib.contains("@techreport{jats,") && bib.contains("author = {{National Information Standards Organization}}"), "{bib}");
        assert!(bib.contains("@misc{zotero,\n  author = {{Corporation for Digital Scholarship}},"), "{bib}");
        // every key once
        let reparsed = parse_bibtex_raw(&bib);
        let keys: Vec<&str> = reparsed.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["blecher2023nougat", "epub33", "jats", "kendall1938", "lo2020s2orc", "lopez2009grobid", "qwen3", "zotero"]);
        // the whole library's .bib has the folder's extra entries too
        assert!(l.kept_bibtex(None).unwrap().contains("@misc{zotero,"));
        assert!(r.text.contains("8 entries into “Lunta paper”"), "{}", r.text);
        assert!(r.text.contains("lo2020s2orc: authors: 2 in the .bib, 5 in Crossref"), "{}", r.text);

        // importing again: same papers, same keys, extras not doubled
        let again = l.import_bib_with(BIB, &c.id, &fake).unwrap();
        assert_eq!(again.keys_kept, 7, "{}", again.text);
        assert_eq!(l.papers(PaperFilter::default(), SortKey::Added).len(), 7);
        assert_eq!(parse_bibtex_raw(&l.collection_bib(c.id.clone())).len(), 1);
    }

    #[test]
    fn bib_import_key_conflicts() {
        let (_d, l) = lib();
        let c = l.create_collection("Thesis".into(), None).unwrap();
        // a paper already has the key "kendall1938"
        let other = l
            .add_paper(PaperMeta { title: "Another paper entirely".into(), authors: vec!["A B".into()], date: "2001".into(), ..Default::default() }, None, None, None)
            .unwrap();
        l.set_cite_key(other.id.clone(), "kendall1938".into()).unwrap();
        let bib = "@article{kendall1938, title={A New Measure of Rank Correlation}, author={Kendall, M. G.}, journal={Biometrika}, year={1938}}\n\
                   @article{kendall-again, title={A New Measure of Rank Correlation}, author={Kendall, M. G.}, journal={Biometrika}, year={1938}}\n\
                   @article{bad key, title={Some other title here}, author={X, Y}, year={2000}}";
        let r = l.import_bib_with(bib, &c.id, &|_: &PaperMeta, _: usize, _: usize| None).unwrap();
        assert_eq!(r.as_is, 3);
        assert_eq!(r.keys_kept, 0, "{}", r.text);
        assert!(r.items[0].notes[0].starts_with("another paper in the library has the key kendall1938"), "{:?}", r.items[0].notes);
        assert!(r.items[1].notes[0].starts_with("the same paper as"), "{:?}", r.items[1].notes);
        // "bad key" has a space: parsed as the key "bad key", which can't be a key
        assert!(r.items[2].notes[0].contains("can't be used"), "{:?}", r.items[2].notes);
        assert!(r.text.contains("Citation keys: 0 kept, 3 not"), "{}", r.text);
        // the other paper keeps its key
        assert_eq!(l.paper(other.id).unwrap().cite_key, "kendall1938");
    }

    #[test]
    fn a_second_bib_keeps_the_fields_another_bib_gave() {
        let (_d, l) = lib();
        let thesis = l.create_collection("Thesis".into(), None).unwrap();
        let grant = l.create_collection("Grant".into(), None).unwrap();
        let none = |_: &PaperMeta, _: usize, _: usize| None;
        let first = "@techreport{epub33, title={{EPUB} 3.3}, author={{World Wide Web Consortium}}, institution={W3C}, type={W3C Recommendation}, note={Accessed today}, year={2023}, doi={10.9999/epub33}}";
        let r = l.import_bib_with(first, &thesis.id, &none).unwrap();
        let id = r.items[0].paper_id.clone();
        let before = l.bib_fields(id.clone());
        assert!(before.iter().any(|f| f.name == "note"), "{before:?}");
        let thesis_bib = l.kept_bibtex(Some(thesis.id.clone())).unwrap();
        // the same paper in another folder's .bib, without the note or type, with a month
        let second = "@techreport{w3c-epub, title={EPUB 3.3}, author={{World Wide Web Consortium}}, institution={W3C}, year={2023}, month=5, doi={10.9999/EPUB33}}";
        let r2 = l.import_bib_with(second, &grant.id, &none).unwrap();
        assert_eq!(r2.items[0].paper_id, id, "the same paper");
        let after = l.bib_fields(id.clone());
        for f in &before {
            assert!(after.contains(f), "{f:?} kept: {after:?}");
        }
        assert!(after.iter().any(|f| f.name == "month"), "the new field added: {after:?}");
        assert!(l.kept_bibtex(Some(thesis.id.clone())).unwrap().contains("note = {Accessed today}"), "{thesis_bib}");
    }

    #[test]
    fn fuller_given_names_kept() {
        let found: Vec<String> = ["M. G. Kendall", "Daniel Weld", "Lucy Lu Wang", "J. Smith", "World Wide Web Consortium"].iter().map(|s| s.to_string()).collect();
        let bib: Vec<String> = ["Maurice G. Kendall", "Daniel S. Weld", "L. Wang", "Jane Doe", "W3C"].iter().map(|s| s.to_string()).collect();
        assert_eq!(fuller_names(&found, &bib), vec!["Maurice G. Kendall", "Daniel S. Weld", "Lucy Lu Wang", "J. Smith", "World Wide Web Consortium"]);
        // a different first name isn't fuller
        assert_eq!(fuller_names(&["M. Kendall".into()], &["Nora Kendall".into()]), vec!["M. Kendall"]);
    }

    #[test]
    fn own_fields_of_an_entry() {
        let e = parse_bibtex_raw(
            "@inproceedings{a, author = {Lo, Kyle and Wang, Lucy Lu and others}, title = {T}, booktitle = {Proc. of {CHI} (CHI '21)}, note = acm, month = 5, year = {2021}}\n\
             @article{b, author = {{Apple}}, title = {Apple Intelligence Foundation Language Models}, journal = {arXiv preprint arXiv:2407.21075}}\n\
             @article{c, author = {Kendall, Maurice G.}, title = {A New Measure}, journal = {Biometrika}, note = {\\todo{check}}}",
        );
        let own = |i: usize| own_fields(&e[i], &entry_from_bib(&e[i].kind, &e[i].fields).meta);
        // "and others" kept as written; the booktitle with its braces; a macro value (acm) left out
        assert_eq!(own(0), vec![("author".into(), "Lo, Kyle and Wang, Lucy Lu and others".into()), ("booktitle".into(), "Proc. of {CHI} (CHI '21)".into()), ("month".into(), "5".into())]);
        assert_eq!(own(1), vec![("author".to_string(), "{Apple}".to_string())]);
        // one person: their names come from the record
        assert_eq!(own(2), vec![("note".to_string(), "\\todo{check}".to_string())]);
    }

    #[test]
    fn extras_replace_by_key() {
        let a = parse_bibtex_raw("@misc{a, title={A}}\n@misc{b, title={B}}");
        let refs: Vec<&RawBibEntry> = a.iter().collect();
        let merged = merge_extras("@misc{B, title={Old B}}\n\n@misc{c, title={C}}", &refs);
        let keys: Vec<String> = parse_bibtex_raw(&merged).into_iter().map(|e| e.key).collect();
        assert_eq!(keys, vec!["c", "a", "b"]);
        assert!(!merged.contains("Old B"));
    }
}
