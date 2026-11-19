//! Metadata lookup: Crossref (DOI), Europe PMC (PMID / PMCID), arXiv.

use crate::model::*;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

const UA: &str = "Lunta/0.1 (reference manager)";

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into()
    })
}

pub(crate) fn get_text(url: &str) -> Result<String> {
    // "too many requests" (OpenAlex's rate limit) or "unavailable": wait a little and try again
    let waits = [Duration::from_millis(1500), Duration::from_secs(4)];
    let mut attempt = 0;
    loop {
        match agent().get(url).header("User-Agent", UA).call() {
            Ok(mut r) => return r.body_mut().read_to_string().map_err(|e| CoreError::Network(e.to_string())),
            Err(ureq::Error::StatusCode(code @ (429 | 503))) => {
                if let Some(w) = waits.get(attempt) {
                    std::thread::sleep(*w);
                    attempt += 1;
                    continue;
                }
                let host = url.split('/').nth(2).unwrap_or("the server");
                return Err(CoreError::Network(if code == 429 {
                    format!("{host} is busy (too many requests right now); try again in a minute")
                } else {
                    format!("{host} is unavailable right now; try again in a minute")
                }));
            }
            Err(e) => return Err(CoreError::Network(e.to_string())),
        }
    }
}

pub(crate) fn get_json(url: &str) -> Result<Value> {
    serde_json::from_str(&get_text(url)?).map_err(|e| CoreError::Network(format!("bad JSON: {e}")))
}

pub(crate) fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/:".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

pub fn doi_in(s: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"(?i)(10\.\d{4,9}/[^\s"<>?#]+)"#).unwrap());
    let m = re.captures(s)?.get(1)?.as_str();
    let mut d = m.trim_end_matches(['.', ',', ';', ')']).to_string();
    for suf in ["/full", "/abstract", "/pdf", ".pdf", ".full"] {
        if d.to_lowercase().ends_with(suf) {
            d.truncate(d.len() - suf.len());
        }
    }
    Some(d)
}

pub fn normalize_doi(s: &str) -> String {
    doi_in(s).unwrap_or_default().to_lowercase()
}

fn arxiv_in(s: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)(?:arxiv\.org/(?:abs|pdf|html)/|arxiv:|10\.48550/arxiv\.)?(\d{4}\.\d{4,5})(?:v\d+)?").unwrap()
    });
    let looks = s.to_lowercase().contains("arxiv") || Regex::new(r"^\d{4}\.\d{4,5}(v\d+)?$").unwrap().is_match(s.trim());
    if !looks {
        return None;
    }
    Some(re.captures(s)?.get(1)?.as_str().to_string())
}

/// Looks up metadata for a DOI, PMID, PMCID, arXiv ID, or a link containing one of these.
pub fn resolve(input: &str) -> Result<PaperMeta> {
    let s = input.trim();
    if let Some(a) = arxiv_in(s) {
        return arxiv(&a);
    }
    if let Some(pmc) = Regex::new(r"(?i)\b(PMC\d{4,})\b").unwrap().captures(s) {
        return europe_pmc(&format!("PMCID:{}", pmc[1].to_uppercase()));
    }
    if let Some(d) = doi_in(s) {
        let mut m = crossref(&d)?;
        // Crossref rarely has PubMed IDs or abstracts for biomedical papers; Europe PMC often does
        if let Ok(e) = europe_pmc(&format!("DOI:\"{d}\"")) {
            fill(&mut m, e);
        }
        return Ok(m);
    }
    let pm = Regex::new(r"(?:pubmed\.ncbi\.nlm\.nih\.gov/|^)(\d{5,9})/?$").unwrap();
    if let Some(c) = pm.captures(s) {
        return europe_pmc(&format!("EXT_ID:{} AND SRC:MED", &c[1]));
    }
    Err(CoreError::Invalid(format!("no DOI, PubMed ID, PMCID or arXiv ID found in “{s}”")))
}

