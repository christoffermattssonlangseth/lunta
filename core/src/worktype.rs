//! What kind of work a paper is (journal article, conference paper, chapter, preprint…), for
//! BibTeX entry types, RIS types and CSL item types; and the clean-ups details from Crossref
//! need: titles and names in capitals, subtitles, and a book's series among its container titles.

use crate::model::PaperMeta;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Article,
    Conference,
    Chapter,
    Book,
    Preprint,
    Report,
    Thesis,
    Dataset,
    Standard,
    /// A web page (CSL `webpage`): a site's article, documentation, a news story online.
    WebPage,
    /// A blog post (CSL `post-weblog`).
    BlogPost,
    Other,
}

/// A work type as Crossref (or OpenAlex, CSL, BibTeX) names it, as one of ours. None for "".
pub fn from_type(t: &str) -> Option<Kind> {
    use Kind::*;
    Some(match t.trim().to_lowercase().as_str() {
        "" => return None,
        "journal-article" | "article-journal" | "article" | "review" | "letter" | "editorial" | "erratum" | "journal-issue"
        | "article-magazine" | "article-newspaper" | "paratext" => Article,
        "proceedings-article" | "paper-conference" | "conference-paper" | "inproceedings" | "conference" | "proceedings" => Conference,
        "book-chapter" | "chapter" | "book-section" | "book-part" | "reference-entry" | "incollection" | "inbook" | "entry" => Chapter,
        "book" | "monograph" | "edited-book" | "reference-book" | "book-set" | "book-series" | "book-track" => Book,
        "posted-content" | "preprint" => Preprint,
        "report" | "report-component" | "report-series" | "techreport" => Report,
        "dissertation" | "thesis" | "phdthesis" | "mastersthesis" => Thesis,
        "dataset" | "database" | "component" => Dataset,
        "standard" | "standard-series" => Standard,
        "webpage" | "web-page" | "online" | "electronic" | "www" | "website" | "post" => WebPage,
        "post-weblog" | "blog-post" | "blogpost" | "blog" => BlogPost,
        _ => Other,
    })
}

/// A web page or blog post (by its stored type, as Lunta stores them: "webpage", "post-weblog").
pub fn is_web(work_type: &str) -> bool {
    matches!(from_type(work_type), Some(Kind::WebPage | Kind::BlogPost))
}

/// The paper's kind: its stored type, or, when that is unknown (papers added before types were
/// kept, typed in by hand, or from a BibTeX `@misc`), what its details suggest.
pub fn kind(m: &PaperMeta) -> Kind {
    match from_type(&m.work_type) {
        Some(Kind::Other) | None => infer(m),
        Some(k) => k,
    }
}

fn infer(m: &PaperMeta) -> Kind {
    let j = m.journal.to_lowercase();
    let doi = m.doi.to_lowercase();
    let preprint_server = ["arxiv", "biorxiv", "medrxiv", "chemrxiv", "psyarxiv", "research square", "ssrn", "preprints.org", "osf preprints", "authorea"];
    if preprint_server.iter().any(|s| j.contains(s)) || j.contains("preprint") || doi.starts_with("10.48550/") {
        return Kind::Preprint;
    }
    if j.is_empty() && (!m.arxiv.is_empty() || crate::metadata::is_preprint_doi(&doi)) {
        return Kind::Preprint;
    }
    if is_conference_name(&j) || series_like(&j) {
        return Kind::Conference;
    }
    if !j.is_empty() {
        return Kind::Article;
    }
    Kind::Other
}

