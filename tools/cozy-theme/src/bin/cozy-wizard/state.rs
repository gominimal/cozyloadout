//! What the wizard remembers between runs.
//!
//! Written on a completed run, read on the next one. Plain data with no
//! knowledge of the UI: the screens convert to and from it, so this file stays
//! the schema and nothing else.
//!
//! It is gitignored on purpose. The answers are one person's — which schemes
//! they like, which of their own directories they patch in — and belong in a
//! checkout rather than in the repository.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The default filename, beside the loadout it configures.
pub const FILE: &str = ".cozy-wizard.toml";

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    /// `"legacy"` or `"blocks"`; anything else falls back to the default.
    pub greeting: Option<String>,

    /// The scheme by name, not by path — a name survives the collection being
    /// re-cloned somewhere else, and it is what `just theme` takes.
    pub theme: Option<String>,

    /// Every optional package that was *offered*, and what was chosen, rather
    /// than only the chosen names.
    ///
    /// A bare list cannot tell "the user turned this off" from "this did not
    /// exist yet", so a package added to the loadout later would arrive
    /// silently switched off. With the full map, an unknown package falls back
    /// to its own default and a removed one is ignored.
    pub packages: BTreeMap<String, bool>,

    /// The free-text package field, kept as typed so it comes back editable.
    pub extra: String,

    /// Absolute paths chosen in the two pickers.
    pub files: Vec<PathBuf>,
    pub dirs: Vec<PathBuf>,
}

impl State {
    /// Read the file, or a default state if it is missing or unreadable.
    ///
    /// A corrupt or hand-edited file must not stop the wizard running: this is
    /// a convenience, and the worst it should ever cost is the convenience.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Write the file. Only called for a completed run — quitting early leaves
    /// whatever was there, because a half-answered wizard is not an answer.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Whether a remembered package choice applies, falling back to the
    /// package's own default when this is a package the file has never seen.
    pub fn wants_package(&self, name: &str, default: bool) -> bool {
        self.packages.get(name).copied().unwrap_or(default)
    }

    /// Remembered paths that are still there.
    ///
    /// A file the user has since deleted should not come back selected, and it
    /// should not be an error either — it is just gone, so the page opens
    /// without it.
    pub fn existing(paths: &[PathBuf]) -> Vec<PathBuf> {
        paths.iter().filter(|p| p.exists()).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cozy-state-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trips_through_toml() {
        let dir = temp("round");
        let path = dir.join(FILE);
        let mut s = State {
            greeting: Some("blocks".into()),
            theme: Some("gruvbox-dark".into()),
            extra: "emacs tmux".into(),
            files: vec![PathBuf::from("/etc/hosts")],
            dirs: vec![PathBuf::from("/tmp")],
            ..State::default()
        };
        s.packages.insert("fzf".into(), true);
        s.packages.insert("glow".into(), false);
        s.save(&path).unwrap();

        let back = State::load(&path);
        assert_eq!(back.greeting.as_deref(), Some("blocks"));
        assert_eq!(back.theme.as_deref(), Some("gruvbox-dark"));
        assert_eq!(back.extra, "emacs tmux");
        assert_eq!(back.files, vec![PathBuf::from("/etc/hosts")]);
        assert_eq!(back.dirs, vec![PathBuf::from("/tmp")]);
        assert!(back.wants_package("fzf", false));
        assert!(!back.wants_package("glow", true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_package_the_file_has_never_seen_keeps_its_own_default() {
        // The reason `packages` is a map and not a list: a package added to the
        // loadout after this file was written must not arrive switched off.
        let mut s = State::default();
        s.packages.insert("fzf".into(), false);
        assert!(!s.wants_package("fzf", true), "a recorded answer wins");
        assert!(
            s.wants_package("brand-new", true),
            "an unknown package takes its default"
        );
        assert!(!s.wants_package("brand-new", false));
    }

    #[test]
    fn a_missing_file_reads_as_defaults() {
        let s = State::load(Path::new("/definitely/not/here.toml"));
        assert!(s.greeting.is_none() && s.theme.is_none() && s.packages.is_empty());
    }

    #[test]
    fn a_corrupt_file_reads_as_defaults_rather_than_failing() {
        // This is a convenience file. A hand-edit that breaks it should cost
        // the convenience, not the ability to run the wizard.
        let dir = temp("corrupt");
        let path = dir.join(FILE);
        std::fs::write(&path, "this is not toml {{{").unwrap();
        let s = State::load(&path);
        assert!(s.theme.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_key_is_rejected_rather_than_silently_ignored() {
        // `deny_unknown_fields` means a renamed field is caught at the next
        // read rather than quietly reverting someone's saved answers. It falls
        // back to defaults, which is the same as any other unreadable file.
        let dir = temp("unknown");
        let path = dir.join(FILE);
        std::fs::write(&path, "theme = \"x\"\nnot_a_field = 1\n").unwrap();
        assert!(State::load(&path).theme.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_that_have_since_been_deleted_are_dropped() {
        let dir = temp("paths");
        let kept = dir.join("still-here");
        std::fs::write(&kept, "x").unwrap();
        let gone = dir.join("deleted");
        let out = State::existing(&[kept.clone(), gone]);
        assert_eq!(
            out,
            vec![kept],
            "a path that no longer exists must not come back"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
