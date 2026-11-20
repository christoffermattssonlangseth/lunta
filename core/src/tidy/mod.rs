//! **Tidy the Library**: suggestions to approve, one by one or all at once. Lunta proposes, the
//! researcher decides: nothing here changes the library until a suggestion is applied, and then
//! through the change log like any other edit (synced; tags, folders and DOIs can be undone with
//! the `TidyChange` an apply returns).
//!
//! - [`duplicates`]: papers in the library twice (same DOI; same title, first author and year,
//!   allowing small differences; a preprint and its published version). `merge_papers` folds them
//!   into one. A preprint whose published version is *not* in the library is pointed to the
//!   checks' "Switch to the Published Version" (`use_published_version`), not duplicated here.
//! - Missing details: no DOI ([`doi`]: arXiv's own, else Crossref with the title, year and
//!   first-author guard), no PDF (the apps' Find PDF), no reading view (Make Missing Reading Views).
//! - [`groups`]: topics to make into tags or folders (the on-device model when there is one, else
//!   shared terms and journals), and existing tags for untagged papers.
//!
//! The prompts and heuristics are here so every app proposes the same: the Apple apps give
//! `ModelPrompt` to their model and hand the answer back (`tidy_groups_from_answer`); the
//! desktop and Android apps can do the same with `AiEngine` (or `tidy_topics_with_model`).

pub mod doi;
pub mod duplicates;
pub mod groups;
pub(crate) mod text;
#[cfg(test)]
mod tests;

use crate::checks::net::Net;
use crate::checks::refcheck::{say, with_live_net};
use crate::model::*;
use crate::store::OpKind;
use crate::{Library, ProgressListener};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Why papers look like one.
#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateReason {
    SameDoi,
    PreprintAndPublished,
    SimilarTitle,
}

/// Papers that look like one paper.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DuplicateGroup {
    /// The ids, sorted, joined by "+": what "Not Duplicates" dismisses.
    pub key: String,
    /// The papers, the one to keep first.
    pub paper_ids: Vec<String>,
    pub reason: DuplicateReason,
    /// "Same DOI", "Same title, authors and year", "A preprint and its published version".
    pub reason_text: String,
    /// The paper to keep: the published version, else the most complete.
    pub keep: String,
    /// What merging does, for under the group.
    pub note: String,
}

/// A preprint whose published version the checks found (not in the library): offered "Switch
/// to the Published Version".
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PublishedSuggestion {
    pub paper_id: String,
    pub version: crate::checks::PublishedVersion,
}

/// Papers missing something, by what they miss (dismissed ones left out).
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct MissingDetails {
    /// No DOI (not books or theses without one, nor papers whose "no DOI" was dismissed).
    pub no_doi: Vec<String>,
    /// No PDF.
    pub no_pdf: Vec<String>,
    /// A PDF but no reading view.
    pub no_reading_view: Vec<String>,
    /// Preprints with a published version to switch to.
    pub published: Vec<PublishedSuggestion>,
}

/// A DOI found for a paper that has none.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DoiSuggestion {
    pub paper_id: String,
    pub doi: String,
    /// The record it comes from (its title, authors, journal, year to show; empty fields of the
    /// paper are filled from it).
    pub found: PaperMeta,
    /// "Crossref" or "arXiv".
    pub source: String,
    /// Another paper in the library already has this DOI (then it's a duplicate to merge).
    pub duplicate_of: Option<String>,
}

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DoiReport {
    pub suggestions: Vec<DoiSuggestion>,
    /// Papers nothing was plainly found for.
    pub not_found: Vec<String>,
    /// Services that couldn't be asked.
    pub errors: Vec<String>,
}

/// A tag or folder to make: its name and the papers it would hold.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GroupProposal {
    /// "Spatial transcriptomics" (the researcher may rename it before creating it).
    pub name: String,
    pub paper_ids: Vec<String>,
    /// Why: "“spatial transcriptomic” in their titles, keywords or abstracts", "papers in this
    /// journal", "proposed by the on-device model".
    pub why: String,
    pub by_model: bool,
}

