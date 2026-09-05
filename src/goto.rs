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

/// The deepest `leaf`/`leaf-list`/`typedef` ancestor containing `byte` — used
/// to resolve a `default` to an enum member of its `type enumeration`.
fn owner_data_node(root: &Statement, byte: usize) -> Option<&Statement> {
    use StatementKind as K;
    let mut best = None;
    for s in root.preorder() {
        if s.range.contains(&byte) && matches!(s.kind, K::Leaf | K::LeafList | K::Typedef) {
            best = Some(s);
        }
    }
    best
}

/// Byte range of the `enum <name>` argument inside `owner`'s `type
/// enumeration`, if any.
fn enum_member_arg(owner: &Statement, name: &str) -> Option<Range<usize>> {
    use StatementKind as K;
    let ty = owner
        .children
        .iter()
        .find(|c| c.kind == K::Type && c.arg.as_ref().is_some_and(|a| a.name() == "enumeration"))?;
    let en = ty
        .children
        .iter()
        .find(|c| c.kind == K::Enum && c.arg.as_ref().is_some_and(|a| a.name() == name))?;
    en.arg.as_ref().map(|a| a.range.clone())
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
    rope: &Rope,
    root: &Statement,
    url: &str,
    byte: usize,
    scope: &str,
    lib: &Library,
) -> Option<Vec<Target>> {
    use StatementKind as K;
    let stmt = root.narrowest_at(byte)?;

    // Extension usage: the head `prefix:name` references an `extension`
    // definition (like a `type` reference references a `typedef`).
    if let K::Unknown(_) = &stmt.kind {
        let kw = stmt.keyword.as_ref()?;
        if !kw.contains(&byte) {
            return None;
        }
        let text = rope.get_byte_slice(kw.clone())?.to_string();
        let (prefix, local) = split_ref(&text);
        let module = module_for(scope, prefix, lib)?;
        let ext = lib.search_extension(module, local)?;
        return Some(vec![Target {
            url: ext.defining.url.to_string(),
            target_range: ext.defining.range.clone(),
            origin_range: kw.clone(),
        }]);
    }

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
        K::IfFeature => {
            let (prefix, local) = split_ref(name);
            let module = module_for(scope, prefix, lib)?;
            let f = lib.search_feature(module, local)?;
            Target {
                url: f.defining.url.to_string(),
                target_range: f.defining.range.clone(),
                origin_range: arg.range.clone(),
            }
        }
        K::Default => {
            // A default value may reference an identity (identityref leaves)
            // or an enum member of the owning leaf/typedef's enumeration.
            let (prefix, local) = split_ref(name);
            if let Some(id) = lib.search_identity(module_for(scope, prefix, lib)?, local) {
                return Some(vec![Target {
                    url: id.defining.url.to_string(),
                    target_range: id.defining.range.clone(),
                    origin_range: arg.range.clone(),
                }]);
            }
            let owner = owner_data_node(root, byte)?;
            let range = enum_member_arg(owner, name)?;
            Target {
                url: url.to_string(),
                target_range: range,
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

    #[test]
    fn goto_jumps_from_extension_usage_to_definition() {
        let mut repo = Repository::new();
        repo.upsert("/extdef.yang", DEF.to_string());
        repo.upsert("/app.yang", USE.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(USE);
        let root = repo.statement("/app.yang").expect("root").clone();
        let byte = USE.find("vendor:info").unwrap() + "vendor:".len();

        let targets = resolve(&rope, &root, "/app.yang", byte, "app", &lib).expect("goto");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "/extdef.yang");
        // target lands on the extension name `info` in the definition file
        let def = Rope::from_str(DEF);
        let name = def
            .get_byte_slice(targets[0].target_range.clone())
            .unwrap()
            .to_string();
        assert_eq!(name, "info");
        assert_eq!(
            targets[0].origin_range,
            root.narrowest_at(byte).unwrap().keyword.clone().unwrap()
        );
    }

    const FEAT: &str = "module fmod {\n  namespace \"urn:f\";\n  prefix f;\n\
      feature turbo;\n\
      leaf x { if-feature turbo; type string; }\n\
      leaf mode { type enumeration { enum auto { value 1; } } default auto; }\n\
      identity auto-id;\n\
      leaf idl { type identityref { base auto-id; } default auto-id; }\n\
    }\n";

    fn text_at(def: &str, range: std::ops::Range<usize>) -> String {
        Rope::from_str(def)
            .get_byte_slice(range)
            .unwrap()
            .to_string()
    }

    #[test]
    fn goto_if_feature_and_default_references() {
        let mut repo = Repository::new();
        repo.upsert("/fmod.yang", FEAT.to_string());
        let out = repo.compile();
        let lib = out.library.expect("library");
        let rope = Rope::from_str(FEAT);
        let root = repo.statement("/fmod.yang").expect("root").clone();

        // if-feature -> feature definition.
        let b = FEAT.find("if-feature turbo").unwrap() + "if-feature ".len();
        let t = resolve(&rope, &root, "/fmod.yang", b, "fmod", &lib).expect("if-feature goto");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].url, "/fmod.yang");
        assert_eq!(text_at(FEAT, t[0].target_range.clone()), "turbo");

        // default enum member -> enum definition.
        let b = FEAT.find("default auto;").unwrap() + "default ".len();
        let t = resolve(&rope, &root, "/fmod.yang", b, "fmod", &lib).expect("enum default goto");
        assert_eq!(t.len(), 1);
        assert_eq!(text_at(FEAT, t[0].target_range.clone()), "auto");

        // default identity -> identity definition.
        let b = FEAT.find("default auto-id;").unwrap() + "default ".len();
        let t =
            resolve(&rope, &root, "/fmod.yang", b, "fmod", &lib).expect("identity default goto");
        assert_eq!(t.len(), 1);
        assert_eq!(text_at(FEAT, t[0].target_range.clone()), "auto-id");
    }
}
