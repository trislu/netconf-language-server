//! Workspace-wide identity graph for `identityref` value completion.
//!
//! Built from the parse-level [`SummaryIndex`] (which folds submodule
//! identities into the parent module), so a leaf can list every identity
//! derived from its `base` across the whole workspace without compiling it.

use std::collections::{HashMap, HashSet, VecDeque};

use yrepo::{Library, SummaryIndex};

/// Resolve a schema `identityref` `base` argument in `leaf_module`'s YANG
/// scope: a compiled module name wins, otherwise the qualifier is an import
/// prefix. `None` when the qualifier does not resolve.
pub fn resolve_base(lib: &Library, leaf_module: &str, raw: &str) -> Option<(String, String)> {
    match raw.split_once(':') {
        None => Some((leaf_module.to_owned(), raw.to_owned())),
        Some((p, local)) => {
            if lib.module(p).is_some() {
                Some((p.to_owned(), local.to_owned()))
            } else {
                lib.import_prefixes(leaf_module)
                    .into_iter()
                    .find(|(prefix, _)| prefix == p)
                    .map(|(_, m)| (m, local.to_owned()))
            }
        }
    }
}

/// One identity a leaf's `base` accepts: where it lives and how an XML value
/// would name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Owning module (submodule identities are attributed to their parent).
    pub module: String,
    /// Identity local name.
    pub name: String,
    /// The module's own YANG prefix, reused as the default XML prefix.
    pub prefix: Option<String>,
    /// The module's namespace URI.
    pub namespace: Option<String>,
}

/// Identity declarations and their `base` edges, over the whole workspace.
#[derive(Debug, Default)]
pub struct IdentityGraph {
    /// `(module, name)` → candidate, for every declared identity.
    nodes: HashMap<(String, String), Candidate>,
    /// base `(module, name)` → directly derived `(module, name)`.
    derived: HashMap<(String, String), Vec<(String, String)>>,
}

impl IdentityGraph {
    /// Build the graph from a scanned summary index. Namespace-less entries
    /// (submodule summaries) are skipped: the fold already moved their
    /// identities into the parent module's summary.
    pub fn build(index: &SummaryIndex) -> IdentityGraph {
        let mut graph = IdentityGraph::default();
        for s in index.summaries() {
            let Some(ns) = s.namespace.as_deref().filter(|n| !n.is_empty()) else {
                continue;
            };
            for id in &s.identities {
                let key = (s.name.clone(), id.name.clone());
                graph.nodes.entry(key.clone()).or_insert_with(|| Candidate {
                    module: s.name.clone(),
                    name: id.name.clone(),
                    prefix: s.prefix.clone(),
                    namespace: Some(ns.to_owned()),
                });
                if let Some(base) = &id.base {
                    graph.derived.entry(base.clone()).or_default().push(key);
                }
            }
        }
        graph
    }

    /// The base identity itself (when it is a declared workspace identity) plus
    /// every identity transitively derived from it, sorted by `(module, name)`.
    pub fn derived_from(&self, base_module: &str, base_name: &str) -> Vec<Candidate> {
        let start = (base_module.to_owned(), base_name.to_owned());
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut queue: VecDeque<(String, String)> = VecDeque::from([start]);
        let mut out = Vec::new();
        while let Some(key) = queue.pop_front() {
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(c) = self.nodes.get(&key) {
                out.push(c.clone());
            }
            if let Some(children) = self.derived.get(&key) {
                queue.extend(children.iter().cloned());
            }
        }
        out.sort_by(|a, b| (&a.module, &a.name).cmp(&(&b.module, &b.name)));
        out
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn scan(files: &[(&str, &str)]) -> SummaryIndex {
        let dir = std::env::temp_dir().join(format!(
            "ncls-idgraph-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths: Vec<PathBuf> = Vec::new();
        for (name, src) in files {
            let p = dir.join(name);
            std::fs::write(&p, src).unwrap();
            paths.push(p);
        }
        let mut index = SummaryIndex::default();
        index.scan_many_files_with(&paths, |p| Some(format!("file://{}", p.display())));
        std::fs::remove_dir_all(&dir).ok();
        index
    }

    #[test]
    fn derived_from_walks_the_base_chain_across_modules() {
        let index = scan(&[
            (
                "ida.yang",
                "module ida { yang-version 1.1; namespace \"urn:ida\"; prefix a; revision 2026-01-01; identity root; }",
            ),
            (
                "idb.yang",
                "module idb { yang-version 1.1; namespace \"urn:idb\"; prefix b; revision 2026-01-01; import ida { prefix a; } identity mid { base a:root; } identity leaf { base mid; } }",
            ),
        ]);
        let g = IdentityGraph::build(&index);
        // `root` and its transitively derived identities, `root` first by sort.
        let got: Vec<(String, String)> = g
            .derived_from("ida", "root")
            .into_iter()
            .map(|c| (c.module, c.name))
            .collect();
        assert_eq!(
            got,
            vec![
                ("ida".to_owned(), "root".to_owned()),
                ("idb".to_owned(), "leaf".to_owned()),
                ("idb".to_owned(), "mid".to_owned()),
            ]
        );
        // A base with no derived identities still offers itself.
        assert_eq!(g.derived_from("ida", "absent").len(), 0);
        // Candidates carry the module prefix/namespace.
        let mid = g
            .derived_from("ida", "root")
            .into_iter()
            .find(|c| c.name == "mid")
            .unwrap();
        assert_eq!(mid.prefix.as_deref(), Some("b"));
        assert_eq!(mid.namespace.as_deref(), Some("urn:idb"));
    }

    #[test]
    fn submodule_identities_are_attributed_to_the_parent() {
        let index = scan(&[
            (
                "idc.yang",
                "module idc { yang-version 1.1; namespace \"urn:idc\"; prefix c; revision 2026-01-01; identity croot; }",
            ),
            (
                "idc-sub.yang",
                "submodule idc-sub { yang-version 1.1; belongs-to idc { prefix c; } identity subid { base croot; } }",
            ),
        ]);
        let g = IdentityGraph::build(&index);
        let got: Vec<(String, String)> = g
            .derived_from("idc", "croot")
            .into_iter()
            .map(|c| (c.module, c.name))
            .collect();
        assert_eq!(
            got,
            vec![
                ("idc".to_owned(), "croot".to_owned()),
                ("idc".to_owned(), "subid".to_owned()),
            ]
        );
    }
}
