//! A small DOM, mirroring `XML.swift` in Paper2EpubKit.
//!
//! Element and attribute names are stored without namespace prefixes (`xlink:href` becomes
//! `href`). Elements inside a MathML `<math>` subtree are flagged `is_math` so they can be
//! serialised back out verbatim. Attributes are kept sorted by their qualified name, like the
//! Swift builder (which sorts `XMLParser`'s attribute dictionary).

use super::ConvertError;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

pub type NodeId = usize;

#[derive(Debug)]
struct NodeData {
    name: String, // local name; "#text" for text nodes
    attrs: Vec<(String, String)>,
    text: String,
    is_math: bool,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
}

/// A parsed document: an arena of nodes. Node 0 is the root element.
#[derive(Debug)]
pub struct Doc {
    nodes: Vec<NodeData>,
}

/// A cheap handle to a node of a [`Doc`].
#[derive(Clone, Copy)]
pub struct Node<'a> {
    doc: &'a Doc,
    pub id: NodeId,
}

impl PartialEq for Node<'_> {
    fn eq(&self, o: &Self) -> bool {
        std::ptr::eq(self.doc, o.doc) && self.id == o.id
    }
}

impl Doc {
    pub fn root(&self) -> Node<'_> {
        Node { doc: self, id: 0 }
    }
    pub fn node(&self, id: NodeId) -> Node<'_> {
        Node { doc: self, id }
    }
}

impl<'a> Node<'a> {
    fn d(&self) -> &'a NodeData {
        &self.doc.nodes[self.id]
    }
    pub fn name(&self) -> &'a str {
        &self.d().name
    }
    pub fn is_text(&self) -> bool {
        self.d().name == "#text"
    }
    pub fn is_math(&self) -> bool {
        self.d().is_math
    }
    /// Text of a text node ("" for elements).
    pub fn text(&self) -> &'a str {
        &self.d().text
    }
    /// Attributes in document-builder order (sorted by qualified name).
    pub fn attrs(&self) -> &'a [(String, String)] {
        &self.d().attrs
    }
    pub fn attr(&self, n: &str) -> Option<&'a str> {
        self.d().attrs.iter().rev().find(|(k, _)| k == n).map(|(_, v)| v.as_str())
    }
    pub fn parent(&self) -> Option<Node<'a>> {
        self.d().parent.map(|id| Node { doc: self.doc, id })
    }
    pub fn children(&self) -> impl Iterator<Item = Node<'a>> + 'a {
        let doc = self.doc;
        self.d().children.iter().map(move |&id| Node { doc, id })
    }
    pub fn has_children(&self) -> bool {
        !self.d().children.is_empty()
    }
    pub fn elements(&self) -> impl Iterator<Item = Node<'a>> + 'a {
        self.children().filter(|c| !c.is_text())
    }
    pub fn child(&self, n: &str) -> Option<Node<'a>> {
        self.children().find(|c| c.name() == n)
    }
    pub fn children_named(&self, n: &'a str) -> impl Iterator<Item = Node<'a>> + 'a {
        self.children().filter(move |c| c.name() == n)
    }
    /// Pre-order descendants with the given name (not including self).
    pub fn descendants(&self, n: &str) -> Vec<Node<'a>> {
        let mut out = Vec::new();
        fn rec<'a>(x: Node<'a>, n: &str, out: &mut Vec<Node<'a>>) {
            for c in x.children() {
                if c.is_text() {
                    continue;
                }
                if c.name() == n {
                    out.push(c);
                }
                rec(c, n, out);
            }
        }
        rec(*self, n, &mut out);
        out
    }
    pub fn first_descendant(&self, n: &str) -> Option<Node<'a>> {
        for c in self.children() {
            if c.is_text() {
                continue;
            }
            if c.name() == n {
                return Some(c);
            }
            if let Some(d) = c.first_descendant(n) {
                return Some(d);
            }
        }
        None
    }
    /// Follows a simple slash path of child names, e.g. "caption/title".
    pub fn path(&self, p: &str) -> Option<Node<'a>> {
        let mut cur = Some(*self);
        for part in p.split('/').filter(|s| !s.is_empty()) {
            cur = cur.and_then(|c| c.child(part));
        }
        cur
    }
    /// All text in this subtree, concatenated.
    pub fn all_text(&self) -> String {
        let mut s = String::new();
        self.collect_text(&mut s);
        s
    }
    fn collect_text(&self, s: &mut String) {
        if self.is_text() {
            s.push_str(self.text());
        } else {
            for c in self.children() {
                c.collect_text(s);
            }
        }
    }
    /// Text with whitespace collapsed.
    pub fn plain(&self) -> String {
        self.all_text().split_whitespace().collect::<Vec<_>>().join(" ")
    }
    pub fn child_text(&self, n: &str) -> Option<String> {
        self.child(n).map(|c| c.all_text())
    }
    pub fn has_class(&self, c: &str) -> bool {
        self.attr("class").map(|v| v.split(' ').any(|x| x == c)).unwrap_or(false)
    }
}

