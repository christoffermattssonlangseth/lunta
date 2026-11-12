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

fn get_text(url: &str) -> Result<String> {
    let mut r = agent()
        .get(url)
        .header("User-Agent", UA)
        .call()
        .map_err(|e| CoreError::Network(e.to_string()))?;
    r.body_mut().read_to_string().map_err(|e| CoreError::Network(e.to_string()))
}

fn get_json(url: &str) -> Result<Value> {
    serde_json::from_str(&get_text(url)?).map_err(|e| CoreError::Network(format!("bad JSON: {e}")))
}

fn enc(s: &str) -> String {
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
    f!(title, authors, date, journal, volume, issue, pages, publisher, doi, pmid, pmcid, arxiv, url, abstract_text, keywords, license);
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.first().map(s).unwrap_or_default(),
        _ => String::new(),
    }
}

fn strip_tags(x: &str) -> String {
    let re = Regex::new(r"<[^>]+>").unwrap();
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
                    let n = format!("{} {}", s(&x["given"]), s(&x["family"])).trim().to_string();
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
    Ok(PaperMeta {
        title: strip_tags(&s(&m["title"])),
        authors,
        date,
        journal: s(&m["container-title"]),
        volume: s(&m["volume"]),
        issue: s(&m["issue"]),
        pages: s(&m["page"]),
        publisher: s(&m["publisher"]),
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
    Ok(PaperMeta {
        title: strip_tags(&s(&r["title"])).trim_end_matches('.').to_string(),
        authors,
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
    })
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

/// Legal open-access PDF locations, best first: published versions, then accepted manuscripts,
/// then preprints. Sources: PMC open-access archive, Europe PMC, OpenAlex, arXiv.
pub fn pdf_candidates(m: &PaperMeta) -> Vec<PdfCandidate> {
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
        "https://api.openalex.org/works?search={}&per_page={}&select=title,authorships,publication_date,primary_location,doi,ids,cited_by_count,open_access,abstract_inverted_index,biblio",
        enc(q).replace('/', "%2F"),
        limit.clamp(1, 50)
    );
    works(&url)
}

const WORK_FIELDS: &str = "title,authorships,publication_date,primary_location,doi,ids,cited_by_count,open_access,abstract_inverted_index,biblio";

/// What a watch follows.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchKind {
    /// Words, as in Discover.
    Topic,
    /// An author's name.
    Author,
    /// Papers citing the work with this DOI.
    Citing,
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
    };
    works(&url)
}

/// Parses an OpenAlex /works list.
fn works(url: &str) -> Result<Vec<DiscoverHit>> {
    let j = get_json(url)?;
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
        let meta = PaperMeta {
            title: strip_tags(&s(&w["title"])),
            authors,
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
        out.push(DiscoverHit {
            meta,
            cited_by: w["cited_by_count"].as_u64().unwrap_or(0) as u32,
            open_access: w["open_access"]["is_oa"].as_bool().unwrap_or(false),
            in_library: None,
        });
    }
    Ok(out)
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
