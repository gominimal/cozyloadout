//! The Minimal package registry, read from the index `min` already keeps on
//! disk.
//!
//! **Offline.** minimal.dev has a search page but no documented API, and a
//! wizard that needed the network to tell you whether `ripgrep` exists would be
//! a wizard that stops working on a train. `min` caches a resolved index under
//! its own cache directory; this reads that. The cost is that someone who has
//! never run `min` has no index — which is why "not in the registry" and "no
//! registry to check against" are different answers here, and only the first
//! one warns.
//!
//! Reading only, no drawing — the same split `picker.rs` and `preview.rs` use.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One package, as much of it as this screen needs.
///
/// The two sources carry different things — the local index has licences and no
/// categories, minimal.dev has categories and advisories and no licence — so a
/// field being empty means "this source did not say", never "there is none".
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub license: String,
    pub categories: Vec<String>,
    /// Active security advisories. Worth surfacing before someone installs
    /// something, which is the one thing the local index cannot tell you.
    pub advisories: u32,
}

/// Where a registry came from, which decides what it can say.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// The index `min` keeps on disk. Offline, has licences, and only exists
    /// once Minimal has resolved a package graph on this machine.
    LocalIndex(PathBuf),
    /// minimal.dev's package bundle. Current, has categories and advisories,
    /// and needs the network.
    Site,
}

/// Everything a registry knows.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    /// By name, so a lookup is a lookup and the list comes out sorted.
    packages: BTreeMap<String, Package>,
    /// Where it came from, for saying so on screen.
    pub source: Option<Source>,
}

// --- the slice of `min`'s index this needs ---------------------------------
//
// Deliberately partial: the real schema carries build commands, dependency
// graphs, output globs and provenance, and none of it is any of this screen's
// business. Everything is `Option` or `default` so a field moving upstream
// costs the field rather than the whole index.

#[derive(Deserialize)]
struct Index {
    #[serde(default)]
    builds: Vec<Option<(u32, Option<Build>)>>,
}

#[derive(Deserialize)]
struct Build {
    name: String,
    #[serde(default)]
    attrs: Option<Attrs>,
}

#[derive(Deserialize)]
struct Attrs {
    #[serde(default)]
    license_spdx: Option<Spanned>,
    #[serde(default)]
    upstream_version: Option<Spanned>,
}

/// A string the index carries with the source span it came from — the span is
/// for `min`'s own error messages and is dropped here.
#[derive(Deserialize)]
struct Spanned {
    #[serde(rename = "String")]
    string: Option<(String, serde_json::Value)>,
}

impl Spanned {
    fn text(v: Option<&Spanned>) -> Option<String> {
        v?.string.as_ref().map(|(s, _)| s.clone())
    }
}

