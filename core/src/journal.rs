//! Journal names as citations write them: the name cleaned of what PubMed and Crossref add
//! ("Bioinformatics (Oxford, England)", "Brain : a journal of neurology", "Nature methods"), and
//! the journal's abbreviation ("Nat Methods") for styles that use abbreviated names (Cell,
//! Nature, Vancouver: CSL's `container-title-short`).
//!
//! The library doesn't keep abbreviations (Crossref's `short-container-title` and PubMed's
//! MedlineTA aren't stored), so they come from a table made from NLM's list of journals
//! (`journal_abbrev.tsv`, ~1,500 life-science journals: the Nature, Cell, Science, PLOS, BMC,
//! Frontiers, Trends, Annual Reviews, Lancet and JAMA families, neuroscience, immunology,
//! genomics…), looked up by the journal's name. A journal not in it is written in full, as
//! before. The abbreviations are NLM's, without full stops ("Nat Methods", "Proc Natl Acad Sci
//! U S A"), as PubMed, Zotero and ReadCube give them.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Words a journal's name keeps in lower case (after its first word).
const SMALL: [&str; 34] = [
    "a", "an", "the", "and", "or", "of", "in", "on", "for", "to", "at", "by", "with", "from", "into", "via", "as", "&", "et", "de", "des", "der",
    "die", "das", "und", "du", "la", "le", "les", "y", "e", "di", "del", "für",
];

/// Names that are written in lower case ("npj Regenerative Medicine").
const LOWER: [&str; 1] = ["npj"];

/// A qualifier in brackets that is part of the name: an acronym ("(BBA)", "(IJDAR)").
fn kept_in_brackets(inner: &str) -> bool {
    !inner.is_empty() && inner.len() <= 12 && !inner.contains(char::is_whitespace) && !inner.chars().any(|c| c.is_lowercase())
}

/// The name without a bracketed qualifier at its end: PubMed's place ("Bioinformatics (Oxford,
/// England)", "Science (New York, N.Y.)", "Journal of immunology (Baltimore, Md. : 1950)") or
/// OpenAlex's host ("bioRxiv (Cold Spring Harbor Laboratory)").
fn without_qualifier(s: &str) -> String {
    let mut s = s.trim().to_string();
    while s.ends_with(')') {
        // the bracket that opens the last one
        let mut depth = 0;
        let mut open = None;
        for (i, c) in s.char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    depth -= 1;
                    if depth == 0 {
                        open = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(i) = open else { break };
        if i == 0 || kept_in_brackets(&s[i + 1..s.len() - 1]) {
            break;
        }
        s = s[..i].trim_end().to_string();
    }
    s
}

/// A journal's name as a citation writes it: without PubMed's and OpenAlex's qualifiers
/// ("(Oxford, England)", "(Cornell University)") and NLM's subtitles and translations ("Brain : a
/// journal of neurology" → "Brain", "Biomedicine & pharmacotherapy = Biomédecine…"), and in title
/// case when it is in sentence case ("Nature methods" → "Nature Methods", "The Journal of
/// biological chemistry" → "The Journal of Biological Chemistry"). Words with capitals inside
/// ("bioRxiv", "eLife", "iScience", "mSystems", "PLoS") and "npj" stay as they are.
pub fn clean_name(journal: &str) -> String {
    let mut s = without_qualifier(&journal.split_whitespace().collect::<Vec<_>>().join(" "));
    for sep in [" : ", " = "] {
        if let Some(i) = s.find(sep) {
            if i > 0 {
                s.truncate(i);
            }
        }
    }
    let s = without_qualifier(&s);
    let words: Vec<&str> = s.split(' ').collect();
    // a word in lower case that starts with a letter ("16th" isn't one)
    let all_lower = |w: &str| w.chars().find(|c| c.is_alphanumeric()).is_some_and(|c| c.is_alphabetic()) && !w.chars().any(|c| c.is_uppercase());
    let small = |w: &str| SMALL.contains(&w.trim_matches(|c: char| !c.is_alphanumeric() && c != '&').to_lowercase().as_str());
    // in sentence case: a word after the first in lower case that title case would capitalize
    if !words.iter().skip(1).any(|w| all_lower(w) && !small(w) && !LOWER.contains(w)) {
        return s;
    }
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if !all_lower(w) || LOWER.contains(w) || (i > 0 && small(w)) {
                return w.to_string();
            }
            // the first letter in capitals ("molecular," → "Molecular,", "cell-based" → "Cell-based")
            let mut done = false;
            w.chars()
                .map(|c| {
                    if !done && c.is_alphabetic() {
                        done = true;
                        c.to_uppercase().collect::<String>()
                    } else {
                        c.to_string()
                    }
                })
                .collect()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A journal's name as the abbreviation table is keyed: lower case, ASCII letters and digits
/// separated by single spaces, without brackets, subtitle (" : "), translation (" = ") or a
/// leading "The"; "&" as "and". "The Lancet. Neurology", "The Lancet Neurology" → "lancet neurology".
pub fn key(journal: &str) -> String {
    static BRACKETS: OnceLock<regex::Regex> = OnceLock::new();
    static SUBTITLE: OnceLock<regex::Regex> = OnceLock::new();
    let s = journal.to_lowercase().replace('&', " and ");
    let s = BRACKETS.get_or_init(|| regex::Regex::new(r"\([^()]*\)").unwrap()).replace_all(&s, " ");
    let s = SUBTITLE.get_or_init(|| regex::Regex::new(r"\s[:=]\s").unwrap()).split(&s).next().unwrap_or("").to_string();
    let words: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let s = words.join(" ");
    s.strip_prefix("the ").map(String::from).unwrap_or(s)
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        include_str!("journal_abbrev.tsv")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t'))
            .collect()
    })
}

