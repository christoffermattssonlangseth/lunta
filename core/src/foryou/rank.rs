//! Ranking on the device (WatchCenter.score): each new paper is compared with your paragraph,
//! the papers in your library (and your own), and the papers you added, turned down or asked for
//! more of. The Apple apps compare Apple's sentence embeddings (NLEmbedding); there is no such
//! model here, so the texts are compared as TF-IDF vectors of their words and word pairs, with the
//! same formula. The two kinds of closeness are on different scales (TF-IDF cosines are lower),
//! so feeds made here and on a Mac rank alike but not identically.

use std::collections::HashMap;

/// A text as a unit vector of weighted terms.
pub type Vector = HashMap<String, f64>;

const STOP: &[&str] = &[
    "a", "about", "above", "across", "after", "again", "against", "all", "also", "among", "an", "and", "any", "are", "as", "at", "be",
    "been", "being", "between", "both", "but", "by", "can", "could", "did", "do", "does", "during", "each", "either", "even", "few",
    "for", "from", "further", "had", "has", "have", "having", "here", "how", "however", "if", "in", "into", "is", "it", "its",
    "itself", "may", "might", "more", "most", "much", "must", "no", "nor", "not", "of", "on", "once", "only", "or", "other", "our",
    "out", "over", "own", "same", "several", "should", "so", "some", "such", "than", "that", "the", "their", "them", "then",
    "there", "these", "they", "this", "those", "through", "thus", "to", "under", "until", "up", "upon", "use", "used", "using",
    "very", "via", "was", "we", "were", "what", "when", "where", "whether", "which", "while", "who", "whom", "why", "will", "with",
    "within", "without", "would", "yet", "you", "your", "i", "my", "me", "work", "study", "studies", "paper", "show", "shows",
    "shown", "found", "find", "results", "result", "based", "new", "novel", "approach", "method", "methods", "data", "analysis",
    "here", "well", "one", "two", "three", "first", "high", "low", "different", "including", "include", "includes", "provide",
    "provides", "across", "large", "via", "towards", "toward",
];

fn stem(w: &str) -> String {
    let n = w.chars().count();
    if n > 4 && w.ends_with("ies") {
        format!("{}y", &w[..w.len() - 3])
    } else if n > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is") {
        w[..w.len() - 1].to_string()
    } else {
        w.to_string()
    }
}

/// The terms of a text: words (without common ones, plurals folded) and adjacent word pairs.
pub fn terms(text: &str) -> Vec<String> {
    let mut words: Vec<Option<String>> = vec![];
    for w in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if w.is_empty() {
            continue;
        }
        if w.chars().count() < 3 || STOP.contains(&w) || w.chars().all(|c| c.is_ascii_digit()) {
            words.push(None); // breaks a pair
        } else {
            words.push(Some(stem(w)));
        }
    }
    let mut out: Vec<String> = words.iter().flatten().cloned().collect();
    for pair in words.windows(2) {
        if let [Some(a), Some(b)] = pair {
            out.push(format!("{a} {b}"));
        }
    }
    out
}

/// Inverse document frequencies over a set of texts.
pub struct Idf {
    df: HashMap<String, usize>,
    n: usize,
}

impl Idf {
    pub fn new<'a>(texts: impl IntoIterator<Item = &'a str>) -> Idf {
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut n = 0;
        for t in texts {
            n += 1;
            let mut seen: Vec<String> = terms(t);
            seen.sort();
            seen.dedup();
            for term in seen {
                *df.entry(term).or_default() += 1;
            }
        }
        Idf { df, n }
    }

    /// A text as a unit vector (None when it has no terms).
    pub fn vector(&self, text: &str) -> Option<Vector> {
        let mut tf: HashMap<String, f64> = HashMap::new();
        for t in terms(text) {
            *tf.entry(t).or_default() += 1.0;
        }
        let mut v: Vector = tf
            .into_iter()
            .map(|(t, c)| {
                let df = *self.df.get(&t).unwrap_or(&0) as f64;
                let idf = ((self.n as f64 + 1.0) / (df + 1.0)).ln() + 1.0;
                // word pairs say more than their words
                let pair = if t.contains(' ') { 1.5 } else { 1.0 };
                (t, (1.0 + c.ln()) * idf * pair)
            })
            .collect();
        let norm = v.values().map(|x| x * x).sum::<f64>().sqrt();
        if norm == 0.0 {
            return None;
        }
        v.values_mut().for_each(|x| *x /= norm);
        Some(v)
    }
}

/// Cosine of two unit vectors.
pub fn cos(a: &Vector, b: &Vector) -> f64 {
    let (small, big) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    small.iter().map(|(t, x)| x * big.get(t).unwrap_or(&0.0)).sum()
}

/// How close each text is to what you want (WatchCenter.score): 0.55 × closeness to your
/// paragraph + 0.35 × the mean of the three closest library papers (the library alone without a
/// paragraph), + 0.25 × (closest added − closest turned down) + 0.3 × closest "more like this".
pub fn score(texts: &[String], interests: &str, library: &[String], liked: &[String], disliked: &[String], boosted: &[String]) -> Vec<f64> {
    let all = texts.iter().chain(library).chain(liked).chain(disliked).chain(boosted).map(String::as_str).chain(std::iter::once(interests));
    let idf = Idf::new(all);
    let p = if interests.is_empty() { None } else { idf.vector(interests) };
    let vecs = |list: &[String]| list.iter().filter_map(|t| idf.vector(t)).collect::<Vec<_>>();
    let (lib, up, down, boost) = (vecs(library), vecs(liked), vecs(disliked), vecs(boosted));
    let max = |list: &[Vector], v: &Vector| list.iter().map(|x| cos(x, v)).fold(None, |m: Option<f64>, c| Some(m.map_or(c, |m| m.max(c)))).unwrap_or(0.0);
    texts
        .iter()
        .map(|t| {
            let Some(v) = idf.vector(t) else { return 0.0 };
            let to_interests = p.as_ref().map_or(0.0, |p| cos(p, &v));
            let mut near: Vec<f64> = lib.iter().map(|x| cos(x, &v)).collect();
            near.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            near.truncate(3);
            let to_library = if near.is_empty() { 0.0 } else { near.iter().sum::<f64>() / near.len() as f64 };
            let base = if interests.is_empty() || p.is_none() { to_library } else { 0.55 * to_interests + 0.35 * to_library };
            base + 0.25 * (max(&up, &v) - max(&down, &v)) + 0.3 * max(&boost, &v)
        })
        .collect()
}

/// Newer first among papers about as close: up to +0.04 for one published today, none at 9 days.
pub fn recency(date: &str, now: i64, tz_min: i32) -> f64 {
    match super::time::published(date, tz_min) {
        Some(d) => 0.04 * (1.0 - (now - d) as f64 / (9.0 * super::time::DAY_MS as f64)).max(0.0),
        None => 0.0,
    }
}
