//! Text made ready for speech, and cut into pieces the voice can start on quickly.
//!
//! `spoken` is the Apple apps' `Narrator.spoken` (apple/Shared/Narrator.swift), rule for rule, so a
//! paper sounds the same on every device, plus a few rules of its own (author–year citations,
//! "Table 2" after "Tab."). `chunks` cuts a paragraph into sentences, joined up to a length, so the
//! natural voice makes the first sentence of a long paragraph in a fraction of the time the whole
//! paragraph would take, and the next one while the first plays.

use regex::Regex;
use std::sync::OnceLock;

/// Apple's rules (Narrator.spoken), in its order.
fn apple_rules() -> &'static [(Regex, &'static str)] {
    static R: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    R.get_or_init(|| {
        [
            (r"\[\s*\d+(\s*[-–,]\s*\d+)*\s*\]", ""), // [12], [1–3], [4, 7]
            (r"https?://\S+", ""),
            (r"\bdoi:\s*\S+", ""),
            (r"\[equation\]", "equation"),
            (r"\bFigs?\.\s*", "Figure "),
            (r"\bEqs?\.\s*", "Equation "),
            (r"\bRefs?\.\s*", "reference "),
            (r"\be\.g\.,?", "for example"),
            (r"\bi\.e\.,?", "that is"),
            (r"\bet al\.", "et al"),
            (r"\bvs\.?\b", "versus"),
            (r"\bapprox\.?", "approximately"),
            (r"\bresp\.", "respectively"),
            (r"±", " plus or minus "),
            (r"×", " times "),
            (r"≤", " less than or equal to "),
            (r"≥", " greater than or equal to "),
            (r"<", " less than "),
            (r">", " greater than "),
            (r"~\s*(\d)", "about $1"), // Swift: ~\s*(?=\d)
            (r"(\d)\s*μm\b", "$1 micrometers"),
            (r"(\d)\s*nm\b", "$1 nanometers"),
            (r"(\d)\s*mm\b", "$1 millimeters"),
            (r"(\d)\s*%", "$1 percent"),
            (r"\bP\s*[<=]\s*", "P "),
            (r"\(\s*\)", ""),
            (r"\s{2,}", " "),
        ]
        .into_iter()
        .map(|(p, r)| (Regex::new(p).unwrap(), r))
        .collect()
    })
}

/// Rules of our own, applied before Apple's.
fn extra_rules() -> &'static [(Regex, &'static str)] {
    static R: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    R.get_or_init(|| {
        // a name in a citation: "Smith", "van der Berg", "O'Neil", "Smith-Jones"
        let name = r"(?:(?:van|von|de|der|den|du|da|di|le|la)\s+)*[A-Z][\p{L}'’\-]+";
        let author = format!(r"{name}(?:\s+(?:et\s+al\.?|and\s+{name}|&\s*{name}))?");
        let one = format!(r"(?:e\.g\.,?\s*|see\s+|cf\.\s*)?{author},?\s+(?:19|20)\d\d[a-z]?(?:,\s*(?:19|20)\d\d[a-z]?)*");
        [
            // (Smith et al., 2020), (Smith and Jones 2019; Lee, 2021a, 2022)
            (format!(r"\s*\(\s*{one}(?:\s*;\s*{one})*\s*\)"), ""),
            // [12a], [S1]: supplementary and lettered references, like [12]
            (r"\[\s*S?\d+[a-z]?(\s*[-–,]\s*S?\d+[a-z]?)*\s*\]".to_string(), ""),
            (r"\bTabs?\.\s*".to_string(), "Table "),
            (r"\bSupp?l?\.\s*".to_string(), "Supplementary "),
            (r"\bcf\.\s*".to_string(), "compare "),
            (r"\bvs\.\s".to_string(), "versus "),
            (r"\bca\.\s*(\d)".to_string(), "about $1"),
            (r"\bvol\.\s*(\d)".to_string(), "volume $1"),
            (r"\bno\.\s*(\d)".to_string(), "number $1"),
        ]
        .into_iter()
        .map(|(p, r)| (Regex::new(&p).unwrap(), r))
        .collect()
    })
}

/// Makes text read well aloud: drops citation markers, URLs and DOIs, expands abbreviations.
/// `kind` is the narration item's kind ("heading", "text", "caption").
pub fn spoken(text: &str, kind: &str) -> String {
    static DASH_RANGE: OnceLock<Regex> = OnceLock::new();
    static DASH: OnceLock<Regex> = OnceLock::new();
    let mut t = text.to_string();
    for (re, rep) in extra_rules().iter().chain(apple_rules()) {
        t = re.replace_all(&t, *rep).into_owned();
    }
    // number ranges read "to" ("10–20 mg"), other dashes are a pause
    t = DASH_RANGE.get_or_init(|| Regex::new(r"(\d)\s*–\s*(\d)").unwrap()).replace_all(&t, "$1 to $2").into_owned();
    t = DASH.get_or_init(|| Regex::new(r"\s*[–—]\s*").unwrap()).replace_all(&t, ", ").into_owned();
    let t = tidy(&t);
    if kind == "caption" && !t.is_empty() {
        format!("Figure caption. {t}")
    } else {
        t
    }
}

