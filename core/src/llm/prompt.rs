//! The words the model is given and what is made of its answers, shared by every app on the open
//! model. The summary rules are apple/Shared/SummaryWriter.swift's, so a summary reads the same
//! whichever device wrote it (and the source hash is the same, so it is reused across devices).

use sha2::{Digest, Sha256};

/// Words of the paper given to the model: luntaSummarySource's budget, as in the Apple apps (the
/// source hash is taken of this text, so it must be the same everywhere).
pub const SUMMARY_WORDS: u32 = 1900;
/// Tokens the summary may take (3–4 sentences are about 120–180).
pub const SUMMARY_TOKENS: u32 = 300;

pub const SUMMARY_INSTRUCTIONS: &str = "You summarize scientific papers for researchers. You get excerpts from one paper: its abstract, \
the aims from the end of its introduction, and the start of its discussion and conclusion. \
Write 3 or 4 plain sentences (60 to 110 words) on what the study did and what it found: \
the question, the approach, and the main findings, with key numbers when the excerpts give them. \
Use only the excerpts and the paper's own terms. Leave out background, praise and hype. \
Start with the substance: never begin with \"This paper\", \"This study\", \"The study\", \"The authors\" or \"In this\". \
No title, no list, no headings, no preamble.";

/// The user turn of a summary: the title, then what luntaSummarySource picked.
pub fn summary_prompt(title: &str, source: &str) -> String {
    let t = title.trim();
    let head = if t.is_empty() { String::new() } else { format!("Title: {t}\n\n") };
    format!("{head}{source}\n\nSummarize this paper in 3 or 4 sentences.")
}

/// The key a saved summary is kept under (SummaryWriter.sourceHash): the first 12 bytes of the
/// SHA-256 of the source, in hex. A remade reading view gives another text, so another summary.
pub fn source_hash(source: &str) -> String {
    Sha256::digest(source.as_bytes()).iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// The answer without a thinking trace (an unfinished one gives "").
pub fn without_thinking(raw: &str) -> String {
    match (raw.find("<think>"), raw.find("</think>")) {
        (Some(_), Some(end)) => raw[end + "</think>".len()..].trim_start().to_string(),
        (Some(_), None) => String::new(),
        _ => raw.to_string(),
    }
}

/// The model's answer as one plain paragraph: no thinking trace, markdown, label or quotes
/// (SummaryWriter.clean).
pub fn clean_summary(raw: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;
    static LABEL: OnceLock<Regex> = OnceLock::new();
    static BULLET: OnceLock<Regex> = OnceLock::new();
    let label = LABEL.get_or_init(|| Regex::new(r"(?i)^\s*(#+\s*)?(Summary|TL;DR)\s*[:\-–—]\s*").unwrap());
    let bullet = BULLET.get_or_init(|| Regex::new(r"(?m)^\s*([-*•]|\d+[.)])\s+").unwrap());
    if raw.contains("<think>") && !raw.contains("</think>") {
        return String::new();
    }
    let s = without_thinking(raw).replace("**", "").replace("__", "");
    let s = label.replace(&s, "");
    let s = bullet.replace_all(&s, "");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_matches(|c| matches!(c, '"' | '“' | '”' | '\''));
    s.trim().to_string()
}

/// A real summary, not a refusal or a fragment (SummaryWriter.acceptable).
pub fn summary_acceptable(s: &str) -> bool {
    if s.chars().count() < 80 || s.split(' ').count() < 15 {
        return false;
    }
    let low = s.to_lowercase();
    const REFUSALS: [&str; 12] = [
        "i'm sorry", "i am sorry", "i cannot", "i can't", "i can not", "as an ai", "i'm unable", "i am unable",
        "cannot assist", "can't assist", "not able to help", "i apologize",
    ];
    !REFUSALS.iter().any(|r| low.starts_with(r) || low.contains(&format!(" {r}")))
}

/// A turn of a conversation.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ChatTurn {
    /// "user" or "assistant"
    pub role: String,
    pub text: String,
}

/// Qwen3's chat template (ChatML), with thinking off: the empty think block is what the
/// template writes for `enable_thinking=False`. Earlier assistant turns go in without their
/// thinking, as the template does.
pub fn chatml(system: &str, turns: &[ChatTurn], think: bool) -> String {
    let mut s = String::new();
    if !system.is_empty() {
        s += &format!("<|im_start|>system\n{}<|im_end|>\n", system.trim());
    }
    for t in turns {
        let role = if t.role == "assistant" { "assistant" } else { "user" };
        let text = if role == "assistant" { without_thinking(&t.text) } else { t.text.clone() };
        s += &format!("<|im_start|>{role}\n{}<|im_end|>\n", text.trim());
    }
    s += "<|im_start|>assistant\n";
    if !think {
        s += "<think>\n\n</think>\n\n";
    }
    s
}

/// "One-hundred eighteen subjects" → "118 subjects": small models misread spelled-out numbers
/// (Assistant.swift's digitize).
pub fn digitize(text: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:(?:zero|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|and)(?:[\s-]+|\b)){2,}").unwrap()
    });
    fn unit(w: &str) -> Option<u64> {
        const U: [&str; 20] = [
            "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
            "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
        ];
        const T: [&str; 8] = ["twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
        U.iter().position(|u| *u == w).map(|i| i as u64).or_else(|| T.iter().position(|t| *t == w).map(|i| 20 + 10 * i as u64))
    }
    re.replace_all(text, |c: &regex::Captures| {
        let raw = &c[0];
        let words: Vec<String> = raw.to_lowercase().split(|ch: char| ch == ' ' || ch == '-' || ch == '\n').filter(|w| !w.is_empty()).map(String::from).collect();
        let scale = |w: &str| match w {
            "hundred" => Some(100u64),
            "thousand" => Some(1000),
            _ => None,
        };
        let has_scale_or_tens = words.iter().any(|w| scale(w).is_some() || (w.ends_with("ty") && unit(w).is_some()));
        if !has_scale_or_tens || words.first().map(String::as_str) == Some("and") || words.last().map(String::as_str) == Some("and") {
            return raw.to_string();
        }
        let (mut total, mut current) = (0u64, 0u64);
        for w in words.iter().filter(|w| *w != "and") {
            if let Some(u) = unit(w) {
                current += u;
            } else if let Some(sc) = scale(w) {
                current = current.max(1) * sc;
                if sc == 1000 {
                    total += current;
                    current = 0;
                }
            }
        }
        let trailing = if raw.ends_with(' ') || raw.ends_with('-') { " " } else { "" };
        format!("{}{trailing}", total + current)
    })
    .to_string()
}