fn fill(m: &mut PaperMeta, o: PaperMeta) {
    macro_rules! f { ($($x:ident),*) => { $( if m.$x.is_empty() { m.$x = o.$x.clone(); } )* } }
    f!(title, authors, date, journal, volume, issue, pages, publisher, doi, pmid, pmcid, arxiv, url, abstract_text, keywords, license, work_type, series);
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.first().map(s).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Removes markup (JATS/HTML tags) from a title or abstract. Only real tags go: a "<" followed by
/// a tag name. A less-than sign in the text ("P<0.001", "n < 5") is kept; the old pattern took
/// everything from it to the next ">" with it, cutting abstracts in half.
pub(crate) fn strip_tags(x: &str) -> String {
    let re = Regex::new(r#"</?[A-Za-z][A-Za-z0-9:_.-]*(?:\s+[A-Za-z_:][-A-Za-z0-9_:.]*(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'<>]+))?)*\s*/?>"#).unwrap();
    let t = re.replace_all(x, " ");
    t.split_whitespace().collect::<Vec<_>>().join(" ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
}

pub fn crossref(doi: &str) -> Result<PaperMeta> {
    let j = get_json(&format!("https://api.crossref.org/works/{}", enc(doi)))?;
    let m = &j["message"];
    if m.is_null() {
        return Err(CoreError::NotFound(format!("Crossref has no record for {doi}")));
    }
    let authors = m["author"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| {
                    // some publishers send names in capitals ("M. G.", "KENDALL")
                    let given = crate::worktype::fix_name_part(&s(&x["given"]), false);
                    let family = crate::worktype::fix_name_part(&s(&x["family"]), true);
                    let n = format!("{given} {family}").trim().to_string();
                    if n.is_empty() { s(&x["name"]) } else { n }
                })
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let parts = m["issued"]["date-parts"][0].as_array().cloned().unwrap_or_default();
    let date = parts
        .iter()
        .filter_map(|p| p.as_i64())
        .enumerate()
        .map(|(i, v)| if i == 0 { format!("{v:04}") } else { format!("{v:02}") })
        .collect::<Vec<_>>()
        .join("-");
    let license = m["license"]
        .as_array()
        .and_then(|a| a.iter().map(|l| s(&l["URL"])).find(|u| u.contains("creativecommons")))
        .unwrap_or_default();
    let work_type = s(&m["type"]);
    let containers: Vec<String> = match &m["container-title"] {
        Value::Array(a) => a.iter().map(|t| strip_tags(&s(t))).collect(),
        v => vec![strip_tags(&s(v))],
    };
    let (mut journal, mut series) = crate::worktype::containers(&containers);
    if work_type == "book" || work_type == "monograph" || work_type == "edited-book" {
        // a book's container is its series
        if series.is_empty() {
            series = std::mem::take(&mut journal);
        }
    }
    // "PDFFigures 2.0" + "Mining Figures from Research Papers"; a title in capitals in title case
    let title = crate::worktype::join_subtitle(&strip_tags(&s(&m["title"])), &strip_tags(&s(&m["subtitle"])));
    let title = crate::worktype::fix_title_case(&title);
    let publisher = match work_type.as_str() {
        // reports and theses: the institution, when Crossref names it
        "report" | "dissertation" if !s(&m["institution"]["name"]).is_empty() => s(&m["institution"]["name"]),
        _ => s(&m["publisher"]),
    };
    Ok(PaperMeta {
        title,
        authors,
        date,
        journal,
        series,
        work_type,
        volume: s(&m["volume"]),
        issue: s(&m["issue"]),
        pages: s(&m["page"]),
        publisher,
        doi: s(&m["DOI"]),
        url: s(&m["URL"]),
        abstract_text: strip_tags(&s(&m["abstract"])).trim_start_matches("Abstract").trim().to_string(),
        keywords: m["subject"].as_array().map(|a| a.iter().map(s).collect()).unwrap_or_default(),
        license,
        ..Default::default()
    })
}

pub fn europe_pmc(query: &str) -> Result<PaperMeta> {
    let url = format!(
        "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={}&resultType=core&format=json",
        enc(query).replace('/', "%2F")
    );
    let j = get_json(&url)?;
    let r = &j["resultList"]["result"][0];
    if r.is_null() {
        return Err(CoreError::NotFound(format!("Europe PMC has no record for {query}")));
    }
    let authors = r["authorList"]["author"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| {
                    let n = format!("{} {}", s(&x["firstName"]), s(&x["lastName"])).trim().to_string();
                    if n.is_empty() { s(&x["fullName"]) } else { n }
                })
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let ji = &r["journalInfo"];
    // preprints (bioRxiv, Research Square…) come from the source PPR
    let work_type = if s(&r["source"]) == "PPR" { "posted-content" } else if s(&ji["journal"]["title"]).is_empty() { "" } else { "journal-article" };
    let mut meta = PaperMeta {
        title: crate::worktype::fix_title_case(strip_tags(&s(&r["title"])).trim_end_matches('.')),
        authors,
        work_type: work_type.into(),
        date: {
            let d = s(&r["firstPublicationDate"]);
            if d.is_empty() { s(&r["pubYear"]) } else { d }
        },
        journal: s(&ji["journal"]["title"]),
        volume: s(&ji["volume"]),
        issue: s(&ji["issue"]),
        pages: s(&r["pageInfo"]),
        doi: s(&r["doi"]),
        pmid: s(&r["pmid"]),
        pmcid: s(&r["pmcid"]),
        abstract_text: strip_tags(&s(&r["abstractText"])),
        keywords: r["keywordList"]["keyword"].as_array().map(|a| a.iter().map(s).collect()).unwrap_or_default(),
        ..Default::default()
    };
    if work_type == "posted-content" && meta.journal.is_empty() {
        meta.journal = s(&r["bookOrReportDetails"]["publisher"]);
    }
    crate::worktype::tidy(&mut meta);
    Ok(meta)
}

fn xml_field(block: &str, tag: &str) -> Vec<String> {
    let re = Regex::new(&format!(r"(?s)<{tag}[^>]*>(.*?)</{tag}>")).unwrap();
    re.captures_iter(block).map(|c| c[1].split_whitespace().collect::<Vec<_>>().join(" ")).collect()
}

pub fn arxiv(id: &str) -> Result<PaperMeta> {
    let xml = get_text(&format!("https://export.arxiv.org/api/query?id_list={}", enc(id)))?;
    let entry = xml.split("<entry>").nth(1).ok_or_else(|| CoreError::NotFound(format!("arXiv has no record for {id}")))?;
    let authors = Regex::new(r"(?s)<author>\s*<name>(.*?)</name>")
        .unwrap()
        .captures_iter(entry)
        .map(|c| c[1].trim().to_string())
        .collect();
    let doi = xml_field(entry, "arxiv:doi").into_iter().next().unwrap_or_default();
    Ok(PaperMeta {
        title: xml_field(entry, "title").into_iter().next().unwrap_or_default(),
        authors,
        date: xml_field(entry, "published").into_iter().next().unwrap_or_default().chars().take(10).collect(),
        journal: "arXiv".into(),
        work_type: "posted-content".into(),
        doi: if doi.is_empty() { format!("10.48550/arXiv.{id}") } else { doi },
        arxiv: id.to_string(),
        url: format!("https://arxiv.org/abs/{id}"),
        abstract_text: xml_field(entry, "summary").into_iter().next().unwrap_or_default(),
        ..Default::default()
    })
}

// ---------------------------------------------------------------- open-access PDFs

/// A place an open-access PDF of a paper can be downloaded from.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PdfCandidate {
    pub url: String,
    /// Human-readable source, e.g. "PubMed Central" or "bioRxiv (preprint)".
    pub label: String,
    /// "published", "accepted" or "preprint".
    pub version: String,
}

/// DOIs of preprint servers (bioRxiv/medRxiv, arXiv, Research Square, SSRN, OSF, PsyArXiv,
/// ChemRxiv, Preprints.org, Authorea).
pub fn is_preprint_doi(doi: &str) -> bool {
    let d = doi.to_lowercase();
    ["10.1101/", "10.64898/", "10.48550/", "10.21203/", "10.2139/ssrn", "10.31219/", "10.31234/", "10.26434/", "10.20944/", "10.22541/"]
        .iter()
        .any(|p| d.starts_with(p))
}

/// Legal open-access PDF locations, best first: published versions, then accepted manuscripts,
/// then preprints. Sources: PMC open-access archive, Europe PMC, OpenAlex, arXiv.
///
/// A published paper (a DOI that isn't a preprint server's) never gets a preprint: the
/// preprint can differ from what was published, so it is better to have no PDF than that one.
pub fn pdf_candidates(m: &PaperMeta) -> Vec<PdfCandidate> {
    let published = !m.doi.is_empty() && !is_preprint_doi(&m.doi);
    let mut out = pdf_candidates_all(m);
    if published {
        // (Europe PMC sometimes lists a bioRxiv link as the article's full text: judge by the URL too)
        let preprint_host = |u: &str| {
            let u = u.to_lowercase();
            ["biorxiv.org", "medrxiv.org", "arxiv.org", "researchsquare.com", "ssrn.com", "osf.io", "chemrxiv.org", "preprints.org", "authorea.com"]
                .iter()
                .any(|h| u.contains(h))
        };
        out.retain(|c| c.version != "preprint" && !c.label.to_lowercase().contains("rxiv") && !preprint_host(&c.url));
    }
    out
}

fn pdf_candidates_all(m: &PaperMeta) -> Vec<PdfCandidate> {
    let mut out: Vec<PdfCandidate> = vec![];
    let mut push = |url: String, label: &str, version: &str| {
        if !url.is_empty() && !out.iter().any(|c| c.url == url) {
            out.push(PdfCandidate { url, label: label.into(), version: version.into() });
        }
    };
    let mut pmcid = m.pmcid.clone();
    let mut epmc_pdfs: Vec<String> = vec![];
    if !m.doi.is_empty() || !pmcid.is_empty() {
        let q = if !pmcid.is_empty() { format!("PMCID:{pmcid}") } else { format!("DOI:\"{}\"", m.doi) };
        let url = format!(
            "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={}&resultType=core&format=json",
            enc(&q).replace('/', "%2F")
        );
        if let Ok(j) = get_json(&url) {
            let r = &j["resultList"]["result"][0];
            if pmcid.is_empty() {
                pmcid = s(&r["pmcid"]);
            }
            for f in r["fullTextUrlList"]["fullTextUrl"].as_array().cloned().unwrap_or_default() {
                let open = matches!(s(&f["availabilityCode"]).as_str(), "OA" | "F");
                if open && s(&f["documentStyle"]) == "pdf" {
                    epmc_pdfs.push(s(&f["url"]));
                }
            }
        }
    }
    if !pmcid.is_empty() {
        if let Some(u) = pmc_archive_pdf(&pmcid) {
            push(u, "PubMed Central", "published");
        }
    }
    for u in epmc_pdfs {
        push(u, "Europe PMC", "published");
    }
    let mut accepted = vec![];
    let mut preprints = vec![];
    if !m.doi.is_empty() {
        if let Ok(j) = get_json(&format!("https://api.openalex.org/works/doi:{}", enc(&m.doi))) {
            for l in j["locations"].as_array().cloned().unwrap_or_default() {
                let u = s(&l["pdf_url"]);
                if u.is_empty() {
                    continue;
                }
                let src = s(&l["source"]["display_name"]);
                let label = if src.is_empty() { "open-access copy".to_string() } else { src };
                match s(&l["version"]).as_str() {
                    "publishedVersion" => push(u, &label, "published"),
                    "acceptedVersion" => accepted.push((u, label)),
                    _ => preprints.push((u, label)),
                }
            }
        }
    }
    if !m.arxiv.is_empty() {
        push(format!("https://arxiv.org/pdf/{}", m.arxiv), "arXiv", "preprint");
    }
    for (u, l) in accepted {
        push(u, &format!("{l} (accepted manuscript)"), "accepted");
    }
    for (u, l) in preprints {
        push(u, &format!("{l} (preprint)"), "preprint");
    }
    out
}

/// The PDF in the PMC open-access archive on AWS (newest article version), if there is one.
fn pmc_archive_pdf(pmcid: &str) -> Option<String> {
    let list = get_text(&format!("https://pmc-oa-opendata.s3.amazonaws.com/?list-type=2&prefix={pmcid}.&max-keys=200")).ok()?;
    let re = Regex::new(&format!(r"<Key>({pmcid}\.(\d+)/{pmcid}\.\d+\.pdf)</Key>")).ok()?;
    re.captures_iter(&list)
        .max_by_key(|c| c[2].parse::<u32>().unwrap_or(0))
        .map(|c| format!("https://pmc-oa-opendata.s3.amazonaws.com/{}", &c[1]))
}

/// Downloads a PDF; fails unless the response really is a PDF (publishers often return HTML).
pub fn download_pdf(url: &str) -> Result<Vec<u8>> {
    let mut r = agent()
        .get(url)
        .header("User-Agent", UA)
        .header("Accept", "application/pdf,*/*;q=0.8")
        .call()
        .map_err(|e| CoreError::Network(e.to_string()))?;
    let data = r
        .body_mut()
        .with_config()
        .limit(200 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| CoreError::Network(e.to_string()))?;
    if !data.starts_with(b"%PDF") {
        return Err(CoreError::Network("not a PDF (the site may require a login)".into()));
    }
    Ok(data)
}

// ---------------------------------------------------------------- discovering new papers

/// A paper found outside the library (OpenAlex).
#[derive(uniffi::Record, Clone, Debug)]
pub struct DiscoverHit {
    pub meta: PaperMeta,
    pub cited_by: u32,
    pub open_access: bool,
    /// Id of the paper if it is already in the library (filled in by `Library::discover`).
    pub in_library: Option<String>,
}

/// Searches OpenAlex (≈250M works) by relevance.
pub fn discover(query: &str, limit: u32) -> Result<Vec<DiscoverHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let url = format!(
        "https://api.openalex.org/works?search={}&per_page={}&select=title,authorships,publication_date,primary_location,doi,ids,cited_by_count,open_access,abstract_inverted_index,biblio,type,type_crossref",
        enc(q).replace('/', "%2F"),
        limit.clamp(1, 50)
    );
    works(&url)
}

const WORK_FIELDS: &str = "title,authorships,publication_date,primary_location,doi,ids,cited_by_count,open_access,abstract_inverted_index,biblio,type,type_crossref";

/// What a watch follows.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchKind {
    /// Words, as in Discover.
    Topic,
    /// An author: an ORCID iD or OpenAlex author id (exact), or a name (the best match).
    Author,
    /// Papers citing the work with this DOI.
    Citing,
    /// New papers in a journal, by its name ("Nature Neuroscience") or ISSN.
    Journal,
}

