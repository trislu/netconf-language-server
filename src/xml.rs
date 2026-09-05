//! XML instance-document parsing via `tree-sitter-xml` (M0/M1).
//!
//! - `parse_root` — the root element's name + namespace declarations (M0
//!   intent sniffing).
//! - `parse` — the **full element tree** with resolved in-scope namespaces and
//!   byte ranges (M1 goto/hover/diagnostics and later features).
//!
//! Namespaces are resolved like an XML processor: a `xmlns` default namespace
//! and `xmlns:p` prefixes apply to the declaring element and its descendants.

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::{Node, Parser};
use tree_sitter_xml::LANGUAGE_XML;

// ---------------------------------------------------------------------------
// Root sniff (M0)
// ---------------------------------------------------------------------------

/// Root element of an XML document: name + in-scope namespace declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlRoot {
    /// Element name exactly as written in the start tag (may be `p:local`).
    pub name: String,
    /// Value of the `xmlns` (default namespace) attribute, if declared.
    pub default_ns: Option<String>,
    /// `xmlns:p` prefix → URI declarations on the root element.
    pub ns_prefixes: HashMap<String, String>,
}

impl XmlRoot {
    /// Element name without any `prefix:`.
    pub fn local_name(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    /// The namespace this element is in: its prefix binding if the name is
    /// prefixed, else the default `xmlns`.
    pub fn effective_ns(&self) -> Option<&str> {
        if let Some((prefix, _)) = self.name.split_once(':') {
            return self.ns_prefixes.get(prefix).map(String::as_str);
        }
        self.default_ns.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Full instance tree (M1+)
// ---------------------------------------------------------------------------

/// One element of an XML instance document.
#[derive(Debug, Clone)]
pub struct XmlNode {
    /// Parent element index (None for the document root element).
    pub parent: Option<usize>,
    /// Child element indices (document order).
    pub children: Vec<usize>,
    /// Element name exactly as written (may be `p:local`).
    pub name: String,
    /// Element name without any `prefix:`.
    pub local: String,
    /// Resolved namespace URI of this element (in-scope default/prefix).
    pub ns: Option<String>,
    /// Byte range of the element's **name** in the start tag.
    pub name_range: Range<usize>,
    /// Byte range of the whole element (start tag through end tag).
    pub elem_range: Range<usize>,
}

/// An XML instance document: an arena of elements, parents before children.
#[derive(Debug, Clone)]
pub struct XmlDoc {
    pub nodes: Vec<XmlNode>,
}

impl XmlDoc {
    /// The deepest element whose span contains `byte`, if any.
    pub fn element_at(&self, byte: usize) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            if n.elem_range.contains(&byte) {
                best = Some(match best {
                    None => i,
                    // Children start later than their parent, so this picks the
                    // deepest enclosing element.
                    Some(b) if n.elem_range.start >= self.nodes[b].elem_range.start => i,
                    Some(b) => b,
                });
            }
        }
        best
    }

    /// The trimmed text content of a **leaf** element `i` (one with no child
    /// elements), plus the byte range of that trimmed content in `text`.
    /// Returns `None` for container elements (or malformed tag structure).
    pub fn leaf_text(&self, text: &str, i: usize) -> Option<(Range<usize>, String)> {
        let n = self.nodes.get(i)?;
        if !n.children.is_empty() {
            return None;
        }
        let bytes = text.as_bytes();
        // End of the start/empty-element tag: the first `>` after the name,
        // honoring quoted attribute values.
        let mut in_q: Option<u8> = None;
        let mut tag_end = None;
        let mut j = n.name_range.start;
        while j < n.elem_range.end {
            let c = bytes[j];
            match in_q {
                Some(q) if c == q => in_q = None,
                Some(_) => {}
                None => match c {
                    b'"' | b'\'' => in_q = Some(c),
                    b'>' => {
                        tag_end = Some(j);
                        break;
                    }
                    _ => {}
                },
            }
            j += 1;
        }
        let tag_end = tag_end?;
        // Self-closing `<x/>`: no content.
        if tag_end > 0 && bytes[tag_end - 1] == b'/' {
            return Some((tag_end - 1..tag_end, String::new()));
        }
        // Content ends where the closing tag begins — the first `</` after the
        // start tag (leaf content cannot contain a raw `<`).
        let close = text[tag_end + 1..].find("</")? + (tag_end + 1);
        let raw = &bytes[tag_end + 1..close];
        let mut start = 0;
        while start < raw.len() && raw[start].is_ascii_whitespace() {
            start += 1;
        }
        let mut end = raw.len();
        while end > start && raw[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        let value = String::from_utf8_lossy(&raw[start..end]).to_string();
        Some(((tag_end + 1 + start)..(tag_end + 1 + end), value))
    }
}

// In-scope namespace scope (default + prefixes), cloned down the tree.
#[derive(Clone, Default)]
struct NsScope {
    default: Option<String>,
    prefixes: HashMap<String, String>,
}

impl NsScope {
    fn resolve(&self, raw_name: &str) -> Option<String> {
        match raw_name.split_once(':') {
            Some((prefix, _)) => self.prefixes.get(prefix).cloned(),
            None => self.default.clone(),
        }
    }
}

fn children<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    for i in 0..n.child_count() {
        if let Some(c) = n.child(i as u32) {
            out.push(c);
        }
    }
    out
}

fn child_of<'t>(n: Node<'t>, kind: &str) -> Option<Node<'t>> {
    children(n).into_iter().find(|c| c.kind() == kind)
}

