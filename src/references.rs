//! Find-references support (P1a): locate the symbol a caret is on and collect
//! every textual reference to it across the open + scanned documents.
//!
//! Kinds covered: typedefs (`type`), groupings (`uses`), identities (`base`),
//! features (`if-feature`) and extensions (unknown-statement heads
//! `prefix:name`). Matching is module-aware: a reference's effective module is
//! its scope module (unprefixed) or the import target of its prefix, so a
//! local name that also exists in another module is not conflated. leafref
//! `path` references are handled by the dedicated leafref engine (P1b).

use std::ops::Range;

use ropey::Rope;
use yrepo::{Library, Statement, StatementKind};

/// The symbol a caret is on (its definition, or a reference to it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Def {
    /// Module instance name owning the definition.
    pub(crate) module: String,
    /// Local (unprefixed) symbol name.
    pub(crate) local: String,
}

fn module_for<'l>(scope: &'l str, prefix: Option<&str>, lib: &'l Library) -> Option<&'l str> {
    match prefix {
        Some(p) => lib.prefix_to_module(scope, p),
        None => Some(scope),
    }
}

fn is_def_kind(k: &StatementKind) -> bool {
    use StatementKind as K;
    matches!(
        k,
        K::Typedef | K::Grouping | K::Identity | K::Feature | K::Extension
    )
}

fn is_ref_kind(k: &StatementKind) -> bool {
    use StatementKind as K;
    matches!(k, K::Type | K::Uses | K::Base | K::IfFeature)
}

fn split_ref(name: &str) -> (Option<&str>, &str) {
    match name.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, name),
    }
}

/// What the caret is on, resolved to its `Def`.
pub(crate) fn def_at(
    rope: &Rope,
    root: &Statement,
    byte: usize,
    scope: &str,
    lib: &Library,
) -> Option<Def> {
    use StatementKind as K;
    let stmt = root.narrowest_at(byte)?;

    // Extension usage head (`prefix:name`) references an extension definition.
    if let K::Unknown(_) = &stmt.kind {
        let kw = stmt.keyword.as_ref()?;
        if !kw.contains(&byte) {
            return None;
        }
        let text = rope.get_byte_slice(kw.clone())?.to_string();
        let (prefix, local) = split_ref(&text);
        let module = module_for(scope, prefix, lib)?.to_string();
        if lib.search_extension(&module, local).is_some() {
            return Some(Def {
                module,
                local: local.to_string(),
            });
        }
        return None;
    }

    let arg = stmt.arg.as_ref()?;
    let name = arg.name();
    let (prefix, local) = split_ref(name);

    if is_def_kind(&stmt.kind) {
        // Caret anywhere on a *definition* statement resolves to that
        // definition: on its own name (the argument), on its keyword, or in a
        // body gap (whitespace/comments directly under it — `narrowest_at`
        // folds those up to the statement because no child covers the byte).
        // A caret on text inside a nested child (e.g. `description` prose)
        // lands on that child instead and is not treated as the definition.
        if !name.is_empty() {
            return Some(Def {
                module: scope.to_string(),
                local: name.to_string(),
            });
        }
        return None;
    }
    if !arg.range.contains(&byte) {
        return None;
    }
    if !is_ref_kind(&stmt.kind) {
        return None;
    }
    if stmt.kind == K::Type && crate::goto::BUILTIN_TYPES.contains(&local) {
        return None;
    }
    let module = module_for(scope, prefix, lib)?.to_string();
    Some(Def {
        module,
        local: local.to_string(),
    })
}

/// Every occurrence of `def` across the given documents.
///
/// `docs`: `(url, root, module_scope)` for each reachable document. Returns
/// `(url, byte range)` hits; the definition itself is included only when
/// `include_declaration` is set.
pub(crate) fn find_references(
    def: &Def,
    docs: &[(String, &Statement, String)],
    lib: &Library,
    include_declaration: bool,
) -> Vec<(String, Range<usize>)> {
    use StatementKind as K;
    let mut hits = Vec::new();
    for (url, root, scope) in docs {
        for stmt in root.preorder() {
            let arg = match &stmt.arg {
                Some(a) => a,
                None => continue,
            };
            let kind = &stmt.kind;
            if is_def_kind(kind) {
                if include_declaration && scope == &def.module && arg.name() == def.local {
                    hits.push((url.clone(), arg.range.clone()));
                }
                continue;
            }
            if !is_ref_kind(kind) {
                continue;
            }
            let (prefix, local) = split_ref(arg.name());
            if local != def.local {
                continue;
            }
            if kind == &K::Type && crate::goto::BUILTIN_TYPES.contains(&local) {
                continue;
            }
            let Some(module) = module_for(scope, prefix, lib) else {
                continue;
            };
            if module == def.module {
                hits.push((url.clone(), arg.range.clone()));
            }
        }
    }
    hits
}

