//! Every answer the wizard collects, as a file.
//!
//! Plain data with no knowledge of the UI. It lives in the library rather than
//! in the wizard because it is not only the wizard's: `cozy-theme --settings`
//! renders straight from one of these, so a settings file is a complete,
//! portable description of a loadout — the thing you commit to a dotfiles repo
//! or hand to a colleague.
//!
//! The automatic one (`.cozy-wizard.toml`) is gitignored on purpose. Those
//! answers are one person's — which schemes they like, which of their own
//! directories they patch in — and belong in a checkout rather than in the
//! repository. A file saved deliberately under its own name is a different
//! thing, and yours to do what you like with.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The automatic file, beside the loadout it configures.
pub const FILE: &str = ".cozy-wizard.toml";

#[derive(Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
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

    /// The session-key chords, in minimal's own config spelling (`ctrl-]`,
    /// `d`). Stored as typed rather than parsed so a chord minimal later stops
    /// accepting reads back as "unset" instead of failing the whole file.
    pub leader: Option<String>,
    pub detach: Option<String>,
    pub forward: Option<String>,
    pub bell_on_leader: Option<bool>,

    /// The six scheme adjustments, each a percentage in -100..=100. Stored
    /// flat rather than as a nested table so a value the wizard later stops
    /// accepting reads back as "unset" instead of failing the whole file.
    pub contrast: Option<i8>,
    pub saturation: Option<i8>,
    pub comments: Option<i8>,
    pub separation: Option<i8>,
    pub background: Option<i8>,
    pub warmth: Option<i8>,

    /// The VM allocation. Re-checked against the host on load — this file
    /// travels with the checkout, and a pick from a bigger machine must not
    /// propose a VM this one cannot boot.
    pub vcpus: Option<u8>,
    pub ram_mib: Option<u32>,
}

impl Settings {
    /// Read the file, or a default state if it is missing or unreadable.
    ///
    /// A corrupt or hand-edited file must not stop the wizard running: this is
    /// a convenience, and the worst it should ever cost is the convenience.
    /// Use [`Self::read`] where the caller named the file and deserves to hear
    /// why it could not be used.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        Self::read(path).unwrap_or_default()
    }

    /// Read the file, reporting why it could not be read.
    ///
    /// The distinction matters for a file the user *named*: silently falling
    /// back to defaults there turns "your TOML has a typo on line 4" into a
    /// loadout that is quietly not the one they asked for.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, or is not valid settings TOML.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Write the file. Only called for a completed run — quitting early leaves
    /// whatever was there, because a half-answered wizard is not an answer.
    ///
    /// Written to a sibling temporary file and renamed, so an interrupted write
    /// cannot leave a truncated file where the previous answers were. `rename`
    /// is atomic within a filesystem, and the temporary is a sibling precisely
    /// so it is on the same one.
    ///
    /// # Errors
    ///
    /// If the settings cannot be serialised, or the write or rename fails.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })
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

impl Settings {
    /// The greeting key, falling back to the renderer's own default when the
    /// file predates the field or names something no longer offered.
    #[must_use]
    pub fn greeting(&self) -> String {
        const OFFERED: [&str; 5] = ["blocks", "geometric", "legacy", "text", "none"];
        self.greeting
            .as_deref()
            .filter(|g| OFFERED.contains(g))
            .unwrap_or("blocks")
            .to_string()
    }

    /// The adjustments, with any out-of-range value dropped.
    ///
    /// Per field rather than wholesale: this file is hand-editable, and one bad
    /// number should cost that knob, not the other five.
    #[must_use]
    pub fn adjust(&self) -> crate::Adjust {
        let knob = |v: Option<i8>| v.filter(|v| (-100..=100).contains(v)).unwrap_or(0);
        crate::Adjust {
            contrast: knob(self.contrast),
            saturation: knob(self.saturation),
            comments: knob(self.comments),
            separation: knob(self.separation),
            background: knob(self.background),
            warmth: knob(self.warmth),
        }
    }

    /// The optional packages to install, as `Options::with` wants them.
    ///
    /// Each offered package falls back to its own default, so a package added
    /// to the loadout after this file was written arrives switched *on* rather
    /// than silently missing. The empty set is a single empty string, because
    /// an empty `with` means "everything" — the opposite of an empty checklist.
    #[must_use]
    pub fn packages(&self, offered: &crate::Packages) -> Vec<String> {
        let mut names: Vec<String> = offered
            .optional
            .iter()
            .filter(|p| self.wants_package(&p.name, p.default))
            .map(|p| p.name.clone())
            .collect();
        names.extend(
            self.extra
                .split_whitespace()
                .filter(|t| crate::is_package_name(t))
                .map(str::to_string),
        );
        names.dedup();
        if names.is_empty() {
            vec![String::new()]
        } else {
            names
        }
    }

