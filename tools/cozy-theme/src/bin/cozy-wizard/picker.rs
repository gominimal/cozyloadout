//! A browsable filesystem picker, used twice on the patches screen: once for
//! files and once for directories.
//!
//! Navigation and selection only — no drawing. That keeps the interesting
//! behaviour (what a directory listing contains, what is selectable, what
//! happens at the filesystem root) testable against real temporary
//! directories rather than through a rendered frame.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What a picker is for. Both kinds show directories — you have to walk
/// through them either way — but only one kind of entry can be chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Files,
    Dirs,
}

impl Pick {
    pub fn title(self) -> &'static str {
        match self {
            Pick::Files => " Files ",
            Pick::Dirs => " Directories ",
        }
    }

    /// Whether an entry of this kind can be selected, as opposed to merely
    /// walked into. Public because the drawing code needs the same answer to
    /// decide what to show a checkbox against — two copies of this rule would
    /// be one too many.
    pub fn accepts(self, is_dir: bool) -> bool {
        match self {
            Pick::Files => !is_dir,
            Pick::Dirs => is_dir,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

pub struct Picker {
    pub kind: Pick,
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    pub row: usize,
    pub top: usize,
    /// Absolute paths, sorted and de-duplicated by `BTreeSet` so the same file
    /// chosen twice from two directions counts once.
    pub chosen: BTreeSet<PathBuf>,
    /// Set when a directory could not be listed. Shown in place of the
    /// listing: a permission error should say so, not look like an empty
    /// folder.
    pub error: Option<String>,
}

impl Picker {
    pub fn new(kind: Pick, start: &Path) -> Self {
        let mut p = Self {
            kind,
            cwd: start.to_path_buf(),
            entries: Vec::new(),
            row: 0,
            top: 0,
            chosen: BTreeSet::new(),
            error: None,
        };
        p.reload();
        p
    }

    /// Read `cwd`. Directories first, then files, each alphabetically —
    /// the order a file manager uses, and the one that puts what you are
    /// likely to walk into at the top.
    ///
    /// Hidden entries are **included**: the whole point is patching in
    /// dotfiles, so a picker that hid `.config` would be useless here.
    pub fn reload(&mut self) {
        self.row = 0;
        self.top = 0;
        self.error = None;
        self.entries.clear();

        let read = match std::fs::read_dir(&self.cwd) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        for entry in read.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            // `file_type` rather than `metadata`: a symlink to a directory
            // should still be walkable, and a broken one should not error the
            // whole listing.
            let is_dir = entry.path().is_dir();
            self.entries.push(Entry { name, is_dir });
        }
        self.entries
            .sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.row)
    }

    pub fn move_cursor(&mut self, delta: isize, rows: usize) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        let row = isize::try_from(self.row).unwrap_or(0).saturating_add(delta);
        let clamped = row.clamp(0, isize::try_from(last).unwrap_or(isize::MAX));
        self.row = usize::try_from(clamped).unwrap_or(0);
        if self.row < self.top {
            self.top = self.row;
        } else if rows > 0 && self.row >= self.top + rows {
            self.top = self.row + 1 - rows;
        }
    }

    /// Walk into the highlighted directory. Does nothing on a file, so the key
    /// is safe to hold down.
    pub fn descend(&mut self) {
        if let Some(entry) = self.current() {
            if entry.is_dir {
                self.cwd = self.cwd.join(&entry.name);
                self.reload();
            }
        }
    }

    /// Walk to the parent, keeping the directory just left under the cursor so
    /// going up and back down does not lose your place.
    pub fn ascend(&mut self) {
        let Some(parent) = self.cwd.parent().map(Path::to_path_buf) else {
            // Already at the filesystem root; nothing above it.
            return;
        };
        let leaving = self
            .cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string);
        self.cwd = parent;
        self.reload();
        if let Some(name) = leaving {
            if let Some(i) = self.entries.iter().position(|e| e.name == name) {
                self.row = i;
            }
        }
    }

    /// Select or deselect the highlighted entry, if this picker accepts that
    /// kind. Returns whether anything changed, so the caller can tell the
    /// difference between "toggled off" and "not selectable".
    pub fn toggle(&mut self) -> bool {
        let Some(entry) = self.current() else {
            return false;
        };
        if !self.kind.accepts(entry.is_dir) {
            return false;
        }
        let path = self.cwd.join(&entry.name);
        if !self.chosen.remove(&path) {
            self.chosen.insert(path);
        }
        true
    }

    pub fn is_chosen(&self, entry: &Entry) -> bool {
        self.chosen.contains(&self.cwd.join(&entry.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private tree per test; tests run in parallel in one process.
    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cozy-picker-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("alpha/nested")).unwrap();
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::write(root.join("zeta.txt"), "z").unwrap();
        std::fs::write(root.join("beta.txt"), "b").unwrap();
        std::fs::write(root.join(".hidden"), "h").unwrap();
        std::fs::write(root.join("alpha/inner.txt"), "i").unwrap();
        root
    }

    #[test]
    fn lists_directories_first_then_files_alphabetically() {
        let root = tree("order");
        let p = Picker::new(Pick::Files, &root);
        let names: Vec<&str> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec![".config", "alpha", ".hidden", "beta.txt", "zeta.txt"]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn hidden_entries_are_shown() {
        // Patching in dotfiles is the entire use case; a picker that hid
        // `.config` would be useless for it.
        let root = tree("hidden");
        let p = Picker::new(Pick::Dirs, &root);
        assert!(
            p.entries.iter().any(|e| e.name == ".config"),
            "{:?}",
            p.entries
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_picker_only_takes_files_and_a_dir_picker_only_takes_dirs() {
        let root = tree("kinds");
        let mut files = Picker::new(Pick::Files, &root);
        // Row 0 is `.config`, a directory.
        assert!(!files.toggle(), "a file picker must not take a directory");
        assert!(files.chosen.is_empty());
        while files.current().unwrap().is_dir {
            files.move_cursor(1, 10);
        }
        assert!(files.toggle(), "and must take a file");
        assert_eq!(files.chosen.len(), 1);

        let mut dirs = Picker::new(Pick::Dirs, &root);
        assert!(dirs.toggle(), "a dir picker must take a directory");
        while !dirs.current().unwrap().is_dir {
            dirs.move_cursor(1, 10);
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn toggling_twice_deselects() {
        let root = tree("toggle");
        let mut p = Picker::new(Pick::Dirs, &root);
        assert!(p.toggle());
        assert_eq!(p.chosen.len(), 1);
        assert!(p.toggle());
        assert!(p.chosen.is_empty(), "the same entry twice should clear it");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn descending_and_ascending_keeps_your_place() {
        let root = tree("walk");
        let mut p = Picker::new(Pick::Files, &root);
        while p.current().unwrap().name != "alpha" {
            p.move_cursor(1, 10);
        }
        p.descend();
        assert_eq!(p.cwd, root.join("alpha"));
        assert!(p.entries.iter().any(|e| e.name == "inner.txt"));

        p.ascend();
        assert_eq!(p.cwd, root);
        assert_eq!(
            p.current().unwrap().name,
            "alpha",
            "coming back up should land on the directory just left"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn descending_into_a_file_does_nothing() {
        let root = tree("descend-file");
        let mut p = Picker::new(Pick::Files, &root);
        while p.current().unwrap().is_dir {
            p.move_cursor(1, 10);
        }
        let before = p.cwd.clone();
        p.descend();
        assert_eq!(p.cwd, before, "a file is not a directory to walk into");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_filesystem_root_has_no_parent_to_climb_to() {
        let mut p = Picker::new(Pick::Dirs, Path::new("/"));
        p.ascend();
        assert_eq!(p.cwd, Path::new("/"), "ascending from / must not wander");
    }

    #[test]
    fn an_unreadable_directory_reports_itself() {
        // An empty listing and a permission error look identical on screen
        // unless the error is kept.
        let mut p = Picker::new(Pick::Files, Path::new("/definitely/not/here"));
        assert!(p.entries.is_empty());
        assert!(p.error.is_some(), "a failed listing should explain itself");
        p.reload();
        assert!(p.error.is_some());
    }

    #[test]
    fn selections_survive_navigating_away() {
        let root = tree("survive");
        let mut p = Picker::new(Pick::Dirs, &root);
        p.toggle();
        let chosen = p.chosen.clone();
        while p.current().unwrap().name != "alpha" {
            p.move_cursor(1, 10);
        }
        p.descend();
        p.ascend();
        assert_eq!(p.chosen, chosen, "walking around must not drop selections");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