/// Spaces before punctuation and doubled punctuation left behind by removed citations.
fn tidy(t: &str) -> String {
    static SP: OnceLock<Regex> = OnceLock::new();
    static PUNCT: OnceLock<Regex> = OnceLock::new();
    let t = SP.get_or_init(|| Regex::new(r"\s+([.,;:!?])").unwrap()).replace_all(t, "$1");
    let t = PUNCT.get_or_init(|| Regex::new(r"([.,;:])[.,;:]+").unwrap()).replace_all(&t, "$1");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Words a full stop doesn't end a sentence after (lower-cased, without the stop).
const ABBREVIATIONS: &[&str] = &[
    "al", "approx", "ca", "cf", "co", "corp", "dept", "dr", "e.g", "eq", "eqs", "est", "et", "etc", "fig", "figs",
    "i.e", "inc", "jr", "ltd", "mr", "mrs", "ms", "nos", "p", "pp", "prof", "ref", "refs", "resp", "sr", "st",
    "suppl", "tab", "u.k", "u.s", "viz", "vol", "vs",
];

/// Cuts text into sentences. A full stop doesn't end one after an abbreviation, an initial
/// ("J. Smith"), inside a number ("3.5") or before a lower-case word.
pub fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '.' | '!' | '?' | '…') {
            // take closing quotes and brackets and repeated marks with the sentence
            let mut end = i + 1;
            while end < chars.len() && matches!(chars[end], '.' | '!' | '?' | '"' | '”' | '’' | '\'' | ')' | ']') {
                end += 1;
            }
            let at_space = end >= chars.len() || chars[end].is_whitespace();
            if at_space && (c != '.' || ends_sentence(&chars, start, i, end)) {
                push_trimmed(&mut out, &chars[start..end]);
                start = end;
            }
            i = end;
            continue;
        }
        i += 1;
    }
    push_trimmed(&mut out, &chars[start..]);
    out
}

fn push_trimmed(out: &mut Vec<String>, s: &[char]) {
    let s: String = s.iter().collect();
    let s = s.trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
}

/// Is the full stop at `dot` the end of a sentence? (`end`: just after the stop and anything closing it.)
fn ends_sentence(chars: &[char], start: usize, dot: usize, end: usize) -> bool {
    // the word before the stop
    let mut w = dot;
    while w > start && !chars[w - 1].is_whitespace() && chars[w - 1] != '(' {
        w -= 1;
    }
    let word: String = chars[w..dot].iter().collect::<String>().to_lowercase();
    let word = word.trim_start_matches(|c: char| !c.is_alphanumeric());
    if ABBREVIATIONS.contains(&word) {
        return false;
    }
    // an initial: one capital letter ("J. Smith"), but not a sentence ending in "I." or a gene "A."
    let letters: Vec<char> = chars[w..dot].iter().filter(|c| c.is_alphabetic()).copied().collect();
    if letters.len() == 1 && letters[0].is_uppercase() && dot - w == 1 {
        return false;
    }
    // the next word starts with a lower-case letter: not a new sentence
    let mut n = end;
    while n < chars.len() && chars[n].is_whitespace() {
        n += 1;
    }
    if n < chars.len() && chars[n].is_lowercase() {
        return false;
    }
    true
}

/// Pieces of a paragraph for the voice: whole sentences, joined while they stay under `max`
/// characters, except the first piece, which is joined only up to `FIRST` characters so playback
/// starts soon. A sentence longer than `max` is cut at a semicolon, colon or comma.
pub fn chunks(text: &str, max: usize) -> Vec<String> {
    let max = max.max(40);
    let mut pieces: Vec<String> = Vec::new();
    for s in sentences(text) {
        if s.chars().count() <= max {
            pieces.push(s);
        } else {
            pieces.extend(split_long(&s, max));
        }
    }
    let mut out: Vec<String> = Vec::new();
    for p in pieces {
        let limit = if out.len() == 1 { FIRST.min(max) } else { max };
        if let Some(last) = out.last_mut() {
            if last.chars().count() + 1 + p.chars().count() <= limit {
                last.push(' ');
                last.push_str(&p);
                continue;
            }
        }
        out.push(p);
    }
    out
}

/// The first piece of a paragraph is joined up to this many characters (a second or so of speech).
pub const FIRST: usize = 60;