/// Papers published on or after `since` (YYYY-MM-DD) for a watch, newest first.
pub fn watch_new(kind: WatchKind, value: &str, since: &str, limit: u32) -> Result<Vec<DiscoverHit>> {
    let v = value.trim();
    if v.is_empty() {
        return Ok(vec![]);
    }
    let per = limit.clamp(1, 50);
    let date = format!("from_publication_date:{}", since.trim());
    let url = match kind {
        WatchKind::Topic => format!(
            // keep relevance order (sorting a search by date surfaces loose matches); callers sort
            "https://api.openalex.org/works?search={}&filter={}&per_page={}&select={}",
            enc(v).replace('/', "%2F"), date, per, WORK_FIELDS
        ),
        WatchKind::Author => {
            let filter = match author_ref(v) {
                AuthorRef::Orcid(o) => format!("authorships.author.orcid:{o}"),
                AuthorRef::OpenAlex(id) => format!("authorships.author.id:{id}"),
                AuthorRef::Name(_) => String::new(),
            };
            if !filter.is_empty() {
                return works(&format!(
                    "https://api.openalex.org/works?filter={},{}&sort=publication_date:desc&per_page={}&select={}",
                    filter, date, per, WORK_FIELDS
                ));
            }
            let a = get_json(&format!("https://api.openalex.org/authors?search={}&per_page=1&select=id", enc(v)))?;
            let id = s(&a["results"][0]["id"]).rsplit('/').next().unwrap_or("").to_string();
            if id.is_empty() {
                return Ok(vec![]);
            }
            format!(
                "https://api.openalex.org/works?filter=author.id:{},{}&sort=publication_date:desc&per_page={}&select={}",
                id, date, per, WORK_FIELDS
            )
        }
        WatchKind::Citing => {
            let doi = v.trim_start_matches("https://doi.org/").trim_start_matches("doi:");
            let w = get_json(&format!("https://api.openalex.org/works/doi:{}?select=id", doi))?;
            let id = s(&w["id"]).rsplit('/').next().unwrap_or("").to_string();
            if id.is_empty() {
                return Ok(vec![]);
            }
            format!(
                "https://api.openalex.org/works?filter=cites:{},{}&sort=publication_date:desc&per_page={}&select={}",
                id, date, per, WORK_FIELDS
            )
        }
        WatchKind::Journal => {
            let id = journal_id(v)?;
            if id.is_empty() {
                return Ok(vec![]);
            }
            format!(
                "https://api.openalex.org/works?filter=primary_location.source.id:{},type:article|review,{}&sort=publication_date:desc&per_page={}&select={}",
                id, date, per, WORK_FIELDS
            )
        }
    };
    works(&url)
}

