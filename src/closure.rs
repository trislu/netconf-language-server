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
use std::path::{Path, PathBuf};

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

/// Bounded caps for the lazy path: headers parsed for one name, and files in
/// the prefix-fallback candidate set.
const MAX_CANDIDATES_PER_NAME: usize = 256;
const MAX_PREFIX_CANDIDATES: usize = 256;

/// Parse-free basename index local to the server (the published yrepo API
/// does not expose one yet): maps a basename minus its `@revision-date`
/// suffix to candidate files, sorted for deterministic tie-breaks.
#[derive(Debug, Default)]
pub struct NameIndex {
    by_name: std::collections::HashMap<String, Vec<PathBuf>>,
}

impl NameIndex {
    /// Takes ownership of a startup walk's paths, so they are moved into the
    /// startup walk's `Vec<PathBuf>` is moved into the index instead of being
    /// cloned (165k paths on a giant workspace).
    pub fn build_owned(paths: Vec<PathBuf>) -> Self {
        let mut by_name: std::collections::HashMap<String, Vec<PathBuf>> =
            std::collections::HashMap::new();
        for path in paths {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let base = stem.split('@').next().unwrap_or(stem);
            if !base.is_empty() {
                by_name.entry(base.to_owned()).or_default().push(path);
            }
        }
        for candidates in by_name.values_mut() {
            candidates.sort();
        }
        NameIndex { by_name }
    }

    pub fn candidates(&self, name: &str) -> &[PathBuf] {
        self.by_name.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Bounded fallback for names whose declared name differs from the
    /// basename: files under keys starting with `name`, sorted and capped.
    pub fn prefix_candidates(&self, name: &str, limit: usize) -> Vec<PathBuf> {
        if name.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<PathBuf> = Vec::new();
        for (key, files) in &self.by_name {
            if key.starts_with(name) {
                out.extend(files.iter().cloned());
                if out.len() >= limit {
                    break;
                }
            }
        }
        out.sort();
        out.truncate(limit);
        out
    }

    pub fn names_len(&self) -> usize {
        self.by_name.len()
    }

    pub fn file_count(&self) -> usize {
        self.by_name.values().map(Vec::len).sum()
    }
}

/// Resolution counters for one lazy closure pass (logging / measurement).
#[derive(Debug, Default, Clone)]
pub struct ResolveStats {
    /// Names visited by the BFS.
    pub names: usize,
    /// Candidate headers parsed (0 when everything was already cached).
    pub parsed: usize,
    /// Names with no filename candidates and no prefix fallback hit.
    pub missing: Vec<String>,
}

/// Lazy variant of [`closure_urls`]: names missing from `index` are resolved
/// on demand by parsing only their [`PathIndex`] candidates (bounded by
/// [`MAX_CANDIDATES_PER_NAME`], with a bounded prefix fallback for files whose
/// declared name differs from the basename). Newly resolved entries stay in
/// `index`, so repeated calls parse nothing.
pub fn lazy_closure_urls<F>(
    index: &mut CatalogIndex,
    paths: &NameIndex,
    seeds: &[Seed],
    url_for: &F,
) -> (HashSet<String>, ResolveStats)
where
    F: Fn(&Path) -> Option<String> + Send + Sync,
{
    let mut needed = HashSet::new();
    let mut queued: HashSet<Seed> = seeds.iter().cloned().collect();
    let mut queue: VecDeque<Seed> = seeds.iter().cloned().collect();
    let mut stats = ResolveStats::default();
    while let Some((name, pin)) = queue.pop_front() {
        stats.names += 1;
        let (winner, parsed) = match index.resolve(&name, pin.as_deref()) {
            Some(entry) => (Some(entry.url.to_string()), 0),
            None => {
                let mut candidates: Vec<PathBuf> = paths.candidates(&name).to_vec();
                if candidates.is_empty() {
                    candidates = paths.prefix_candidates(&name, MAX_PREFIX_CANDIDATES);
                }
                candidates.truncate(MAX_CANDIDATES_PER_NAME);
                if candidates.is_empty() {
                    (None, 0)
                } else {
                    let parsed = index.scan_many_files_with(candidates.iter(), url_for);
                    let winner = index
                        .resolve(&name, pin.as_deref())
                        .map(|e| e.url.to_string());
                    (winner, parsed)
                }
            }
        };
        stats.parsed += parsed;
        let Some(url) = winner else {
            if !stats.missing.contains(&name) {
                stats.missing.push(name);
            }
            continue;
        };
        needed.insert(url.clone());
        if let Some(entry) = index.of_url(&url) {
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
    }
    (needed, stats)
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
        let mut index = index;
        let (urls, _stats) = lazy_closure_urls(
            &mut index,
            &NameIndex::default(),
            &[("a".to_owned(), None)],
            &|_p: &Path| None,
        );
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
        let mut index = index;
        let (urls, _stats) = lazy_closure_urls(
            &mut index,
            &NameIndex::default(),
            &[("d".to_owned(), None)],
            &|_p: &Path| None,
        );
        assert!(urls.contains("/d/d.yang"));
        assert_eq!(urls.len(), 1, "ghost is not in the tree and is skipped");
    }

    #[test]
    fn lazy_closure_resolves_candidates_and_caches() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!(
            "ncls-lazy-closure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, src: &str| {
            let p = dir.join(name);
            fs::write(&p, src).unwrap();
            p
        };
        let a = write(
            "a.yang",
            "module a { namespace \"urn:a\"; prefix a; import b { prefix b; } import devs { prefix d; } }",
        );
        let b_old = write(
            "b@2019-01-01.yang",
            "module b { namespace \"urn:b\"; prefix b; revision 2019-01-01; }",
        );
        let b_new = write(
            "b@2021-01-01.yang",
            "module b { namespace \"urn:b\"; prefix b; revision 2021-01-01; }",
        );
        let devs = write(
            "devs-spi.yang",
            "module devs { namespace \"urn:d\"; prefix d; }",
        );
        let paths = NameIndex::build_owned(vec![a, b_old, b_new, devs]);
        let url_for = |p: &Path| Some(format!("file://{}", p.display()));
        let seeds = vec![("a".to_owned(), None)];

        let mut index = CatalogIndex::default();
        let (needed, stats) = lazy_closure_urls(&mut index, &paths, &seeds, &url_for);
        assert!(needed.iter().any(|u| u.ends_with("a.yang")));
        assert!(
            needed.iter().any(|u| u.contains("b@2021-01-01")),
            "unpinned import resolves to the highest revision"
        );
        assert!(
            needed.iter().any(|u| u.ends_with("devs-spi.yang")),
            "prefix fallback covers a declared name that differs from the basename"
        );
        assert!(stats.parsed >= 4, "a + two b candidates + devs fallback");
        assert!(stats.missing.is_empty());

        let (needed2, stats2) = lazy_closure_urls(&mut index, &paths, &seeds, &url_for);
        assert_eq!(stats2.parsed, 0, "second pass uses the cached catalog");
        assert_eq!(needed, needed2);
    }

    #[test]
    fn lazy_closure_reports_missing_names() {
        let paths = NameIndex::default();
        let mut index = CatalogIndex::default();
        let url_for = |p: &Path| Some(format!("file://{}", p.display()));
        let (needed, stats) =
            lazy_closure_urls(&mut index, &paths, &[("ghost".to_owned(), None)], &url_for);
        assert!(needed.is_empty());
        assert_eq!(stats.missing, vec!["ghost".to_owned()]);
        assert_eq!(stats.parsed, 0);
    }
}