/// The journal's abbreviation (NLM's: "Nature Methods" → "Nat Methods", "Bioinformatics
/// (Oxford, England)" → none: its abbreviation is its name), if the table has it.
pub fn abbreviation(journal: &str) -> Option<&'static str> {
    table().get(key(journal).as_str()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_as_citations_write_them() {
        for (stored, written) in [
            ("Nature methods", "Nature Methods"),
            ("Genome biology", "Genome Biology"),
            ("Bioinformatics (Oxford, England)", "Bioinformatics"),
            ("Science (New York, N.Y.)", "Science"),
            ("Methods in molecular biology (Clifton, N.J.)", "Methods in Molecular Biology"),
            ("Journal of immunology (Baltimore, Md. : 1950)", "Journal of Immunology"),
            ("Multiple sclerosis (Houndmills, Basingstoke, England)", "Multiple Sclerosis"),
            ("bioRxiv (Cold Spring Harbor Laboratory)", "bioRxiv"),
            ("arXiv (Cornell University)", "arXiv"),
            ("Brain : a journal of neurology", "Brain"),
            ("Breast cancer research : BCR", "Breast Cancer Research"),
            ("BioEssays : news and reviews in molecular, cellular and developmental biology", "BioEssays"),
            ("The Journal of biological chemistry", "The Journal of Biological Chemistry"),
            ("PLoS biology", "PLoS Biology"),
            ("BMC biology", "BMC Biology"),
            ("Biochimica et biophysica acta", "Biochimica et Biophysica Acta"),
            ("AJNR. American journal of neuroradiology", "AJNR. American Journal of Neuroradiology"),
            ("Proceedings of the 16th international conference on intelligent systems", "Proceedings of the 16th International Conference on Intelligent Systems"),
            // as they are: title case, odd capitals, acronyms in brackets
            ("Proceedings of the National Academy of Sciences", "Proceedings of the National Academy of Sciences"),
            ("eLife", "eLife"),
            ("iScience", "iScience"),
            ("mSystems", "mSystems"),
            ("npj Regenerative Medicine", "npj Regenerative Medicine"),
            ("npj regenerative medicine", "npj Regenerative Medicine"),
            ("PNAS", "PNAS"),
            ("Cell Death & Differentiation", "Cell Death & Differentiation"),
            ("Biochimica et Biophysica Acta (BBA) - Molecular Basis of Disease", "Biochimica et Biophysica Acta (BBA) - Molecular Basis of Disease"),
            ("International Journal on Document Analysis and Recognition (IJDAR)", "International Journal on Document Analysis and Recognition (IJDAR)"),
            ("Wiley Interdisciplinary Reviews: Systems Biology and Medicine", "Wiley Interdisciplinary Reviews: Systems Biology and Medicine"),
        ] {
            assert_eq!(clean_name(stored), written, "{stored}");
        }
    }

    #[test]
    fn abbreviations_from_nlm() {
        for (journal, abbr) in [
            ("Nature Methods", Some("Nat Methods")),
            ("Nature methods", Some("Nat Methods")),
            ("Genome Biology", Some("Genome Biol")),
            ("Nature Biotechnology", Some("Nat Biotechnol")),
            ("Proceedings of the National Academy of Sciences", Some("Proc Natl Acad Sci U S A")),
            ("Proceedings of the National Academy of Sciences of the United States of America", Some("Proc Natl Acad Sci U S A")),
            ("The Lancet Neurology", Some("Lancet Neurol")),
            ("The Journal of Neuroscience", Some("J Neurosci")),
            ("Journal of immunology (Baltimore, Md. : 1950)", Some("J Immunol")),
            ("Cell Death & Differentiation", Some("Cell Death Differ")),
            ("Biochimica et Biophysica Acta (BBA) - Molecular Basis of Disease", Some("Biochim Biophys Acta Mol Basis Dis")),
            ("New England Journal of Medicine", Some("N Engl J Med")),
            ("Nucleic Acids Research", Some("Nucleic Acids Res")),
            // the abbreviation is the name, or not a journal in the table
            ("Bioinformatics", None),
            ("Cell", None),
            ("eLife", None),
            ("Proceedings of the 2023 CHI Conference on Human Factors in Computing Systems", None),
        ] {
            assert_eq!(abbreviation(journal), abbr, "{journal}");
        }
    }
}
