//! An impact prior for new papers: how much the place a paper appeared in, and the attention it
//! has had so far, say about it before anyone has read it. Used two ways:
//!
//! - **The feed** (run.rs, and WatchCenter.score in apple/Shared/Watches.swift through
//!   [`foryou_weigh`]): closeness stays what decides; a paper in a top journal or already cited
//!   gets at most [`IMPACT_WEIGHT`] (15 %) more of its closeness: `closeness × (1 + 0.15 ×
//!   impact)`. Two papers swap places only when they were within 15 % of each other.
//! - **Across science** (across.rs): impact is what picks the day's standout paper in each field.
//!
//! The impact of a paper is between 0 and 1: `0.75 × tier + 0.25 × attention`.
//!
//! - **tier**: the journal's, from [`JOURNALS`] (by ISSN or name): 1 for Nature, Science and Cell,
//!   0.9 for the medical flagships, 0.8 for a field's flagships, 0.6–0.7 for strong journals, 0.4–0.5
//!   for large broad journals (PNAS, Nature Communications, Science Advances, eLife); 0 for any
//!   other journal and for preprints. The same table gives "From the journals you follow" its
//!   ISSNs (journals.rs).
//! - **attention**: what is in the data already, never an extra request: OpenAlex's
//!   `cited_by_count` (log scale, 1 at 50 citations) and, where OpenAlex gave it, the field-weighted
//!   citation impact `fwci` (1 at an FWCI of 10); the larger of the two. Papers from the last days
//!   rarely have either, so for them the journal is what counts.
//!
//! Author track records are not used: they would need a request per author.

/// How much of a paper's closeness impact can add in the feed (closeness × (1 + IMPACT_WEIGHT ×
/// impact)). Modest on purpose: the feed is about your research; impact only orders papers that
/// are about as close.
pub const IMPACT_WEIGHT: f64 = 0.15;

/// A journal with its ISSNs (print, electronic), its tier, and the broad field it publishes in
/// ("" for journals across fields; across.rs's field ids).
#[derive(Clone, Copy, Debug)]
pub struct TierJournal {
    pub name: &'static str,
    pub issn: &'static [&'static str],
    pub tier: f64,
    pub field: &'static str,
}

