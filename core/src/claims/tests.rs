//! Check Citations: sentences around citations (text and LaTeX), passages and BM25, the model's
//! answers checked, and the results without a model.

use super::*;
use std::sync::Mutex;

fn u16(text: &str, part: &str) -> (u32, u32) {
    let b = text.find(part).unwrap();
    let s = text[..b].encode_utf16().count() as u32;
    (s, s + part.encode_utf16().count() as u32)
}

fn cite(text: &str, part: &str, others: &[&str], before: &str) -> CitingSentence {
    let (s, e) = u16(text, part);
    let o: Vec<CiteSpan> = others.iter().map(|p| { let (a, b) = u16(text, p); CiteSpan { start: a, end: b } }).collect();
    citing_sentence(text, s, e, &o, before)
}

#[test]
fn the_sentence_around_a_citation() {
    let p = "Microglia are the brain's immune cells. Chronic active lesions have a rim of iron-laden microglia (Kukanja et al., 2024). Remyelination is more common early (Lee 2020).";
    let c = cite(p, "(Kukanja et al., 2024)", &["(Lee 2020)"], "");
    assert_eq!(c.sentence, "Chronic active lesions have a rim of iron-laden microglia (Kukanja et al., 2024).");
    assert_eq!(c.claim, "Chronic active lesions have a rim of iron-laden microglia.");
    assert_eq!(c.previous, "Microglia are the brain's immune cells.");
    // the last one: the other citation taken out of it
    let c = cite(p, "(Lee 2020)", &["(Kukanja et al., 2024)"], "");
    assert_eq!(c.claim, "Remyelination is more common early.");
    assert_eq!(c.previous, "Chronic active lesions have a rim of iron-laden microglia.");
    // the first sentence: the one before is the paragraph before's last
    let c = cite("Cells differ by region [4]. More text here.", "[4]", &[], "Intro sentence one. The second intro sentence.");
    assert_eq!((c.claim.as_str(), c.previous.as_str()), ("Cells differ by region.", "The second intro sentence."));
}

#[test]
fn abbreviations_and_numbers_dont_end_sentences() {
    let p = "As shown in Fig. 2 by Smith et al. the dose was 2.5 mg, e.g. in mice [3]. Next one.";
    let c = cite(p, "[3]", &[], "");
    assert_eq!(c.claim, "As shown in Fig. 2 by Smith et al. the dose was 2.5 mg, e.g. in mice.");
    let p = "J. R. Smith found that T cells expand [1]. Next.";
    assert_eq!(cite(p, "[1]", &[], "").claim, "J. R. Smith found that T cells expand.");
}

#[test]
fn a_number_after_the_full_stop_is_the_sentence_befores() {
    // Nature style: the superscript after the full stop
    let p = "Iron accumulates in microglia.12 Remyelination fails in old lesions.";
    let c = cite(p, "12", &[], "");
    assert_eq!(c.claim, "Iron accumulates in microglia.");
    assert_eq!(c.sentence, "Iron accumulates in microglia.12");
    let p = "Iron accumulates in microglia. [5] Remyelination fails in old lesions.";
    assert_eq!(cite(p, "[5]", &[], "").claim, "Iron accumulates in microglia.");
    // a narrative citation that starts a sentence is that sentence's
    let p = "Old text here. Smith et al. (2020) showed that lesions expand.";
    let c = cite(p, "Smith et al. (2020)", &[], "");
    assert_eq!(c.claim, "showed that lesions expand.");
    assert_eq!(c.sentence, "Smith et al. (2020) showed that lesions expand.");
    assert_eq!(c.previous, "Old text here.");
}

#[test]
fn citations_typed_as_text_are_taken_out() {
    assert_eq!(strip_citations("Cells differ [1–3, 7] by region (Smith et al., 2019; Lee 2020a)."), "Cells differ by region.");
    assert_eq!(strip_citations("As before (ref. 4), it holds (e.g., Wu 2018, p. 4)."), "As before, it holds.");
    // brackets that aren't citations stay
    assert_eq!(strip_citations("The dose (2 mg) worked [in mice]."), "The dose (2 mg) worked [in mice].");
}

#[test]
fn utf16_offsets() {
    // an emoji and an accent before the citation: JavaScript counts the emoji as two
    let p = "Über 😀 cells grow fast [2]. Then.";
    let c = cite(p, "[2]", &[], "");
    assert_eq!(c.claim, "Über 😀 cells grow fast.");
}