/// The OpenAlex id of a journal ("S…"), from an ISSN or its name; the exact name wins over a
/// closer search match ("Nature" is Nature, not Nature Communications). Empty if none.
fn journal_id(v: &str) -> Result<String> {
    let issn = v.len() == 9 && v.as_bytes()[4] == b'-';
    let url = if issn {
        format!("https://api.openalex.org/sources/issn:{}?select=id", enc(v))
    } else {
        format!("https://api.openalex.org/sources?search={}&filter=type:journal&per_page=10&select=id,display_name,works_count", enc(v))
    };
    let j = get_json(&url)?;
    let id = if issn {
        s(&j["id"])
    } else {
        let list = j["results"].as_array().cloned().unwrap_or_default();
        let exact = list.iter().find(|x| s(&x["display_name"]).eq_ignore_ascii_case(v.trim()));
        exact.or(list.first()).map(|x| s(&x["id"])).unwrap_or_default()
    };
    Ok(id.rsplit('/').next().unwrap_or("").to_string())
}

/// The journal's name as OpenAlex has it ("nature neuro" → "Nature Neuroscience"), to show and
/// store when following it. None if no journal matches.
pub fn journal_name(query: &str) -> Result<Option<String>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(None);
    }
    let j = get_json(&format!("https://api.openalex.org/sources?search={}&filter=type:journal&per_page=10&select=display_name", enc(q)))?;
    let list: Vec<String> = j["results"].as_array().cloned().unwrap_or_default().iter().map(|x| s(&x["display_name"])).collect();
    Ok(list.iter().find(|n| n.eq_ignore_ascii_case(q)).or(list.first()).cloned())
}