/// "Proceedings of the 2021 CHI Conference…", "ICML Workshop…": a conference's proceedings, not a
/// journal that happens to be called Proceedings (PNAS, Proc. R. Soc., Proceedings of the IEEE,
/// the ACM's PACM journals).
pub fn is_conference_name(j: &str) -> bool {
    let j = j.to_lowercase();
    let journals = [
        "national academy", "royal society", "proc natl acad", "proc. natl. acad", "proceedings of the ieee", "proc. ieee",
        "proceedings of the acm on", "vldb endowment", "mathematical society", "proceedings. biological", "nutrition society",
        "japan academy", "mayo clinic", "proceedings of the american", "proceedings of the zoological", "proceedings of the society",
        "proceedings of the institution", "journal",
    ];
    if journals.iter().any(|x| j.contains(x)) {
        return false;
    }
    ["proceedings", "conference", "symposium", "workshop", "colloquium", "proc. ", "meeting of the", "congress on"]
        .iter()
        .any(|x| j.contains(x))
}

/// A book series rather than a book: "Lecture Notes in Computer Science", "Communications in
/// Computer and Information Science", "Methods in Molecular Biology", "… Series".
pub fn series_like(t: &str) -> bool {
    let t = t.to_lowercase();
    ["lecture notes", "communications in computer", "advances in intelligent systems", "ifip advances", "methods in molecular biology",
     "smart innovation, systems", "studies in computational intelligence", "advances in experimental medicine", "springer proceedings in"]
        .iter()
        .any(|x| t.contains(x))
        || t.ends_with(" series")
}

/// Conference papers in a series (LNCS chapters) are `@inproceedings`, as BibTeX users cite
/// them; other chapters are `@incollection`.
pub fn chapter_in_proceedings(m: &PaperMeta) -> bool {
    let s = m.series.to_lowercase();
    is_conference_name(&m.journal)
        || s.contains("lecture notes")
        || s.contains("communications in computer")
        || s.contains("ifip")
        || s.contains("proceedings")
        || series_like(&m.journal)
}

/// The book (proceedings) title and the series among a Crossref record's container titles:
/// Springer chapters list both (["Lecture Notes in Computer Science", "Research and Advanced
/// Technology for Digital Libraries"]), the series in either place.
pub fn containers(titles: &[String]) -> (String, String) {
    let titles: Vec<&String> = titles.iter().filter(|t| !t.trim().is_empty()).collect();
    match titles.len() {
        0 => (String::new(), String::new()),
        1 => (titles[0].trim().to_string(), String::new()),
        _ => match titles.iter().position(|t| series_like(t)) {
            Some(i) => {
                let book = titles.iter().enumerate().find(|(k, _)| *k != i).map(|(_, t)| t.trim().to_string()).unwrap_or_default();
                (book, titles[i].trim().to_string())
            }
            None => (titles[0].trim().to_string(), String::new()),
        },
    }
}

/// "PDFFigures 2.0" + "Mining Figures from Research Papers" → "PDFFigures 2.0: Mining Figures…"
pub fn join_subtitle(title: &str, subtitle: &str) -> String {
    let (t, s) = (title.trim(), subtitle.trim());
    if s.is_empty() || t.to_lowercase().contains(&s.to_lowercase()) {
        return t.to_string();
    }
    if t.is_empty() {
        return s.to_string();
    }
    if t.ends_with([':', '?', '!', '.', '—', '–']) {
        format!("{t} {s}")
    } else {
        format!("{t}: {s}")
    }
}

/// At least 80% of the letters are capitals (and there are a few): a title or name typed in
/// capitals ("A NEW MEASURE OF RANK CORRELATION"), not one with acronyms ("RNA-seq of BRCA1").
pub fn mostly_upper(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    // a few words at least: "EPUB 3.3", "SPATIAL OMICS" or "CRISPR" may well be meant that way
    let words = s.split_whitespace().filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2).count();
    if letters.len() < 10 || words < 3 {
        return false;
    }
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    upper * 5 >= letters.len() * 4
}