impl Registry {
    /// Load from `min`'s cache, or an empty registry if there is none.
    ///
    /// Never an error: not having `min` installed is an ordinary state, and the
    /// difference between "empty" and "absent" is [`Self::is_available`].
    pub fn load(cache_dir: &std::path::Path) -> Self {
        let lc = cache_dir.join("lc");
        // Entries live directly in `lc/` *and* under a `v<N>/` namespace —
        // minimal added the version directory so entries written by one build
        // are never handed to another build's deserializer, and both layouts
        // exist on a machine that has been upgraded. Reading only the top level
        // finds nothing at all on a current install.
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        let collect = |dir: &std::path::Path, into: &mut Vec<_>| {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return Vec::new();
            };
            let mut dirs = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    dirs.push(path);
                } else if let Ok(modified) = meta.modified() {
                    into.push((modified, path));
                }
            }
            dirs
        };
        // One level down only: the namespace is `v1`, not a tree, and walking
        // an unbounded depth of somebody's cache is not this screen's business.
        for dir in collect(&lc, &mut files) {
            collect(&dir, &mut files);
        }
        if files.is_empty() {
            return Self::default();
        }
        // Newest first, so a package present in several target-specific indexes
        // takes its version and licence from the most recent one.
        files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));

        let mut packages = BTreeMap::new();
        let mut source = None;
        for (_, path) in &files {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let Ok(index) = serde_json::from_str::<Index>(&text) else {
                continue;
            };
            // Every index is merged, not just the newest: they are per target,
            // and a package that exists only for one arch still exists. A false
            // "not in the registry" is the annoying failure here.
            for build in index.builds.into_iter().flatten().filter_map(|(_, b)| b) {
                packages
                    .entry(build.name.clone())
                    .or_insert_with(|| Package {
                        name: build.name,
                        version: Spanned::text(
                            build
                                .attrs
                                .as_ref()
                                .and_then(|a| a.upstream_version.as_ref()),
                        )
                        .unwrap_or_default(),
                        license: Spanned::text(
                            build.attrs.as_ref().and_then(|a| a.license_spdx.as_ref()),
                        )
                        .unwrap_or_default(),
                        // The local index carries neither; empty means "this
                        // source did not say", not "there are none".
                        ..Package::default()
                    });
            }
            source.get_or_insert_with(|| Source::LocalIndex(path.clone()));
        }
        Self { packages, source }
    }

    /// Parse minimal.dev's package bundle out of the search page.
    ///
    /// The site is Astro-rendered and bakes the whole dataset into a
    /// `<script type="application/json" id="pkgs-bundle">`, so one GET of the
    /// page is the entire registry. There is no public JSON endpoint —
    /// `/pkgs/search.json` and friends are 404 — so the bundle is what there is.
    ///
    /// # Errors
    ///
    /// If the tag is missing or its contents are not the expected shape, which
    /// is what a redesign of that page would look like from here.
    pub fn from_bundle(html: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Bundle {
            #[serde(default)]
            packages: Vec<SitePackage>,
        }
        #[derive(Deserialize)]
        struct SitePackage {
            name: String,
            #[serde(default)]
            version: Option<String>,
            #[serde(default)]
            categories: Vec<String>,
            #[serde(rename = "activeAdvisoryCount", default)]
            active_advisory_count: u32,
        }
        // Located by id rather than by position: it is the only stable handle
        // on the page, and matching the first `application/json` would silently
        // start reading something else the day another one is added.
        let start = html
            .find(r#"id="pkgs-bundle""#)
            .ok_or("no pkgs-bundle on the page")?;
        let open = html[start..]
            .find('>')
            .map(|i| start + i + 1)
            .ok_or("malformed pkgs-bundle tag")?;
        let end = html[open..]
            .find("</script>")
            .map(|i| open + i)
            .ok_or("unterminated pkgs-bundle")?;

        let bundle: Bundle =
            serde_json::from_str(&html[open..end]).map_err(|e| format!("pkgs-bundle: {e}"))?;
        if bundle.packages.is_empty() {
            return Err("pkgs-bundle carried no packages".to_string());
        }
        let packages = bundle
            .packages
            .into_iter()
            .map(|p| {
                (
                    p.name.clone(),
                    Package {
                        name: p.name,
                        version: p.version.unwrap_or_default(),
                        // The site does not publish licences; the local index
                        // does. Empty rather than guessed.
                        license: String::new(),
                        categories: p.categories,
                        advisories: p.active_advisory_count,
                    },
                )
            })
            .collect();
        Ok(Self {
            packages,
            source: Some(Source::Site),
        })
    }

    /// Fetch the bundle in the background, the way the scheme fetch does.
    ///
    /// `curl` rather than an HTTP client: a TLS stack is a large dependency for
    /// one GET, and this repository already shells out to `git` and `minvmd`
    /// for the same reason. No curl, no network, a redesigned page — all of it
    /// lands as an `Err` on the channel and leaves the local index in place.
    pub fn spawn_fetch() -> std::sync::mpsc::Receiver<Result<Self, String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new("curl")
                .args([
                    "-sS",
                    "--max-time",
                    "15",
                    "--fail",
                    "https://minimal.dev/pkgs/search",
                ])
                .output();
            let result = match out {
                Ok(o) if o.status.success() => {
                    Self::from_bundle(&String::from_utf8_lossy(&o.stdout))
                }
                Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
                Err(e) => Err(format!("could not run curl: {e}")),
            };
            // The receiver is gone if the wizard moved on; nothing to do.
            drop(tx.send(result));
        });
        rx
    }

    /// Where `min` keeps its cache: `$XDG_CACHE_HOME/minimal`, or the
    /// platform's own cache directory, matching `paths::minimal_cache_dir`.
    pub fn cache_dir(home: &std::path::Path) -> PathBuf {
        if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            return xdg.join("minimal");
        }
        // macOS puts caches under Library/Caches; everywhere else it is
        // ~/.cache. `dirs` decides this for minimal itself, and this is the
        // same rule without the dependency.
        if cfg!(target_os = "macos") {
            home.join("Library/Caches/minimal")
        } else {
            home.join(".cache/minimal")
        }
    }

    /// Whether there is an index at all.
    ///
    /// The distinction that keeps this honest: with no index, a name cannot be
    /// checked, and saying "not in the registry" would be a claim rather than a
    /// finding.
    pub fn is_available(&self) -> bool {
        !self.packages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.packages.len()
    }

    #[cfg(test)]
    pub fn get(&self, name: &str) -> Option<&Package> {
        self.packages.get(name)
    }

    /// Whether a name is one the registry knows. Always `true` with no index,
    /// so nothing is warned about that could not be checked.
    pub fn knows(&self, name: &str) -> bool {
        !self.is_available() || self.packages.contains_key(name)
    }

    /// Registry packages matching `query`, best first.
    pub fn search(&self, query: &str) -> Vec<&Package> {
        let all: Vec<&Package> = self.packages.values().collect();
        crate::fuzzy::filter(&all, query, |p| p.name.as_str())
            .into_iter()
            .map(|i| all[i])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An index in the shape `min` writes, including the awkward parts: a null
    /// entry in `builds`, a build with no attrs, and the span tuple.
    const INDEX: &str = r#"{
      "upstream": null,
      "builds": [
        [0, {"name": "ripgrep", "attrs": {
              "license_spdx": {"String": ["Unlicense", {"start_offset": 1, "end_offset": 2}]},
              "upstream_version": {"String": ["15.2.0", {"start_offset": 3, "end_offset": 4}]}}}],
        null,
        [0, {"name": "base", "attrs": null}],
        [0, {"name": "fzf", "attrs": {
              "license_spdx": {"String": ["MIT", {"start_offset": 5, "end_offset": 6}]},
              "upstream_version": {"String": ["0.74.2", {"start_offset": 7, "end_offset": 8}]}}}]
      ]
    }"#;

    /// `files` are relative to `lc/`, so a test can put one under a `v1/`
    /// namespace exactly as minimal does.
    fn with_index(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cozy-registry-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (name, body) in files {
            let path = root.join("lc").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        std::fs::create_dir_all(root.join("lc")).unwrap();
        root
    }

    #[test]
    fn it_reads_the_index_min_already_keeps() {
        let root = with_index("read", &[("a", INDEX)]);
        let r = Registry::load(&root);
        assert!(r.is_available());
        assert_eq!(r.len(), 3, "two with attrs and one without");
        let rg = r.get("ripgrep").unwrap();
        assert_eq!(rg.version, "15.2.0");
        assert_eq!(rg.license, "Unlicense");
        assert!(r.source.is_some(), "it should say where it read from");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_build_with_no_attrs_is_still_a_package() {
        // `base` has none. Dropping it would make the registry claim a package
        // does not exist because its metadata is thin.
        let root = with_index("attrless", &[("a", INDEX)]);
        let r = Registry::load(&root);
        let base = r.get("base").expect("still in the registry");
        assert_eq!(base.version, "");
        assert_eq!(base.license, "");
        assert!(r.knows("base"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn every_index_is_merged_not_only_the_newest() {
        // They are per target. A package that exists only for one architecture
        // still exists, and a false "not in the registry" is the annoying
        // failure.
        let other = r#"{"builds": [[0, {"name": "only-here", "attrs": null}]]}"#;
        let root = with_index("merge", &[("a", INDEX), ("b", other)]);
        let r = Registry::load(&root);
        assert!(r.knows("only-here"), "the second index counts too");
        assert!(r.knows("fzf"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unreadable_or_corrupt_index_is_skipped_not_fatal() {
        let root = with_index(
            "corrupt",
            &[("bad", "this is not json {{{"), ("good", INDEX)],
        );
        let r = Registry::load(&root);
        assert!(r.knows("fzf"), "the readable one still counts");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_index_means_unavailable_rather_than_empty() {
        // The distinction the warnings rest on.
        let r = Registry::load(std::path::Path::new("/definitely/not/here"));
        assert!(!r.is_available());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn with_no_index_nothing_is_reported_as_unknown() {
        // Saying "not in the registry" without a registry would be a claim
        // rather than a finding.
        let r = Registry::load(std::path::Path::new("/definitely/not/here"));
        assert!(r.knows("anything-at-all"));
        assert!(r.knows("obvious-nonsense"));
    }

    #[test]
    fn a_name_the_registry_does_not_have_is_reported() {
        let root = with_index("unknown", &[("a", INDEX)]);
        let r = Registry::load(&root);
        assert!(r.knows("ripgrep"));
        assert!(!r.knows("ripgrepp"), "a typo should be caught");
        assert!(!r.knows("notarealpackage"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn search_is_fuzzy_and_ranked() {
        let root = with_index("search", &[("a", INDEX)]);
        let r = Registry::load(&root);
        let hits: Vec<&str> = r.search("rg").iter().map(|p| p.name.as_str()).collect();
        assert!(hits.contains(&"ripgrep"), "{hits:?}");
        assert_eq!(r.search("ripgrep")[0].name, "ripgrep");
        assert!(r.search("zzzznope").is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_empty_search_lists_everything_in_name_order() {
        let root = with_index("all", &[("a", INDEX)]);
        let r = Registry::load(&root);
        let hits: Vec<&str> = r.search("").iter().map(|p| p.name.as_str()).collect();
        assert_eq!(hits, vec!["base", "fzf", "ripgrep"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_cache_directory_matches_where_min_puts_it() {
        let home = std::path::Path::new("/home/someone");
        let dir = Registry::cache_dir(home);
        assert!(dir.ends_with("minimal"), "{}", dir.display());
        if cfg!(target_os = "macos") {
            assert_eq!(dir, home.join("Library/Caches/minimal"));
        } else {
            assert_eq!(dir, home.join(".cache/minimal"));
        }
    }

    #[test]
    #[ignore = "reads the real index on this machine"]
    fn against_the_real_registry() {
        let home = std::env::var("HOME").unwrap();
        let dir = Registry::cache_dir(std::path::Path::new(&home));
        let t = std::time::Instant::now();
        let r = Registry::load(&dir);
        println!(
            "{} packages from {:?} in {:?}",
            r.len(),
            r.source,
            t.elapsed()
        );
        for name in [
            "ripgrep",
            "fzf",
            "helix",
            "kittyview",
            "glow",
            "notarealpkg",
        ] {
            match r.get(name) {
                Some(p) => println!("  {:12} {:10} {}", p.name, p.version, p.license),
                None => println!("  {name:12} NOT IN REGISTRY"),
            }
        }
        let hits: Vec<&str> = r
            .search("rpgrp")
            .iter()
            .take(3)
            .map(|p| p.name.as_str())
            .collect();
        println!("  fuzzy 'rpgrp' -> {hits:?}");
    }

    #[test]
    fn the_versioned_layout_is_read_too() {
        // minimal namespaces layer-cache entries under `v<N>` so one build's
        // entries are never handed to another build's deserializer. Reading
        // only the top level found nothing at all on an up-to-date install —
        // which is exactly the state the machine this was written on was one
        // session away from.
        let root = with_index("versioned", &[("v1/abc", INDEX)]);
        let r = Registry::load(&root);
        assert!(r.is_available(), "a v1 entry is still an index");
        assert!(r.knows("ripgrep"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn both_layouts_are_merged_when_a_machine_has_been_upgraded() {
        // Old flat entries do not disappear when the namespace arrives.
        let newer = r#"{"builds": [[0, {"name": "only-in-v1", "attrs": null}]]}"#;
        let root = with_index("both", &[("flat", INDEX), ("v1/abc", newer)]);
        let r = Registry::load(&root);
        assert!(r.knows("only-in-v1"), "the namespaced entry counts");
        assert!(r.knows("fzf"), "and so does the flat one");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_walk_does_not_descend_past_the_namespace() {
        // One level, not a tree: this is somebody's cache directory, and there
        // is no reason to walk all of it.
        let root = with_index("deep", &[("v1/nested/deeper/x", INDEX)]);
        assert!(!Registry::load(&root).is_available());
        std::fs::remove_dir_all(&root).unwrap();
    }

    const BUNDLE: &str = r#"<html><body>
<script type="application/json" id="pkgs-bundle" data-bundle-sha="abc">
{"schemaVersion":4,"packages":[
  {"name":"ripgrep","categories":["tool"],"version":"15.2.0","activeAdvisoryCount":0},
  {"name":"openssl","categories":["library","system"],"version":"3.5.0","activeAdvisoryCount":2}
]}
</script></body></html>"#;

    #[test]
    fn the_site_bundle_is_read_out_of_the_page() {
        // minimal.dev bakes the whole dataset into the search page; there is no
        // JSON endpoint (`/pkgs/search.json` is a 404), so this is what there is.
        let r = Registry::from_bundle(BUNDLE).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r.source, Some(Source::Site));
        let rg = r.get("ripgrep").unwrap();
        assert_eq!(rg.version, "15.2.0");
        assert_eq!(rg.categories, vec!["tool"]);
        assert_eq!(rg.license, "", "the site does not publish licences");
    }

    #[test]
    fn advisories_come_through() {
        // The one thing worth knowing before installing something that the
        // local index cannot tell you.
        let r = Registry::from_bundle(BUNDLE).unwrap();
        assert_eq!(r.get("openssl").unwrap().advisories, 2);
        assert_eq!(r.get("ripgrep").unwrap().advisories, 0);
    }

    #[test]
    fn the_bundle_is_found_by_id_not_by_position() {
        // Matching the first `application/json` would silently start reading
        // something else the day another one is added to the page.
        let decoy = r#"<script type="application/json" id="analytics">{"packages":[{"name":"WRONG"}]}</script>"#;
        let r = Registry::from_bundle(&format!("{decoy}{BUNDLE}")).unwrap();
        assert!(r.knows("ripgrep"));
        assert!(!r.knows("WRONG"));
    }

    #[test]
    fn a_page_without_the_bundle_is_an_error_not_an_empty_registry() {
        // An empty registry would silently stop warning about typos; an error
        // keeps whatever was already loaded.
        for html in [
            "<html>nothing here</html>",
            r#"<script type="application/json" id="pkgs-bundle">not json</script>"#,
        ] {
            assert!(Registry::from_bundle(html).is_err(), "{html}");
        }
        let empty = r#"<script type="application/json" id="pkgs-bundle">{"packages":[]}</script>"#;
        assert!(
            Registry::from_bundle(empty).is_err(),
            "no packages is a failure too"
        );
    }

    #[test]
    #[ignore = "reaches minimal.dev"]
    fn against_the_real_site() {
        let rx = Registry::spawn_fetch();
        let r = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the fetch thread should answer")
            .expect("the bundle should parse");
        println!("{} packages from the site", r.len());
        for name in ["ripgrep", "openssl", "helix"] {
            match r.get(name) {
                Some(p) => println!(
                    "  {:10} {:12} {:24} advisories={}",
                    p.name,
                    p.version,
                    p.categories.join(","),
                    p.advisories
                ),
                None => println!("  {name} NOT FOUND"),
            }
        }
        assert!(r.len() > 100, "the real registry is bigger than that");
    }
}
