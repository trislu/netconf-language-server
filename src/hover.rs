//! Hover (`textDocument/hover`).
//!
//! Provides markdown for reference-bearing arguments by resolving through the
//! compiled `Library`: import modules, prefixed-reference bindings, typedef
//! chains (`resolve_type`) and identity ancestry (`resolve_identity`).

use ropey::Rope;
use yrepo::{Library, Statement, StatementKind};

/// RFC 7950 built-in type names (skip hover chains for these).
const BUILTIN_TYPES: &[&str] = &[
    "binary", "bits", "boolean", "decimal64", "empty", "enumeration",
    "identityref", "instance-identifier", "int8", "int16", "int32", "int64",
    "leafref", "string", "uint8", "uint16", "uint32", "uint64", "union",
];

fn code_fence(text: &str) -> String {
    format!("```yang\n{text}\n```")
}

/// Raw source of an `import` statement bound to `prefix`, if any.
fn binding_snippet(
    rope: &Rope,
    root: &Statement,
    prefix: &str,
    scope: &str,
    lib: &Library,
) -> Option<String> {
    use StatementKind as K;
    for imp in root.find(&[K::Import]) {
        let p = imp.find_one(K::Prefix).and_then(|s| s.arg.as_ref())?;
        if p.name() == prefix {
            let text = rope.get_byte_slice(imp.span())?.to_string();
            return Some(code_fence(&text));
        }
    }
    // The module's own prefix.
    if lib.module(scope).and_then(|m| m.prefix()) == Some(prefix) {
        return Some("this module's own prefix".to_owned());
    }
    None
}

/// A prefixed reference: show what module the prefix binds to.
fn prefixed_ref(rope: &Rope, root: &Statement, scope: &str, lib: &Library, prefix: &str) -> Option<String> {
    let mut out = format!("prefix **`{prefix}`**");
    if let Some(module) = lib.prefix_to_module(scope, prefix) {
        out.push_str(&format!(" → module **`{module}`**"));
    }
    if let Some(snippet) = binding_snippet(rope, root, prefix, scope, lib) {
        out.push('\n');
        out.push_str(&snippet);
    }
    Some(out)
}

pub(crate) fn handle(
    rope: &Rope,
    root: &Statement,
    byte: usize,
    scope: &str,
    lib: &Library,
) -> Option<String> {
    use StatementKind as K;
    let stmt = root.narrowest_at(byte)?;
    let arg = stmt.arg.as_ref()?;
    if !arg.range.contains(&byte) {
        return None;
    }
    let name = arg.name();

    let split = name.split_once(':');

    match &stmt.kind {
        K::Import => {
            let m = lib.module(name)?;
            let mut out = format!("**module** `{name}`");
            if let Some(ns) = m.namespace() {
                out.push_str(&format!("\n- namespace: `{ns}`"));
            }
            if let Some(p) = m.prefix() {
                out.push_str(&format!("\n- prefix: `{p}`"));
            }
            Some(out)
        }
        K::Type if split.is_some() => {
            let prefix = split?.0;
            prefixed_ref(rope, root, scope, lib, prefix)
        }
        K::Type => {
            let local = name;
            if BUILTIN_TYPES.contains(&local) {
                return Some(format!("built-in type **`{local}`**"));
            }
            let r = lib.resolve_type(scope, local)?;
            let mut out = format!("**type** `{local}`");
            for step in &r.typedefs {
                out.push_str(&format!(
                    "\n- typedef `{}` (in module `{}`)",
                    step.name, step.module
                ));
            }
            if let Some(b) = &r.builtin {
                out.push_str(&format!("\n→ built-in **`{b}`**"));
            }
            if !r.complete {
                out.push_str("\n*typedef chain is incomplete*");
            }
            Some(out)
        }
        K::Base if split.is_some() => {
            let prefix = split?.0;
            prefixed_ref(rope, root, scope, lib, prefix)
        }
        K::Base => {
            let r = lib.resolve_identity(scope, name)?;
            let mut out = format!("**identity** `{}` (module `{}`)", r.root.name, r.root.module);
            for b in &r.bases {
                out.push_str(&format!("\n- base: `{}` (module `{}`)", b.name, b.module));
            }
            Some(out)
        }
        K::Uses => {
            if let Some((prefix, local)) = split {
                if let Some(snippet) = prefixed_ref(rope, root, scope, lib, prefix) {
                    let out = format!("grouping **`{local}`** via prefix\n{snippet}");
                    return Some(out);
                }
                return None;
            }
            let g = lib.search_grouping(scope, name)?;
            Some(format!(
                "grouping **`{}`** (defined in module `{}`)",
                g.name, g.defining.url
            ))
        }
        K::Augment => {
            let path = arg.path();
            if !path.starts_with('/') {
                return None;
            }
            let node = lib.resolve_abs_schema_node_id(scope, &path)?;
            Some(format!("target node **`{}`** (kind: {:?})", node.name(), node.kind()))
        }
        _ => None,
    }
}