const SMALL_WORDS: [&str; 22] = [
    "a", "an", "the", "and", "but", "or", "nor", "for", "so", "yet", "as", "at", "by", "in", "of", "off", "on", "per", "to", "up", "via", "vs",
];
/// Acronyms that stay in capitals when a title in capitals is put in title case.
const ACRONYMS: [&str; 36] = [
    "DNA", "RNA", "MRNA", "PCR", "HIV", "AIDS", "SARS", "COVID", "USA", "UK", "EU", "MRI", "CT", "ICU", "NMR", "ATP",
    "CRISPR", "UV", "GPS", "AI", "ML", "NLP", "PDF", "HTML", "XML", "API", "CPU", "GPU", "NASA", "IEEE", "ACM", "ECG", "EEG",
    "TB", "BMI", "ADHD",
];

/// II, IV, XIV… (not "I", which is also a word, nor words such as "CIVIL" or "MIX").
fn roman(w: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    w.len() > 1 && RE.get_or_init(|| regex::Regex::new(r"^X{0,3}(IX|IV|V?I{0,3})$").unwrap()).is_match(w)
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// One word of a title in capitals, in title case. `first`: at the start or after a colon.
fn title_word(w: &str, first: bool) -> String {
    // keep words with digits (BRCA1, COVID-19, H1N1), acronyms and Roman numerals as they are
    let core: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
    if core.chars().any(|c| c.is_ascii_digit()) || ACRONYMS.contains(&core.as_str()) || roman(&core) {
        return w.to_string();
    }
    let lower = w.to_lowercase();
    if !first && SMALL_WORDS.contains(&lower.trim_matches(|c: char| !c.is_alphanumeric())) {
        return lower;
    }
    // "RANK-ORDER" → "Rank-Order", "KENDALL'S" → "Kendall's", "(RE)ANALYSIS" → "(Re)analysis"
    let mut out = String::new();
    let mut start = true;
    for c in w.chars() {
        if c.is_alphabetic() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = matches!(c, '-' | '/' | '(' | '"' | '“');
        }
    }
    out
}

/// A title in capitals in title case; other titles as they are.
pub fn fix_title_case(title: &str) -> String {
    if !mostly_upper(title) {
        return title.to_string();
    }
    let mut out = vec![];
    let mut first = true;
    for w in title.split_whitespace() {
        out.push(title_word(w, first));
        first = w.ends_with([':', '?', '!', '.', '—']);
    }
    out.join(" ")
}