const fn j(name: &'static str, issn: &'static [&'static str], tier: f64, field: &'static str) -> TierJournal {
    TierJournal { name, issn, tier, field }
}

/// Top multidisciplinary journals and each field's flagships. The first ISSN of each is the one
/// Across science asks OpenAlex for (OpenAlex matches any of a journal's ISSNs).
pub const JOURNALS: &[TierJournal] = &[
    // across fields
    j("Nature", &["0028-0836", "1476-4687"], 1.0, ""),
    j("Science", &["0036-8075", "1095-9203"], 1.0, ""),
    j("Cell", &["0092-8674"], 1.0, ""),
    j("PNAS", &["0027-8424", "1091-6490"], 0.5, ""),
    j("Nature Communications", &["2041-1723"], 0.4, ""),
    j("Science Advances", &["2375-2548"], 0.4, ""),
    j("eLife", &["2050-084X"], 0.4, ""),
    // cell and molecular biology
    j("Molecular Cell", &["1097-2765"], 0.8, "cellbio"),
    j("Nature Cell Biology", &["1465-7392", "1476-4679"], 0.8, "cellbio"),
    j("Nature Structural & Molecular Biology", &["1545-9993", "1545-9985"], 0.8, "cellbio"),
    j("Cell Stem Cell", &["1934-5909"], 0.8, "cellbio"),
    j("Nature Chemical Biology", &["1552-4450", "1552-4469"], 0.7, "cellbio"),
    j("Developmental Cell", &["1534-5807"], 0.6, "cellbio"),
    j("The EMBO Journal", &["0261-4189", "1460-2075"], 0.6, "cellbio"),
    // genetics and genomics
    j("Nature Genetics", &["1061-4036", "1546-1718"], 0.8, "genetics"),
    j("Nature Biotechnology", &["1087-0156", "1546-1696"], 0.8, "genetics"),
    j("Nature Methods", &["1548-7091", "1548-7105"], 0.8, "genetics"),
    j("American Journal of Human Genetics", &["0002-9297", "1537-6605"], 0.6, "genetics"),
    j("Genome Research", &["1088-9051", "1549-5469"], 0.6, "genetics"),
    // neuroscience
    j("Nature Neuroscience", &["1097-6256", "1546-1726"], 0.8, "neuro"),
    j("Neuron", &["0896-6273"], 0.8, "neuro"),
    // immunology and microbiology
    j("Nature Immunology", &["1529-2908", "1529-2916"], 0.8, "immuno"),
    j("Immunity", &["1074-7613"], 0.8, "immuno"),
    j("Science Immunology", &["2470-9468"], 0.7, "immuno"),
    j("Nature Microbiology", &["2058-5276"], 0.8, "immuno"),
    j("Cell Host & Microbe", &["1931-3128"], 0.6, "immuno"),
    // medicine
    j("New England Journal of Medicine", &["0028-4793", "1533-4406"], 0.9, "medicine"),
    j("The Lancet", &["0140-6736", "1474-547X"], 0.9, "medicine"),
    j("JAMA", &["0098-7484", "1538-3598"], 0.9, "medicine"),
    j("Nature Medicine", &["1078-8956", "1546-170X"], 0.9, "medicine"),
    j("BMJ", &["0959-8138", "1756-1833"], 0.8, "medicine"),
    j("Science Translational Medicine", &["1946-6234", "1946-6242"], 0.8, "medicine"),
    // ecology, evolution and plants
    j("Nature Ecology & Evolution", &["2397-334X"], 0.8, "ecology"),
    j("Nature Plants", &["2055-0278"], 0.8, "ecology"),
    j("Current Biology", &["0960-9822"], 0.6, "ecology"),
    j("Ecology Letters", &["1461-023X", "1461-0248"], 0.6, "ecology"),
    // chemistry
    j("Journal of the American Chemical Society", &["0002-7863", "1520-5126"], 0.8, "chemistry"),
    j("Nature Chemistry", &["1755-4330", "1755-4349"], 0.8, "chemistry"),
    j("Nature Catalysis", &["2520-1158"], 0.8, "chemistry"),
    j("Angewandte Chemie International Edition", &["1433-7851", "1521-3773"], 0.7, "chemistry"),
    j("Chem", &["2451-9294"], 0.7, "chemistry"),
    // materials, energy and engineering
    j("Nature Materials", &["1476-1122", "1476-4660"], 0.8, "materials"),
    j("Nature Nanotechnology", &["1748-3387", "1748-3395"], 0.8, "materials"),
    j("Nature Energy", &["2058-7546"], 0.8, "materials"),
    j("Nature Electronics", &["2520-1131"], 0.8, "materials"),
    j("Joule", &["2542-4351"], 0.7, "materials"),
    j("Advanced Materials", &["0935-9648", "1521-4095"], 0.6, "materials"),
    // physics and astronomy
    j("Physical Review Letters", &["0031-9007", "1079-7114"], 0.8, "physics"),
    j("Nature Physics", &["1745-2473", "1745-2481"], 0.8, "physics"),
    j("Nature Photonics", &["1749-4885", "1749-4893"], 0.8, "physics"),
    j("Nature Astronomy", &["2397-3366"], 0.8, "physics"),
    j("Physical Review X", &["2160-3308"], 0.8, "physics"),
    // earth and environment
    j("Nature Geoscience", &["1752-0894", "1752-0908"], 0.8, "earth"),
    j("Nature Climate Change", &["1758-678X", "1758-6798"], 0.8, "earth"),
    j("Nature Sustainability", &["2398-9629"], 0.8, "earth"),
    j("Nature Water", &["2731-6084"], 0.7, "earth"),
    j("One Earth", &["2590-3322"], 0.6, "earth"),
    // computer science and AI
    j("Nature Machine Intelligence", &["2522-5839"], 0.8, "computing"),
    j("Nature Computational Science", &["2662-8457"], 0.8, "computing"),
    j("IEEE Transactions on Pattern Analysis and Machine Intelligence", &["0162-8828", "1939-3539"], 0.7, "computing"),
    j("Journal of Machine Learning Research", &["1532-4435", "1533-7928"], 0.7, "computing"),
    // mathematics
    j("Annals of Mathematics", &["0003-486X", "1939-8980"], 0.8, "math"),
    j("Inventiones mathematicae", &["0020-9910", "1432-1297"], 0.8, "math"),
    j("Journal of the American Mathematical Society", &["0894-0347", "1088-6834"], 0.8, "math"),
    j("Acta Mathematica", &["0001-5962", "1871-2509"], 0.8, "math"),
    j("Publications mathématiques de l'IHÉS", &["0073-8301", "1618-1913"], 0.8, "math"),
    // psychology and social sciences
    j("Nature Human Behaviour", &["2397-3374"], 0.8, "society"),
    j("American Economic Review", &["0002-8282", "1944-7981"], 0.8, "society"),
    j("The Quarterly Journal of Economics", &["0033-5533", "1531-4650"], 0.8, "society"),
    j("Psychological Science", &["0956-7976", "1467-9280"], 0.7, "society"),
    j("American Sociological Review", &["0003-1224", "1939-8271"], 0.7, "society"),
];

/// Other names of these journals (as OpenAlex, Crossref or people write them).
pub const ALIASES: &[(&str, &str)] = &[
    ("Proceedings of the National Academy of Sciences", "PNAS"),
    ("Proceedings of the National Academy of Sciences of the United States of America", "PNAS"),
    ("The New England Journal of Medicine", "New England Journal of Medicine"),
    ("NEJM", "New England Journal of Medicine"),
    ("Lancet", "The Lancet"),
    ("JAMA: The Journal of the American Medical Association", "JAMA"),
    ("Journal of the American Medical Association", "JAMA"),
    ("British Medical Journal", "BMJ"),
    ("JACS", "Journal of the American Chemical Society"),
    ("Angewandte Chemie", "Angewandte Chemie International Edition"),
    ("EMBO Journal", "The EMBO Journal"),
    ("The American Journal of Human Genetics", "American Journal of Human Genetics"),
    ("PRL", "Physical Review Letters"),
    ("Quarterly Journal of Economics", "The Quarterly Journal of Economics"),
    ("TPAMI", "IEEE Transactions on Pattern Analysis and Machine Intelligence"),
    ("JMLR", "Journal of Machine Learning Research"),
    ("Publications Mathématiques de l'IHÉS", "Publications mathématiques de l'IHÉS"),
];

/// A journal name for comparing: lower case, letters and digits, without "the" and "and".
pub fn norm(name: &str) -> String {
    let n: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    n.split_whitespace().filter(|w| *w != "the" && *w != "and").collect::<Vec<_>>().join(" ")
}

/// The journal in the table by this name (or another name for it).
pub fn by_name(name: &str) -> Option<&'static TierJournal> {
    let n = norm(name);
    if n.is_empty() {
        return None;
    }
    let n = ALIASES.iter().find(|(a, _)| norm(a) == n).map(|(_, to)| norm(to)).unwrap_or(n);
    JOURNALS.iter().find(|j| norm(j.name) == n)
}

