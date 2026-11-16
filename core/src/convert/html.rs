//! A forgiving HTML parser for publisher pages (which an XML parser rejects: unclosed `<p>` and
//! `<li>`, unquoted attributes, `<br>`, bare `&`). A port of `HTMLDOM.swift`; builds the same
//! [`xml::Doc`] tree the XML converters use, so the readers can share its helpers.
//!
//! Not a full HTML5 tree builder; it handles what article pages need: void elements, raw-text
//! elements (script, style), implied end tags for p / li / dt / dd / tr / td / th / option, end
//! tags without a matching start (ignored) and entities. MathML (`<math>` subtrees) is flagged
//! `is_math`, as the XML parser does. The root is a synthetic `#document` element.

use super::xml::{self, Doc, NodeId};
use std::collections::HashMap;
use std::sync::OnceLock;

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr", "keygen",
    "command",
];
const RAW_TEXT: &[&str] = &["script", "style", "textarea", "title", "noscript", "template", "iframe", "xmp"];
/// Opening one of these closes an open `<p>` (as in HTML).
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "details", "div", "dl", "fieldset", "figcaption", "figure", "footer", "form",
    "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "main", "nav", "ol", "p", "pre", "section", "table", "ul", "menu",
];
const P_SCOPE: &[&str] = &["div", "section", "article", "td", "th", "li", "blockquote", "figure", "table", "button"];

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

struct Builder {
    doc: Doc,
    stack: Vec<NodeId>,
}

impl Builder {
    fn name(&self, id: NodeId) -> &str {
        self.doc.node(id).name()
    }
    fn top(&self) -> NodeId {
        *self.stack.last().unwrap()
    }
    fn add_text(&mut self, t: &str) {
        if t.is_empty() {
            return;
        }
        let top = self.top();
        let m = self.doc.node(top).is_math();
        self.doc.add_text(top, t, m);
    }
    fn in_stack(&self, name: &str, stop_at: &[&str]) -> Option<usize> {
        let mut k = self.stack.len() - 1;
        while k > 0 {
            let n = self.name(self.stack[k]);
            if n == name {
                return Some(k);
            }
            if stop_at.contains(&n) {
                return None;
            }
            k -= 1;
        }
        None
    }
    fn pop_to(&mut self, k: usize) {
        self.stack.truncate(k);
    }
    fn open(&mut self, name: &str, attrs: Vec<(String, String)>, self_closing: bool) {
        let parent_math = self.doc.node(self.top()).is_math();
        let is_math = parent_math || name == "math";
        let is_foreign = is_math || name == "svg" || self.stack.iter().any(|&s| self.name(s) == "svg");
        if !is_foreign {
            // implied end tags
            if CLOSES_P.contains(&name) {
                if let Some(k) = self.in_stack("p", P_SCOPE) {
                    self.pop_to(k);
                }
            }
            let k = match name {
                "li" => self.in_stack("li", &["ul", "ol", "menu"]),
                "dt" | "dd" => self.in_stack("dd", &["dl"]).or_else(|| self.in_stack("dt", &["dl"])),
                "tr" => self.in_stack("tr", &["table", "thead", "tbody", "tfoot"]),
                "td" | "th" => self.in_stack("td", &["tr", "table"]).or_else(|| self.in_stack("th", &["tr", "table"])),
                "thead" | "tbody" | "tfoot" => self
                    .in_stack("thead", &["table"])
                    .or_else(|| self.in_stack("tbody", &["table"]))
                    .or_else(|| self.in_stack("tfoot", &["table"])),
                "option" => self.in_stack("option", &["select", "datalist"]),
                _ => None,
            };
            if let Some(k) = k {
                self.pop_to(k);
            }
        }
        let mut list: Vec<(String, String)> = Vec::new();
        for (k, v) in attrs {
            let mut key = k;
            if let Some(c) = key.rfind(':') {
                if key.starts_with("xmlns") || is_math {
                    continue;
                }
                key = key[c + 1..].to_string();
            }
            if key == "xmlns" {
                continue;
            }
            match list.iter_mut().find(|(n, _)| *n == key) {
                Some(x) => x.1 = v,
                None => list.push((key, v)),
            }
        }
        let id = self.doc.push(name, list, String::new(), is_math);
        let top = self.top();
        self.doc.append(top, &[id]);
        let void = VOID.contains(&name);
        if !(void && !is_foreign) && !(self_closing && is_foreign) && !(self_closing && void) {
            self.stack.push(id);
        }
    }
    fn close(&mut self, name: &str) {
        if name == "p" && self.in_stack("p", &[]).is_none() {
            return; // a stray </p>
        }
        if let Some(k) = self.in_stack(name, &[]) {
            self.pop_to(k);
        }
    }
}

