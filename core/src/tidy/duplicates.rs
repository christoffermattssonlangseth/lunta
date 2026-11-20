//! Papers that are in the library twice: the same DOI; the same title (allowing small differences:
//! punctuation, accents, a word or two, a subtitle left out) with the same first author and years
//! at most one apart; and a preprint with its published version (the same work, the published
//! one has another DOI), found by title or by what the checks found (`PaperUpdates`).
//!
//! Each group says which paper to keep (the published one, else the most complete) and why the
//! papers look alike. Nothing is merged until the researcher says so.

use super::{DuplicateGroup, DuplicateReason};
use crate::checks::{self, PaperUpdates};
use crate::model::Paper;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Titles at least this alike (Dice on words) are the same title.
const SIMILAR: f64 = 0.9;
/// A preprint's title may change a little more on the way to publication.
const SIMILAR_PREPRINT: f64 = 0.7;

fn doi(p: &Paper) -> String {
    crate::metadata::normalize_doi(&p.meta.doi)
}

fn year(p: &Paper) -> Option<i32> {
    p.meta.year().parse().ok()
}

fn first_author(p: &Paper) -> String {
    p.meta.authors.first().map(|a| checks::family_key(a)).unwrap_or_default()
}

fn is_preprint(p: &Paper) -> bool {
    checks::is_preprint(&p.meta)
}

/// The first authors are the same (or one paper has none).
fn same_first_author(a: &Paper, b: &Paper) -> bool {
    let (x, y) = (first_author(a), first_author(b));
    x.is_empty() || y.is_empty() || x == y || b.meta.authors.iter().any(|n| checks::family_key(n) == x) && a.meta.authors.iter().any(|n| checks::family_key(n) == y)
}

fn titles_alike(a: &Paper, b: &Paper, threshold: f64) -> bool {
    let (ta, tb) = (&a.meta.title, &b.meta.title);
    if checks::title_words(ta).len() < 3 || checks::title_words(tb).len() < 3 {
        // short titles ("Deep learning") must be exactly the same, and have the same authors
        return checks::title_words(ta) == checks::title_words(tb) && !first_author(a).is_empty() && first_author(a) == first_author(b);
    }
    checks::same_title(ta, tb) || checks::title_similarity(ta, tb) >= threshold
}

/// Why two papers look like one, if they do.
fn alike(a: &Paper, b: &Paper, ua: Option<&PaperUpdates>, ub: Option<&PaperUpdates>) -> Option<DuplicateReason> {
    let (da, db) = (doi(a), doi(b));
    if !da.is_empty() && da == db {
        return Some(DuplicateReason::SameDoi);
    }
    // the checks found this published version for the preprint
    let found = |u: Option<&PaperUpdates>, other: &str| {
        !other.is_empty()
            && u.is_some_and(|u| [&u.published, &u.linked].into_iter().flatten().any(|v| crate::metadata::normalize_doi(&v.doi) == other))
    };
    if found(ua, &db) || found(ub, &da) {
        return Some(DuplicateReason::PreprintAndPublished);
    }
    let (pa, pb) = (is_preprint(a), is_preprint(b));
    if pa != pb {
        // the published version: same authors, the same title (more or less), not earlier
        let (pre, publ) = if pa { (a, b) } else { (b, a) };
        let later = match (year(pre), year(publ)) {
            (Some(x), Some(y)) => y >= x && y - x <= 4,
            _ => true,
        };
        if later && same_first_author(a, b) && titles_alike(a, b, SIMILAR_PREPRINT) {
            return Some(DuplicateReason::PreprintAndPublished);
        }
        return None;
    }
    // two different DOIs are two works (a correction, a reply, part 1 and 2), unless arXiv's own
    if !da.is_empty() && !db.is_empty() && !(da.starts_with("10.48550/") || db.starts_with("10.48550/")) {
        return None;
    }
    let years_ok = match (year(a), year(b)) {
        (Some(x), Some(y)) => (x - y).abs() <= 1,
        _ => true,
    };
    if years_ok && same_first_author(a, b) && titles_alike(a, b, SIMILAR) {
        return Some(DuplicateReason::SimilarTitle);
    }
    None
}

