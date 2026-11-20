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

/// The numbers in a title, as digits or roman numerals ("Part II", "Phase 3", "COVID-19"), which
/// `words` leaves out but tell companion papers apart.
fn numerals(s: &str) -> Vec<String> {
    let roman = |w: &str| -> Option<u32> {
        let v = |c| match c {
            'i' => Some(1),
            'v' => Some(5),
            'x' => Some(10),
            'l' => Some(50),
            'c' => Some(100),
            _ => None,
        };
        let ds: Vec<u32> = w.chars().map(v).collect::<Option<_>>()?;
        let mut n = 0i64;
        for (i, d) in ds.iter().enumerate() {
            if ds.get(i + 1).is_some_and(|e| e > d) { n -= *d as i64 } else { n += *d as i64 }
        }
        // only a numeral written the usual way ("civil" isn't one; "xiv" is)
        let canon = |mut n: u32| {
            let mut out = String::new();
            for (k, r) in [(100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")] {
                while n >= k {
                    out.push_str(r);
                    n -= k;
                }
            }
            out
        };
        (n > 0 && canon(n as u32) == w).then_some(n as u32)
    };
    let mut out: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter_map(|w| {
            if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) {
                Some(w.trim_start_matches('0').to_string())
            } else {
                roman(&w).map(|n| n.to_string())
            }
        })
        .collect();
    out.sort();
    out.dedup();
    out
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
            // Part I and Part II, Phase 2 and Phase 3: other papers
            .filter(|p| numerals(&p.meta.title) == numerals(&m.title))
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

    #[test]
    fn companion_papers_with_other_numbers_are_not_close() {
        let lib = vec![
            paper("p1", meta("Mapping the developing spinal cord, part I: progenitors", &["Jane Smith"], "2022", "")),
            paper("ph2", meta("A randomized phase 2 trial of drug X in glioma", &["Ann Berg"], "2021", "")),
        ];
        let close = |t: &str, a: &str, y: &str| match_in(&lib, &meta(t, &[a], y, "")).paper_id;
        assert_eq!(close("Mapping the developing spinal cord, part II: progenitors", "Smith, J.", "2022"), "");
        assert_eq!(close("A randomized phase 3 trial of drug X in glioma", "Berg, A.", "2021"), "");
        // the same numbers, a word or subtitle off: still close
        assert_eq!(close("Mapping the developing spinal cord, part I: the progenitors", "Smith, J.", "2022"), "p1");
        assert_eq!(close("A randomised phase II trial of drug X in glioma", "Berg, A.", "2021"), "ph2");
        assert_eq!(numerals("Part IV of the civil mix, phase 02"), vec!["2".to_string(), "4".to_string()]);
    }
}