/// Parses an OpenAlex /works list.
fn works(url: &str) -> Result<Vec<DiscoverHit>> {
    Ok(parse_works(&get_json(url)?))
}

fn parse_works(j: &Value) -> Vec<DiscoverHit> {
    let mut out = vec![];
    for w in j["results"].as_array().cloned().unwrap_or_default() {
        let doi = s(&w["doi"]).trim_start_matches("https://doi.org/").to_string();
        let pmid = s(&w["ids"]["pmid"]).rsplit('/').next().unwrap_or("").to_string();
        let pmcid = s(&w["ids"]["pmcid"]).rsplit('/').next().unwrap_or("").to_string();
        let authors = w["authorships"]
            .as_array()
            .map(|a| a.iter().map(|x| s(&x["author"]["display_name"])).filter(|n| !n.is_empty()).collect())
            .unwrap_or_default();
        let b = &w["biblio"];
        let pages = match (s(&b["first_page"]), s(&b["last_page"])) {
            (f, l) if !f.is_empty() && !l.is_empty() && f != l => format!("{f}-{l}"),
            (f, _) => f,
        };
        // OpenAlex's own types lump conference papers in with articles; type_crossref doesn't
        let work_type = match s(&w["type_crossref"]) {
            t if !t.is_empty() => t,
            _ => match s(&w["type"]).as_str() {
                "preprint" => "posted-content".to_string(),
                "conference-paper" => "proceedings-article".to_string(),
                "article" => String::new(),
                t => t.to_string(),
            },
        };
        let mut meta = PaperMeta {
            title: strip_tags(&s(&w["title"])),
            authors,
            work_type,
            date: s(&w["publication_date"]),
            journal: s(&w["primary_location"]["source"]["display_name"]),
            volume: s(&b["volume"]),
            issue: s(&b["issue"]),
            pages,
            doi,
            pmid,
            pmcid: if pmcid.to_uppercase().starts_with("PMC") { pmcid.to_uppercase() } else { String::new() },
            abstract_text: inverted_abstract(&w["abstract_inverted_index"]),
            ..Default::default()
        };
        if meta.title.is_empty() {
            continue;
        }
        crate::worktype::tidy(&mut meta);
        out.push(DiscoverHit {
            meta,
            cited_by: w["cited_by_count"].as_u64().unwrap_or(0) as u32,
            open_access: w["open_access"]["is_oa"].as_bool().unwrap_or(false),
            in_library: None,
        });
    }
    out
}

