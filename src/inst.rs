//! Instance-document **intent classification** (M0, decision D19).
//!
//! An `.xml`/`.json` document is classified against the compiled YANG library
//! into one of the document intents (§12 M0; D18–D20): a NETCONF message, a
//! config payload, a data tree of a module, or **not** a NETCONF document
//! (dormant — the LS provides no features for it).

use crate::json::JsonRoot;
use crate::xml::XmlRoot;

/// NETCONF base namespace (RFC 6241).
pub const NETCONF_BASE_NS: &str = "urn:ietf:params:xml:ns:netconf:base:1.0";
/// NETCONF notification namespace (RFC 5277).
pub const NETCONF_NOTIFICATION_NS: &str = "urn:ietf:params:xml:ns:netconf:notification:1.0";

/// Schema summary the classifier needs per compiled module.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// Module name.
    pub name: String,
    /// Module `namespace` URI.
    pub namespace: String,
    /// Names of the module's **top-level data nodes** (not rpc/notification).
    pub top_data: Vec<String>,
}

/// How an instance document is interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocKind {
    /// NETCONF message envelope (`hello`/`rpc`/`rpc-reply`/`notification`).
    NetconfMessage,
    /// NETCONF payload wrapper root (`<config>`/`<data>`/`<filter>`).
    ConfigPayload,
    /// Data tree rooted in this module (matches one of its top data nodes).
    DataTree(String),
    /// Not a NETCONF/YANG instance document — the LS stays dormant.
    NotNetconf,
}

/// Classify an XML document given the workspace's compiled modules.
///
/// Pure over [`XmlRoot`] so it is unit-testable without a live `Library`.
pub fn classify_xml(root: &XmlRoot, modules: &[ModuleInfo]) -> DocKind {
    match root.effective_ns() {
        Some(NETCONF_BASE_NS) => match root.local_name() {
            "hello" | "rpc" | "rpc-reply" => DocKind::NetconfMessage,
            // `config` normally nests under edit-config, but a payload file may
            // use it (or `data`/`filter`) as its root.
            "config" | "data" | "filter" => DocKind::ConfigPayload,
            _ => DocKind::NotNetconf,
        },
        Some(NETCONF_NOTIFICATION_NS) => DocKind::NetconfMessage,
        ns => {
            // Data tree: the root's namespace must belong to a compiled module
            // and its local name to that module's top-level data nodes.
            let local = root.local_name();
            for m in modules {
                if Some(m.namespace.as_str()) == ns && m.top_data.iter().any(|t| t == local) {
                    return DocKind::DataTree(m.name.clone());
                }
            }
            DocKind::NotNetconf
        }
    }
}

/// Classify a JSON (RFC 7951) document given the workspace's compiled modules.
///
/// A data-tree intent is recognized when any top-level member key is
/// `module:name` where `module` is compiled and `name` is one of its top-level
/// data nodes.
pub fn classify_json(root: &JsonRoot, modules: &[ModuleInfo]) -> DocKind {
    for key in &root.top_keys {
        let Some((module, local)) = key.split_once(':') else {
            continue;
        };
        let Some(m) = modules.iter().find(|m| m.name == module) else {
            continue;
        };
        if m.top_data.iter().any(|t| t == local) {
            return DocKind::DataTree(module.to_owned());
        }
    }
    DocKind::NotNetconf
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn xml(name: &str, default_ns: Option<&str>) -> XmlRoot {
        XmlRoot {
            name: name.to_owned(),
            default_ns: default_ns.map(str::to_owned),
            ns_prefixes: HashMap::new(),
        }
    }

    fn modules() -> Vec<ModuleInfo> {
        vec![
            ModuleInfo {
                name: "ietf-interfaces".to_owned(),
                namespace: "urn:ietf:params:xml:ns:yang:ietf-interfaces".to_owned(),
                top_data: vec!["interfaces".to_owned(), "interfaces-state".to_owned()],
            },
            ModuleInfo {
                name: "example-demo".to_owned(),
                namespace: "urn:example:demo".to_owned(),
                top_data: vec!["system".to_owned()],
            },
        ]
    }

    #[test]
    fn xml_netconf_message_and_payload() {
        let rpc = xml("rpc", Some(NETCONF_BASE_NS));
        assert_eq!(classify_xml(&rpc, &modules()), DocKind::NetconfMessage);
        let hello = xml("hello", Some(NETCONF_BASE_NS));
        assert_eq!(classify_xml(&hello, &modules()), DocKind::NetconfMessage);
        let config = xml("config", Some(NETCONF_BASE_NS));
        assert_eq!(classify_xml(&config, &modules()), DocKind::ConfigPayload);
        let notify = xml("notification", Some(NETCONF_NOTIFICATION_NS));
        assert_eq!(classify_xml(&notify, &modules()), DocKind::NetconfMessage);
    }

    #[test]
    fn xml_data_tree_matches_module_namespace_and_top_node() {
        let data = xml(
            "interfaces",
            Some("urn:ietf:params:xml:ns:yang:ietf-interfaces"),
        );
        assert_eq!(
            classify_xml(&data, &modules()),
            DocKind::DataTree("ietf-interfaces".to_owned())
        );
    }

    #[test]
    fn xml_unknown_namespace_or_wrong_local_is_dormant() {
        // Namespace not compiled → dormant.
        let foreign = xml("root", Some("urn:some:other:thing"));
        assert_eq!(classify_xml(&foreign, &modules()), DocKind::NotNetconf);
        // Compiled namespace but not a top-level data node → dormant.
        let deep = xml(
            "system",
            Some("urn:ietf:params:xml:ns:yang:ietf-interfaces"),
        );
        assert_eq!(classify_xml(&deep, &modules()), DocKind::NotNetconf);
        // No default namespace at all → dormant.
        assert_eq!(
            classify_xml(&xml("root", None), &modules()),
            DocKind::NotNetconf
        );
    }

    #[test]
    fn json_data_tree_matches_module_qualified_keys() {
        let root = JsonRoot {
            top_keys: vec![
                "ietf-interfaces:interfaces".to_owned(),
                "example-demo:system".to_owned(),
            ],
        };
        assert_eq!(
            classify_json(&root, &modules()),
            DocKind::DataTree("ietf-interfaces".to_owned())
        );
    }

    #[test]
    fn json_unknown_module_or_missing_top_is_dormant() {
        let root = JsonRoot {
            top_keys: vec!["other-module:whatever".to_owned()],
        };
        assert_eq!(classify_json(&root, &modules()), DocKind::NotNetconf);
        let root = JsonRoot {
            top_keys: vec!["ietf-interfaces:not-a-top".to_owned()],
        };
        assert_eq!(classify_json(&root, &modules()), DocKind::NotNetconf);
        let root = JsonRoot { top_keys: vec![] };
        assert_eq!(classify_json(&root, &modules()), DocKind::NotNetconf);
    }
}