const TEX: &str = r#"\documentclass{article}
\usepackage{natbib}
\begin{document}
\section{Introduction}
Microglia are the brain's immune cells. % a comment with \cite{nope}
Chronic active lesions have a rim of \emph{iron-laden} microglia~\citep[p.~4]{kukanja2024,lee2020}.
Remyelination is more common in early lesions \cite{lee2020}; it is 50\% faster.

\citet{smith2019} showed that T cells expand (Fig.~\ref{fig1}).
\begin{equation}
x = \cite{eq}
\end{equation}
\begin{figure}
\caption{Lesions in mice \cite{wu2018}.}
\end{figure}
\end{document}
"#;

#[test]
fn citations_in_latex() {
    let t = tex_claims(TEX);
    let keys: Vec<Vec<String>> = t.iter().map(|c| c.keys.clone()).collect();
    assert_eq!(keys, vec![vec!["kukanja2024".to_string(), "lee2020".into()], vec!["lee2020".into()], vec!["smith2019".into()], vec!["wu2018".into()]]);
    assert_eq!(t[0].claim, "Chronic active lesions have a rim of iron-laden microglia.");
    assert_eq!(t[0].previous, "Microglia are the brain's immune cells.");
    assert_eq!(t[0].line, 6);
    assert_eq!(t[0].command, r"\citep[p.~4]{kukanja2024,lee2020}");
    assert_eq!(t[1].claim, "Remyelination is more common in early lesions; it is 50% faster.");
    assert_eq!(t[1].line, 7);
    // a new paragraph: the sentence before is the last paragraph's
    assert_eq!(t[2].claim, "showed that T cells expand (Fig. X).");
    assert_eq!(t[2].previous, "Remyelination is more common in early lesions; it is 50% faster.");
    assert_eq!(t[3].claim, "Lesions in mice.");
    assert_eq!(tex_bib_files("\\bibliography{refs, more.bib} % \\bibliography{old}\n\\addbibresource[label=x]{lib.bib}"), vec!["refs.bib", "more.bib", "lib.bib"]);
    // a .tex without \begin{document} (an \input chapter) is all body
    let t = tex_claims("Text before. Cells grow \\cite{a}.\n");
    assert_eq!((t.len(), t[0].claim.as_str(), t[0].line), (1, "Cells grow.", 1));
}

const PAPER: &str = "Iron rims in multiple sclerosis lesions
Abstract
Chronic active lesions in multiple sclerosis are surrounded by a rim of iron-laden microglia that sustain inflammation.
Introduction
Multiple sclerosis is a disease of the central nervous system in which demyelinated lesions form in white and grey matter over many years.
Results
We found that chronic active lesions had a rim of iron-laden microglia in 42 of 50 patients, whereas inactive lesions had none. The rims persisted for years. Remyelination was rare in lesions with rims; most remyelinated lesions were early lesions without iron. Microglia at the rim expressed CD163 and ferritin. Astrocytes at the rim were reactive. These findings suggest that iron-laden microglia drive lesion expansion over time, a process that is slow.
Figure 2
References
1. Smith A. Chronic active lesions and iron-laden microglia rim. Brain. 2019.
";

#[test]
fn passages_with_their_sections() {
    let ps = passages(PAPER);
    assert_eq!(ps.iter().map(|p| p.section.as_str()).collect::<Vec<_>>(), vec!["Abstract", "Introduction", "Results"]);
    // the reference list is left out, and every passage is word for word in the text
    assert!(ps.iter().all(|p| !p.text.contains("Smith A.")));
    for p in &ps {
        let at: String = PAPER.chars().skip(p.start as usize).take(p.text.chars().count()).collect();
        assert_eq!(at, p.text);
    }
}

#[test]
fn long_paragraphs_are_cut_in_runs_of_sentences() {
    let s = "Sentence number one says something about microglia and iron in lesions. ";
    let text = format!("Results\n{}\n", s.repeat(20).trim());
    let ps = passages(&text);
    assert!(ps.len() >= 3, "{}", ps.len());
    assert!(ps.iter().all(|p| p.text.chars().count() <= 600 && text.contains(&p.text)));
}

#[test]
fn bm25_finds_the_passage() {
    let top = claim_passages(PAPER.into(), "Chronic active lesions have a rim of iron-laden microglia.".into(), 3);
    assert_eq!(top[0].section, "Abstract");
    assert!(top.len() == 3 && top[0].score >= 0.8, "{top:?}");
    let top = claim_passages(PAPER.into(), "Remyelination is more common in early lesions.".into(), 3);
    assert_eq!(top[0].section, "Results");
    assert!(top[0].text.contains("most remyelinated lesions were early lesions"));
    // plural and singular are one word
    assert_eq!(stem("lesions"), "lesion");
    assert_eq!(stem("studies"), "study");
    assert!(overlap("microglial rims", "a rim of microglia") > 0.4);
    // nothing in common: nothing
    assert!(claim_passages(PAPER.into(), "Quantum chromodynamics of gluons.".into(), 3).is_empty());
}

