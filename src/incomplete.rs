//! Tolerant parsing for *in-progress* instance documents.
//!
//! The instance-document completions must work on the text a user actually has
//! while typing: an XML document that ends in a lone `<` (or an unclosed
//! element) and a JSON document that ends in a lone `"` in a fresh member slot
//! are both invalid as complete documents, yet the editor asks for completion
//! at exactly that moment. These helpers try the strict parser first and then
//! a couple of bounded, deterministic repairs (drop the trailing incomplete
//! token; append the missing closing tags/braces) before giving up.

use crate::json::JsonDoc;
use crate::xml::XmlDoc;

/// Parse `text` as XML, tolerating the trailing incomplete constructs above.
pub fn tolerant_xml(text: &str) -> Option<XmlDoc> {
    if let Some(doc) = crate::xml::parse(text) {
        return Some(doc);
    }
    for candidate in xml_candidates(text) {
        if let Some(doc) = crate::xml::parse(&candidate) {
            return Some(doc);
        }
    }
    None
}

/// Parse `text` as JSON, tolerating a lone trailing `"` in a fresh member slot
/// and unbalanced braces/brackets.
pub fn tolerant_json(text: &str) -> Option<JsonDoc> {
    if let Some(doc) = crate::json::parse(text) {
        return Some(doc);
    }
    for candidate in json_candidates(text) {
        if let Some(doc) = crate::json::parse(&candidate) {
            return Some(doc);
        }
    }
    None
}

/// Bounded repair candidates for an in-progress XML document.
fn xml_candidates(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    // A trailing `<` / `<nam` with no `>` after it is the "just typed `<`" case.
    let stripped = match text.rfind('<') {
        Some(i) if !text[i..].contains('>') => &text[..i],
        _ => text,
    };
    if let Some(balanced) = balance_xml(stripped)
        && balanced != stripped
    {
        out.push(balanced);
    }
    if stripped != text {
        out.push(stripped.to_string());
    }
    out
}

/// Append the closing tags for every element left open in `text` (skipping
/// comments, CDATA, processing instructions and self-closing tags). `None`
/// when the text has an unterminated comment/CDATA/declaration.
fn balance_xml(text: &str) -> Option<String> {
    let mut stack: Vec<&str> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        let mut consumed = None;
        for (open, close) in [
            ("<!--", "-->"),
            ("<![CDATA[", "]]>"),
            ("<?", "?>"),
            ("<!", ">"),
        ] {
            if let Some(body) = rest.strip_prefix(open) {
                consumed = Some(open.len() + body.find(close)? + close.len());
                break;
            }
        }
        if let Some(n) = consumed {
            i += n;
            continue;
        }
        let end = rest.find('>')?;
        let inner = &rest[1..end];
        if inner.starts_with('/') {
            stack.pop();
        } else if !inner.ends_with('/') {
            let name = inner.split_whitespace().next().unwrap_or("");
            if !name.is_empty() {
                stack.push(name);
            }
        }
        i += end + 1;
    }
    if stack.is_empty() {
        return Some(text.to_string());
    }
    let mut out = String::with_capacity(text.len() + stack.len() * 8);
    out.push_str(text);
    for name in stack.iter().rev() {
        out.push_str("</");
        out.push_str(name);
        out.push('>');
    }
    Some(out)
}

/// Bounded repair candidates for an in-progress JSON document.
fn json_candidates(text: &str) -> Vec<String> {
    let mut stripped = text.trim_end().to_string();
    // `{"` (or `{"a": 1, "`) — a lone quote in a fresh member slot.
    if stripped.ends_with('"') {
        let without = stripped[..stripped.len() - 1].trim_end();
        if matches!(without.chars().last(), Some('{') | Some(',')) {
            stripped = without.to_string();
        }
    }
    if stripped.ends_with(',') {
        stripped.truncate(stripped.trim_end_matches(',').len());
        stripped = stripped.trim_end().to_string();
    }
    let mut out = Vec::new();
    if let Some(balanced) = balance_json(&stripped)
        && balanced != stripped
    {
        out.push(balanced);
    }
    out.push(stripped);
    out
}

/// Append the missing `}`/`]` for unbalanced containers. `None` when a string
/// is left unterminated (the repair would be ambiguous).
fn balance_json(text: &str) -> Option<String> {
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    if in_string {
        return None;
    }
    if stack.is_empty() {
        return Some(text.to_string());
    }
    let mut out = String::with_capacity(text.len() + stack.len());
    out.push_str(text);
    while let Some(c) = stack.pop() {
        out.push(c);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = "urn:ietf:params:xml:ns:netconf:base:1.0";

    #[test]
    fn xml_tolerates_a_lone_open_angle() {
        let doc = tolerant_xml(&format!(r#"<rpc xmlns="{NS}"><"#)).expect("repairable");
        assert!(!doc.nodes.is_empty());
        assert!(tolerant_xml(&format!(r#"<rpc xmlns="{NS}">"#)).is_some());
        assert!(tolerant_xml("<").is_none());
    }

    #[test]
    fn json_tolerates_a_lone_quote_in_a_member_slot() {
        assert!(tolerant_json(r#"{"#).is_some());
        assert!(tolerant_json("{\n  \"").is_some());
        assert!(tolerant_json(r#"{"a": 1, ""#).is_some());
        assert!(tolerant_json("{}").is_some());
    }
}