/// How complete a paper is: files, highlights, details (to suggest which to keep).
fn completeness(p: &Paper) -> i32 {
    (if p.has_pdf { 8 } else { 0 })
        + (if p.has_epub { 4 } else { 0 })
        + p.annotation_count as i32 * 2
        + p.tags.len() as i32
        + p.collections.len() as i32
        + (if p.meta.abstract_text.is_empty() { 0 } else { 2 })
        + (if p.meta.doi.is_empty() { 0 } else { 2 })
        + (if p.meta.journal.is_empty() { 0 } else { 1 })
}

/// The paper to keep: the published version over a preprint, then the most complete, then the
/// oldest in the library.
pub(crate) fn best(papers: &[&Paper]) -> String {
    papers
        .iter()
        .max_by(|a, b| {
            (!is_preprint(a)).cmp(&!is_preprint(b)).then(completeness(a).cmp(&completeness(b))).then(b.added.cmp(&a.added))
        })
        .map(|p| p.id.clone())
        .unwrap_or_default()
}

/// The key a group is dismissed by ("Not Duplicates"): its paper ids, sorted, joined by "+"
/// (the Apple apps' DuplicateGroup.id).
pub fn group_key(ids: &[String]) -> String {
    let mut v = ids.to_vec();
    v.sort();
    v.join("+")
}

/// Candidate pairs by shared rare title words, so a library of thousands isn't compared pair by pair.
fn candidates(papers: &[Paper]) -> BTreeSet<(usize, usize)> {
    let words: Vec<Vec<String>> = papers
        .iter()
        .map(|p| {
            let mut w: Vec<String> = checks::title_words(checks::unmarked_title(&p.meta.title)).into_iter().filter(|w| w.len() >= 3 && !super::text::is_stop(w)).collect();
            w.sort();
            w.dedup();
            w
        })
        .collect();
    let mut df: HashMap<&str, usize> = HashMap::new();
    for ws in &words {
        for w in ws {
            *df.entry(w.as_str()).or_default() += 1;
        }
    }
    let mut postings: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, ws) in words.iter().enumerate() {
        // its three rarest words: two titles alike share at least one of them
        let mut r: Vec<&String> = ws.iter().collect();
        r.sort_by_key(|w| (df[w.as_str()], w.len() as isize * -1));
        for w in r.into_iter().take(3) {
            postings.entry(w.as_str()).or_default().push(i);
        }
    }
    let mut pairs = BTreeSet::new();
    for list in postings.values() {
        if list.len() > 200 {
            continue;
        }
        for (k, &i) in list.iter().enumerate() {
            for &j in &list[k + 1..] {
                pairs.insert((i.min(j), i.max(j)));
            }
        }
    }
    // the same DOI, whatever the titles say
    let mut by_doi: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in papers.iter().enumerate() {
        let d = doi(p);
        if !d.is_empty() {
            by_doi.entry(d).or_default().push(i);
        }
    }
    for list in by_doi.values() {
        for (k, &i) in list.iter().enumerate() {
            for &j in &list[k + 1..] {
                pairs.insert((i.min(j), i.max(j)));
            }
        }
    }
    pairs
}