fn paper_check(passages: Vec<&str>) -> ClaimPaper {
    ClaimPaper {
        paper_id: "p1".into(),
        title: "Iron rims".into(),
        label: "Kukanja et al. 2024".into(),
        source: ClaimTextSource::FullText,
        passages: passages.into_iter().map(|t| ClaimPassage { text: t.into(), section: "Results".into(), score: 0.5, bm25: 1.0, start: 0, link: String::new() }).collect(),
        prompt: None,
    }
}

const P1: &str = "We found that chronic active lesions had a rim of iron-laden microglia in 42 of 50 patients, whereas inactive lesions had none.";
const P2: &str = "Remyelination was rare in lesions with rims; most remyelinated lesions were early lesions without iron.";

#[test]
fn the_models_quotes_must_be_in_the_paper() {
    let p = paper_check(vec![P1, P2]);
    let ok = judge(&p, Some("VERDICT: SUPPORTED\nEVIDENCE: [1] \"chronic active lesions had a rim of iron-laden microglia\"\nREASON: The paper reports rims in most patients."));
    assert_eq!((ok.verdict, ok.by_model), (ClaimVerdict::Supported, true));
    assert_eq!(ok.evidence.len(), 1);
    assert_eq!(ok.evidence[0].section, "Results");
    assert!(ok.evidence[0].link.starts_with("lunta://open?id=p1&find=chronic%20active%20lesions"), "{}", ok.evidence[0].link);
    assert_eq!(ok.reason, "The paper reports rims in most patients.");
    // curly quotes, other case and spacing, pieces joined by an ellipsis: still word for word
    let r = judge(&p, Some("VERDICT: PARTLY\nEVIDENCE: [2] “Remyelination was rare in lesions … were early  lesions without iron”"));
    assert_eq!((r.verdict, r.evidence.len()), (ClaimVerdict::Partly, 1));
    // a paraphrase isn't evidence: support without a quote is "not found"
    let r = judge(&p, Some("VERDICT: SUPPORTED\nEVIDENCE: [1] \"most patients had iron rims around their lesions\"\nREASON: yes"));
    assert_eq!((r.verdict, r.by_model, r.evidence.len()), (ClaimVerdict::NotFound, true, 0));
    assert!(r.note.contains("no quote that is word for word"), "{}", r.note);
    // contradicted needs a quote too
    let r = judge(&p, Some("VERDICT: CONTRADICTED\nREASON: The paper says the opposite."));
    assert_eq!(r.verdict, ClaimVerdict::NotFound);
    let r = judge(&p, Some("VERDICT: CONTRADICTED\nEVIDENCE: [2] \"Remyelination was rare in lesions with rims\""));
    assert_eq!(r.verdict, ClaimVerdict::Contradicted);
    // not found: quotes dropped
    let r = judge(&p, Some("VERDICT: NOT FOUND\nEVIDENCE: [1] \"whereas inactive lesions had none\"\nREASON: Not addressed."));
    assert_eq!((r.verdict, r.evidence.len()), (ClaimVerdict::NotFound, 0));
    // too short a quote ("a rim") proves nothing
    assert!(quote_in("a rim", &p.passages).is_none());
    assert_eq!(quote_in("in 42 of 50 patients", &p.passages), Some(0));
    // a quote from two passages joined isn't in one
    assert!(quote_in("iron-laden microglia in 42 … early lesions without iron", &p.passages).is_none());
    // Qwen3's thinking is left out
    let r = judge(&p, Some("<think>maybe contradicted</think>\nVERDICT: PARTLY SUPPORTED\nEVIDENCE: [2] \"most remyelinated lesions were early lesions\""));
    assert_eq!(r.verdict, ClaimVerdict::Partly);
}

