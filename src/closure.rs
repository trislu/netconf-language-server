//! Open-closure serving (Phase B): the pure helpers that keep the yrepo
//! `Repository` equal to "every open buffer, full-parsed, plus every module on
//! disk they can see" — imports, includes (submodules) and, for submodules,
//! the belongs-to parent module — resolved through the workspace catalog.
//!
//! See `docs/serving-large-trees.md`: the workspace is indexed cheaply
//! (`Catalog::scan`, header facts only) and full parses are limited to the
//! open closure, so retention and compile cost stop scaling with the whole
//! tree. Diagnostics/features then see exactly the modules a document can
//! reach, matching what the old whole-tree scan provided for open files.

use std::collections::{HashSet, VecDeque};

use yrepo::{CatalogIndex, Statement, StatementKind};

/// One module-name dependency: the module/submodule name and, when the
/// depending statement pins one, the exact revision-date to resolve.
pub type Seed = (String, Option<String>);

/// The cross-file dependencies an open document's header declares: `import`
/// (module + optional revision-date pin), `include` (submodule + optional
/// pin) and, for a submodule root, `belongs-to` (parent module). The caller
/// feeds these seeds into [`closure_urls`].
pub fn header_seeds(root: &Statement) -> Vec<Seed> {
    use StatementKind as K;
    let mut out = Vec::new();
    for stmt in root.find(&[K::Import, K::Include, K::BelongsTo]) {
        let name = stmt.arg.as_ref().map(|a| a.name().to_owned());
        let pin = stmt
            .find_one(K::RevisionDate)
            .and_then(|r| r.arg.as_ref())
            .map(|a| a.name().to_owned());
        if let Some(name) = name {
            out.push((name, pin));
        }
    }
    out
}

/// The urls reachable from `seeds` through the catalog: each resolved module
/// plus the transitive closure of its imports (honoring revision-date pins)
/// and includes. Names absent from the catalog are skipped (a dangling import
/// surfaces later as a diagnostic), mirroring whole-tree resolution.
pub fn closure_urls(index: &CatalogIndex, seeds: &[Seed]) -> HashSet<String> {
    let mut needed = HashSet::new();
    let mut queued: HashSet<Seed> = seeds.iter().cloned().collect();
    let mut queue: VecDeque<Seed> = seeds.iter().cloned().collect();
    while let Some((name, pin)) = queue.pop_front() {
        let Some(entry) = index.resolve(&name, pin.as_deref()) else {
            continue; // not in this tree
        };
        needed.insert(entry.url.to_string());
        for imp in &entry.imports {
            let seed = (imp.module.clone(), imp.revision.clone());
            if queued.insert(seed.clone()) {
                queue.push_back(seed);
            }
        }
        for sub in &entry.includes {
            let seed = (sub.clone(), None);
            if queued.insert(seed.clone()) {
                queue.push_back(seed);
            }
        }
    }
    needed
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrepo::Repository;

    fn doc(repo: &mut Repository, url: &str, src: &str) -> Statement {
        repo.upsert(url, src);
        repo.statement(url).expect("statement").clone()
    }

    #[test]
    fn header_seeds_covers_imports_includes_and_belongs_to() {
        let mut repo = Repository::new();
        let root = doc(
            &mut repo,
            "/w/m.yang",
            r#"module m {
                namespace "urn:m"; prefix m;
                import b { prefix b; revision-date 2019-01-01; }
                include m-sub;
                leaf x { type string; }
            }"#,
        );
        let seeds = header_seeds(&root);
        assert!(seeds.contains(&("b".to_owned(), Some("2019-01-01".to_owned()))));
        assert!(seeds.contains(&("m-sub".to_owned(), None)));

        // A submodule root contributes its belongs-to parent module.
        let sub = doc(
            &mut repo,
            "/w/m-sub.yang",
            r#"submodule m-sub {
                belongs-to m { prefix m; }
                leaf y { type string; }
            }"#,
        );
        assert_eq!(
            header_seeds(&sub),
            vec![("m".to_owned(), None)],
            "submodule seeds its parent module"
        );
    }

    fn index_with(files: &[(&str, &str)]) -> CatalogIndex {
        let mut index = CatalogIndex::default();
        for (url, src) in files {
            index.push(yrepo::Catalog::scan(*url, *src));
        }
        index
    }

    #[test]
    fn closure_urls_honors_pins_and_follows_includes() {
        let index = index_with(&[
            (
                "/d/a.yang",
                "module a { namespace \"urn:a\"; prefix a; import b { prefix b; revision-date 2019-01-01; } include a-sub; }",
            ),
            (
                "/d/b.yang",
                "module b { namespace \"urn:b\"; prefix b; revision 2020-01-01; import c { prefix c; } }",
            ),
            (
                "/d/b@2019-01-01.yang",
                "module b { namespace \"urn:b\"; prefix b; revision 2019-01-01; import c { prefix c; } }",
            ),
            ("/d/c.yang", "module c { namespace \"urn:c\"; prefix c; }"),
            (
                "/d/a-sub.yang",
                "submodule a-sub { belongs-to a { prefix a; } leaf s { type string; } }",
            ),
        ]);
        // Pinned import resolves to the exact revision file.
        let urls = closure_urls(&index, &[("a".to_owned(), None)]);
        assert!(urls.contains("/d/a.yang"));
        assert!(urls.contains("/d/b@2019-01-01.yang"));
        assert!(
            !urls.contains("/d/b.yang"),
            "latest b is not the pinned one"
        );
        assert!(urls.contains("/d/c.yang"));
        assert!(
            urls.contains("/d/a-sub.yang"),
            "include edge materializes the submodule"
        );
    }

    #[test]
    fn closure_urls_skips_unresolvable_names() {
        let index = index_with(&[(
            "/d/d.yang",
            "module d { namespace \"urn:d\"; prefix d; import ghost { prefix g; } }",
        )]);
        let urls = closure_urls(&index, &[("d".to_owned(), None)]);
        assert!(urls.contains("/d/d.yang"));
        assert_eq!(urls.len(), 1, "ghost is not in the tree and is skipped");
    }
}
