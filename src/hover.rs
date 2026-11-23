//! Hover (`textDocument/hover`).
//!
//! Provides markdown for reference-bearing arguments by resolving through the
//! compiled `Library`: import modules, prefixed-reference bindings, typedef
//! chains (`resolve_type`) and identity ancestry (`resolve_identity`).

use ropey::Rope;
use yrepo::{Library, Statement, StatementKind};

use crate::goto::{RefPart, ref_part_at};

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

/// Byte offset of the first character that is neither a space nor a tab.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// Strip the indentation the continuation lines share.
///
/// A hover renders a source span inside a code fence, and the span begins at
/// the statement keyword: its first line has no leading whitespace while the
/// lines below it keep the file's. The block then looks ragged — the first line
/// flush, the rest pushed right. Removing the margin common to the continuation
/// lines makes the block start at the left edge and keeps the relative nesting
/// that shows the tree shape.
///
/// The first line is left alone: it is already where the dedented block
/// starts. Blank lines become empty lines instead of keeping stale whitespace.
pub(crate) fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let Some((first, rest)) = lines.split_first() else {
        return String::new();
    };
    let margin = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| indent_of(line))
        .min()
        .unwrap_or(0);
    let mut out = first.trim_end().to_owned();
    for line in rest {
        out.push('\n');
        // A blank continuation line can be shorter than the margin; cutting it
        // at its own indentation leaves nothing, which is what it should be.
        out.push_str(line[indent_of(line).min(margin)..].trim_end());
    }
    out
}

