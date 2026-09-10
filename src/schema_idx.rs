//! Schema-side glue for instance documents (M0, D29/D30).
//!
//! Builds the per-module summaries the classifier needs from a compiled
//! `yrepo::Library`, and answers namespace → module lookups. The full
//! data↔schema resolver (data_children / data_child / schema_nodeid over a
//! snapshot) is consumed directly from `yrepo` in M1.

use yrepo::{Library, SummaryIndex};

use crate::inst::ModuleInfo;

/// Summaries of every compiled module for instance-document classification.
pub fn module_summaries(lib: &Library) -> Vec<ModuleInfo> {
    lib.modules()
        .iter()
        .filter_map(|m| {
            let namespace = m.namespace()?.to_owned();
            let top_data = m
                .top_nodes()
                .iter()
                .filter_map(|&id| {
                    let n = m.node(id)?;
                    if n.kind().is_data() {
                        Some(n.name().to_owned())
                    } else {
                        None
                    }
                })
                .collect();
            Some(ModuleInfo {
                name: m.name().to_owned(),
                namespace,
                top_data,
            })
        })
        .collect()
}

/// Summaries of every indexed module from the Tier-1 [`SummaryIndex`] (the
/// parse-free-of-compile fallback used when no compiled library exists yet).
///
/// Same [`ModuleInfo`] shape as [`module_summaries`]: submodules (no
/// namespace) are skipped, and `top_data` are exactly the summary's top-level
/// data-node names (rpcs/notifications are carried separately by the index).
pub fn module_summaries_from_summary(index: &SummaryIndex) -> Vec<ModuleInfo> {
    index
        .summaries()
        .filter_map(|s| {
            let namespace = s.namespace.clone().filter(|n| !n.is_empty())?;
            Some(ModuleInfo {
                name: s.name.clone(),
                namespace,
                top_data: s.top_data.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_projection_keeps_namespace_and_top_data_only() {
        let dir = std::env::temp_dir().join(format!(
            "ncls-schema-idx-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let module = dir.join("demo.yang");
        std::fs::write(
            &module,
            r#"module demo {
  yang-version 1.1;
  namespace "urn:demo";
  prefix d;
  revision 2026-01-01;
  container system { leaf hostname { type string; } }
  leaf flag { type boolean; }
  rpc reset;
  notification alarm { leaf what { type string; } }
}"#,
        )
        .unwrap();
        // A submodule has no namespace and must not produce a ModuleInfo.
        let sub = dir.join("demo-sub.yang");
        std::fs::write(
            &sub,
            r#"submodule demo-sub {
  belongs-to demo { prefix d; }
  container extra { leaf x { type string; } }
}"#,
        )
        .unwrap();
        let mut index = SummaryIndex::default();
        index.scan_many_files_with([&module, &sub], |p| Some(format!("file://{}", p.display())));

        let infos = module_summaries_from_summary(&index);
        let demo = infos.iter().find(|m| m.name == "demo").expect("demo");
        assert_eq!(demo.namespace, "urn:demo");
        assert_eq!(
            demo.top_data,
            vec!["system".to_owned(), "flag".to_owned()],
            "only top-level data nodes (no rpc/notification)"
        );
        assert!(
            !infos.iter().any(|m| m.name == "demo-sub"),
            "a submodule has no namespace and is skipped: {infos:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