/// An existing tag for an untagged paper.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TagSuggestion {
    pub paper_id: String,
    pub tag: String,
    /// "shares “cortex”, “interneuron” with 6 papers tagged Neuroscience".
    pub because: String,
    pub score: f64,
}

/// What to give a model: the instructions, the prompt, how long the answer may be, and the
/// paper (or highlight) ids the prompt's numbers stand for.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ModelPrompt {
    pub instructions: String,
    pub prompt: String,
    pub max_tokens: u32,
    /// Number n in the prompt is ids[n - 1].
    pub ids: Vec<String>,
}

/// Everything Tidy Library suggests that needs no network (DOIs are looked up on request).
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct TidyReport {
    pub duplicates: Vec<DuplicateGroup>,
    pub missing: MissingDetails,
    /// Topics found without a model (the apps ask the model when there is one).
    pub topics: Vec<GroupProposal>,
    pub tag_suggestions: Vec<TagSuggestion>,
    /// Papers in the library.
    pub papers: u32,
}

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TagMark {
    pub paper_id: String,
    pub tag: String,
}

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FolderMark {
    pub collection: String,
    pub paper_id: String,
}

#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MetaBefore {
    pub paper_id: String,
    pub meta: PaperMeta,
    /// The DOI the change set (undo only restores the paper while it still has it).
    pub doi_set: String,
}

/// What an apply did, to undo it.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct TidyChange {
    /// "Added the tag Spatial transcriptomics to 34 papers."
    pub summary: String,
    pub tags: Vec<TagMark>,
    pub members: Vec<FolderMark>,
    pub created_folders: Vec<String>,
    pub metas: Vec<MetaBefore>,
}

