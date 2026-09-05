//! Schema-side glue for instance documents (M0, D29/D30).
//!
//! Builds the per-module summaries the classifier needs from a compiled
//! `yrepo::Library`, and answers namespace → module lookups. The full
//! data↔schema resolver (data_children / data_child / schema_nodeid over a
//! snapshot) is consumed directly from `yrepo` in M1.

use yrepo::Library;

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