    /// Fill in everything this file decides, leaving the paths to the caller.
    ///
    /// The wizard's apply screen and `cozy-theme --settings` both go through
    /// here, so what the wizard builds and what the saved file rebuilds cannot
    /// drift apart.
    pub fn apply_to(&self, options: &mut crate::Options, offered: &crate::Packages) {
        options.greeting = self.greeting();
        options.adjust = self.adjust();
        options.with = self.packages(offered);
        options.patch_files = Self::existing(&self.files);
        options.patch_dirs = Self::existing(&self.dirs);
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
        let mut s = Settings {
            greeting: Some("blocks".into()),
            theme: Some("gruvbox-dark".into()),
            extra: "emacs tmux".into(),
            files: vec![PathBuf::from("/etc/hosts")],
            dirs: vec![PathBuf::from("/tmp")],
            leader: Some("ctrl-a".into()),
            bell_on_leader: Some(true),
            vcpus: Some(4),
            ram_mib: Some(8192),
            ..Settings::default()
        };
        s.packages.insert("fzf".into(), true);
        s.packages.insert("glow".into(), false);
        s.save(&path).unwrap();

        let back = Settings::load(&path);
        assert_eq!(back.greeting.as_deref(), Some("blocks"));
        assert_eq!(back.theme.as_deref(), Some("gruvbox-dark"));
        assert_eq!(back.extra, "emacs tmux");
        assert_eq!(back.files, vec![PathBuf::from("/etc/hosts")]);
        assert_eq!(back.dirs, vec![PathBuf::from("/tmp")]);
        assert!(back.wants_package("fzf", false));
        assert!(!back.wants_package("glow", true));
        assert_eq!(back.leader.as_deref(), Some("ctrl-a"));
        assert_eq!(back.bell_on_leader, Some(true));
        assert_eq!(back.vcpus, Some(4));
        assert_eq!(back.ram_mib, Some(8192));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_package_the_file_has_never_seen_keeps_its_own_default() {
        // The reason `packages` is a map and not a list: a package added to the
        // loadout after this file was written must not arrive switched off.
        let mut s = Settings::default();
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
        let s = Settings::load(Path::new("/definitely/not/here.toml"));
        assert!(s.greeting.is_none() && s.theme.is_none() && s.packages.is_empty());
    }

    #[test]
    fn a_corrupt_file_reads_as_defaults_rather_than_failing() {
        // This is a convenience file. A hand-edit that breaks it should cost
        // the convenience, not the ability to run the wizard.
        let dir = temp("corrupt");
        let path = dir.join(FILE);
        std::fs::write(&path, "this is not toml {{{").unwrap();
        let s = Settings::load(&path);
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
        assert!(Settings::load(&path).theme.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_that_have_since_been_deleted_are_dropped() {
        let dir = temp("paths");
        let kept = dir.join("still-here");
        std::fs::write(&kept, "x").unwrap();
        let gone = dir.join("deleted");
        let out = Settings::existing(&[kept.clone(), gone]);
        assert_eq!(
            out,
            vec![kept],
            "a path that no longer exists must not come back"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_named_file_reports_why_it_could_not_be_read() {
        // `load` swallows the reason, which is right for the automatic file and
        // wrong for one the user named: a typo should say where, not silently
        // render defaults.
        let dir = temp("read-err");
        let path = dir.join(FILE);
        std::fs::write(&path, "this is not toml {{{").unwrap();
        let err = Settings::read(&path).unwrap_err();
        assert!(err.contains("TOML parse error"), "{err}");
        assert!(
            err.contains(path.to_str().unwrap()),
            "and name the file: {err}"
        );
        assert_eq!(
            Settings::load(&path),
            Settings::default(),
            "load still falls back"
        );

        let err = Settings::read(&dir.join("absent.toml")).unwrap_err();
        assert!(err.contains("absent.toml"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_write_leaves_the_previous_answers_intact() {
        // Written via a sibling temporary and renamed, so there is no window
        // where the file on disk is half of the new content.
        let dir = temp("atomic");
        let path = dir.join(FILE);
        let first = Settings {
            theme: Some("keep-me".into()),
            ..Settings::default()
        };
        first.save(&path).unwrap();

        // A directory where the temporary wants to go: the rename cannot
        // happen, and the original must survive it.
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::create_dir(&tmp).unwrap();
        let second = Settings {
            theme: Some("lose-me".into()),
            ..Settings::default()
        };
        assert!(second.save(&path).is_err());
        assert_eq!(
            Settings::load(&path).theme.as_deref(),
            Some("keep-me"),
            "the previous answers must still be there"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_creates_the_directory_it_was_pointed_at() {
        // "Save these settings to a file" takes a path the user typed; a
        // reasonable one names a directory that does not exist yet.
        let dir = temp("mkdir");
        let path = dir.join("nested/deeper/mine.toml");
        Settings::default().save(&path).unwrap();
        assert!(path.is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_temporary_file_is_left_behind() {
        let dir = temp("no-litter");
        let path = dir.join(FILE);
        Settings::default().save(&path).unwrap();
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec![FILE.to_string()], "{left:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