/// The byte range of the LOCAL (unprefixed) part of an argument occurrence
/// whose full text `rope[full]` may be prefix-qualified (`a:speed` → `speed`).
/// Rename edits must replace only the local part.
pub(crate) fn local_name_range(rope: &Rope, full: Range<usize>) -> Range<usize> {
    let text = rope
        .get_byte_slice(full.clone())
        .map(|s| s.to_string())
        .unwrap_or_default();
    match text.find(':') {
        Some(i) => (full.start + i + 1)..full.end,
        None => full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrepo::Repository;

    const A: &str = "module liba { namespace \"urn:a\"; prefix a;\n\
      typedef speed { type uint32; }\n\
      grouping gear { leaf g { type uint8; } }\n\
      identity mode;\n\
      feature turbo;\n\
      container c { leaf s { type speed; }\n\
      uses gear; leaf m { type identityref { base mode; } }\n\
      leaf f { if-feature turbo; type string; } }\n\
    }\n";
    const B: &str = "module libb { namespace \"urn:b\"; prefix b;\n\
      import liba { prefix a; }\n\
      typedef own { type a:speed; }\n\
      leaf s2 { type own; }\n\
      leaf s3 { type a:speed; }\n\
      container g2 { uses a:gear; }\n\
    }\n";

    fn caret_in(hay: &str, needle: &str) -> usize {
        // caret inside the first char of the token right after `needle`
        hay.find(needle).unwrap() + needle.len()
    }

    #[test]
    fn typedef_def_and_cross_module_references() {
        let mut r = Repository::new();
        r.upsert("/a.yang", A.to_string());
        r.upsert("/b.yang", B.to_string());
        let out = r.compile();
        let lib = out.library.expect("lib");
        let root_a = r.statement("/a.yang").expect("root a");
        let root_b = r.statement("/b.yang").expect("root b");

        // caret on the definition name -> Def liba::speed
        let rope_a = Rope::from_str(A);
        let def = def_at(&rope_a, root_a, caret_in(A, "typedef "), "liba", &lib).expect("def");
        assert_eq!((def.module.as_str(), def.local.as_str()), ("liba", "speed"));

        // caret on a prefixed cross-module reference -> same Def
        let rope_b = Rope::from_str(B);
        let def2 = def_at(&rope_b, root_b, caret_in(B, "type a:"), "libb", &lib).expect("ref def");
        assert_eq!(
            (def2.module.as_str(), def2.local.as_str()),
            ("liba", "speed")
        );

        let docs = vec![
            ("/a.yang".to_string(), root_a, "liba".to_string()),
            ("/b.yang".to_string(), root_b, "libb".to_string()),
        ];
        let hits = find_references(&def, &docs, &lib, false);
        // A: leaf s `type speed`; B: typedef own's base `type a:speed` and
        // leaf s3 `type a:speed` (B's own typedef is derived from liba speed).
        assert_eq!(hits.len(), 3, "hits: {hits:?}");
        assert!(hits.iter().any(|(u, _)| u == "/a.yang"));
        assert!(hits.iter().any(|(u, _)| u == "/b.yang"));

        // include_declaration adds the definition in A.
        let hits = find_references(&def, &docs, &lib, true);
        assert_eq!(hits.len(), 4, "hits: {hits:?}");
    }

    #[test]
    fn self_prefixed_module_def_at_mirrors_ietf_yang_types() {
        // ietf-yang-types style: a module whose own prefix is `yang` (used to
        // reference its own typedefs as `yang:counter32`), with description
        // prose that also contains the bare word `counter32`.
        const T: &str = "module ietf-yang-types {\n  namespace \"urn:ietf\";\n  prefix yang;\n\n\
        \x20 typedef counter32 {\n    type uint32;\n    description\n\
        \x20    \"a schema node of type counter32 at times other than re-init\";\n  }\n\
        \x20 typedef zero-based-counter32 {\n    type yang:counter32;\n  }\n\
        \x20 typedef other {\n    type uint32;\n  }\n}\n";
        let mut r = Repository::new();
        r.upsert("/t.yang", T.to_string());
        let out = r.compile();
        let lib = out.library.expect("lib");
        let root = r.statement("/t.yang").expect("root");
        let rope = Rope::from_str(T);

        // Caret on the *definition* name `counter32` (line `typedef counter32`).
        let def = def_at(
            &rope,
            root,
            caret_in(T, "typedef "),
            "ietf-yang-types",
            &lib,
        )
        .expect("def on typedef name");
        assert_eq!(
            (def.module.as_str(), def.local.as_str()),
            ("ietf-yang-types", "counter32")
        );

        // Caret on the self-prefixed reference `type yang:counter32`.
        let def2 = def_at(
            &rope,
            root,
            caret_in(T, "type yang:"),
            "ietf-yang-types",
            &lib,
        )
        .expect("def on self-prefixed reference");
        assert_eq!(
            (def2.module.as_str(), def2.local.as_str()),
            ("ietf-yang-types", "counter32")
        );

        // Caret in the body gap right after the name (before `{`) still
        // resolves to the definition — the observed `Typedef (body)` case.
        let gap = T.find("counter32 {").unwrap() + "counter32".len();
        let def3 = def_at(&rope, root, gap, "ietf-yang-types", &lib).expect("def on body gap");
        assert_eq!(
            (def3.module.as_str(), def3.local.as_str()),
            ("ietf-yang-types", "counter32")
        );

        // Caret on the statement keyword `typedef` resolves to it as well.
        let kw = T.find("typedef counter32").unwrap() + 2;
        let def4 = def_at(&rope, root, kw, "ietf-yang-types", &lib).expect("def on keyword");
        assert_eq!(
            (def4.module.as_str(), def4.local.as_str()),
            ("ietf-yang-types", "counter32")
        );

        // The prose mention of `counter32` inside a description is NOT a
        // definition (it must not resolve).
        let prose = T.find("type counter32 at").unwrap() + "type ".len();
        assert!(
            def_at(&rope, root, prose, "ietf-yang-types", &lib).is_none(),
            "prose inside a description is not a reference"
        );
    }

    #[test]
    fn local_name_range_strips_prefix_keeps_unprefixed() {
        let text = "type a:speed; type own;";
        let rope = Rope::from_str(text);
        let start = text.find("a:speed").unwrap();
        let full = start..start + "a:speed".len();
        let r = local_name_range(&rope, full.clone());
        assert_eq!(&text[r.clone()], "speed");
        let start2 = text.find("type own").unwrap() + "type ".len();
        let full2 = start2..start2 + "own".len();
        let r2 = local_name_range(&rope, full2.clone());
        assert_eq!(r2, full2);
        assert_eq!(&text[r2.clone()], "own");
    }

    #[test]
    fn grouping_identity_feature_references() {
        let mut r = Repository::new();
        r.upsert("/a.yang", A.to_string());
        r.upsert("/b.yang", B.to_string());
        let out = r.compile();
        let lib = out.library.expect("lib");
        let root_a = r.statement("/a.yang").expect("root a");
        let root_b = r.statement("/b.yang").expect("root b");
        let docs = vec![
            ("/a.yang".to_string(), root_a, "liba".to_string()),
            ("/b.yang".to_string(), root_b, "libb".to_string()),
        ];

        let rope_a = Rope::from_str(A);
        // grouping gear: definition in A, one uses in A and one in B.
        let def = def_at(&rope_a, root_a, caret_in(A, "grouping "), "liba", &lib).unwrap();
        let hits = find_references(&def, &docs, &lib, false);
        assert_eq!(hits.len(), 2, "grouping hits: {hits:?}");

        // identity mode: defined in A, based in A only.
        let def = def_at(&rope_a, root_a, caret_in(A, "identity "), "liba", &lib).unwrap();
        let hits = find_references(&def, &docs, &lib, false);
        assert_eq!(hits.len(), 1, "identity hits: {hits:?}");
        assert_eq!(hits[0].0, "/a.yang");

        // feature turbo: A definition, A if-feature.
        let def = def_at(&rope_a, root_a, caret_in(A, "feature "), "liba", &lib).unwrap();
        let hits = find_references(&def, &docs, &lib, false);
        assert_eq!(hits.len(), 1, "feature hits: {hits:?}");
    }
}