/// The answer without a thinking trace (Qwen3's `<think>…</think>`).
pub(crate) fn without_thinking(raw: &str) -> String {
    match (raw.find("<think>"), raw.find("</think>")) {
        (Some(_), Some(end)) => raw[end + "</think>".len()..].trim_start().to_string(),
        (Some(_), None) => String::new(),
        _ => raw.to_string(),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Works that often have no DOI and needn't be nagged about: books, theses, standards, reports
/// without a publisher's DOI… only articles, conference papers, chapters and preprints are listed.
fn wants_doi(m: &PaperMeta) -> bool {
    use crate::worktype::Kind;
    matches!(crate::worktype::kind(m), Kind::Article | Kind::Conference | Kind::Chapter | Kind::Preprint | Kind::Other) && !m.title.trim().is_empty()
}

fn dismissed_file(cache: &std::path::Path) -> PathBuf {
    cache.join("tidy").join("dismissed.json")
}

impl Library {
    fn all_papers(&self) -> Vec<Paper> {
        self.papers(PaperFilter::default(), SortKey::Added)
    }

    fn dismissed(&self) -> BTreeSet<String> {
        let f = dismissed_file(&self.inner.lock().unwrap().cache);
        std::fs::read_to_string(f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn folder_names(&self) -> Vec<String> {
        self.collections().into_iter().map(|c| c.name).collect()
    }

    /// Looks up DOIs with `net` (the services, or recorded answers in tests).
    pub(crate) fn find_dois_with(&self, net: &dyn Net, ids: &[String], progress: &dyn Fn(String)) -> DoiReport {
        let papers: Vec<Paper> = if ids.is_empty() {
            let m = self.missing_details(&self.all_papers(), &self.dismissed());
            self.collect(m.no_doi)
        } else {
            self.collect(ids.to_vec())
        };
        let mut r = DoiReport::default();
        let todo: Vec<&Paper> = papers.iter().filter(|p| p.meta.doi.trim().is_empty()).collect();
        for (n, p) in todo.iter().enumerate() {
            progress(format!("Looking up DOIs ({} of {})…", n + 1, todo.len()));
            match doi::find(net, &p.id, &p.meta) {
                Ok(Some(mut s)) => {
                    s.duplicate_of = self.find_by_doi(s.doi.clone()).map(|x| x.id).filter(|x| x != &p.id);
                    r.suggestions.push(s);
                }
                Ok(None) => r.not_found.push(p.id.clone()),
                Err(e) => {
                    let m = e.to_string();
                    if !r.errors.contains(&m) {
                        r.errors.push(m);
                    }
                    r.not_found.push(p.id.clone());
                }
            }
        }
        r
    }

    fn missing_details(&self, papers: &[Paper], dismissed: &BTreeSet<String>) -> MissingDetails {
        let doi_of: BTreeSet<String> = papers.iter().map(|p| crate::metadata::normalize_doi(&p.meta.doi)).filter(|d| !d.is_empty()).collect();
        let updates = self.all_paper_updates();
        let mut m = MissingDetails::default();
        for p in papers {
            if p.meta.doi.trim().is_empty() && wants_doi(&p.meta) && !dismissed.contains(&format!("doi:{}", p.id)) {
                m.no_doi.push(p.id.clone());
            }
            if !p.has_pdf && !dismissed.contains(&format!("pdf:{}", p.id)) {
                m.no_pdf.push(p.id.clone());
            }
            if p.has_pdf && !p.has_epub && !dismissed.contains(&format!("epub:{}", p.id)) {
                m.no_reading_view.push(p.id.clone());
            }
            if let Some(v) = updates.get(&p.id).and_then(|u| u.published.clone()) {
                // the published version already in the library is a duplicate to merge instead
                if !doi_of.contains(&crate::metadata::normalize_doi(&v.doi)) {
                    m.published.push(PublishedSuggestion { paper_id: p.id.clone(), version: v });
                }
            }
        }
        m
    }

    /// Writes ops without replaying the log after each (tags, memberships, folders: they don't
    /// touch citation keys), then replays once.
    fn write_batch(&self, ops: Vec<OpKind>) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        {
            let mut g = self.inner.lock().unwrap();
            for op in ops {
                g.store.append(op)?;
            }
            g.state = g.store.replay()?;
        }
        self.reindex_quick()?;
        Ok(())
    }
}

#[uniffi::export]
impl Library {
    /// Everything Tidy Library suggests without going online: duplicates, papers missing a DOI,
    /// a PDF or a reading view, preprints with a published version, topics (found without a
    /// model) and existing tags for untagged papers.
    pub fn tidy_report(&self) -> TidyReport {
        let papers = self.all_papers();
        let dismissed = self.dismissed();
        let updates = self.all_paper_updates();
        let tags = self.tags();
        let folders = self.folder_names();
        let dismissed_topics = |g: &GroupProposal| dismissed.contains(&format!("topic:{}", text::name_key(&g.name)));
        TidyReport {
            duplicates: duplicates::find(&papers, &updates, &dismissed),
            missing: self.missing_details(&papers, &dismissed),
            topics: groups::topics(&papers, &tags, &folders).into_iter().filter(|g| !dismissed_topics(g)).collect(),
            tag_suggestions: groups::tag_suggestions(&papers, &dismissed),
            papers: papers.len() as u32,
        }
    }

    /// The duplicate groups only (fast; after a merge).
    pub fn tidy_duplicates(&self) -> Vec<DuplicateGroup> {
        duplicates::find(&self.all_papers(), &self.all_paper_updates(), &self.dismissed())
    }

    /// Don't suggest this again (on this device): a duplicate group's key ("Not Duplicates"),
    /// "doi:<id>" (has no DOI), "pdf:<id>", "epub:<id>", "tag:<id>:<tag>", "topic:<name>".
    pub fn tidy_dismiss(&self, key: String) -> Result<()> {
        let mut d = self.dismissed();
        d.insert(key);
        let f = dismissed_file(&self.inner.lock().unwrap().cache);
        std::fs::create_dir_all(f.parent().unwrap())?;
        std::fs::write(f, serde_json::to_string(&d).unwrap_or_default())?;
        Ok(())
    }

    /// Suggests every dismissed suggestion again.
    pub fn tidy_reset_dismissed(&self) -> Result<()> {
        let f = dismissed_file(&self.inner.lock().unwrap().cache);
        let _ = std::fs::remove_file(f);
        Ok(())
    }

    /// Merges papers into `keep`: its details' empty fields filled from the others', their tags,
    /// folders, star, read and Must Read state, highlights and notes, and a PDF, reading view or
    /// full text it lacks (when on this device); a fixed citation key of another paper is kept
    /// when `keep`'s isn't fixed (a manuscript may cite it). Then the others are deleted. Not
    /// undoable: the apps ask first.
    pub fn merge_papers(&self, keep: String, others: Vec<String>) -> Result<Paper> {
        let keeper = self.paper(keep.clone()).ok_or_else(|| CoreError::NotFound(keep.clone()))?;
        let mut meta = keeper.meta.clone();
        let mut ops: Vec<OpKind> = vec![];
        let mut key_to_keep: Option<String> = None;
        let (mut starred, mut read, mut must_read) = (keeper.starred, keeper.read, keeper.must_read);
        for o in others.iter().filter(|o| **o != keep) {
            let Some(p) = self.paper(o.clone()) else { continue };
            crate::merge_meta(&mut meta, &p.meta);
            for t in p.tags.iter().filter(|t| !keeper.tags.contains(t)) {
                ops.push(OpKind::Tag { paper: keep.clone(), tag: t.clone(), present: true });
            }
            for c in p.collections.iter().filter(|c| !keeper.collections.contains(c)) {
                ops.push(OpKind::Member { collection: c.clone(), paper: keep.clone(), present: true });
            }
            if p.starred && !starred {
                starred = true;
                ops.push(OpKind::SetStarred { paper: keep.clone(), starred: true });
            }
            if p.read && !read {
                read = true;
                ops.push(OpKind::SetRead { paper: keep.clone(), read: true });
            }
            if p.must_read && !must_read && !read {
                must_read = true;
                ops.push(OpKind::SetMustRead { paper: keep.clone(), must_read: true });
            }
            let now = now_ms();
            for mut a in self.annotations(p.id.clone()) {
                a.id = uuid::Uuid::new_v4().to_string();
                a.paper_id = keep.clone();
                a.modified = now;
                ops.push(OpKind::AnnotationUpsert { annotation: a });
            }
            {
                let g = self.inner.lock().unwrap();
                for kind in [FileKind::Pdf, FileKind::Epub, FileKind::Fulltext] {
                    let mine = g.store.file_path(&keep, kind);
                    let theirs = g.store.file_path(&p.id, kind);
                    if !mine.exists() && !g.store.has_file(&keep, kind) && theirs.exists() {
                        if let Ok(data) = std::fs::read(&theirs) {
                            g.store.write_file(&keep, kind, &data)?;
                        }
                    }
                }
                if key_to_keep.is_none() && !g.state.fixed_keys.contains(&keep) && g.state.fixed_keys.contains(&p.id) {
                    key_to_keep = g.state.cite_keys.get(&p.id).cloned();
                }
            }
        }
        self.write_batch(ops)?;
        {
            let mut g = self.inner.lock().unwrap();
            if meta != keeper.meta {
                g.write(OpKind::SetMeta { paper: keep.clone(), meta })?;
            }
            for o in others.iter().filter(|o| **o != keep) {
                if g.state.papers.contains_key(o) {
                    g.write(OpKind::DeletePaper { paper: o.clone() })?;
                }
            }
        }
        if let Some(k) = key_to_keep {
            let _ = self.set_cite_key(keep.clone(), k);
        }
        self.reindex()?;
        self.paper(keep.clone()).ok_or(CoreError::NotFound(keep))
    }

    /// Looks up a DOI for papers that have none (`ids`; empty: every paper listed as missing
    /// one): arXiv's own DOI, else Crossref, taken only when title, year and first author agree.
    /// Nothing is changed: apply the suggestions the researcher approves with
    /// `apply_doi_suggestions`. Blocking (about a second a paper).
    pub fn find_missing_dois(&self, ids: Vec<String>, progress: Option<Arc<dyn ProgressListener>>) -> DoiReport {
        let cache = self.inner.lock().unwrap().cache.to_string_lossy().to_string();
        with_live_net(&cache, Duration::from_secs(7 * 24 * 3600), |net| self.find_dois_with(net, &ids, &say(&progress)))
    }

    /// Sets the approved DOIs (and fills details the papers lack from the records found).
    pub fn apply_doi_suggestions(&self, suggestions: Vec<DoiSuggestion>) -> Result<TidyChange> {
        let mut ch = TidyChange::default();
        {
            let mut g = self.inner.lock().unwrap();
            for s in &suggestions {
                let Some(p) = g.state.papers.get(&s.paper_id) else { continue };
                if !p.meta.doi.trim().is_empty() {
                    continue;
                }
                let before = p.meta.clone();
                let after = doi::with_doi(&before, s);
                g.write(OpKind::SetMeta { paper: s.paper_id.clone(), meta: after })?;
                ch.metas.push(MetaBefore { paper_id: s.paper_id.clone(), meta: before, doi_set: s.doi.clone() });
            }
        }
        self.reindex()?;
        ch.summary = format!("Added a DOI to {}.", plural(ch.metas.len(), "paper", "papers"));
        Ok(ch)
    }

    /// Topics found without a model (shared terms, journals): at most ten, each with its papers.
    pub fn tidy_topics(&self) -> Vec<GroupProposal> {
        groups::topics(&self.all_papers(), &self.tags(), &self.folder_names())
    }

    /// The model's prompt for topics: the latest `max_papers` papers' titles, numbered (about
    /// 60 fit Apple's on-device model; 150 the open model). Give the answer to
    /// `tidy_groups_from_answer`.
    pub fn tidy_topic_prompt(&self, max_papers: u32) -> ModelPrompt {
        groups::topic_prompt(&self.all_papers(), &self.tags(), &self.folder_names(), max_papers as usize)
    }

    /// The topics in the model's answer to `tidy_topic_prompt` (only papers from the prompt;
    /// names already used left out).
    pub fn tidy_groups_from_answer(&self, prompt: ModelPrompt, answer: String) -> Vec<GroupProposal> {
        let dismissed = self.dismissed();
        groups::parse_topics(&prompt, &answer, &self.tags(), &self.folder_names())
            .into_iter()
            .filter(|g| !dismissed.contains(&format!("topic:{}", text::name_key(&g.name))))
            .collect()
    }

    /// Makes the ticked topics into tags (or folders, `as_folders`) on their papers. A folder
    /// with the name is reused.
    pub fn create_groups(&self, proposals: Vec<GroupProposal>, as_folders: bool) -> Result<TidyChange> {
        let mut ch = TidyChange::default();
        let mut ops = vec![];
        let existing = self.collections();
        let mut counts: Vec<String> = vec![];
        for g in &proposals {
            let name = g.name.split_whitespace().collect::<Vec<_>>().join(" ");
            if name.is_empty() {
                continue;
            }
            let mut n = 0;
            if as_folders {
                let id = match existing.iter().find(|c| c.name.eq_ignore_ascii_case(&name) && c.parent.is_none()) {
                    Some(c) => c.id.clone(),
                    None => {
                        let id = uuid::Uuid::new_v4().to_string();
                        ops.push(OpKind::CollectionUpsert { id: id.clone(), name: name.clone(), parent: None });
                        ch.created_folders.push(id.clone());
                        id
                    }
                };
                for pid in &g.paper_ids {
                    if self.paper(pid.clone()).is_some_and(|p| !p.collections.contains(&id)) {
                        ops.push(OpKind::Member { collection: id.clone(), paper: pid.clone(), present: true });
                        ch.members.push(FolderMark { collection: id.clone(), paper_id: pid.clone() });
                        n += 1;
                    }
                }
            } else {
                for pid in &g.paper_ids {
                    if self.paper(pid.clone()).is_some_and(|p| !p.tags.contains(&name)) {
                        ops.push(OpKind::Tag { paper: pid.clone(), tag: name.clone(), present: true });
                        ch.tags.push(TagMark { paper_id: pid.clone(), tag: name.clone() });
                        n += 1;
                    }
                }
            }
            counts.push(format!("{name} ({n})"));
        }
        self.write_batch(ops)?;
        ch.summary = match (as_folders, counts.len()) {
            (_, 0) => "Nothing to add.".into(),
            (true, 1) => format!("Made the folder {}.", counts[0]),
            (true, _) => format!("Made {} folders: {}.", counts.len(), counts.join(", ")),
            (false, 1) => format!("Added the tag {}.", counts[0]),
            (false, _) => format!("Added {} tags: {}.", counts.len(), counts.join(", ")),
        };
        Ok(ch)
    }

    /// Existing tags for untagged papers (at most two each, with why).
    pub fn tidy_tag_suggestions(&self) -> Vec<TagSuggestion> {
        groups::tag_suggestions(&self.all_papers(), &self.dismissed())
    }

    /// Adds the approved tags.
    pub fn apply_tag_suggestions(&self, suggestions: Vec<TagSuggestion>) -> Result<TidyChange> {
        let mut ch = TidyChange::default();
        let mut ops = vec![];
        for s in &suggestions {
            if self.paper(s.paper_id.clone()).is_some_and(|p| !p.tags.contains(&s.tag)) && !ch.tags.iter().any(|t| t.paper_id == s.paper_id && t.tag == s.tag) {
                ops.push(OpKind::Tag { paper: s.paper_id.clone(), tag: s.tag.clone(), present: true });
                ch.tags.push(TagMark { paper_id: s.paper_id.clone(), tag: s.tag.clone() });
            }
        }
        self.write_batch(ops)?;
        let papers: BTreeSet<&String> = ch.tags.iter().map(|t| &t.paper_id).collect();
        ch.summary = format!("Tagged {}.", plural(papers.len(), "paper", "papers"));
        Ok(ch)
    }

    /// Undoes an apply: the tags and folder memberships it added are taken away, folders it made
    /// are deleted (unless other papers were put in them since), DOIs it set are taken back
    /// (unless the paper's DOI was changed since).
    pub fn undo_tidy_change(&self, change: TidyChange) -> Result<()> {
        let mut ops = vec![];
        for t in &change.tags {
            ops.push(OpKind::Tag { paper: t.paper_id.clone(), tag: t.tag.clone(), present: false });
        }
        for m in &change.members {
            ops.push(OpKind::Member { collection: m.collection.clone(), paper: m.paper_id.clone(), present: false });
        }
        let ours: BTreeSet<(&String, &String)> = change.members.iter().map(|m| (&m.collection, &m.paper_id)).collect();
        for f in &change.created_folders {
            let others = self.papers(PaperFilter { collection: Some(f.clone()), ..Default::default() }, SortKey::Added).into_iter().any(|p| !ours.contains(&(f, &p.id)));
            if !others {
                ops.push(OpKind::CollectionDelete { id: f.clone() });
            }
        }
        self.write_batch(ops)?;
        {
            let mut g = self.inner.lock().unwrap();
            for m in &change.metas {
                let now = g.state.papers.get(&m.paper_id).map(|p| crate::metadata::normalize_doi(&p.meta.doi));
                if now.as_deref() == Some(crate::metadata::normalize_doi(&m.doi_set).as_str()) {
                    g.write(OpKind::SetMeta { paper: m.paper_id.clone(), meta: m.meta.clone() })?;
                }
            }
        }
        if !change.metas.is_empty() {
            self.reindex()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- with the open model

#[cfg(feature = "llm")]
#[uniffi::export]
impl Library {
    /// Topics from the open model (desktop, Android): `tidy_topic_prompt` answered by `engine`
    /// and read with `tidy_groups_from_answer`. Empty when the model gives nothing usable (then
    /// show `tidy_topics`). Blocking.
    pub fn tidy_topics_with_model(&self, engine: Arc<crate::llm::AiEngine>, max_papers: u32) -> Vec<GroupProposal> {
        let p = self.tidy_topic_prompt(max_papers);
        match engine.plain_answer(&p.instructions, &p.prompt, p.max_tokens) {
            Ok(a) => self.tidy_groups_from_answer(p, a),
            Err(_) => vec![],
        }
    }
}