/// A name part in capitals ("KENDALL", "VAN DER WAALS", "O'BRIEN", "MCDONALD") in proper case;
/// a part with lower-case letters, initials ("M. G.", "MG") and Roman numerals stay as they are.
/// `family`: the family name, where two capitals ("LI", "WU") are a name, not initials.
pub fn fix_name_part(part: &str, family: bool) -> String {
    let letters: Vec<char> = part.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 2 || letters.iter().any(|c| c.is_lowercase()) {
        return part.to_string();
    }
    let words: Vec<&str> = part.split_whitespace().collect();
    let n = words.len();
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let core: String = w.chars().filter(|c| c.is_alphabetic()).collect();
            // initials: dotted ("M.G."), short given names in capitals ("MG"), or two capitals
            // without a vowel; a two-letter family name ("LI", "WU") is a name
            let vowel = core.chars().any(|c| "AEIOUY".contains(c));
            if roman(&core) || core.len() < 2 || w.contains('.') || (!family && core.len() < 3) || (core.len() == 2 && !vowel) {
                return w.to_string();
            }
            let lower = w.to_lowercase();
            let particles = ["van", "von", "de", "der", "den", "da", "di", "du", "del", "della", "la", "le", "ten", "ter", "dos", "das", "bin"];
            if family && i + 1 < n && particles.contains(&lower.as_str()) {
                return lower;
            }
            let mut out = String::new();
            let mut start = true;
            for c in w.chars() {
                if c.is_alphabetic() {
                    if start { out.extend(c.to_uppercase()) } else { out.extend(c.to_lowercase()) }
                    start = false;
                } else {
                    out.push(c);
                    start = c == '-' || c == '\'' || c == '’';
                }
            }
            // "Mcdonald" → "McDonald"
            if out.len() > 3 && out.starts_with("Mc") {
                let (a, b) = out.split_at(2);
                out = format!("{a}{}", capitalize(b));
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A whole author name ("M. G. KENDALL", "KENDALL, M. G.") with its parts in capitals fixed.
pub fn fix_name_case(name: &str) -> String {
    let n = name.trim();
    if let Some((fam, giv)) = n.split_once(',') {
        return format!("{}, {}", fix_name_part(fam.trim(), true), fix_name_part(giv.trim(), false));
    }
    let (fam, giv) = crate::export::split_name(n);
    if giv.is_empty() {
        return n.to_string(); // one word: maybe an acronym (an organization)
    }
    format!("{} {}", fix_name_part(&giv, false), fix_name_part(&fam, true))
}

/// An author that is an organization ("World Wide Web Consortium", "Qwen Team"), written whole
/// in BibTeX (`{…}`) and CSL (a literal) rather than split into family and given names.
pub fn is_organization(name: &str) -> bool {
    let words = [
        "consortium", "organization", "organisation", "institute", "association", "society", "committee", "collaboration", "team",
        "project", "group", "foundation", "agency", "council", "university", "center", "centre", "inc", "inc.", "llc", "ltd", "ltd.",
        "corporation", "company", "network", "initiative", "contributors", "developers", "authors", "programme", "program",
        "office", "department", "ministry", "board", "commission", "laboratory", "labs", "alliance", "federation", "working",
    ];
    !name.contains(',') && name.split_whitespace().count() >= 2 && name.split_whitespace().any(|w| words.contains(&w.to_lowercase().as_str()))
}

/// A proceedings title without the stray "on" in ACM's Crossref records: "Proceedings of the 16th
/// ACM/IEEE-CS on Joint Conference on Digital Libraries" → "Proceedings of the 16th ACM/IEEE-CS
/// Joint Conference on Digital Libraries", "Proceedings of the 2016 on Genetic and Evolutionary
/// Computation Conference" → "… 2016 Genetic and …". Only when every word between "Proceedings of
/// the" and "on" is an ordinal, a year or an acronym (ACM, IEEE-CS, SIGPLAN) and a conference,
/// symposium, workshop or congress follows: "Proceedings of the ACM on Programming Languages" (a
/// journal) and "… Workshop on …" stay. No acronym such as "(JCDL)" is added: Crossref doesn't
/// have it, and an invented one could be wrong.
pub fn fix_proceedings_title(j: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^(Proceedings of the (?:(?:\d+(?:st|nd|rd|th)|\d{4}|[A-Z][A-Z0-9/&-]*[A-Z0-9]) )+)on ([A-Z].*\b(?:Conference|Symposium|Workshop|Congress)\b.*)$").unwrap()
    });
    match re.captures(j) {
        Some(c) => format!("{}{}", &c[1], &c[2]),
        None => j.to_string(),
    }
}

/// The details as they are written out (BibTeX, RIS, citations): a title or names in capitals
/// in proper case, a proceedings title without Crossref's stray "on". The library itself is not
/// changed.
pub fn for_export(m: &PaperMeta) -> PaperMeta {
    let mut o = m.clone();
    o.journal = fix_proceedings_title(&m.journal);
    o.title = fix_title_case(&m.title);
    o.authors = m.authors.iter().map(|a| if is_organization(a) { a.clone() } else { fix_name_case(a) }).collect();
    if mostly_upper(&m.journal) && m.journal.split_whitespace().count() > 1 {
        o.journal = fix_title_case(&o.journal);
    }
    o
}

