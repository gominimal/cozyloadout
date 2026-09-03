//! Cloning or updating the upstream scheme collection.
//!
//! Kept apart from the screen that drives it: what a fetch *is* — whether the
//! directory is already a checkout, what git command that implies, and what a
//! running fetch reports back — is decidable without a terminal, and is tested
//! against real temporary directories rather than through a rendered frame.

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

/// Upstream scheme collection. Kept identical to the `fetch-schemes` recipe in
/// the justfile, which `fetch_matches_the_justfile_recipe` asserts.
pub const SCHEMES_REPO: &str = "https://github.com/tinted-theming/schemes.git";

// ---------------------------------------------------------------------------
// Fetching the upstream schemes
// ---------------------------------------------------------------------------

/// What fetching would do, decided by what is already on disk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FetchKind {
    /// Nothing there yet: `git clone --depth 1`.
    Clone,
    /// Already a checkout: `git pull --ff-only`.
    Update,
    /// The directory exists but is not a git checkout. `just fetch-schemes`
    /// deletes it and re-clones; the wizard refuses instead. Deleting a
    /// directory the user did not name, from a full-screen UI that has just
    /// covered the scrollback, is not something to do on a keypress.
    Blocked,
}

impl FetchKind {
    pub fn detect(dir: &Path) -> Self {
        if dir.join(".git").is_dir() {
            Self::Update
        } else if dir.exists() {
            Self::Blocked
        } else {
            Self::Clone
        }
    }

    pub fn question(self) -> &'static str {
        match self {
            Self::Clone => "Download the upstream scheme collection now?",
            Self::Update => "Update the upstream scheme collection now?",
            Self::Blocked => "The scheme directory is in the way.",
        }
    }

    pub fn detail(self, dir: &Path) -> String {
        let dir = dir.display();
        match self {
            Self::Clone => format!(
                "Shallow-clones tinted-theming into {dir}: hundreds of base16 and \
                 base24 schemes to build the loadout from. Skipping is fine — the \
                 two schemes in this repo work without it."
            ),
            Self::Update => {
                format!("Fast-forwards the checkout in {dir} to pick up new schemes.")
            }
            Self::Blocked => format!(
                "{dir} exists but is not a git checkout, so it cannot be updated \
                 and will not be deleted from here. Remove it yourself, or run \
                 `just fetch-schemes`, which replaces it."
            ),
        }
    }
}

/// Progress of the fetch, as far as the UI is concerned.
pub enum Fetch {
    Idle,
    /// The `u8` is the spinner frame; the receiver carries the outcome.
    Running(u8, Receiver<Result<String, String>>),
    Done(String),
    Failed(String),
}

/// Build the command that fetches the schemes. Mirrors `just fetch-schemes`.
pub fn fetch_command(kind: FetchKind, dir: &Path) -> Option<Command> {
    let mut cmd = Command::new("git");
    match kind {
        FetchKind::Update => {
            cmd.arg("-C").arg(dir).args(["pull", "--ff-only"]);
        }
        FetchKind::Clone => {
            cmd.args(["clone", "--depth", "1", SCHEMES_REPO]).arg(dir);
        }
        FetchKind::Blocked => return None,
    }
    Some(cmd)
}

/// Run the fetch on a thread so the UI keeps redrawing and stays quittable.
/// Doing it inline would freeze the terminal for the length of a clone, with no
/// way out of a stalled network but killing the process.
pub fn spawn_fetch(kind: FetchKind, dir: &Path) -> Fetch {
    let Some(mut cmd) = fetch_command(kind, dir) else {
        return Fetch::Failed("nothing to run".into());
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = match cmd.output() {
            Ok(out) if out.status.success() => {
                // git says everything useful on stderr, including progress and
                // "Already up to date.", so prefer it and fall back to stdout.
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let out = String::from_utf8_lossy(&out.stdout).trim().to_string();
                Ok(if err.is_empty() { out } else { err })
            }
            Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            Err(e) => Err(format!("could not run git: {e}")),
        };
        // The receiver is gone if the user quit mid-fetch; nothing to do.
        drop(tx.send(outcome));
    });
    Fetch::Running(0, rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;
    use clap::Parser as _;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cozy-fetch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn detection_distinguishes_clone_update_and_blocked() {
        let dir = temp_dir("detect");
        assert_eq!(
            FetchKind::detect(&dir),
            FetchKind::Clone,
            "missing means clone"
        );

        std::fs::create_dir_all(dir.join(".git")).unwrap();
        assert_eq!(
            FetchKind::detect(&dir),
            FetchKind::Update,
            "a checkout means update"
        );

        std::fs::remove_dir_all(dir.join(".git")).unwrap();
        assert_eq!(
            FetchKind::detect(&dir),
            FetchKind::Blocked,
            "a non-checkout directory must not be silently deleted"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fetch_command_matches_the_kind() {
        let dir = Path::new("schemes/vendor");
        let clone = fetch_command(FetchKind::Clone, dir).unwrap();
        assert_eq!(clone.get_program(), "git");
        let args: Vec<_> = clone
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"clone".to_string()), "{args:?}");
        assert!(args.contains(&"--depth".to_string()), "{args:?}");
        assert!(args.contains(&SCHEMES_REPO.to_string()), "{args:?}");

        let update = fetch_command(FetchKind::Update, dir).unwrap();
        let args: Vec<_> = update
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"pull".to_string()), "{args:?}");
        assert!(args.contains(&"--ff-only".to_string()), "{args:?}");

        assert!(fetch_command(FetchKind::Blocked, dir).is_none());
    }

    #[test]
    fn fetch_matches_the_justfile_recipe() {
        // Two places now know how to fetch the schemes, and they must not
        // drift: a wizard that clones a different repo, or into a different
        // directory, than `just fetch-schemes` would be worse than no wizard.
        let justfile = include_str!("../../../../../justfile");
        assert!(
            justfile.contains(SCHEMES_REPO),
            "justfile no longer clones {SCHEMES_REPO}"
        );
        assert!(
            justfile.contains("--depth 1") && justfile.contains("--ff-only"),
            "justfile no longer uses a shallow clone and a fast-forward pull"
        );
        assert!(
            justfile.contains("schemes/vendor") || justfile.contains("{{SCHEMES}}/vendor"),
            "justfile no longer uses schemes/vendor"
        );
        assert_eq!(
            Args::parse_from(["cozy-wizard"]).schemes,
            PathBuf::from("schemes/vendor")
        );
    }
}