/// Cuts a long sentence at clause marks (then at spaces) into pieces up to `max` characters.
fn split_long(s: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while rest.chars().count() > max {
        let cut_byte = rest.char_indices().nth(max).map(|(b, _)| b).unwrap_or(rest.len());
        let head = &rest[..cut_byte];
        // prefer ; then : then , then a space, in the later half of the window
        let half = head.len() / 2;
        let at = [";", ":", ","]
            .iter()
            .filter_map(|m| head.rfind(m).filter(|&p| p >= half).map(|p| p + m.len()))
            .next()
            .or_else(|| head.rfind(' ').filter(|&p| p > 0))
            .unwrap_or(cut_byte);
        out.push(rest[..at].trim().to_string());
        rest = rest[at..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_numbered_citations_and_links() {
        assert_eq!(spoken("Lactate rises [12] in CSF [1–3], see [4, 7].", "text"), "Lactate rises in CSF, see.");
        assert_eq!(spoken("Shown before [S2, 12a].", "text"), "Shown before.");
        assert_eq!(spoken("Data at https://example.org/x and doi:10.1/abc here.", "text"), "Data at and here.");
    }

    #[test]
    fn drops_author_year_citations() {
        assert_eq!(spoken("as shown before (Smith et al., 2020).", "text"), "as shown before.");
        assert_eq!(spoken("as shown (Smith and Jones 2019; Lee, 2021a, 2022) here", "text"), "as shown here");
        assert_eq!(spoken("as reviewed (e.g., van der Berg et al. 2018).", "text"), "as reviewed.");
        // a parenthesis that is not a citation stays
        assert_eq!(spoken("the cohort (n = 35, 2019 to 2021) was", "text"), "the cohort (n = 35, 2019 to 2021) was");
    }

    #[test]
    fn expands_abbreviations_and_symbols() {
        assert_eq!(spoken("See Fig. 2 and Figs. 3, Eq. 4, Tab. 1.", "text"), "See Figure 2 and Figure 3, Equation 4, Table 1.");
        assert_eq!(spoken("e.g., mice vs. rats, i.e. rodents", "text"), "for example mice versus rats, that is rodents");
        assert_eq!(spoken("Smith et al. found", "text"), "Smith et al found");
        assert_eq!(spoken("5 ± 2 mm, 3×, ≥ 4, 12%", "text"), "5 plus or minus 2 millimeters, 3 times, greater than or equal to 4, 12 percent");
        assert_eq!(spoken("~5 μm", "text"), "about 5 micrometers");
        assert_eq!(spoken("P < 0.05 and P = 0.3", "text"), "P less than 0.05 and P 0.3");
        assert_eq!(spoken("10–20 mg — twice", "text"), "10 to 20 mg, twice");
    }

    #[test]
    fn captions_and_equations() {
        assert_eq!(spoken("Cells in culture.", "caption"), "Figure caption. Cells in culture.");
        assert_eq!(spoken("where [equation] holds", "text"), "where equation holds");
        assert_eq!(spoken("[1]", "caption"), "");
    }

    #[test]
    fn sentences_respect_abbreviations_numbers_and_initials() {
        let s = sentences("Dr. Smith measured 3.5 mg. Then J. R. Jones et al. repeated it! Was it right? Yes (see Fig. 2). done.");
        assert_eq!(
            s,
            vec![
                "Dr. Smith measured 3.5 mg.",
                "Then J. R. Jones et al. repeated it!",
                "Was it right?",
                "Yes (see Fig. 2). done.",
            ]
        );
        assert_eq!(sentences("He said \"stop.\" Then left."), vec!["He said \"stop.\"", "Then left."]);
        assert_eq!(sentences("Ends without a stop"), vec!["Ends without a stop"]);
        assert_eq!(sentences("  "), Vec::<String>::new());
        assert_eq!(sentences("In the U.S. Army. Next."), vec!["In the U.S. Army.", "Next."]);
    }

    #[test]
    fn chunks_start_small_and_join_up_to_max() {
        let p = "First sentence here. Second one is a bit longer than that. Third. Fourth sentence ends it.";
        let c = chunks(p, 80);
        assert_eq!(c, vec!["First sentence here. Second one is a bit longer than that.", "Third. Fourth sentence ends it."]);
        assert_eq!(c.join(" "), p);
        // a long first sentence is a piece of its own; the rest join up to the limit
        let p = "This first sentence is long enough to be a piece of its own here. Two. Three. Four.";
        assert_eq!(chunks(p, 200), vec!["This first sentence is long enough to be a piece of its own here.", "Two. Three. Four."]);
    }

    #[test]
    fn long_sentences_are_cut_at_clauses() {
        let long = "We measured lactate, glucose and protein in all samples; the values were compared across groups, \
                    adjusted for age and sex, and tested with a mixed model that accounted for repeated measures within patients";
        let c = chunks(long, 100);
        assert!(c.len() >= 2, "{c:?}");
        assert!(c.iter().all(|x| x.chars().count() <= 100), "{c:?}");
        assert!(c[0].ends_with(';') || c[0].ends_with(','), "{c:?}");
        assert_eq!(c.join(" ").split_whitespace().count(), long.split_whitespace().count());
        // one long word longer than the limit is kept whole rather than lost
        let w = "x".repeat(150);
        assert_eq!(chunks(&w, 100).concat(), w);
    }
}