/// Parses XML into a DOM.
pub fn parse(data: &[u8]) -> Result<Doc, ConvertError> {
    let s = prepare(data);
    parse_prepared(&s).map_err(|e| ConvertError::Message(format!("XML parse error: {e}")))
}

fn local(n: &str) -> &str {
    match n.rfind(':') {
        Some(i) => &n[i + 1..],
        None => n,
    }
}

fn normalize_attr(v: &str) -> String {
    v.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }).collect()
}

fn parse_prepared(s: &str) -> Result<Doc, String> {
    let mut r = Reader::from_str(s);
    {
        let c = r.config_mut();
        c.trim_text(false);
        c.expand_empty_elements = false;
        c.check_end_names = true;
    }
    let mut doc = Doc { nodes: Vec::new() };
    let mut stack: Vec<NodeId> = Vec::new();
    let mut done = false;

    fn add_text(doc: &mut Doc, stack: &[NodeId], t: &str) {
        let Some(&top) = stack.last() else { return };
        if let Some(&last) = doc.nodes[top].children.last() {
            if doc.nodes[last].name == "#text" {
                doc.nodes[last].text.push_str(t);
                return;
            }
        }
        let is_math = doc.nodes[top].is_math;
        let id = doc.nodes.len();
        doc.nodes.push(NodeData {
            name: "#text".into(),
            attrs: vec![],
            text: t.to_string(),
            is_math,
            parent: Some(top),
            children: vec![],
        });
        doc.nodes[top].children.push(id);
    }

    fn start(doc: &mut Doc, stack: &mut Vec<NodeId>, e: &BytesStart, done: bool) -> Result<NodeId, String> {
        if stack.is_empty() && done {
            return Err("extra content at the end of the document".into());
        }
        let qname = std::str::from_utf8(e.name().as_ref()).map_err(|e| e.to_string())?.to_string();
        let name = local(&qname).to_string();
        let parent_math = stack.last().map(|&p| doc.nodes[p].is_math).unwrap_or(false);
        let is_math = parent_math || name == "math";
        let mut raw: Vec<(String, String)> = Vec::new();
        for a in e.attributes() {
            let a = a.map_err(|e| e.to_string())?;
            let k = std::str::from_utf8(a.key.as_ref()).map_err(|e| e.to_string())?.to_string();
            let v = a.unescape_value().map_err(|e| e.to_string())?;
            raw.push((k, normalize_attr(&v)));
        }
        raw.sort_by(|a, b| a.0.cmp(&b.0));
        let mut attrs: Vec<(String, String)> = Vec::new();
        for (k, v) in raw {
            if k.starts_with("xmlns") {
                continue;
            }
            if is_math && k.contains(':') {
                continue; // drop namespaced attrs inside MathML
            }
            let lk = local(&k).to_string();
            if let Some(x) = attrs.iter_mut().find(|(n, _)| *n == lk) {
                x.1 = v; // same local name twice (href + xlink:href): keep one, the last
            } else {
                attrs.push((lk, v));
            }
        }
        let id = doc.nodes.len();
        let parent = stack.last().copied();
        doc.nodes.push(NodeData { name, attrs, text: String::new(), is_math, parent, children: vec![] });
        if let Some(p) = parent {
            doc.nodes[p].children.push(id);
        }
        Ok(id)
    }

    loop {
        let ev = r.read_event().map_err(|e| format!("{e} (at byte {})", r.error_position()))?;
        match ev {
            Event::Start(e) => {
                let id = start(&mut doc, &mut stack, &e, done)?;
                stack.push(id);
            }
            Event::Empty(e) => {
                let id = start(&mut doc, &mut stack, &e, done)?;
                if stack.is_empty() {
                    done = true;
                    let _ = id;
                }
            }
            Event::End(_) => {
                stack.pop();
                if stack.is_empty() {
                    done = true;
                }
            }
            Event::Text(t) => {
                let t = t.decode().map_err(|e| e.to_string())?;
                if stack.is_empty() {
                    if !t.trim().is_empty() {
                        return Err("text outside the root element".into());
                    }
                } else {
                    add_text(&mut doc, &stack, &t);
                }
            }
            Event::CData(t) => {
                let t = t.decode().map_err(|e| e.to_string())?;
                add_text(&mut doc, &stack, &t);
            }
            Event::GeneralRef(e) => {
                let txt = if let Some(c) = e.resolve_char_ref().map_err(|e| e.to_string())? {
                    c.to_string()
                } else {
                    let n = e.decode().map_err(|e| e.to_string())?;
                    match n.as_ref() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        other => return Err(format!("undefined entity &{other};")),
                    }
                };
                add_text(&mut doc, &stack, &txt);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if doc.nodes.is_empty() {
        return Err("no root element".into());
    }
    if !stack.is_empty() {
        return Err("unexpected end of document".into());
    }
    Ok(doc)
}

/// Removes the DOCTYPE (DTDs are never loaded) and turns HTML-style named entities into numeric
/// references, which an XML parser would otherwise reject.
pub fn prepare(data: &[u8]) -> String {
    let mut s = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => data.iter().map(|&b| b as char).collect(), // ISO Latin-1
    };
    if s.contains('\r') {
        s = s.replace("\r\n", "\n").replace('\r', "\n");
    }
    static DT: OnceLock<Regex> = OnceLock::new();
    let dt = DT.get_or_init(|| Regex::new(r"<!DOCTYPE[^>\[]*(\[[\s\S]*?\])?\s*>").unwrap());
    if let Some(m) = dt.find(&s) {
        s.replace_range(m.range(), "");
    }
    static ENT: OnceLock<Regex> = OnceLock::new();
    let ent = ENT.get_or_init(|| Regex::new(r"&([A-Za-z][A-Za-z0-9]*);").unwrap());
    if ent.is_match(&s) {
        let table = named_entities();
        s = ent
            .replace_all(&s, |c: &regex::Captures| {
                let name = &c[1];
                if ["amp", "lt", "gt", "quot", "apos"].contains(&name) {
                    format!("&{name};")
                } else if let Some(cp) = table.get(name) {
                    format!("&#{cp};")
                } else {
                    format!("&amp;{name};")
                }
            })
            .into_owned();
    }
    s
}

fn named_entities() -> &'static HashMap<&'static str, u32> {
    static T: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    T.get_or_init(|| {
        [
            ("nbsp", 160), ("ensp", 8194), ("emsp", 8195), ("thinsp", 8201), ("hairsp", 8202), ("zwnj", 8204),
            ("zwj", 8205), ("ndash", 8211), ("mdash", 8212), ("hellip", 8230), ("lsquo", 8216), ("rsquo", 8217),
            ("ldquo", 8220), ("rdquo", 8221), ("bull", 8226), ("middot", 183), ("times", 215), ("divide", 247),
            ("minus", 8722), ("plusmn", 177), ("deg", 176), ("micro", 181), ("prime", 8242), ("Prime", 8243),
            ("le", 8804), ("ge", 8805), ("ne", 8800), ("asymp", 8776), ("infin", 8734), ("rarr", 8594),
            ("larr", 8592), ("harr", 8596), ("uarr", 8593), ("darr", 8595), ("sim", 8764), ("copy", 169),
            ("reg", 174), ("trade", 8482), ("sect", 167), ("para", 182), ("dagger", 8224), ("Dagger", 8225),
            ("alpha", 945), ("beta", 946), ("gamma", 947), ("delta", 948), ("epsilon", 949), ("zeta", 950),
            ("eta", 951), ("theta", 952), ("iota", 953), ("kappa", 954), ("lambda", 955), ("mu", 956), ("nu", 957),
            ("xi", 958), ("pi", 960), ("rho", 961), ("sigma", 963), ("tau", 964), ("upsilon", 965), ("phi", 966),
            ("chi", 967), ("psi", 968), ("omega", 969), ("Gamma", 915), ("Delta", 916), ("Theta", 920),
            ("Lambda", 923), ("Xi", 926), ("Pi", 928), ("Sigma", 931), ("Phi", 934), ("Psi", 936), ("Omega", 937),
            ("auml", 228), ("ouml", 246), ("uuml", 252), ("Auml", 196), ("Ouml", 214), ("Uuml", 220),
            ("aring", 229), ("Aring", 197), ("eacute", 233), ("egrave", 232), ("aacute", 225), ("iacute", 237),
            ("oacute", 243), ("uacute", 250), ("ccedil", 231), ("ntilde", 241), ("szlig", 223), ("oslash", 248),
            ("aelig", 230),
        ]
        .into_iter()
        .collect()
    })
}

/// XML-escapes text and drops characters that are illegal in XML 1.0.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => {
                let v = ch as u32;
                if v == 0x9
                    || v == 0xA
                    || v == 0xD
                    || (0x20..=0xD7FF).contains(&v)
                    || (0xE000..=0xFFFD).contains(&v)
                    || v >= 0x10000
                {
                    out.push(ch);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaces_entities_and_math() {
        let x = br#"<?xml version="1.0"?><!DOCTYPE article PUBLIC "x" "y.dtd"><article xmlns:xlink="http://www.w3.org/1999/xlink"><p>a&nbsp;b &amp; c<graphic xlink:href="f1.jpg"/><mml:math display="block" mml:foo="1"><mml:mi>x</mml:mi></mml:math></p></article>"#;
        let d = parse(x).unwrap();
        let r = d.root();
        assert_eq!(r.name(), "article");
        let p = r.child("p").unwrap();
        assert_eq!(p.children().next().unwrap().text(), "a\u{a0}b & c");
        assert_eq!(p.child("graphic").unwrap().attr("href"), Some("f1.jpg"));
        let m = p.child("math").unwrap();
        assert!(m.is_math());
        assert_eq!(m.attrs().len(), 1);
        assert!(m.child("mi").unwrap().is_math());
    }
}