/// Parses a whole page (or fragment).
pub fn parse(html: &str) -> Doc {
    let mut b = Builder { doc: Doc::with_root("#document"), stack: vec![0] };
    let s = html.as_bytes();
    let n = s.len();
    let mut i = 0;
    let mut text_start = 0;
    let flush = |b: &mut Builder, start: usize, end: usize| {
        if end > start {
            b.add_text(&decode(&html[start..end]));
        }
    };
    while i < n {
        if s[i] != b'<' {
            i += 1;
            continue;
        }
        // comment
        if i + 3 < n && s[i + 1] == b'!' && s[i + 2] == b'-' && s[i + 3] == b'-' {
            flush(&mut b, text_start, i);
            let mut j = i + 4;
            while j + 2 < n && !(s[j] == b'-' && s[j + 1] == b'-' && s[j + 2] == b'>') {
                j += 1;
            }
            i = n.min(j + 3);
            text_start = i;
            continue;
        }
        // CDATA
        if i + 8 < n && &s[i..i + 9] == b"<![CDATA[" {
            flush(&mut b, text_start, i);
            let mut j = i + 9;
            while j + 2 < n && !(s[j] == b']' && s[j + 1] == b']' && s[j + 2] == b'>') {
                j += 1;
            }
            let e = j.min(n);
            b.add_text(&html[i + 9..e]);
            i = n.min(j + 3);
            text_start = i;
            continue;
        }
        // doctype / processing instruction
        if i + 1 < n && (s[i + 1] == b'!' || s[i + 1] == b'?') {
            flush(&mut b, text_start, i);
            let mut j = i + 2;
            while j < n && s[j] != b'>' {
                j += 1;
            }
            i = n.min(j + 1);
            text_start = i;
            continue;
        }
        let is_end = i + 1 < n && s[i + 1] == b'/';
        let name_start = i + if is_end { 2 } else { 1 };
        if !(name_start < n && s[name_start].is_ascii_alphabetic()) {
            i += 1; // a literal "<"
            continue;
        }
        flush(&mut b, text_start, i);
        let mut j = name_start;
        while j < n && !is_space(s[j]) && s[j] != b'>' && s[j] != b'/' {
            j += 1;
        }
        let name = html[name_start..j].to_lowercase();
        // attributes
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut self_closing = false;
        while j < n {
            while j < n && is_space(s[j]) {
                j += 1;
            }
            if j >= n {
                break;
            }
            if s[j] == b'>' {
                j += 1;
                break;
            }
            if s[j] == b'/' {
                self_closing = true;
                j += 1;
                continue;
            }
            self_closing = false;
            let as0 = j;
            while j < n && !is_space(s[j]) && s[j] != b'>' && s[j] != b'=' && !(s[j] == b'/' && j + 1 < n && s[j + 1] == b'>') {
                j += 1;
            }
            if j == as0 {
                j += 1;
                continue;
            }
            let an = String::from_utf8_lossy(&s[as0..j]).to_lowercase();
            while j < n && is_space(s[j]) {
                j += 1;
            }
            let mut value = String::new();
            if j < n && s[j] == b'=' {
                j += 1;
                while j < n && is_space(s[j]) {
                    j += 1;
                }
                if j < n && (s[j] == b'"' || s[j] == b'\'') {
                    let q = s[j];
                    j += 1;
                    let vs = j;
                    while j < n && s[j] != q {
                        j += 1;
                    }
                    value = String::from_utf8_lossy(&s[vs..j.min(n)]).into_owned();
                    j += 1;
                } else {
                    let vs = j;
                    while j < n && !is_space(s[j]) && s[j] != b'>' {
                        j += 1;
                    }
                    value = String::from_utf8_lossy(&s[vs..j]).into_owned();
                }
            }
            if !is_end {
                attrs.push((an, decode(&value)));
            }
        }
        i = j.min(n);
        text_start = i;
        if is_end {
            b.close(&name);
            continue;
        }
        b.open(&name, attrs, self_closing);
        // raw text: everything up to the matching end tag
        if RAW_TEXT.contains(&name.as_str()) && !self_closing {
            let end_tag = format!("</{name}");
            let et = end_tag.as_bytes();
            let mut k = i;
            while k < n {
                if s[k] == b'<' && k + et.len() <= n && s[k..k + et.len()].eq_ignore_ascii_case(et) {
                    break;
                }
                k += 1;
            }
            let top = b.top();
            if b.name(top) == name {
                let raw = String::from_utf8_lossy(&s[i..k]).into_owned();
                let t = b.doc.push("#text", vec![], raw, false);
                b.doc.append(top, &[t]);
                b.stack.pop();
            }
            let mut e = k;
            while e < n && s[e] != b'>' {
                e += 1;
            }
            i = n.min(e + 1);
            text_start = i;
        }
    }
    flush(&mut b, text_start, n);
    b.doc
}