#[test]
fn verdicts_without_a_model() {
    let p = paper_check(vec![P1, P2]);
    let r = judge(&p, None);
    assert_eq!((r.verdict, r.by_model, r.passages.len()), (ClaimVerdict::Possible, false, 2));
    assert_eq!(r.verdict.label(r.by_model), "Possible support");
    let none = judge(&paper_check(vec![]), None);
    assert_eq!((none.verdict, none.verdict.label(false)), (ClaimVerdict::NotFound, "No matching passage"));
    assert_eq!(ClaimVerdict::NotFound.label(true), "Not found in this paper");
    // an answer that can't be read: as without a model, and said
    let r = judge(&p, Some("I am not sure."));
    assert_eq!((r.verdict, r.by_model), (ClaimVerdict::Possible, false));
    assert!(r.note.contains("couldn’t be read"));
    // only the abstract: said
    let mut a = paper_check(vec![P1]);
    a.source = ClaimTextSource::Abstract;
    assert!(judge(&a, None).note.starts_with("Only the abstract"));
    // no text, or not in the library
    let mut n = paper_check(vec![]);
    n.source = ClaimTextSource::Missing;
    let r = judge(&n, Some("VERDICT: SUPPORTED"));
    assert_eq!((r.verdict, r.by_model), (ClaimVerdict::NoText, false));
}

struct FakeModel {
    answers: Mutex<Vec<Option<String>>>,
    asked: Mutex<Vec<String>>,
}

impl ClaimJudge for FakeModel {
    fn answer(&self, _instructions: String, prompt: String, _max: u32) -> Option<String> {
        self.asked.lock().unwrap().push(prompt);
        let mut a = self.answers.lock().unwrap();
        if a.is_empty() { None } else { a.remove(0) }
    }
}

fn library() -> (tempfile::TempDir, Arc<Library>) {
    let d = tempfile::tempdir().unwrap();
    let l = Library::open(d.path().join("lib").to_string_lossy().into(), d.path().join("cache").to_string_lossy().into(), "test".into()).unwrap();
    (d, l)
}

#[test]
fn checking_against_the_library() {
    let (_d, l) = library();
    let full = l
        .add_paper(PaperMeta { title: "Iron rims".into(), authors: vec!["Petra Kukanja".into(), "B".into(), "C".into()], date: "2024".into(), doi: "10.1016/j.rims.2024.01.001".into(), ..Default::default() }, None, None, Some(PAPER.into()))
        .unwrap()
        .id;
    let abs = l
        .add_paper(PaperMeta { title: "Remyelination".into(), authors: vec!["Ann Lee".into()], date: "2020".into(),
            abstract_text: "Remyelination is common in early lesions. It declines with age. Old lesions rarely remyelinate.".into(), ..Default::default() }, None, None, None)
        .unwrap()
        .id;
    let bare = l.add_paper(PaperMeta { title: "Nothing".into(), authors: vec!["Wu".into()], date: "2018".into(), ..Default::default() }, None, None, None).unwrap().id;

    let claim = "Chronic active lesions have a rim of iron-laden microglia.";
    let ps = l.claim_papers(claim.into(), String::new(), vec![full.clone()]);
    assert_eq!((ps[0].source, ps[0].label.as_str()), (ClaimTextSource::FullText, "Kukanja et al. 2024"));
    let pr = ps[0].prompt.as_ref().unwrap();
    assert!(pr.prompt.starts_with("Sentence (it cites this paper): Chronic active lesions"), "{}", pr.prompt);
    assert!(pr.prompt.contains("[1] (Abstract) Chronic active lesions in multiple sclerosis"));
    assert!(pr.instructions.contains("VERDICT: SUPPORTED, PARTLY, NOT FOUND or CONTRADICTED"));
    assert!(ps[0].passages[0].link.starts_with(&format!("lunta://open?id={full}&find=Chronic%20active")));

    // without a model
    let r = l.check_claim_with(claim, "", &[full.clone(), abs.clone(), bare.clone(), "gone".into()], None);
    assert!(!r.judged && r.note.contains("No on-device model") && r.note.contains("nothing is sent anywhere"));
    let v: Vec<(ClaimVerdict, ClaimTextSource)> = r.results.iter().map(|x| (x.verdict, x.source)).collect();
    assert_eq!(v, vec![(ClaimVerdict::Possible, ClaimTextSource::FullText), (ClaimVerdict::Possible, ClaimTextSource::Abstract),
        (ClaimVerdict::NoText, ClaimTextSource::None), (ClaimVerdict::NoText, ClaimTextSource::Missing)]);
    assert!(r.results[1].passages[0].section == "Abstract");

    // with a model: one answer per paper with text; a paper without passages isn't asked
    let m = FakeModel {
        answers: Mutex::new(vec![
            Some("VERDICT: SUPPORTED\nEVIDENCE: [1] \"surrounded by a rim of iron-laden microglia\"\nREASON: Stated in the abstract.".into()),
            Some("VERDICT: NOT FOUND\nREASON: About remyelination.".into()),
        ]),
        asked: Mutex::new(vec![]),
    };
    let r = l.check_claim_with(claim, "Microglia are immune cells.", &[full.clone(), abs.clone(), bare.clone()], Some(&m));
    assert!(r.judged && r.note.starts_with("Suggested by the on-device model"));
    assert_eq!(r.results.iter().map(|x| x.verdict).collect::<Vec<_>>(), vec![ClaimVerdict::Supported, ClaimVerdict::NotFound, ClaimVerdict::NoText]);
    assert_eq!(m.asked.lock().unwrap().len(), 2);
    assert!(m.asked.lock().unwrap()[0].contains("The sentence before it (context only): Microglia are immune cells."));
    assert!(m.asked.lock().unwrap()[1].contains("Only the paper's abstract is available."));
    // the model gone (None): not asked again, the rest as without one
    let m = FakeModel { answers: Mutex::new(vec![]), asked: Mutex::new(vec![]) };
    let r = l.check_claim_with(claim, "", &[full.clone(), abs.clone()], Some(&m));
    assert!(!r.judged && m.asked.lock().unwrap().len() == 1);

    // citation keys: the library's, else the .bib's entry found by DOI
    let key = l.cite_keys(vec![abs.clone()])[0].clone();
    let bib = "@article{rims2024, title={Iron {R}ims}, doi={10.1016/j.rims.2024.01.001}}\n@article{other, title={Unknown paper}}";
    assert_eq!(l.cite_key_papers(vec![key.to_uppercase(), "rims2024".into(), "other".into()], bib.into()), vec![abs.clone(), full.clone(), String::new()]);
}

