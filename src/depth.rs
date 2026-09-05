//! Shared "diagnostics depth" analysis (M4).
//!
//! Given an **instantiated** schema data node (module + arena id), compare
//! which of its *direct* data children are present in the instance document
//! and report, best effort (D27):
//!
//! - a missing required child: a `mandatory true` data node, or one of a
//!   list entry's `key` leaves;
//! - a `choice` with no instantiated case (and no `default` case, which would
//!   make absence legal — RFC 7950 §7.9.3).
//!
//! Only **direct** children are required of the present node: a mandatory leaf
//! inside a `case` is a property of that case (satisfied by choosing the
//! case), not a bare requirement of the parent — so `choice`s are analysed as
//! their own groups and mandatory children under them are not double-counted.
//!
//! Both the XML mapper (`inst_map`) and the JSON mapper (`jmap`) consume this
//! so the two formats share one required-set definition.

use yrepo::{Library, NodeKind};

/// A `choice` with no instantiated case.
#[derive(Debug, Clone)]
pub struct ChoiceGap {
    /// The choice's schema name.
    pub name: String,
    /// Local names of the case content that would satisfy it.
    pub options: Vec<String>,
}

/// The depth findings for one present data node.
#[derive(Debug, Clone, Default)]
pub struct DepthReport {
    /// Names of required direct data children (mandatory, or a list entry's
    /// keys) that are absent.
    pub missing: Vec<String>,
    /// `choice`s with no instantiated case and no default.
    pub choices: Vec<ChoiceGap>,
}

/// Analyse the required direct children of schema node `(module, id)`.
///
/// `present(schema_id)` must answer whether that direct data child (or one of
/// a choice's selectable children) is instantiated in the document.
pub fn analyze(
    lib: &Library,
    module: &str,
    id: usize,
    present: &dyn Fn(usize) -> bool,
) -> DepthReport {
    let mut report = DepthReport::default();
    let Some(rec) = lib.module(module) else {
        return report;
    };
    let Some(node) = rec.node(id) else {
        return report;
    };
    if !matches!(node.kind(), NodeKind::Container | NodeKind::List) {
        return report;
    }
    let is_list = node.kind() == NodeKind::List;
    for &c in node.children() {
        let Some(child) = rec.node(c) else {
            continue;
        };
        match child.kind() {
            NodeKind::Container
            | NodeKind::Leaf
            | NodeKind::LeafList
            | NodeKind::List
            | NodeKind::Anyxml
            | NodeKind::Anydata => {
                // A list entry's `key` leaves and `mandatory true` data nodes
                // must be present when the node is.
                let required = child.is_mandatory() || (is_list && child.is_key());
                if required && !present(c) {
                    report.missing.push(child.name().to_owned());
                }
            }
            NodeKind::Choice => {
                // A `default` case makes an absent choice legal.
                if child.default().is_some() {
                    continue;
                }
                let selectable = rec.data_children(c);
                if !selectable.iter().any(|&s| present(s)) {
                    report.choices.push(ChoiceGap {
                        name: child.name().to_owned(),
                        options: selectable
                            .iter()
                            .filter_map(|&s| rec.node(s).map(|n| n.name().to_owned()))
                            .collect(),
                    });
                }
            }
            _ => {}
        }
    }
    report
}
