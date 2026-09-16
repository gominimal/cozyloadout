//! A browsable filesystem picker, used once on the patches screen.
//!
//! Navigation and selection only — no drawing. That keeps the interesting
//! behaviour (what a directory listing contains, what happens at the filesystem
//! root) testable against real temporary directories rather than through a
//! rendered frame.
//!
//! It used to be *two* pickers side by side, one taking files and one taking
//! directories. They listed the same entries and differed only in which rows
//! had a checkbox, so the second pane showed the same information again with
//! most of it greyed out. One list that takes either is the same capability in
//! half the screen, and the file/directory distinction survives where it
//! matters — in [`Picker::chosen`], which remembers what each pick was.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub name: String,
    /// Whether it *behaves* as a directory — a symlink to one counts, because
    /// walking into it is what the cursor does.
    pub is_dir: bool,
    /// Where it points, when it is a symlink.
    ///
    /// Kept because a dotfile tree is very often a symlink farm (Home Manager,
    /// stow, chezmoi) and the patch walker treats links specially — see the
    /// warning the preview pane draws for a linked directory.
    pub link: Option<String>,
}

pub struct Picker {
    pub cwd: PathBuf,
    /// Everything in `cwd`. `entries` is this filtered by `query`; the full
    /// list is kept so clearing the filter costs nothing and does not re-read
    /// the directory.
    all: Vec<Entry>,
    /// What the listing currently shows.
    pub entries: Vec<Entry>,
    /// The `/` filter. Empty means no filter, which is not the same as a filter
    /// that happens to match everything — see `fuzzy::filter`.
    pub query: String,
    pub row: usize,
    pub top: usize,
    /// Absolute paths to whether each is a directory, sorted and de-duplicated
    /// by `BTreeMap` so the same path chosen twice from two directions counts
    /// once.
    ///
    /// The flag is stored rather than re-checked because it decides the shape
    /// of the patch — a directory becomes a `**/*` glob with a trailing-slash
    /// dest — and a path that has since been deleted must still be describable.
    pub chosen: BTreeMap<PathBuf, bool>,
    /// Set when a directory could not be listed. Shown in place of the
    /// listing: a permission error should say so, not look like an empty
    /// folder.
    pub error: Option<String>,
}

impl Picker {
    pub fn new(start: &Path) -> Self {
        let mut p = Self {
            cwd: start.to_path_buf(),
            all: Vec::new(),
            entries: Vec::new(),
            query: String::new(),
            row: 0,
            top: 0,
            chosen: BTreeMap::new(),
            error: None,
        };
        p.reload();
        p
    }

    /// The chosen paths of one kind, in sorted order.
    pub fn chosen_of(&self, want_dir: bool) -> Vec<PathBuf> {
        self.chosen
            .iter()
            .filter(|(_, is_dir)| **is_dir == want_dir)
            .map(|(p, _)| p.clone())
            .collect()
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
        self.all.clear();
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
            // `symlink_metadata` does not follow, which is the only way to tell
            // a link from what it points at.
            let link = entry
                .path()
                .symlink_metadata()
                .ok()
                .filter(std::fs::Metadata::is_symlink)
                .and_then(|_| std::fs::read_link(entry.path()).ok())
                .map(|t| t.display().to_string());
            self.all.push(Entry { name, is_dir, link });
        }
        self.all
            .sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        self.apply_filter();
    }

    /// Set the `/` filter and re-derive what the listing shows.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.apply_filter();
    }

    /// Rebuild `entries` from `all` and the query, keeping the cursor on the
    /// same *entry* where it survives the filter.
    ///
    /// Following the entry rather than the row index is the difference between
    /// a filter that narrows around what you were looking at and one that
    /// dumps you back at the top on every keystroke.
    fn apply_filter(&mut self) {
        let under_cursor = self.entries.get(self.row).map(|e| e.name.clone());
        self.entries = crate::fuzzy::filter(&self.all, &self.query, |e| e.name.as_str())
            .into_iter()
            .map(|i| self.all[i].clone())
            .collect();
        self.row = under_cursor
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .unwrap_or(0);
        self.top = self.top.min(self.row);
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
                // A filter is about the listing it was typed against. Carrying
                // it into a new directory hides most of wherever you just
                // arrived, which reads as an empty folder rather than a filter.
                self.query.clear();
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
        self.query.clear();
        self.reload();
        if let Some(name) = leaving {
            if let Some(i) = self.entries.iter().position(|e| e.name == name) {
                self.row = i;
            }
        }
    }

    /// Select or deselect the highlighted entry. Returns whether anything
    /// changed, so a caller can tell "toggled off" from "there was nothing
    /// under the cursor".
    ///
    /// Everything in the listing is selectable now: a file becomes a file
    /// patch and a directory becomes a directory patch, which is a difference
    /// in what gets written rather than in what you are allowed to point at.
    pub fn toggle(&mut self) -> bool {
        let Some(entry) = self.current() else {
            return false;
        };
        let (path, is_dir) = (self.cwd.join(&entry.name), entry.is_dir);
        if self.chosen.remove(&path).is_none() {
            self.chosen.insert(path, is_dir);
        }
        true
    }

    pub fn is_chosen(&self, entry: &Entry) -> bool {
        self.chosen.contains_key(&self.cwd.join(&entry.name))
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
        let p = Picker::new(&root);
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
        let p = Picker::new(&root);
        assert!(
            p.entries.iter().any(|e| e.name == ".config"),
            "{:?}",
            p.entries
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn either_kind_can_be_chosen_and_remembers_which_it_was() {
        // The whole point of one list: a directory is as patchable as a file,
        // and what it *is* decides the shape of the patch rather than which
        // pane you were standing in.
        let root = tree("kinds");
        let mut p = Picker::new(&root);
        assert!(p.current().unwrap().is_dir, "row 0 is `.config`");
        assert!(p.toggle(), "a directory is selectable");

        while p.current().unwrap().is_dir {
            p.move_cursor(1, 10);
        }
        assert!(p.toggle(), "and so is a file");
        assert_eq!(p.chosen.len(), 2);
        assert_eq!(p.chosen_of(true).len(), 1, "one directory");
        assert_eq!(p.chosen_of(false).len(), 1, "one file");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn toggling_twice_deselects() {
        let root = tree("toggle");
        let mut p = Picker::new(&root);
        assert!(p.toggle());
        assert_eq!(p.chosen.len(), 1);
        assert!(p.toggle());
        assert!(p.chosen.is_empty(), "the same entry twice should clear it");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn descending_and_ascending_keeps_your_place() {
        let root = tree("walk");
        let mut p = Picker::new(&root);
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
        let mut p = Picker::new(&root);
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
        let mut p = Picker::new(Path::new("/"));
        p.ascend();
        assert_eq!(p.cwd, Path::new("/"), "ascending from / must not wander");
    }

    #[test]
    fn an_unreadable_directory_reports_itself() {
        // An empty listing and a permission error look identical on screen
        // unless the error is kept.
        let mut p = Picker::new(Path::new("/definitely/not/here"));
        assert!(p.entries.is_empty());
        assert!(p.error.is_some(), "a failed listing should explain itself");
        p.reload();
        assert!(p.error.is_some());
    }

    #[test]
    fn selections_survive_navigating_away() {
        let root = tree("survive");
        let mut p = Picker::new(&root);
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