fn extra_entities() -> &'static HashMap<&'static str, u32> {
    static T: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    T.get_or_init(|| {
        [
            ("apos", 39), ("quot", 34), ("amp", 38), ("lt", 60), ("gt", 62), ("shy", 173), ("laquo", 171), ("raquo", 187),
            ("sbquo", 8218), ("bdquo", 8222), ("lsaquo", 8249), ("rsaquo", 8250), ("euro", 8364), ("pound", 163), ("yen", 165),
            ("cent", 162), ("frac12", 189), ("frac14", 188), ("frac34", 190), ("sup1", 185), ("sup2", 178), ("sup3", 179),
            ("ordf", 170), ("ordm", 186), ("not", 172), ("acute", 180), ("cedil", 184), ("uml", 168), ("macr", 175),
            ("iexcl", 161), ("iquest", 191), ("agrave", 224), ("egrave", 232), ("igrave", 236), ("ograve", 242),
            ("ugrave", 249), ("acirc", 226), ("ecirc", 234), ("icirc", 238), ("ocirc", 244), ("ucirc", 251), ("atilde", 227),
            ("otilde", 245), ("euml", 235), ("iuml", 239), ("yuml", 255), ("Eacute", 201), ("Ccedil", 199), ("Oslash", 216),
            ("AElig", 198), ("oelig", 339), ("OElig", 338), ("scaron", 353), ("Scaron", 352), ("zcaron", 382),
            ("Zcaron", 381), ("isin", 8712), ("notin", 8713), ("sum", 8721), ("prod", 8719), ("radic", 8730),
            ("prop", 8733), ("part", 8706), ("nabla", 8711), ("forall", 8704), ("exist", 8707), ("empty", 8709),
            ("cap", 8745), ("cup", 8746), ("sub", 8834), ("sup", 8835), ("and", 8743), ("or", 8744), ("equiv", 8801),
            ("cong", 8773), ("lowast", 8727), ("sdot", 8901), ("larr", 8592), ("rArr", 8658), ("lArr", 8656),
            ("hArr", 8660), ("varepsilon", 949), ("thetasym", 977), ("sigmaf", 962), ("upsih", 978), ("piv", 982),
            ("Alpha", 913), ("Beta", 914), ("Epsilon", 917), ("Zeta", 918), ("Eta", 919), ("Iota", 921), ("Kappa", 922),
            ("Mu", 924), ("Nu", 925), ("Omicron", 927), ("omicron", 959), ("Rho", 929), ("Tau", 932), ("Upsilon", 933),
            ("Chi", 935), ("zwsp", 8203), ("lrm", 8206), ("rlm", 8207), ("NewLine", 10), ("Tab", 9), ("colon", 58),
            ("comma", 44), ("period", 46), ("lpar", 40), ("rpar", 41), ("lsqb", 91), ("rsqb", 93), ("lbrack", 91),
            ("rbrack", 93), ("sol", 47), ("bsol", 92), ("ast", 42), ("num", 35), ("percnt", 37), ("plus", 43),
            ("equals", 61), ("excl", 33), ("quest", 63), ("commat", 64), ("dollar", 36), ("lowbar", 95), ("verbar", 124),
            ("vert", 124), ("lcub", 123), ("rcub", 125), ("Hat", 94), ("grave", 96), ("half", 189), ("hyphen", 8208),
            ("dash", 8208),
        ]
        .into_iter()
        .collect()
    })
}

/// Decodes character references (named, decimal, hex).
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('&') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        let mut end = 0;
        for (len, (i, c)) in after.char_indices().enumerate() {
            if len >= 32 || !(c.is_alphabetic() || c.is_numeric() || c == '#') {
                break;
            }
            end = i + c.len_utf8();
        }
        let name = &after[..end];
        let consumed = if after[end..].starts_with(';') { end + 1 } else { end };
        let scalar = if let Some(h) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
        } else if let Some(d) = name.strip_prefix('#') {
            d.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            xml::named_entities().get(name).or_else(|| extra_entities().get(name)).and_then(|&v| char::from_u32(v))
        };
        match scalar {
            Some(c) if !name.is_empty() => {
                out.push(c);
                rest = &after[consumed..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgiving_markup() {
        let d = parse("<!DOCTYPE html><html><body><p class=a id=x>One<p>Two &amp; three&nbsp;&copy &bogus; <br>end</p></p><ul><li>a<li>b</ul><script>if (a<b) x()</script><table><tr><td>1<td>2<tr><td>3</table><math><mi>x</mi></math></body></html>");
        let body = d.root().first(|n| n.name() == "body").unwrap();
        let ps: Vec<_> = body.all(|n| n.name() == "p");
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].attr("class"), Some("a"));
        assert_eq!(ps[0].plain(), "One");
        assert_eq!(ps[1].all_text(), "Two & three\u{a0}© &bogus; end");
        assert_eq!(body.all(|n| n.name() == "li").len(), 2);
        let script = body.first(|n| n.name() == "script").unwrap();
        assert_eq!(script.all_text(), "if (a<b) x()");
        let rows = body.all(|n| n.name() == "tr");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].all(|n| n.name() == "td").len(), 2);
        assert!(body.first(|n| n.name() == "mi").unwrap().is_math());
    }

    #[test]
    fn decodes_references() {
        assert_eq!(decode("a&#x41;&#66;&amp&lt;b&gt; &unknown; & x"), "aAB&<b> &unknown; & x");
        assert_eq!(decode("&alpha;&rArr;&hyphen;"), "α⇒‐");
    }
}
