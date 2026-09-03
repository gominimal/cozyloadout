//! What a highlighted file or directory actually contains.
//!
//! Reading only, with no drawing in it — the same split `picker.rs` uses, so
//! the interesting decisions (is this text, how much of it do we read, what does
//! patching this directory in actually copy) are testable against real
//! temporary directories rather than through a rendered frame.
//!
//! Everything here is **bounded**. The patches screen points at arbitrary paths
//! in a home directory: a preview that read a 4 GB file or walked a whole
//! `node_modules` would hang the interface on a cursor move.

use std::path::{Path, PathBuf};

/// Most of a file we will read. Enough for any config, small enough that
/// landing on a database dump costs nothing.
const MAX_BYTES: u64 = 64 * 1024;

/// Most entries we will walk for a directory. A count that stops here is
/// reported as "at least", never as the truth.
const MAX_WALK: usize = 2000;

/// What to show for the entry under the cursor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Preview {
    /// A text file, already split into lines.
    Text { lines: Vec<String>, more: bool },
    /// A file that is not text. Previewing it is not useful, but its size is.
    Binary { bytes: u64 },
    /// A file with nothing in it — distinct from one we could not read.
    Empty,
    /// A directory, described by what patching it in would copy.
    Dir {
        files: usize,
        dirs: usize,
        bytes: u64,
        /// Paths relative to the directory, for a sense of what is in there.
        sample: Vec<String>,
        /// Whether the walk stopped early, making the counts a lower bound.
        capped: bool,
    },
    /// Unreadable, and why. A permission error and an empty file look the same
    /// on screen unless the reason is kept.
    Error(String),
}

impl Preview {
    /// Read whatever is at `path`.
    ///
    /// `lines` bounds how much text comes back, so a caller sized to the pane
    /// does not carry a thousand lines it will never draw.
    pub fn read(path: &Path, is_dir: bool, lines: usize) -> Preview {
        if is_dir {
            Self::read_dir(path, lines)
        } else {
            Self::read_file(path, lines)
        }
    }

    fn read_file(path: &Path, lines: usize) -> Preview {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => return Preview::Error(e.to_string()),
        };
        if meta.len() == 0 {
            return Preview::Empty;
        }
        // Read a prefix, not the file: `read_to_string` on something large is
        // the whole problem, and a preview only ever shows the top.
        // `MAX_BYTES` bounds this before the cast, so `usize` always holds it
        // even where a pointer is 32 bits wide.
        let want = usize::try_from(MAX_BYTES.min(meta.len())).unwrap_or(usize::MAX);
        let mut buf = vec![0u8; want];
        let read = {
            use std::io::Read as _;
            match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
                Ok(n) => n,
                Err(e) => return Preview::Error(e.to_string()),
            }
        };
        buf.truncate(read);

        // A NUL byte is the oldest and most reliable "this is not text" signal,
        // and it is what stops the pane rendering control characters.
        if buf.contains(&0) {
            return Preview::Binary { bytes: meta.len() };
        }
        let Ok(text) = String::from_utf8(buf) else {
            return Preview::Binary { bytes: meta.len() };
        };
        let mut out: Vec<String> = text
            .lines()
            .take(lines)
            .map(|l| l.replace('\t', "    "))
            .collect();
        let more = text.lines().nth(lines).is_some()
            || u64::try_from(read).unwrap_or(u64::MAX) < meta.len();
        if out.is_empty() {
            out.push(String::new());
        }
        Preview::Text { lines: out, more }
    }

    /// Walk a directory the way the patch does: everything under it,
    /// recursively, because the source becomes `<dir>/**/*`.
    fn read_dir(path: &Path, lines: usize) -> Preview {
        let mut files = 0usize;
        let mut dirs = 0usize;
        let mut bytes = 0u64;
        let mut sample: Vec<String> = Vec::new();
        let mut capped = false;
        let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];
        let mut seen = 0usize;

        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                // The top-level failure is the interesting one; a subdirectory
                // we cannot open just contributes nothing.
                Err(e) if dir == path => return Preview::Error(e.to_string()),
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                seen += 1;
                if seen > MAX_WALK {
                    capped = true;
                    break;
                }
                let p = entry.path();
                // `file_type`, not `metadata`: a symlink is not followed, so a
                // loop cannot turn this walk into a hang.
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_dir() {
                    dirs += 1;
                    stack.push(p);
                } else {
                    files += 1;
                    bytes += entry.metadata().map_or(0, |m| m.len());
                    if sample.len() < lines {
                        if let Ok(rel) = p.strip_prefix(path) {
                            sample.push(rel.to_string_lossy().into_owned());
                        }
                    }
                }
            }
            if capped {
                break;
            }
        }
        sample.sort();
        Preview::Dir {
            files,
            dirs,
            bytes,
            sample,
            capped,
        }
    }
}

