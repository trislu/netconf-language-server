//! Go-to-definition (`textDocument/definition`) — returns `LocationLink`s.
//!
//! Two stages (see `docs/architecture.md` §8.4): caret context from the
//! statement tree, then resolution through the compiled `Library`.

use std::collections::HashMap;
use std::ops::Range;

use ropey::Rope;
use tower_lsp_server::ls_types::{LocationLink, Uri};
use yrepo::{Library, Statement, StatementKind};

use crate::convert;

/// The RFC 7950 built-in type names (used to skip goto on `type` args).
const BUILTIN_TYPES: &[&str] = &[
    "binary", "bits", "boolean", "decimal64", "empty", "enumeration",
    "identityref", "instance-identifier", "int8", "int16", "int32", "int64",
    "leafref", "string", "uint8", "uint16", "uint32", "uint64", "union",
];

/// A resolved jump target (byte ranges in `url`).
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub(crate) url: String,
    pub(crate) target_range: Range<usize>,
    pub(crate) origin_range: Range<usize>,
}

fn split_ref(name: &str) -> (Option<&str>, &str) {
    match name.split_once(':') {
        Some((p, local)) => (Some(p), local),
        None => (None, name),
    }
}

/// Resolve `[prefix:]name` against `scope`: module name where it is defined.
fn module_for<'l>(scope: &'l str, prefix: Option<&str>, lib: &'l Library) -> Option<&'l str> {
    match prefix {
        Some(p) => lib.prefix_to_module(scope, p),
        None => Some(scope),
    }
}

fn file_top(url: &str) -> Target {
    Target {
        url: url.to_owned(),
        target_range: 0..0,
        origin_range: 0..0,
    }
}

/// Build the jump target list for the statement under `byte`.
pub(crate) fn resolve(
    root: &Statement,
    byte: usize,
    scope: &str,
    lib: &Library,
) -> Option<Vec<Target>> {
    use StatementKind as K;
    let stmt = root.narrowest_at(byte)?;
    let arg = stmt.arg.as_ref()?;
    if !arg.range.contains(&byte) {
        return None;
    }
    let name = arg.name();

    let target = match &stmt.kind {
        K::Import => {
            let m = lib.module(name)?;
            let url = m.source_urls().first()?.to_string();
            file_top(&url)
        }
        K::Include => {
            let s = lib.submodule(name)?;
            file_top(s.url().as_ref())
        }
        K::BelongsTo => {
            let m = lib.module(name)?;
            let url = m.source_urls().first()?.to_string();
            file_top(&url)
        }
        K::Uses => {
            let (prefix, local) = split_ref(name);
            let module = module_for(scope, prefix, lib)?;
            let g = lib.search_grouping(module, local)?;
            Target {
                url: g.defining.url.to_string(),
                target_range: g.defining.range.clone(),
                origin_range: arg.range.clone(),
            }
        }
        K::Type => {
            let (prefix, local) = split_ref(name);
            if BUILTIN_TYPES.contains(&local) {
                return None;
            }
            let module = module_for(scope, prefix, lib)?;
            let t = lib.search_type(module, local)?;
            Target {
                url: t.defining.url.to_string(),
                target_range: t.defining.range.clone(),
                origin_range: arg.range.clone(),
            }
        }
        K::Base => {
            let (prefix, local) = split_ref(name);
            let module = module_for(scope, prefix, lib)?;
            let id = lib.search_identity(module, local)?;
            Target {
                url: id.defining.url.to_string(),
                target_range: id.defining.range.clone(),
                origin_range: arg.range.clone(),
            }
        }
        K::Augment => {
            let path = arg.path();
            if !path.starts_with('/') {
                return None;
            }
            let node = lib.resolve_abs_schema_node_id(scope, &path)?;
            let loc = node.defining();
            Target {
                url: loc.url.to_string(),
                target_range: loc.range.clone(),
                origin_range: arg.range.clone(),
            }
        }
        // A non-navigable statement (or a `uses-augment` descendant path).
        _ => return None,
    };
    Some(vec![target])
}

/// Convert resolved targets into LSP `LocationLink`s, mapping byte ranges with
/// each target file's own text (from `textmap`; falls back to the source rope).
pub(crate) fn to_links(
    source_rope: &Rope,
    targets: &[Target],
    textmap: &HashMap<String, Rope>,
) -> Vec<LocationLink> {
    targets
        .iter()
        .filter_map(|t| {
            let target_rope = textmap.get(&t.url).unwrap_or(source_rope);
            let target_uri = t.url.parse::<Uri>().ok()?;
            Some(LocationLink {
                target_uri,
                target_range: convert::range_to_lsp(target_rope, t.target_range.clone()),
                target_selection_range: convert::range_to_lsp(target_rope, t.target_range.clone()),
                origin_selection_range: Some(convert::range_to_lsp(
                    source_rope,
                    t.origin_range.clone(),
                )),
            })
        })
        .collect()
}