/// The journal in the table with this ISSN.
pub fn by_issn(issn: &str) -> Option<&'static TierJournal> {
    let i = issn.trim().to_uppercase();
    JOURNALS.iter().find(|j| j.issn.iter().any(|x| *x == i))
}

/// The tier of a journal (by name), 0 when it isn't in the table.
pub fn tier(journal: &str) -> f64 {
    by_name(journal).map_or(0.0, |j| j.tier)
}

/// Early attention, 0 to 1: citations on a log scale (1 at 50) or the FWCI (1 at 10), whichever
/// says more.
pub fn attention(cited_by: u32, fwci: f64) -> f64 {
    let cites = ((1.0 + cited_by as f64).ln() / 51f64.ln()).min(1.0);
    let f = if fwci.is_finite() { (fwci / 10.0).clamp(0.0, 1.0) } else { 0.0 };
    cites.max(f)
}

/// A paper's impact prior, 0 to 1, from a journal's tier and its attention so far.
pub fn impact_of(tier: f64, cited_by: u32, fwci: f64) -> f64 {
    (0.75 * tier + 0.25 * attention(cited_by, fwci)).clamp(0.0, 1.0)
}

/// A paper's impact prior by its journal's name.
pub fn impact(journal: &str, cited_by: u32, fwci: f64) -> f64 {
    impact_of(tier(journal), cited_by, fwci)
}

/// Closeness weighted by impact: `closeness × (1 + IMPACT_WEIGHT × impact)`. A closeness of 0 or
/// below (nothing in common, or like papers you turned down) is left as it is.
pub fn weigh(closeness: f64, impact: f64) -> f64 {
    if closeness > 0.0 && closeness.is_finite() {
        closeness * (1.0 + IMPACT_WEIGHT * impact.clamp(0.0, 1.0))
    } else {
        closeness
    }
}

/// The feed's weighting for the Apple apps (WatchCenter.score's closeness of a paper, its journal
/// and its OpenAlex citation count), so feeds made on a Mac and elsewhere rank alike.
#[uniffi::export]
pub fn foryou_weigh(closeness: f64, journal: String, cited_by: u32) -> f64 {
    weigh(closeness, impact(&journal, cited_by, 0.0))
}