/// OpenAlex stores abstracts as {word: [positions]}.
fn inverted_abstract(v: &Value) -> String {
    let Some(map) = v.as_object() else { return String::new() };
    let mut words: Vec<(u64, &str)> = vec![];
    for (w, pos) in map {
        for p in pos.as_array().into_iter().flatten() {
            if let Some(i) = p.as_u64() {
                words.push((i, w.as_str()));
            }
        }
    }
    words.sort_by_key(|x| x.0);
    words.into_iter().map(|x| x.1).collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- ORCID (through OpenAlex)

/// "0000-0002-1825-0097" from an ORCID iD as typed or pasted ("https://orcid.org/…", "orcid:…",
/// without dashes, a lowercase x); None unless it has 16 characters and a valid check digit
/// (ISO 7064 11,2).
pub fn normalize_orcid(s: &str) -> Option<String> {
    let mut t = s.trim();
    for p in ["https://orcid.org/", "http://orcid.org/", "orcid.org/", "orcid:", "ORCID:"] {
        if t.len() >= p.len() && t[..p.len()].eq_ignore_ascii_case(p) {
            t = t[p.len()..].trim();
        }
    }
    let chars: Vec<char> = t.chars().filter(|c| *c != '-' && *c != ' ').map(|c| c.to_ascii_uppercase()).collect();
    if chars.len() != 16 || !chars[..15].iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut total = 0u32;
    for c in &chars[..15] {
        total = (total + c.to_digit(10)?) * 2;
    }
    let check = (12 - total % 11) % 11;
    let expected = if check == 10 { 'X' } else { char::from_digit(check, 10)? };
    if chars[15] != expected {
        return None;
    }
    let d: String = chars.into_iter().collect();
    Some(format!("{}-{}-{}-{}", &d[0..4], &d[4..8], &d[8..12], &d[12..16]))
}

/// Who an author watch follows.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthorRef {
    Orcid(String),
    /// "A5023888391"
    OpenAlex(String),
    Name(String),
}