/// Details as they come from Crossref and the other sources, cleaned: capitals, the type.
pub fn tidy(m: &mut PaperMeta) {
    m.title = fix_title_case(&m.title);
    m.journal = fix_proceedings_title(&m.journal);
    m.authors = m.authors.iter().map(|a| if is_organization(a) { a.clone() } else { fix_name_case(a) }).collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(journal: &str, doi: &str, arxiv: &str) -> PaperMeta {
        PaperMeta { title: "T".into(), journal: journal.into(), doi: doi.into(), arxiv: arxiv.into(), ..Default::default() }
    }

    #[test]
    fn proceedings_titles_without_stray_on() {
        for (crossref, fixed) in [
            ("Proceedings of the 16th ACM/IEEE-CS on Joint Conference on Digital Libraries", "Proceedings of the 16th ACM/IEEE-CS Joint Conference on Digital Libraries"),
            ("Proceedings of the 2016 on Genetic and Evolutionary Computation Conference", "Proceedings of the 2016 Genetic and Evolutionary Computation Conference"),
            // journals and ordinary titles stay
            ("Proceedings of the ACM on Programming Languages", "Proceedings of the ACM on Programming Languages"),
            ("Proceedings of the ACM on Human-Computer Interaction", "Proceedings of the ACM on Human-Computer Interaction"),
            ("Proceedings of the 2nd ACM Workshop on Hot Topics in Networks", "Proceedings of the 2nd ACM Workshop on Hot Topics in Networks"),
            ("Proceedings of the SIGCHI Conference on Human Factors in Computing Systems", "Proceedings of the SIGCHI Conference on Human Factors in Computing Systems"),
        ] {
            assert_eq!(fix_proceedings_title(crossref), fixed);
        }
    }

    #[test]
    fn crossref_types() {
        for (t, k) in [
            ("journal-article", Kind::Article),
            ("proceedings-article", Kind::Conference),
            ("book-chapter", Kind::Chapter),
            ("book-part", Kind::Chapter),
            ("reference-entry", Kind::Chapter),
            ("book", Kind::Book),
            ("monograph", Kind::Book),
            ("edited-book", Kind::Book),
            ("posted-content", Kind::Preprint),
            ("report", Kind::Report),
            ("dissertation", Kind::Thesis),
            ("dataset", Kind::Dataset),
            ("standard", Kind::Standard),
            ("webpage", Kind::WebPage),
            ("post-weblog", Kind::BlogPost),
            ("online", Kind::WebPage),
            ("peer-review", Kind::Other),
        ] {
            assert_eq!(from_type(t), Some(k), "{t}");
        }
        assert_eq!(from_type(""), None);
    }

    #[test]
    fn types_inferred_when_unknown() {
        assert_eq!(kind(&meta("Proceedings of the 2021 CHI Conference on Human Factors in Computing Systems", "10.1145/1", "")), Kind::Conference);
        assert_eq!(kind(&meta("Lecture Notes in Computer Science", "10.1007/978-3-642-04346-8_62", "")), Kind::Conference);
        assert_eq!(kind(&meta("arXiv", "10.48550/arXiv.2308.13418", "2308.13418")), Kind::Preprint);
        assert_eq!(kind(&meta("", "", "2308.13418")), Kind::Preprint);
        assert_eq!(kind(&meta("", "10.1101/2024.01.01.123456", "")), Kind::Preprint);
        assert_eq!(kind(&meta("bioRxiv", "10.1101/2024.01.01.123456", "")), Kind::Preprint);
        assert_eq!(kind(&meta("Proceedings of the National Academy of Sciences", "10.1073/pnas.1", "")), Kind::Article);
        assert_eq!(kind(&meta("Proceedings of the IEEE", "", "")), Kind::Article);
        assert_eq!(kind(&meta("Proceedings of the ACM on Human-Computer Interaction", "", "")), Kind::Article);
        assert_eq!(kind(&meta("Nature", "10.1038/x", "")), Kind::Article);
        assert_eq!(kind(&meta("", "", "")), Kind::Other);
        // a stored type wins over the guess
        let mut m = meta("arXiv", "", "2308.13418");
        m.work_type = "journal-article".into();
        assert_eq!(kind(&m), Kind::Article);
    }

    #[test]
    fn containers_with_series() {
        let t = |v: &[&str]| containers(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            t(&["Lecture Notes in Computer Science", "Research and Advanced Technology for Digital Libraries"]),
            ("Research and Advanced Technology for Digital Libraries".into(), "Lecture Notes in Computer Science".into())
        );
        assert_eq!(
            t(&["Stabilization, Safety, and Security of Distributed Systems", "Lecture Notes in Computer Science"]),
            ("Stabilization, Safety, and Security of Distributed Systems".into(), "Lecture Notes in Computer Science".into())
        );
        assert_eq!(t(&["Biometrika"]), ("Biometrika".into(), String::new()));
        assert_eq!(t(&[]), (String::new(), String::new()));
    }

    #[test]
    fn subtitles() {
        assert_eq!(join_subtitle("PDFFigures 2.0", "Mining Figures from Research Papers"), "PDFFigures 2.0: Mining Figures from Research Papers");
        assert_eq!(join_subtitle("Why read?", "A study"), "Why read? A study");
        assert_eq!(join_subtitle("CiteSee: Augmenting citations", "Augmenting citations"), "CiteSee: Augmenting citations");
        assert_eq!(join_subtitle("Title", ""), "Title");
    }

    #[test]
    fn titles_in_capitals() {
        assert_eq!(fix_title_case("A NEW MEASURE OF RANK CORRELATION"), "A New Measure of Rank Correlation");
        assert_eq!(fix_title_case("THE ROLE OF BRCA1 IN DNA REPAIR: A REVIEW"), "The Role of BRCA1 in DNA Repair: A Review");
        assert_eq!(fix_title_case("COVID-19 AND THE RANK-ORDER OF KENDALL'S TAU"), "COVID-19 and the Rank-Order of Kendall's Tau");
        assert_eq!(fix_title_case("WORLD WAR II AND SCIENCE"), "World War II and Science");
        // mixed case: left alone, acronyms and all
        for t in [
            "RNA-seq reveals BRCA1 targets in COVID-19 patients",
            "CRISPR screens in HEK293T cells",
            "GROBID: Combining Automatic Bibliographic Data Recognition",
            "A new measure of rank correlation",
            "DNA",
            "EPUB 3.3",
            "JATS: Journal Article Tag Suite",
            "SPATIAL OMICS",
        ] {
            assert_eq!(fix_title_case(t), t);
        }
        assert!(!mostly_upper("RNA-seq of BRCA1 in TP53 mutant AML"));
        assert!(mostly_upper("RNA-SEQ OF BRCA1 IN TP53 MUTANT AML"));
    }

    #[test]
    fn names_in_capitals() {
        assert_eq!(fix_name_case("M. G. KENDALL"), "M. G. Kendall");
        assert_eq!(fix_name_case("KENDALL, M. G."), "Kendall, M. G.");
        assert_eq!(fix_name_case("MAURICE G. KENDALL"), "Maurice G. Kendall");
        assert_eq!(fix_name_case("Wei ZHANG"), "Wei Zhang");
        assert_eq!(fix_name_case("Jing LI"), "Jing Li");
        assert_eq!(fix_name_case("PETER VAN HARDENBERG"), "Peter van Hardenberg");
        assert_eq!(fix_name_case("SEAN O'BRIEN"), "Sean O'Brien");
        assert_eq!(fix_name_case("ANNA SMITH-JONES"), "Anna Smith-Jones");
        assert_eq!(fix_name_case("JOHN MCDONALD"), "John McDonald");
        // untouched: proper names, initials, single words (organizations, acronyms)
        for n in ["Christoffer Mattsson Langseth", "Kendall MG", "MG Kendall", "J. J. Cadiz", "WHO", "Ludwig van Beethoven", "A. J. Bernheim Brush"] {
            assert_eq!(fix_name_case(n), n);
        }
    }

    #[test]
    fn organizations() {
        assert!(is_organization("World Wide Web Consortium"));
        assert!(is_organization("Qwen Team"));
        assert!(is_organization("National Information Standards Organization"));
        assert!(!is_organization("Maurice G. Kendall"));
        assert!(!is_organization("Apple"));
    }
}