/// `(raw name, attributes)` of a start/empty-element tag.
fn read_start_tag(start: Node<'_>, text: &str) -> Option<(String, Vec<(String, String)>)> {
    let name = child_of(start, "Name")?
        .utf8_text(text.as_bytes())
        .ok()?
        .to_owned();
    let mut attrs = Vec::new();
    for attr in children(start)
        .into_iter()
        .filter(|c| c.kind() == "Attribute")
    {
        let raw = attr.utf8_text(text.as_bytes()).ok()?.trim();
        let Some((aname, aval)) = raw.split_once('=') else {
            continue;
        };
        let aval = aval
            .trim()
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| {
                let aval = aval.trim();
                aval.strip_prefix('\'').and_then(|s| s.strip_suffix('\''))
            })
            .unwrap_or_else(|| aval.trim())
            .to_owned();
        attrs.push((aname.trim().to_owned(), aval));
    }
    Some((name, attrs))
}

/// Apply `xmlns`/`xmlns:p` attributes onto a copied scope.
fn apply_ns(scope: &NsScope, attrs: &[(String, String)]) -> NsScope {
    let mut out = scope.clone();
    for (name, value) in attrs {
        if name == "xmlns" {
            out.default = Some(value.clone());
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            out.prefixes.insert(prefix.to_owned(), value.clone());
        }
    }
    out
}

fn build_element(
    el: Node<'_>,
    parent: Option<usize>,
    scope: NsScope,
    nodes: &mut Vec<XmlNode>,
    text: &str,
) -> Option<usize> {
    let start = child_of(el, "STag").or_else(|| child_of(el, "EmptyElemTag"))?;
    let (name, attrs) = read_start_tag(start, text)?;
    // The element's own xmlns declarations apply to itself and descendants.
    let scope = apply_ns(&scope, &attrs);
    let name_node = child_of(start, "Name")?;
    let idx = nodes.len();
    nodes.push(XmlNode {
        parent,
        children: Vec::new(),
        name_range: name_node.byte_range(),
        elem_range: el.byte_range(),
        local: name.rsplit(':').next().unwrap_or(&name).to_owned(),
        ns: scope.resolve(&name),
        name,
    });
    // Direct child elements live in the <content> node of a non-empty element.
    if let Some(content) = child_of(el, "content") {
        for c in children(content)
            .into_iter()
            .filter(|c| c.kind() == "element")
        {
            if let Some(ci) = build_element(c, Some(idx), scope.clone(), nodes, text) {
                nodes[idx].children.push(ci);
            }
        }
    }
    Some(idx)
}

/// Parse an XML document into its full element tree.
///
/// Returns `None` when the text does not parse to a document with a root
/// element.
pub fn parse(text: &str) -> Option<XmlDoc> {
    let mut parser = Parser::new();
    parser.set_language(&LANGUAGE_XML.into()).ok()?;
    let tree = parser.parse(text, None)?;
    let element = tree.root_node().child_by_field_name("root")?;
    let mut nodes = Vec::new();
    build_element(element, None, NsScope::default(), &mut nodes, text)?;
    Some(XmlDoc { nodes })
}