/// An ORCID iD or an OpenAlex author id ("A123…", "https://openalex.org/A123…") follows that
/// author exactly; anything else is a name to search for.
pub(crate) fn author_ref(v: &str) -> AuthorRef {
    let v = v.trim();
    if let Some(o) = normalize_orcid(v) {
        return AuthorRef::Orcid(o);
    }
    let tail = v.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let bare = v.len() == tail.len();
    let from_openalex = v.to_lowercase().contains("openalex.org/");
    if (bare || from_openalex) && tail.len() >= 4 && tail[..1].eq_ignore_ascii_case("a") && tail[1..].chars().all(|c| c.is_ascii_digit()) {
        return AuthorRef::OpenAlex(format!("A{}", &tail[1..]));
    }
    AuthorRef::Name(v.to_string())
}

/// Someone the researcher publishes with.
#[derive(uniffi::Record, Clone, Debug)]
pub struct Coauthor {
    pub name: String,
    /// OpenAlex author id ("A…").
    pub openalex_id: String,
    /// "0000-…", or "" when OpenAlex has none.
    pub orcid: String,
    /// Papers written together (among the researcher's recent ones).
    pub shared: u32,
}

/// A researcher's own work, found by their ORCID iD on OpenAlex.
#[derive(uniffi::Record, Clone, Debug)]
pub struct OrcidProfile {
    pub orcid: String,
    pub name: String,
    /// OpenAlex author id ("A…").
    pub openalex_id: String,
    pub works_count: u32,
    pub cited_by_count: u32,
    /// Their latest papers, newest first (up to 50).
    pub works: Vec<DiscoverHit>,
    /// Their most cited papers, most cited first (up to 15).
    pub most_cited: Vec<DiscoverHit>,
    /// The people they publish with most often (up to 12).
    pub coauthors: Vec<Coauthor>,
}

/// Papers and the like (no datasets, errata, peer reviews or paratext).
const OWN_TYPES: &str = "type:article|review|preprint|letter|book-chapter|editorial";

fn short_id(v: &Value) -> String {
    s(v).trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
}