/// A hover code block for a YANG source snippet.
pub(crate) fn code_fence(text: &str) -> String {
    format!("```yang\n{}\n```", dedent(text))
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
        // The head splits as well: the prefix names its import, the local
        // name the extension.
        if let Some((RefPart::Prefix, _)) = ref_part_at(rope, kw.clone(), byte) {
            let prefix = prefix?;
            return prefixed_ref(rope, root, scope, lib, prefix);
        }
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
        K::Type => {
            // A prefixed reference is two references: the prefix names its
            // `import` binding, the local name a typedef in that module.
            if let Some((RefPart::Prefix, _)) = ref_part_at(rope, arg.range.clone(), byte) {
                let prefix = split?.0;
                return prefixed_ref(rope, root, scope, lib, prefix);
            }
            // A builtin is never prefixed, so this only matches a bare name.
            if BUILTIN_TYPES.contains(&name) {
                return Some(format!("built-in type **`{name}`**"));
            }
            let r = lib.resolve_type(scope, name)?;
            let mut out = format!("**type** `{name}`");
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
        K::Base => {
            // Same split as `type`: the prefix names its import, the local
            // name an identity in that module.
            if let Some((RefPart::Prefix, _)) = ref_part_at(rope, arg.range.clone(), byte) {
                let prefix = split?.0;
                return prefixed_ref(rope, root, scope, lib, prefix);
            }
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
            if let Some((RefPart::Prefix, _)) = ref_part_at(rope, arg.range.clone(), byte) {
                let prefix = split?.0;
                return prefixed_ref(rope, root, scope, lib, prefix);
            }
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
            if let Some((RefPart::Prefix, _)) = ref_part_at(rope, arg.range.clone(), byte) {
                let prefix = split?.0;
                return prefixed_ref(rope, root, scope, lib, prefix);
            }
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
            if let Some((RefPart::Prefix, _)) = ref_part_at(rope, arg.range.clone(), byte) {
                let prefix = split?.0;
                return prefixed_ref(rope, root, scope, lib, prefix);
            }
            let (prefix, local) = match split {
                Some((p, l)) => (Some(p), l),
                None => (None, name),
            };
            let module = match prefix {
                Some(p) => lib.prefix_to_module(scope, p)?,
                None => scope,
            };
            let g = lib.search_grouping(module, local)?;
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
        // A `leafref` `path`: absolute paths describe the referenced node
        // (predicates stripped); relative paths await the leafref engine.
        K::Path => {
            let path = arg.path();
            if !path.starts_with('/') {
                return None;
            }
            let node = lib.resolve_abs_schema_node_id(scope, &path)?;
            Some(format!(
                "leafref target **`{}`** (kind: {:?})",
                node.name(),
                node.kind()
            ))
        }
        // A `deviation` target describes the affected node.
        K::Deviation => {
            let path = arg.path();
            if !path.starts_with('/') {
                return None;
            }
            let node = lib.resolve_abs_schema_node_id(scope, &path)?;
            Some(format!(
                "deviation target **`{}`** (kind: {:?})",
                node.name(),
                node.kind()
            ))
        }
        K::Refine => {
            let t = crate::goto::refine_target_in_file(root, stmt, scope, lib)?;
            let name = t
                .arg
                .as_ref()
                .map(|a| a.name())
                .unwrap_or_default()
                .to_string();
            Some(format!("refine target node **`{name}`**"))
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

    const LRF: &str = "module lrbase {\n  namespace \"urn:lb\";\n  prefix b;\n\
      container top { list item { key name; leaf name { type string; }\n\
      leaf port { type uint16; } } }\n\
    }\n";
    const LR: &str = "module lr {\n  namespace \"urn:lr\";\n  prefix l;\n\
      import lrbase { prefix b; }\n\
      leaf ref { type leafref { path \"/b:top/b:item/b:port\"; } }\n\
    }\n";
    const DEV: &str = "module devbase {\n  namespace \"urn:db\";\n  prefix db;\n\
      container c { leaf mtu { type uint32; } }\n\
    }\n";
    const DV: &str = "module dv {\n  namespace \"urn:dv\";\n  prefix dv;\n\
      import devbase { prefix db; }\n\
      deviation \"/db:c/db:mtu\" { deviate replace { type uint8; } }\n\
    }\n";

    #[test]
    fn hover_leafref_path_and_deviation() {
        let mut repo = Repository::new();
        repo.upsert("/lrbase.yang", LRF.to_string());
        repo.upsert("/lr.yang", LR.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(LR);
        let root = repo.statement("/lr.yang").expect("root").clone();
        let b = LR.find("path \"/b:top/b:item/b:port\"").unwrap() + "path \"".len();
        let t = handle(&rope, &root, b, "lr", &lib).expect("leafref hover");
        assert!(t.contains("leafref target **`port`**"), "{t}");

        let mut repo = Repository::new();
        repo.upsert("/devbase.yang", DEV.to_string());
        repo.upsert("/dv.yang", DV.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(DV);
        let root = repo.statement("/dv.yang").expect("root").clone();
        let b = DV.find("deviation \"/db:c/db:mtu\"").unwrap() + "deviation \"".len();
        let t = handle(&rope, &root, b, "dv", &lib).expect("deviation hover");
        assert!(t.contains("deviation target **`mtu`**"), "{t}");
    }

    #[test]
    fn hover_refine_target() {
        const RF: &str = "module rf {\n  namespace \"urn:rf\";\n  prefix rf;\n\
          grouping cfg { container inner { leaf host { type string; } } }\n\
          container app { uses cfg { refine inner/host { default \"x\"; } } }\n\
        }\n";
        let mut repo = Repository::new();
        repo.upsert("/rf.yang", RF.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(RF);
        let root = repo.statement("/rf.yang").expect("root");
        let b = RF.find("refine inner/host").unwrap() + "refine inner/".len() + 1;
        let t = handle(&rope, root, b, "rf", &lib).expect("refine hover");
        assert!(t.contains("refine target node **`host`**"), "{t}");
    }

    #[test]
    fn dedent_strips_the_margin_shared_by_continuation_lines() {
        // A span begins at the statement keyword, so its first line is flush
        // while the body keeps the file's indentation.
        assert_eq!(
            dedent("import extdef {\n    prefix vendor;\n  }"),
            "import extdef {\n  prefix vendor;\n}"
        );
    }

    #[test]
    fn dedent_keeps_relative_nesting() {
        assert_eq!(
            dedent("container c {\n    leaf x {\n      type string;\n    }\n  }"),
            "container c {\n  leaf x {\n    type string;\n  }\n}"
        );
    }

    #[test]
    fn dedent_leaves_unindented_and_single_line_snippets_alone() {
        assert_eq!(dedent("leaf x { type string; }"), "leaf x { type string; }");
        assert_eq!(dedent("leaf a;\nleaf b;"), "leaf a;\nleaf b;");
        assert_eq!(dedent(""), "");
    }

    #[test]
    fn dedent_turns_whitespace_only_lines_into_empty_lines() {
        assert_eq!(
            dedent("leaf x {\n    type string;\n   \n  }"),
            "leaf x {\n  type string;\n\n}"
        );
    }

    #[test]
    fn code_fence_dedents_its_content() {
        assert_eq!(
            code_fence("import extdef {\n    prefix vendor;\n  }"),
            "```yang\nimport extdef {\n  prefix vendor;\n}\n```"
        );
    }

    const EXT: &str = "module ext {\n  namespace \"urn:ext\";\n  prefix vendor;\n\
      typedef speed { type uint32; }\n  identity base-id;\n  feature turbo;\n\
      grouping cfg { leaf host { type string; } }\n\
      extension info { argument name; }\n}\n";
    const APP: &str = "module app {\n  namespace \"urn:app\";\n  prefix a;\n\
      import ext {\n    prefix vendor;\n  }\n\
      leaf x { type vendor:speed; }\n\
      identity derived { base vendor:base-id; }\n\
      leaf f { if-feature vendor:turbo; type string; }\n\
      container uses-cfg { uses vendor:cfg; }\n\
      leaf d { type identityref { base vendor:base-id; } default vendor:base-id; }\n\
      vendor:info \"x\";\n}\n";

    fn prefixed_ref_repo() -> (Rope, yrepo::Statement, std::sync::Arc<yrepo::Library>) {
        let mut repo = Repository::new();
        repo.upsert("/ext.yang", EXT.to_string());
        repo.upsert("/app.yang", APP.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(APP);
        let root = repo.statement("/app.yang").expect("root").clone();
        (rope, root, lib)
    }

    /// The prefix half names the `import` that binds it, and the snippet must
    /// start at the fence's left edge or the block reads as a ragged stack.
    #[test]
    fn hover_on_a_prefixed_ref_prefix_shows_the_import() {
        let (rope, root, lib) = prefixed_ref_repo();
        let at = APP.find("vendor:speed").unwrap();
        // Both the prefix and the colon resolve to the binding.
        for byte in [at, at + "vendor".len()] {
            let text = handle(&rope, &root, byte, "app", &lib).expect("hover");
            assert!(text.contains("prefix **`vendor`**"), "{byte}: {text}");
            assert!(
                text.contains("```yang\nimport ext {\n  prefix vendor;\n}\n```"),
                "{byte}: {text}"
            );
        }
    }

    /// The local-name half resolves like a bare name: the typedef chain for a
    /// type, the identity for a base.
    #[test]
    fn hover_on_a_prefixed_ref_name_shows_the_definition() {
        let (rope, root, lib) = prefixed_ref_repo();

        let b = APP.find("vendor:speed").unwrap() + "vendor:".len();
        let text = handle(&rope, &root, b, "app", &lib).expect("type hover");
        assert!(text.contains("**type** `vendor:speed`"), "{text}");
        assert!(text.contains("typedef `speed` (in module `ext`)"), "{text}");
        assert!(text.contains("→ built-in **`uint32`**"), "{text}");

        let b = APP.find("vendor:base-id").unwrap() + "vendor:".len();
        let text = handle(&rope, &root, b, "app", &lib).expect("base hover");
        assert!(
            text.contains("**identity** `base-id` (module `ext`)"),
            "{text}"
        );
    }

    /// Every prefixed reference splits the same way, not just `type`/`base`.
    #[test]
    fn hover_splits_uses_if_feature_default_and_extension_at_the_colon() {
        let (rope, root, lib) = prefixed_ref_repo();

        let references = [
            (
                "if-feature ",
                APP.find("if-feature vendor:turbo").unwrap(),
                "feature **`turbo`** (module `ext`)",
            ),
            (
                "uses ",
                APP.find("uses vendor:cfg").unwrap(),
                "grouping **`cfg`**",
            ),
            (
                "default ",
                APP.find("default vendor:base-id").unwrap(),
                "identity value **`base-id`** (module `ext`)",
            ),
        ];
        for (keyword, at, definition) in references {
            let start = at + keyword.len();
            let text = handle(&rope, &root, start, "app", &lib).expect("prefix hover");
            assert!(text.contains("prefix **`vendor`**"), "{keyword}: {text}");
            let text =
                handle(&rope, &root, start + "vendor:".len(), "app", &lib).expect("name hover");
            assert!(text.contains(definition), "{keyword}: {text}");
        }

        // An extension usage head is `prefix:name` too.
        let at = APP.find("vendor:info").unwrap();
        let text = handle(&rope, &root, at, "app", &lib).expect("head prefix hover");
        assert!(text.contains("prefix **`vendor`**"), "{text}");
        let text = handle(&rope, &root, at + "vendor:".len(), "app", &lib).expect("head hover");
        assert!(text.contains("extension **`info`**"), "{text}");
    }
}
