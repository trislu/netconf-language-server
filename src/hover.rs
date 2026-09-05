//! Hover (`textDocument/hover`).
//!
//! Provides markdown for reference-bearing arguments by resolving through the
//! compiled `Library`: import modules, prefixed-reference bindings, typedef
//! chains (`resolve_type`) and identity ancestry (`resolve_identity`).

use ropey::Rope;
use yrepo::{Library, Statement, StatementKind};

/// RFC 7950 built-in type names (skip hover chains for these).
const BUILTIN_TYPES: &[&str] = &[
    "binary",
    "bits",
    "boolean",
    "decimal64",
    "empty",
    "enumeration",
    "identityref",
    "instance-identifier",
    "int8",
    "int16",
    "int32",
    "int64",
    "leafref",
    "string",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "union",
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
fn prefixed_ref(
    rope: &Rope,
    root: &Statement,
    scope: &str,
    lib: &Library,
    prefix: &str,
) -> Option<String> {
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

/// The deepest `leaf`/`leaf-list`/`typedef` ancestor containing `byte` — for a
/// `default` that names an enum member.
fn data_node_owner(root: &Statement, byte: usize) -> Option<&Statement> {
    use StatementKind as K;
    let mut best = None;
    for s in root.preorder() {
        if s.range.contains(&byte) && matches!(s.kind, K::Leaf | K::LeafList | K::Typedef) {
            best = Some(s);
        }
    }
    best
}

/// The name of the `enum <name>` member of `owner`'s `type enumeration`, if any.
fn enum_owner_name(owner: &Statement, name: &str) -> Option<String> {
    use StatementKind as K;
    let ty = owner
        .children
        .iter()
        .find(|c| c.kind == K::Type && c.arg.as_ref().is_some_and(|a| a.name() == "enumeration"))?;
    let en = ty
        .children
        .iter()
        .find(|c| c.kind == K::Enum && c.arg.as_ref().is_some_and(|a| a.name() == name))?;
    Some(en.arg.as_ref()?.name().to_string())
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

    // Extension usage head → describe the referenced extension definition.
    if let K::Unknown(_) = &stmt.kind {
        let kw = stmt.keyword.as_ref()?;
        if !kw.contains(&byte) {
            return None;
        }
        let text = rope.get_byte_slice(kw.clone())?.to_string();
        let (prefix, local) = match text.split_once(':') {
            Some((p, l)) => (Some(p), l),
            None => (None, text.as_str()),
        };
        let module = match prefix {
            Some(p) => lib.prefix_to_module(scope, p)?,
            None => scope,
        };
        let ext = lib.search_extension(module, local)?;
        let mut out = format!("extension **`{local}`** (module `{module}`)");
        if let Some(a) = &ext.argument {
            out.push_str(&format!("\n- argument: `{a}`"));
        }
        return Some(out);
    }

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
            let mut out = format!(
                "**identity** `{}` (module `{}`)",
                r.root.name, r.root.module
            );
            for b in &r.bases {
                out.push_str(&format!("\n- base: `{}` (module `{}`)", b.name, b.module));
            }
            Some(out)
        }
        K::IfFeature => {
            let (prefix, local) = match split {
                Some((p, l)) => (Some(p), l),
                None => (None, name),
            };
            let module = match prefix {
                Some(p) => lib.prefix_to_module(scope, p)?,
                None => scope,
            };
            lib.search_feature(module, local)?;
            Some(format!("feature **`{local}`** (module `{module}`)"))
        }
        K::Default => {
            let (prefix, local) = match split {
                Some((p, l)) => (Some(p), l),
                None => (None, name),
            };
            let module = match prefix {
                Some(p) => lib.prefix_to_module(scope, p)?,
                None => scope,
            };
            if lib.search_identity(module, local).is_some() {
                return Some(format!("identity value **`{local}`** (module `{module}`)"));
            }
            if prefix.is_some() {
                return None;
            }
            let owner = data_node_owner(root, byte)?;
            let member = enum_owner_name(owner, name)?;
            Some(format!("enum value **`{member}`**"))
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
            Some(format!(
                "target node **`{}`** (kind: {:?})",
                node.name(),
                node.kind()
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrepo::Repository;

    const DEF: &str = "module extdef {\n  namespace \"urn:extdef\";\n  prefix vendor;\n\
      extension info { argument name; }\n\
    }\n";
    const USE: &str = "module app {\n  namespace \"urn:app\";\n  prefix a;\n\
      import extdef { prefix vendor; }\n\
      leaf x { type string; vendor:info \"x\"; }\n\
    }\n";

    fn usage_repo() -> (Rope, yrepo::Statement, std::sync::Arc<yrepo::Library>) {
        let mut repo = Repository::new();
        repo.upsert("/extdef.yang", DEF.to_string());
        repo.upsert("/app.yang", USE.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(USE);
        let root = repo.statement("/app.yang").expect("root").clone();
        (rope, root, lib)
    }

    /// Byte offset of the `info` name in the `vendor:info` usage head.
    fn head_byte() -> usize {
        USE.find("vendor:info").unwrap() + "vendor:".len()
    }

    #[test]
    fn hover_resolves_extension_usage() {
        let (rope, root, lib) = usage_repo();
        let text = handle(&rope, &root, head_byte(), "app", &lib).expect("hover");
        assert!(text.contains("extension **`info`**"), "hover text: {text}");
        assert!(text.contains("argument: `name`"), "hover text: {text}");
        // caret not on the head (e.g. on the quoted arg) → no extension hover
        let arg_byte = USE.find("info").unwrap() + "info \"x\"".len();
        assert!(handle(&rope, &root, arg_byte, "app", &lib).is_none());
    }

    const FEAT: &str = "module fmod {\n  namespace \"urn:f\";\n  prefix f;\n\
      feature turbo;\n\
      leaf x { if-feature turbo; type string; }\n\
      leaf mode { type enumeration { enum auto { value 1; } } default auto; }\n\
      identity auto-id;\n\
      leaf idl { type identityref { base auto-id; } default auto-id; }\n\
    }\n";

    #[test]
    fn hover_if_feature_and_default_references() {
        let mut repo = Repository::new();
        repo.upsert("/fmod.yang", FEAT.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(FEAT);
        let root = repo.statement("/fmod.yang").expect("root").clone();

        let b = FEAT.find("if-feature turbo").unwrap() + "if-feature ".len();
        let t = handle(&rope, &root, b, "fmod", &lib).expect("if-feature hover");
        assert!(t.contains("feature **`turbo`**"), "{t}");

        let b = FEAT.find("default auto;").unwrap() + "default ".len();
        let t = handle(&rope, &root, b, "fmod", &lib).expect("enum default hover");
        assert!(t.contains("enum value **`auto`**"), "{t}");

        let b = FEAT.find("default auto-id;").unwrap() + "default ".len();
        let t = handle(&rope, &root, b, "fmod", &lib).expect("identity default hover");
        assert!(t.contains("identity value **`auto-id`**"), "{t}");
    }
}
