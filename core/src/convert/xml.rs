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

impl<'a> Node<'a> {
    /// The document this node belongs to.
    pub fn doc(&self) -> &'a Doc {
        self.doc
    }
    /// Class names (split on whitespace).
    pub fn classes(&self) -> Vec<&'a str> {
        self.attr("class").map(|c| c.split_whitespace().collect()).unwrap_or_default()
    }
    /// Class and id words split on `-` and `_` ("c-article-share-box" → c, article, share, box),
    /// lower-cased.
    pub fn name_tokens(&self) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        for c in self.classes().into_iter().chain(std::iter::once(self.attr("id").unwrap_or(""))) {
            for t in c.to_lowercase().split(['-', '_']) {
                if !t.is_empty() {
                    out.insert(t.to_string());
                }
            }
        }
        out
    }
    /// Pre-order elements (not including self) for which `pred` holds.
    pub fn all(&self, pred: impl Fn(Node<'a>) -> bool) -> Vec<Node<'a>> {
        fn rec<'a>(x: Node<'a>, pred: &dyn Fn(Node<'a>) -> bool, out: &mut Vec<Node<'a>>) {
            for c in x.elements() {
                if pred(c) {
                    out.push(c);
                }
                rec(c, pred, out);
            }
        }
        let mut out = Vec::new();
        rec(*self, &pred, &mut out);
        out
    }
    /// The first element (pre-order, not self) for which `pred` holds.
    pub fn first(&self, pred: impl Fn(Node<'a>) -> bool) -> Option<Node<'a>> {
        fn rec<'a>(x: Node<'a>, pred: &dyn Fn(Node<'a>) -> bool) -> Option<Node<'a>> {
            for c in x.elements() {
                if pred(c) {
                    return Some(c);
                }
                if let Some(d) = rec(c, pred) {
                    return Some(d);
                }
            }
            None
        }
        rec(*self, &pred)
    }
    /// Parent, grandparent, … up to the root.
    pub fn ancestors(&self) -> Vec<Node<'a>> {
        let mut out = Vec::new();
        let mut p = self.parent();
        while let Some(x) = p {
            out.push(x);
            p = x.parent();
        }
        out
    }
    /// Whether any ancestor satisfies `pred`.
    pub fn has_ancestor(&self, pred: impl Fn(Node<'a>) -> bool) -> bool {
        let mut p = self.parent();
        while let Some(x) = p {
            if pred(x) {
                return true;
            }
            p = x.parent();
        }
        false
    }
    /// Whether `self` is inside `other` (not counting itself).
    pub fn is_inside(&self, other: Node<'a>) -> bool {
        self.has_ancestor(|a| a.id == other.id)
    }
    /// Index among the parent's children.
    pub fn index_in_parent(&self) -> Option<usize> {
        let p = self.parent()?;
        p.d().children.iter().position(|&c| c == self.id)
    }
}

// Building and rewriting a document (the web-page readers normalise publisher markup in place).
// Nodes taken out stay in the arena, detached (no parent).
impl Doc {
    /// An empty document whose root (node 0) is an element named `root`.
    pub fn with_root(root: &str) -> Doc {
        Doc {
            nodes: vec![NodeData {
                name: root.into(),
                attrs: vec![],
                text: String::new(),
                is_math: false,
                parent: None,
                children: vec![],
            }],
        }
    }
    /// A new detached element.
    pub fn new_element(&mut self, name: &str, attrs: Vec<(String, String)>) -> NodeId {
        self.push(name, attrs, String::new(), false)
    }
    /// A new detached text node.
    pub fn new_text(&mut self, t: &str) -> NodeId {
        self.push("#text", vec![], t.to_string(), false)
    }
    pub(crate) fn push(&mut self, name: &str, attrs: Vec<(String, String)>, text: String, is_math: bool) -> NodeId {
        self.nodes.push(NodeData { name: name.into(), attrs, text, is_math, parent: None, children: vec![] });
        self.nodes.len() - 1
    }
    /// Appends a text node to `parent` (joined to a text node that is its last child).
    pub(crate) fn add_text(&mut self, parent: NodeId, t: &str, is_math: bool) {
        if let Some(&last) = self.nodes[parent].children.last() {
            if self.nodes[last].name == "#text" {
                self.nodes[last].text.push_str(t);
                return;
            }
        }
        let id = self.push("#text", vec![], t.to_string(), is_math);
        self.nodes[id].parent = Some(parent);
        self.nodes[parent].children.push(id);
    }
    /// Takes the node out of its parent.
    pub fn remove(&mut self, id: NodeId) {
        if let Some(p) = self.nodes[id].parent.take() {
            self.nodes[p].children.retain(|&c| c != id);
        }
    }
    /// Adds nodes as the last children of `parent` (taking them out of where they were).
    pub fn append(&mut self, parent: NodeId, kids: &[NodeId]) {
        for &k in kids {
            self.remove(k);
            self.nodes[k].parent = Some(parent);
            self.nodes[parent].children.push(k);
        }
    }
    /// Puts `with` where `id` is (and takes `id` out).
    pub fn replace(&mut self, id: NodeId, with: &[NodeId]) {
        let Some(p) = self.nodes[id].parent else { return };
        for &n in with {
            if n != id {
                self.remove(n);
            }
        }
        let Some(i) = self.nodes[p].children.iter().position(|&c| c == id) else { return };
        self.nodes[p].children.remove(i);
        self.nodes[id].parent = None;
        for (k, &n) in with.iter().enumerate() {
            self.nodes[n].parent = Some(p);
            self.nodes[p].children.insert(i + k, n);
        }
    }
    /// Detaches all children.
    pub fn clear_children(&mut self, id: NodeId) {
        for c in std::mem::take(&mut self.nodes[id].children) {
            self.nodes[c].parent = None;
        }
    }
    /// The children's ids.
    pub fn child_ids(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes[id].children.clone()
    }
    /// Sets (Some) or removes (None) an attribute.
    pub fn set_attr(&mut self, id: NodeId, k: &str, v: Option<&str>) {
        let attrs = &mut self.nodes[id].attrs;
        match v {
            Some(v) => match attrs.iter_mut().find(|(n, _)| n == k) {
                Some(x) => x.1 = v.to_string(),
                None => attrs.push((k.to_string(), v.to_string())),
            },
            None => attrs.retain(|(n, _)| n != k),
        }
    }
    /// A copy of a node of another document (with its subtree), detached.
    pub fn import(&mut self, n: Node) -> NodeId {
        let d = n.d();
        let id = self.push(&d.name, d.attrs.clone(), d.text.clone(), d.is_math);
        for c in n.children() {
            let k = self.import(c);
            self.nodes[k].parent = Some(id);
            self.nodes[id].children.push(k);
        }
        id
    }
    /// Replaces a text node's text.
    pub fn set_text(&mut self, id: NodeId, t: &str) {
        self.nodes[id].text = t.to_string();
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

pub(crate) fn named_entities() -> &'static HashMap<&'static str, u32> {
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