#[test]
fn the_citation_api() {
    let (_d, l) = library();
    let full = l.add_paper(PaperMeta { title: "Iron rims".into(), authors: vec!["Kukanja".into()], date: "2024".into(), doi: "10.1016/j.rims.2024.01.001".into(), ..Default::default() }, None, None, Some(PAPER.into())).unwrap().id;
    let text = "Microglia are immune cells. Chronic active lesions have a rim of iron-laden microglia (Kukanja, 2024). Next.";
    let (s, e) = u16(text, "(Kukanja, 2024)");
    let body = json!({"text": text, "start": s, "end": e, "ids": [full]}).to_string();
    let (st, v) = l.claims_answer(&body, None);
    assert_eq!(st, 200);
    assert_eq!(v["claim"], "Chronic active lesions have a rim of iron-laden microglia.");
    assert_eq!(v["context"], "Microglia are immune cells.");
    assert_eq!(v["judged"], false);
    assert_eq!(v["privacy"], PRIVACY);
    let r = &v["results"][0];
    assert_eq!((r["verdict"].as_str(), r["verdictLabel"].as_str(), r["source"].as_str()), (Some("possible"), Some("Possible support"), Some("fullText")));
    assert!(r["passages"][0]["score"].as_f64().unwrap() > 0.7);
    assert!(r["passages"][0]["link"].as_str().unwrap().starts_with("lunta://open?id="));
    // a paper the document has under another id: found by DOI, answered under the document's id
    let body = json!({"claim": "Iron-laden microglia rim chronic active lesions (Kukanja 2024).", "ids": ["doc-1"], "papers": {"doc-1": {"title": "x", "doi": "10.1016/j.rims.2024.01.001"}}}).to_string();
    let m = FakeModel { answers: Mutex::new(vec![Some("VERDICT: SUPPORTED\nEVIDENCE: [1] \"a rim of iron-laden microglia\"".into())]), asked: Mutex::new(vec![]) };
    let (_, v) = l.claims_answer(&body, Some(&m));
    assert_eq!(v["claim"], "Iron-laden microglia rim chronic active lesions.");
    assert_eq!((v["results"][0]["id"].as_str(), v["results"][0]["libraryId"].as_str()), (Some("doc-1"), Some(full.as_str())));
    assert_eq!((v["judged"].as_bool(), v["results"][0]["verdict"].as_str()), (Some(true), Some("supported")));
    // judge: false: no model asked
    let body = json!({"claim": "Iron-laden microglia.", "ids": [full], "judge": false}).to_string();
    let m = FakeModel { answers: Mutex::new(vec![]), asked: Mutex::new(vec![]) };
    let (_, v) = l.claims_answer(&body, Some(&m));
    assert!(v["judged"] == false && m.asked.lock().unwrap().is_empty());
    // bad requests
    assert_eq!(l.claims_answer("[]", None).0, 400);
    assert_eq!(l.claims_answer(r#"{"claim":"x"}"#, None).0, 400);
    let (st, v) = l.claims_answer(&json!({"claim": "[3]", "ids": [full]}).to_string(), None);
    assert!(st == 200 && v["results"].as_array().unwrap().is_empty());
}
