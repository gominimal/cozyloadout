//! The two settings the wizard writes that are *not* the loadout's.
//!
//! Session keys live in minimal's client config and VM resources live in
//! minvmd's state; a loadout carries neither. Both are applied from here so the
//! one place that reaches outside this repo is a file you can read in one go —
//! and so the rules about *how* they are written (edit, never rewrite; go
//! through minvmd's own CLI) sit next to the code that follows them.

use crate::keys::Bindings;
use crate::resources;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Minimal's client config, which is where `[session-keys]` lives.
///
/// `$XDG_CONFIG_HOME` first, matching `paths::minimal_config_dir` in minimal —
/// and matching the XDG spec's rule that a relative value is invalid and
/// ignored. Otherwise `~/.config`, the same assumption the loadouts directory
/// already makes.
pub fn client_config_path(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
        .join("minimal/config.toml")
}

/// Write `[session-keys]` into minimal's client config.
///
/// Edited rather than rewritten: this is a file the user may have written by
/// hand, with comments and a `[loadouts]` section the wizard knows nothing
/// about. `toml_edit` keeps all of it — formatting, comments, key order — and
/// changes only the four values this page owns.
///
/// Returns what changed, for the summary line.
pub fn apply_client(path: &Path, bindings: &Bindings) -> Result<String, String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc = existing.parse::<toml_edit::DocumentMut>().map_err(|e| {
        format!(
            "{} is not valid TOML ({e}); leaving it alone",
            path.display()
        )
    })?;

    let table = doc["session-keys"].or_insert(toml_edit::table());
    // `implicit` would omit the header, which is wrong for a table the user is
    // meant to find and edit later.
    if let Some(t) = table.as_table_mut() {
        t.set_implicit(false);
    }
    table["leader"] = toml_edit::value(bindings.leader.as_config_str());
    table["bell_on_leader"] = toml_edit::value(bindings.bell);
    let subs = table["subcommands"].or_insert(toml_edit::table());
    if let Some(t) = subs.as_table_mut() {
        t.set_implicit(false);
    }
    subs["detach"] = toml_edit::value(bindings.detach.as_config_str());
    subs["forward"] = toml_edit::value(bindings.forward.as_config_str());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!(
        "detach is {} ({})",
        bindings.hint(),
        path.display()
    ))
}