/// The researcher with this ORCID iD on OpenAlex: name, recent and most cited papers, co-authors.
pub fn orcid_profile(orcid: &str) -> Result<OrcidProfile> {
    let o = normalize_orcid(orcid).ok_or_else(|| CoreError::Invalid(format!("“{}” is not an ORCID iD", orcid.trim())))?;
    let a = match get_json(&format!(
        "https://api.openalex.org/authors/orcid:{o}?select=id,display_name,works_count,cited_by_count"
    )) {
        Ok(a) => a,
        Err(CoreError::Network(e)) if e.contains("404") => {
            return Err(CoreError::NotFound(format!("no author with ORCID iD {o} on OpenAlex")))
        }
        Err(e) => return Err(e),
    };
    let id = short_id(&a["id"]);
    if id.is_empty() {
        return Err(CoreError::NotFound(format!("no author with ORCID iD {o} on OpenAlex")));
    }
    let recent = get_json(&format!(
        "https://api.openalex.org/works?filter=authorships.author.id:{id},{OWN_TYPES}&sort=publication_date:desc&per_page=100&select={WORK_FIELDS}"
    ))?;
    let cited = get_json(&format!(
        "https://api.openalex.org/works?filter=authorships.author.id:{id},{OWN_TYPES}&sort=cited_by_count:desc&per_page=15&select={WORK_FIELDS}"
    ))?;
    let mut works = parse_works(&recent);
    works.truncate(50);
    Ok(OrcidProfile {
        orcid: o.clone(),
        name: s(&a["display_name"]),
        openalex_id: id.clone(),
        works_count: a["works_count"].as_u64().unwrap_or(0) as u32,
        cited_by_count: a["cited_by_count"].as_u64().unwrap_or(0) as u32,
        works,
        most_cited: parse_works(&cited),
        coauthors: coauthors(&recent, &id, &o, 12),
    })
}

/// The people on most of these works (an OpenAlex /works list), leaving out the researcher and
/// big consortium papers (over 30 authors), most shared papers first.
pub(crate) fn coauthors(works: &Value, me: &str, my_orcid: &str, limit: usize) -> Vec<Coauthor> {
    let mut by_id: std::collections::HashMap<String, Coauthor> = Default::default();
    let mut order: Vec<String> = vec![];
    for w in works["results"].as_array().into_iter().flatten() {
        let list = w["authorships"].as_array().cloned().unwrap_or_default();
        if list.len() > 30 {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        for x in list {
            let id = short_id(&x["author"]["id"]);
            let orcid = normalize_orcid(&s(&x["author"]["orcid"])).unwrap_or_default();
            let name = s(&x["author"]["display_name"]);
            if id.is_empty() || name.is_empty() || id == me || (!orcid.is_empty() && orcid == my_orcid) || !seen.insert(id.clone()) {
                continue;
            }
            let e = by_id.entry(id.clone()).or_insert_with(|| {
                order.push(id.clone());
                Coauthor { name, openalex_id: id.clone(), orcid: String::new(), shared: 0 }
            });
            e.shared += 1;
            if e.orcid.is_empty() {
                e.orcid = orcid;
            }
        }
    }
    let mut out: Vec<Coauthor> = order.into_iter().filter_map(|id| by_id.remove(&id)).collect();
    // most shared first; among equals, the most recent collaborators (the list is newest first)
    out.sort_by(|a, b| b.shared.cmp(&a.shared));
    out.truncate(limit);
    out
}

/// Papers citing the work with this DOI published since `since`, leaving out those by the author
/// with this OpenAlex id (self-citations) when one is given.
pub fn citing_new(doi: &str, exclude_author: &str, since: &str, limit: u32) -> Result<Vec<DiscoverHit>> {
    let doi = doi.trim().trim_start_matches("https://doi.org/").trim_start_matches("doi:");
    if doi.is_empty() {
        return Ok(vec![]);
    }
    let w = get_json(&format!("https://api.openalex.org/works/doi:{}?select=id", doi))?;
    let id = short_id(&w["id"]);
    if id.is_empty() {
        return Ok(vec![]);
    }
    let not_by = match author_ref(exclude_author) {
        AuthorRef::OpenAlex(a) => format!(",authorships.author.id:!{a}"),
        AuthorRef::Orcid(o) => format!(",authorships.author.orcid:!{o}"),
        AuthorRef::Name(_) => String::new(),
    };
    works(&format!(
        "https://api.openalex.org/works?filter=cites:{id}{not_by},from_publication_date:{}&sort=publication_date:desc&per_page={}&select={WORK_FIELDS}",
        since.trim(),
        limit.clamp(1, 50)
    ))
}