/// The duplicate groups among `papers` (without those dismissed).
pub(crate) fn find(papers: &[Paper], updates: &HashMap<String, PaperUpdates>, dismissed: &BTreeSet<String>) -> Vec<DuplicateGroup> {
    // published DOIs the checks found, to pair them even when titles differ
    let doi_index: HashMap<String, usize> = papers.iter().enumerate().filter(|(_, p)| !doi(p).is_empty()).map(|(i, p)| (doi(p), i)).collect();
    let mut pairs = candidates(papers);
    for (i, p) in papers.iter().enumerate() {
        if let Some(u) = updates.get(&p.id) {
            for v in [&u.published, &u.linked].into_iter().flatten() {
                if let Some(&j) = doi_index.get(&crate::metadata::normalize_doi(&v.doi)) {
                    if i != j {
                        pairs.insert((i.min(j), i.max(j)));
                    }
                }
            }
        }
    }
    // union-find over the pairs that are alike; a group's reason is the strongest of its pairs
    let mut parent: Vec<usize> = (0..papers.len()).collect();
    fn root(p: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut c = i;
        while p[c] != r {
            let n = p[c];
            p[c] = r;
            c = n;
        }
        r
    }
    // Two groups are joined only when every paper of one is alike with every paper of the other,
    // so a preprint alike with two different published works (two DOIs) doesn't chain them into
    // one group, where merging would delete one of them.
    let mut members: HashMap<usize, Vec<usize>> = (0..papers.len()).map(|i| (i, vec![i])).collect();
    let mut reason: HashMap<(usize, usize), DuplicateReason> = HashMap::new();
    for (i, j) in pairs {
        let Some(r) = alike(&papers[i], &papers[j], updates.get(&papers[i].id), updates.get(&papers[j].id)) else { continue };
        let (a, b) = (root(&mut parent, i), root(&mut parent, j));
        if a != b {
            let all_alike = members[&a].iter().all(|&x| {
                members[&b].iter().all(|&y| (x == i && y == j) || alike(&papers[x], &papers[y], updates.get(&papers[x].id), updates.get(&papers[y].id)).is_some())
            });
            if !all_alike {
                continue;
            }
            parent[a] = b;
            let moved = members.remove(&a).unwrap_or_default();
            members.get_mut(&b).unwrap().extend(moved);
        }
        reason.insert((i, j), r);
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..papers.len() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out = vec![];
    for (_, members) in groups.into_iter().filter(|(_, m)| m.len() > 1) {
        let ids: Vec<String> = members.iter().map(|&i| papers[i].id.clone()).collect();
        let key = group_key(&ids);
        if dismissed.contains(&key) || dismissed.contains(&format!("dup:{key}")) {
            continue;
        }
        let rs: Vec<DuplicateReason> = reason.iter().filter(|((i, _), _)| members.contains(i)).map(|(_, r)| *r).collect();
        let r = if rs.contains(&DuplicateReason::SameDoi) {
            DuplicateReason::SameDoi
        } else if rs.contains(&DuplicateReason::PreprintAndPublished) {
            DuplicateReason::PreprintAndPublished
        } else {
            DuplicateReason::SimilarTitle
        };
        let ps: Vec<&Paper> = members.iter().map(|&i| &papers[i]).collect();
        let keep = best(&ps);
        let (reason_text, note) = match r {
            DuplicateReason::SameDoi => ("Same DOI".to_string(), "Merging keeps one paper and moves tags, folders, highlights, notes, stars and a missing PDF or reading view onto it.".to_string()),
            DuplicateReason::SimilarTitle => (
                "Same title, authors and year".to_string(),
                "The titles differ only a little (punctuation, a word, a subtitle). Merging keeps one paper and moves tags, folders, highlights, notes and files onto it.".to_string(),
            ),
            DuplicateReason::PreprintAndPublished => (
                "A preprint and its published version".to_string(),
                "Merging keeps the published version, with the preprint's highlights, notes, tags and folders. (A preprint whose published version isn't in the library: use Switch to the Published Version.)".to_string(),
            ),
        };
        let mut ids = ids;
        // the one to keep first, then as they were added
        ids.sort_by_key(|id| (id != &keep, papers.iter().find(|p| &p.id == id).map(|p| p.added).unwrap_or(0)));
        out.push(DuplicateGroup { key, paper_ids: ids, reason: r, reason_text, keep, note });
    }
    out.sort_by(|a, b| a.reason.cmp(&b.reason).then(a.key.cmp(&b.key)));
    out
}
