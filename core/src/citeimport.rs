//! Word and Google Docs citations: journal-style citations with each paper's options (locator,
//! prefix, suffix, author left out or in the text), and finding the papers of citations made with
//! another reference manager (Zotero, EndNote, Mendeley) in the library, to convert them.
//!
//! The add-ins read the other manager's citations from the document (their details: title, DOI,
//! authors, year) and ask the library for each paper with [`Library::match_cited_papers`]: the DOI,
//! then the PubMed id, then the title (with the year, when both have one), then the first author,
//! the year and nearly the same title. A paper found nowhere is offered to be added (`/add`).

use crate::csl::{self, CslCiteItem, CslRendered};
use crate::metadata;
use crate::model::{CitedPaper, Paper, PaperFilter, PaperMeta, Result, SortKey};
use crate::Library;

/// The library's paper for a cited work, and how it was found.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CitedMatch {
    /// The library's id ("" when the library doesn't have it).
    pub paper_id: String,
    /// "doi", "pmid", "title", "close" (first author, year and nearly the same title), or "".
    pub how: String,
}

fn norm_title(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

fn words(s: &str) -> Vec<String> {
    let mut w: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(|w| w.to_lowercase())
        .collect();
    w.sort();
    w.dedup();
    w
}

/// Shared words over all words (0–1).
fn similarity(a: &str, b: &str) -> f64 {
    let (x, y) = (words(a), words(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    let common = x.iter().filter(|w| y.binary_search(w).is_ok()).count() as f64;
    common / (x.len() + y.len()) as f64 * 2.0
}

fn year(m: &PaperMeta) -> Option<i32> {
    m.date.get(..4).and_then(|y| y.parse().ok())
}

fn first_family(m: &PaperMeta) -> String {
    m.authors.first().map(|a| crate::family_name(a.clone())).unwrap_or_default().to_lowercase()
}

/// The years agree (one off: online first, a preprint), or one isn't known.
fn years_fit(a: &PaperMeta, b: &PaperMeta) -> bool {
    match (year(a), year(b)) {
        (Some(x), Some(y)) => (x - y).abs() <= 1,
        _ => true,
    }
}

/// The paper in `papers` a document's details mean.
pub(crate) fn match_in(papers: &[Paper], m: &PaperMeta) -> CitedMatch {
    let found = |p: &Paper, how: &str| CitedMatch { paper_id: p.id.clone(), how: how.into() };
    let doi = metadata::normalize_doi(&m.doi);
    if !doi.is_empty() {
        if let Some(p) = papers.iter().find(|p| metadata::normalize_doi(&p.meta.doi) == doi) {
            return found(p, "doi");
        }
    }
    let pmid = m.pmid.trim();
    if !pmid.is_empty() {
        if let Some(p) = papers.iter().find(|p| p.meta.pmid.trim() == pmid) {
            return found(p, "pmid");
        }
    }
    let t = norm_title(&m.title);
    if t.len() >= 12 {
        if let Some(p) = papers.iter().find(|p| norm_title(&p.meta.title) == t && years_fit(&p.meta, m)) {
            return found(p, "title");
        }
    }
    // the same first author and year, nearly the same title (a subtitle, a typo, another dash)
    let fam = first_family(m);
    if !fam.is_empty() && year(m).is_some() && words(&m.title).len() >= 3 {
        let best = papers
            .iter()
            .filter(|p| year(&p.meta) == year(m) && first_family(&p.meta) == fam)
            .map(|p| (similarity(&p.meta.title, &m.title), p))
            .filter(|(s, _)| *s >= 0.8)
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((_, p)) = best {
            return found(p, "close");
        }
    }
    CitedMatch { paper_id: String::new(), how: String::new() }
}

#[uniffi::export]
impl Library {
    /// For each work a document cites (its details as another reference manager kept them), the
    /// library's paper, if it has it. Nothing is added.
    pub fn match_cited_papers(&self, metas: Vec<PaperMeta>) -> Vec<CitedMatch> {
        let papers = self.papers(PaperFilter::default(), SortKey::Added);
        metas.iter().map(|m| match_in(&papers, m)).collect()
    }

    /// `cite_csl_with`, with each cited paper's locator, prefix, suffix and author mode.
    pub fn cite_csl_items(
        &self,
        cache_dir: String,
        style_id: String,
        citations: Vec<Vec<CslCiteItem>>,
        extra: Vec<CitedPaper>,
    ) -> Result<CslRendered> {
        let mut ids: Vec<String> = Vec::new();
        for c in citations.iter().flatten() {
            if !ids.contains(&c.id) {
                ids.push(c.id.clone());
            }
        }
        let papers: Vec<Paper> = ids
            .into_iter()
            .filter_map(|id| {
                self.paper(id.clone()).or_else(|| extra.iter().find(|e| e.id == id).map(|e| crate::cited_paper(&e.id, e.meta.clone())))
            })
            .collect();
        csl::render_items(&cache_dir, &style_id, &papers, &citations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(title: &str, authors: &[&str], date: &str, doi: &str) -> PaperMeta {
        PaperMeta {
            title: title.into(),
            authors: authors.iter().map(|a| a.to_string()).collect(),
            date: date.into(),
            doi: doi.into(),
            ..Default::default()
        }
    }

    fn paper(id: &str, m: PaperMeta) -> Paper {
        crate::cited_paper(id, m)
    }

    #[test]
    fn finds_by_doi_pmid_title_and_close_title() {
        let mut withpmid = meta("Islet cells in situ", &["Ada Lee"], "2021", "");
        withpmid.pmid = "123456".into();
        let lib = vec![
            paper("a", meta("Cellular architecture of evolving neuroinflammatory lesions", &["Christoffer Mattsson Langseth", "Petra Kukanja"], "2024-03", "10.1016/j.cell.2024.02.030")),
            paper("b", withpmid),
            paper("c", meta("A quite long title about the spinal cord", &["Jane Smith"], "2019", "")),
        ];
        // DOI, as a URL and in capitals
        assert_eq!(match_in(&lib, &meta("", &[], "", "https://doi.org/10.1016/J.CELL.2024.02.030")).how, "doi");
        let mut p = meta("", &[], "", "");
        p.pmid = "123456".into();
        assert_eq!(match_in(&lib, &p).paper_id, "b");
        // the same title in other case and punctuation; a year too far off doesn't count
        assert_eq!(match_in(&lib, &meta("A quite long title about the spinal cord.", &[], "2020", "")).how, "title");
        assert_eq!(match_in(&lib, &meta("A Quite Long Title About The Spinal Cord", &[], "2015", "")).how, "");
        // first author, year and nearly the same title (a subtitle dropped)
        let close = match_in(&lib, &meta("Cellular architecture of evolving neuroinflammatory lesions in mice", &["Langseth, C. M."], "2024", ""));
        assert_eq!((close.paper_id.as_str(), close.how.as_str()), ("a", "close"));
        // another first author: not
        assert_eq!(match_in(&lib, &meta("Cellular architecture of evolving neuroinflammatory lesions in mice", &["Kukanja, P."], "2024", "")).how, "");
        assert_eq!(match_in(&lib, &meta("Something else entirely here", &["Nobody"], "2001", "10.9/x")).paper_id, "");
    }
}