/// Parse an XML document and describe its root element (M0 sniffing).
pub fn parse_root(text: &str) -> Option<XmlRoot> {
    let doc = parse(text)?;
    let root = doc.nodes.first()?;
    let mut parser = Parser::new();
    parser.set_language(&LANGUAGE_XML.into()).ok()?;
    let tree = parser.parse(text, None)?;
    let element = tree.root_node().child_by_field_name("root")?;
    let start = child_of(element, "STag").or_else(|| child_of(element, "EmptyElemTag"))?;
    let (_name, attrs) = read_start_tag(start, text)?;
    let mut ns_prefixes = HashMap::new();
    let mut default_ns = None;
    for (aname, aval) in attrs {
        if aname == "xmlns" {
            default_ns = Some(aval);
        } else if let Some(p) = aname.strip_prefix("xmlns:") {
            ns_prefixes.insert(p.to_owned(), aval);
        }
    }
    Some(XmlRoot {
        name: root.name.clone(),
        default_ns,
        ns_prefixes,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_root;
    use crate::xml::parse;

    #[test]
    fn parses_default_namespace_root() {
        let root = parse_root(
            r#"<?xml version="1.0"?>
<interfaces xmlns="urn:example:interfaces">
  <interface><name>eth0</name></interface>
</interfaces>"#,
        )
        .expect("parses");
        assert_eq!(root.name, "interfaces");
        assert_eq!(root.default_ns.as_deref(), Some("urn:example:interfaces"));
        assert_eq!(root.local_name(), "interfaces");
        assert_eq!(root.effective_ns(), Some("urn:example:interfaces"));
    }

    #[test]
    fn parses_prefixed_root_via_xmlns_binding() {
        let root = parse_root(r#"<if:interfaces xmlns:if="urn:example:interfaces"/>"#)
            .expect("parses self-closing prefixed root");
        assert_eq!(root.name, "if:interfaces");
        assert_eq!(root.local_name(), "interfaces");
        assert_eq!(root.effective_ns(), Some("urn:example:interfaces"));
    }

    #[test]
    fn parses_netconf_rpc_envelope() {
        let root = parse_root(
            r#"<rpc message-id="101" xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <get-config><source><running/></source></get-config>
</rpc>"#,
        )
        .expect("parses");
        assert_eq!(root.name, "rpc");
        assert_eq!(
            root.default_ns.as_deref(),
            Some("urn:ietf:params:xml:ns:netconf:base:1.0")
        );
        assert_eq!(root.ns_prefixes.len(), 0);
    }

    #[test]
    fn self_closing_empty_root() {
        let root = parse_root(r#"<config xmlns="urn:ietf:params:xml:ns:netconf:base:1.0"/>"#)
            .expect("parses empty config");
        assert_eq!(root.name, "config");
        assert_eq!(root.local_name(), "config");
    }

    #[test]
    fn non_xml_returns_none() {
        assert!(parse_root("this is not xml").is_none());
        assert!(parse_root("").is_none());
    }

    #[test]
    fn full_tree_keeps_structure_and_namespaces() {
        let doc = parse(
            r#"<if:interfaces xmlns:if="urn:if">
  <if:interface>
    <if:name>eth0</if:name>
    <other:admin-down xmlns:other="urn:demo">true</other:admin-down>
  </if:interface>
</if:interfaces>"#,
        )
        .expect("parses");
        let n = &doc.nodes;
        assert_eq!(n.len(), 4);
        assert_eq!(n[0].name, "if:interfaces");
        assert_eq!(n[0].ns.as_deref(), Some("urn:if"));
        assert_eq!(n[1].parent, Some(0));
        assert_eq!(n[0].children, vec![1]);
        assert_eq!(n[2].local, "name");
        assert_eq!(n[2].ns.as_deref(), Some("urn:if"));
        // Child in a different namespace via xmlns:other.
        assert_eq!(n[3].name, "other:admin-down");
        assert_eq!(n[3].ns.as_deref(), Some("urn:demo"));
        assert!(n[0].elem_range.start < n[1].elem_range.start);
        assert!(n[0].name_range.start < n[1].name_range.start);
        // element_at picks the deepest containing element.
        let byte = n[2].elem_range.start + 1;
        assert_eq!(doc.element_at(byte), Some(2));
    }

    #[test]
    fn nested_default_ns_rebindings() {
        let doc =
            parse(r#"<a xmlns="urn:a"><b><c xmlns="urn:c"><d/></c><e/></b></a>"#).expect("parses");
        let n = &doc.nodes;
        let ns = |i: usize| n[i].ns.as_deref().unwrap_or("");
        assert_eq!(
            (ns(0), ns(1), ns(2), ns(3), ns(4)),
            ("urn:a", "urn:a", "urn:c", "urn:c", "urn:a")
        );
        assert!(n[2].name_range.end <= n[2].elem_range.end);
    }
}