/// Persist the VM allocation by running `minvmd config set`.
///
/// Not by writing minvmd's `config.toml` directly: that command validates
/// against host capacity, serialises the read-modify-write under the lifecycle
/// lock, and derives its own state directory. Reproducing any of that here
/// would be a guess that breaks silently when minvmd moves.
pub fn apply_resources(alloc: resources::Allocation) -> Result<String, String> {
    let Some(bin) = resources::minvmd_on_path() else {
        return Err("minvmd is not on PATH".to_string());
    };
    let out = Command::new(&bin)
        .args([
            "config",
            "set",
            "--vcpus",
            &alloc.vcpus.to_string(),
            "--ram-mib",
            &alloc.ram_mib.to_string(),
        ])
        .output()
        .map_err(|e| format!("running {}: {e}", bin.display()))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() {
            format!("minvmd config set failed ({})", out.status)
        } else {
            why
        });
    }
    Ok(format!(
        "VM gets {} cores and {}",
        alloc.vcpus,
        resources::format_mib(alloc.ram_mib)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cozy-hostcfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writing_the_client_config_keeps_everything_else_in_the_file() {
        // The whole reason this uses toml_edit. It is the user's file: it may
        // have a [loadouts] section the wizard knows nothing about, and
        // comments explaining why. Rewriting it from a value tree would throw
        // both away silently.
        let dir = temp_dir("client-cfg");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# my notes\n[loadouts]\ndefault_loadouts = [\"cozy\"]\nfollow_symlinks = true\n",
        )
        .unwrap();

        let b = Bindings {
            leader: Key::parse("ctrl-a").unwrap(),
            forward: Key::parse("ctrl-a").unwrap(),
            ..Bindings::default()
        };
        apply_client(&path, &b).unwrap();

        let back = std::fs::read_to_string(&path).unwrap();
        assert!(
            back.contains("# my notes"),
            "the comment must survive:\n{back}"
        );
        assert!(back.contains("default_loadouts"), "{back}");
        assert!(back.contains("follow_symlinks = true"), "{back}");
        assert!(back.contains("[session-keys]"), "{back}");
        assert!(back.contains("leader = \"ctrl-a\""), "{back}");
        assert!(back.contains("[session-keys.subcommands]"), "{back}");
        assert!(back.contains("detach = \"d\""), "{back}");

        // And it has to be the shape minimal reads back.
        let parsed: toml::Value = toml::from_str(&back).unwrap();
        let sk = &parsed["session-keys"];
        assert_eq!(sk["leader"].as_str(), Some("ctrl-a"));
        assert_eq!(sk["bell_on_leader"].as_bool(), Some(false));
        assert_eq!(sk["subcommands"]["detach"].as_str(), Some("d"));
        assert_eq!(sk["subcommands"]["forward"].as_str(), Some("ctrl-a"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_written_shape_is_one_minimal_can_actually_read() {
        // minimal's `SessionKeysConfig` schema, transcribed with
        // `deny_unknown_fields` on — the same trick the generated atuin and
        // lazygit configs are checked with. A misspelled key or a wrong nesting
        // level fails here rather than at the user's next attach.
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Root {
            #[serde(default, rename = "session-keys")]
            session_keys: SessionKeysConfig,
            // Present only so `deny_unknown_fields` above tolerates the
            // section the wizard must not disturb.
            #[serde(default)]
            #[allow(dead_code)]
            loadouts: Option<toml::Value>,
        }
        #[derive(Default, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SessionKeysConfig {
            #[serde(default)]
            leader: Option<String>,
            #[serde(default)]
            subcommands: SubcommandsConfig,
            #[serde(default)]
            bell_on_leader: bool,
        }
        #[derive(Default, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SubcommandsConfig {
            #[serde(default)]
            detach: Option<String>,
            #[serde(default)]
            forward: Option<String>,
        }

        let dir = temp_dir("client-schema");
        let path = dir.join("config.toml");
        let b = Bindings {
            leader: Key::parse("ctrl-a").unwrap(),
            detach: Key::parse("q").unwrap(),
            forward: Key::parse("ctrl-b").unwrap(),
            bell: true,
        };
        apply_client(&path, &b).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let root: Root = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("minimal would reject this file: {e}\n{text}"));
        assert_eq!(root.session_keys.leader.as_deref(), Some("ctrl-a"));
        assert_eq!(root.session_keys.subcommands.detach.as_deref(), Some("q"));
        assert_eq!(
            root.session_keys.subcommands.forward.as_deref(),
            Some("ctrl-b")
        );
        assert!(root.session_keys.bell_on_leader);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writing_the_client_config_creates_it_when_there_is_none() {
        let dir = temp_dir("client-new");
        let path = dir.join("nested/config.toml");
        apply_client(&path, &Bindings::default()).unwrap();
        let back = std::fs::read_to_string(&path).unwrap();
        assert!(back.contains("[session-keys]"), "{back}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_write_updates_rather_than_appends() {
        let dir = temp_dir("client-twice");
        let path = dir.join("config.toml");
        apply_client(&path, &Bindings::default()).unwrap();
        let b = Bindings {
            detach: Key::parse("x").unwrap(),
            ..Bindings::default()
        };
        apply_client(&path, &b).unwrap();

        let back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(back.matches("[session-keys]").count(), 1, "{back}");
        let parsed: toml::Value = toml::from_str(&back).unwrap();
        assert_eq!(
            parsed["session-keys"]["subcommands"]["detach"].as_str(),
            Some("x")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_config_that_is_not_valid_toml_is_left_alone() {
        // Refusing to write is the right answer: the alternative is
        // overwriting a file whose contents we could not read.
        let dir = temp_dir("client-broken");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "this is not toml {{{").unwrap();
        let err = apply_client(&path, &Bindings::default()).unwrap_err();
        assert!(err.contains("not valid TOML"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "this is not toml {{{",
            "the file must be untouched"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_client_config_path_follows_xdg_when_it_is_absolute() {
        // Matching minimal's own resolution, including the spec's rule that a
        // relative XDG_CONFIG_HOME is invalid and ignored.
        let home = PathBuf::from("/home/someone");
        assert_eq!(
            client_config_path(&home),
            PathBuf::from("/home/someone/.config/minimal/config.toml"),
            "with no XDG_CONFIG_HOME set in this test process"
        );
    }

    /// Not a check — writes a real file so the result can be read by eye.
    /// `cargo test --bin cozy-wizard -- --ignored show_written --nocapture`
    #[test]
    #[ignore = "writes a real file; run it deliberately"]
    fn show_written_client_config() {
        let path = std::env::var("COZY_SHOW_CONFIG").expect("set COZY_SHOW_CONFIG");
        let b = Bindings {
            leader: Key::parse("ctrl-a").unwrap(),
            detach: Key::parse("q").unwrap(),
            forward: Key::parse("ctrl-a").unwrap(),
            bell: true,
        };
        println!("{}", apply_client(Path::new(&path), &b).unwrap());
    }
}