/// Bytes as a person reads them.
pub fn format_bytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.0} KiB", b as f64 / 1024.0),
        b if b < 1024 * 1024 * 1024 => format!("{:.1} MiB", b as f64 / (1024.0 * 1024.0)),
        b => format!("{:.1} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cozy-preview-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        std::fs::write(root.join("config.toml"), "a = 1\nb = 2\nc = 3\n").unwrap();
        std::fs::write(root.join("sub/inner.txt"), "inner").unwrap();
        std::fs::write(root.join("sub/deeper/leaf.txt"), "leaf").unwrap();
        root
    }

    #[test]
    fn a_text_file_comes_back_as_lines() {
        let root = tree("text");
        let p = Preview::read(&root.join("config.toml"), false, 10);
        let Preview::Text { lines, more } = p else {
            panic!("{p:?}")
        };
        assert_eq!(lines, vec!["a = 1", "b = 2", "c = 3"]);
        assert!(!more);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_long_file_is_cut_and_says_so() {
        // The pane has a fixed height; carrying a thousand lines to draw ten of
        // them is pure cost.
        let root = tree("long");
        let path = root.join("long.txt");
        std::fs::write(&path, "line\n".repeat(500)).unwrap();
        let Preview::Text { lines, more } = Preview::read(&path, false, 10) else {
            panic!()
        };
        assert_eq!(lines.len(), 10);
        assert!(more, "the reader has to know there is more");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_huge_file_is_not_read_whole() {
        // The screen points at arbitrary paths in a home directory. Reading one
        // of these on a cursor move is the difference between a preview and a
        // hang.
        let root = tree("huge");
        let path = root.join("huge.bin");
        let big = usize::try_from(MAX_BYTES).unwrap() * 4;
        std::fs::write(&path, "x".repeat(big)).unwrap();
        let Preview::Text { lines, more } = Preview::read(&path, false, 5) else {
            panic!("a file of x's is still text")
        };
        assert!(more, "cut off, so it must say so");
        assert!(lines.len() <= 5);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_binary_file_reports_its_size_rather_than_its_bytes() {
        let root = tree("binary");
        let path = root.join("thing.bin");
        std::fs::write(&path, [0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01, 0x02]).unwrap();
        assert_eq!(
            Preview::read(&path, false, 10),
            Preview::Binary { bytes: 7 }
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn invalid_utf8_counts_as_binary_rather_than_erroring() {
        let root = tree("utf8");
        let path = root.join("latin1.txt");
        std::fs::write(&path, [0xff, 0xfe, 0x41]).unwrap();
        assert!(matches!(
            Preview::read(&path, false, 10),
            Preview::Binary { .. }
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_empty_file_is_not_an_error() {
        let root = tree("empty");
        let path = root.join("nothing");
        std::fs::write(&path, "").unwrap();
        assert_eq!(Preview::read(&path, false, 10), Preview::Empty);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_missing_path_explains_itself() {
        let p = Preview::read(Path::new("/definitely/not/here"), false, 10);
        assert!(matches!(p, Preview::Error(_)), "{p:?}");
    }

    #[test]
    fn a_directory_counts_what_patching_it_in_would_copy() {
        // Recursively, because the patch source becomes `<dir>/**/*` — a
        // top-level listing would understate it.
        let root = tree("dir");
        let p = Preview::read(&root, true, 10);
        let Preview::Dir {
            files,
            dirs,
            sample,
            capped,
            ..
        } = p
        else {
            panic!("{p:?}")
        };
        assert_eq!(files, 3, "config.toml, inner.txt, leaf.txt");
        assert_eq!(dirs, 2, "sub and deeper");
        assert!(!capped);
        assert!(
            sample.contains(&"sub/deeper/leaf.txt".to_string()),
            "paths are relative to the directory: {sample:?}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_directory_walk_stops_and_says_the_count_is_a_floor() {
        let root = tree("wide");
        let many = root.join("many");
        std::fs::create_dir_all(&many).unwrap();
        for i in 0..(MAX_WALK + 50) {
            std::fs::write(many.join(format!("f{i}")), "x").unwrap();
        }
        let Preview::Dir { capped, files, .. } = Preview::read(&root, true, 5) else {
            panic!()
        };
        assert!(capped, "a huge tree must not be walked to the end");
        assert!(
            files <= MAX_WALK + 8,
            "and the walk really did stop: {files}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unreadable_directory_reports_itself() {
        let p = Preview::read(Path::new("/definitely/not/here"), true, 10);
        assert!(matches!(p, Preview::Error(_)), "{p:?}");
    }

    #[test]
    fn sizes_read_the_way_a_person_reads_them() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MiB");
    }
}
