//! Nerd Font glyphs for the file pickers, the way `eza --icons` draws them.
//!
//! **Font Awesome 4 codepoints only** (`U+F000`–`U+F2FF`). Nerd Fonts patch in
//! several icon sets, and the fashionable file-type icons come from Seti and
//! Devicons — but those sets moved codepoints between Nerd Font v2 and v3,
//! while the Font Awesome block has been stable and present in every patched
//! font since the beginning. A handful of glyphs that always render beats a
//! larger table that shows boxes on half the machines it meets.
//!
//! The table is by *kind*, not by language, for the same reason: a gear for
//! anything that configures something is more useful at a glance than fifteen
//! logos you have to learn.

use std::path::Path;

/// The glyph for a directory. Open, because in a picker you are about to walk
/// into it.
const FOLDER: char = '\u{f07b}';
/// Anything with no better answer.
const FILE: char = '\u{f016}';

/// Extension to glyph. Grouped by what the file *is for*, since that is what
/// you are scanning a list to find.
const BY_EXTENSION: &[(&str, char)] = &[
    // Configuration — the overwhelming majority of what this page shows.
    ("toml", '\u{f013}'),
    ("yaml", '\u{f013}'),
    ("yml", '\u{f013}'),
    ("ini", '\u{f013}'),
    ("cfg", '\u{f013}'),
    ("conf", '\u{f013}'),
    ("kdl", '\u{f013}'),
    ("hjson", '\u{f013}'),
    ("json", '\u{f013}'),
    ("plist", '\u{f013}'),
    ("tmtheme", '\u{f1fc}'),
    // Shells and scripts.
    ("fish", '\u{f120}'),
    ("sh", '\u{f120}'),
    ("bash", '\u{f120}'),
    ("zsh", '\u{f120}'),
    ("nu", '\u{f120}'),
    // Source.
    ("rs", '\u{f121}'),
    ("go", '\u{f121}'),
    ("py", '\u{f121}'),
    ("js", '\u{f121}'),
    ("ts", '\u{f121}'),
    ("c", '\u{f121}'),
    ("h", '\u{f121}'),
    ("lua", '\u{f121}'),
    ("vim", '\u{f121}'),
    ("nix", '\u{f121}'),
    // Prose and data.
    ("md", '\u{f0f6}'),
    ("markdown", '\u{f0f6}'),
    ("txt", '\u{f0f6}'),
    ("rst", '\u{f0f6}'),
    ("log", '\u{f0f6}'),
    ("csv", '\u{f0ce}'),
    ("sql", '\u{f1c0}'),
    // Media.
    ("png", '\u{f1c5}'),
    ("jpg", '\u{f1c5}'),
    ("jpeg", '\u{f1c5}'),
    ("gif", '\u{f1c5}'),
    ("svg", '\u{f1c5}'),
    ("webp", '\u{f1c5}'),
    ("pdf", '\u{f1c1}'),
    ("mp3", '\u{f001}'),
    ("wav", '\u{f001}'),
    ("mp4", '\u{f008}'),
    // Archives and keys.
    ("zip", '\u{f1c6}'),
    ("gz", '\u{f1c6}'),
    ("xz", '\u{f1c6}'),
    ("zst", '\u{f1c6}'),
    ("tar", '\u{f1c6}'),
    ("pem", '\u{f084}'),
    ("key", '\u{f084}'),
    ("pub", '\u{f084}'),
    ("lock", '\u{f023}'),
];

/// Whole filenames that deserve their own glyph regardless of extension.
///
/// Matched case-insensitively on the full name, so `.gitignore` and `Makefile`
/// — neither of which has a usable extension — are reachable at all.
const BY_NAME: &[(&str, char)] = &[
    (".gitignore", '\u{f1d3}'),
    (".gitconfig", '\u{f1d3}'),
    (".gitmodules", '\u{f1d3}'),
    (".gitattributes", '\u{f1d3}'),
    ("makefile", '\u{f085}'),
    ("justfile", '\u{f085}'),
    ("dockerfile", '\u{f085}'),
    ("license", '\u{f02d}'),
    ("readme", '\u{f02d}'),
    ("readme.md", '\u{f02d}'),
];

/// The glyph for one listing entry.
pub fn for_entry(name: &str, is_dir: bool) -> char {
    if is_dir {
        return FOLDER;
    }
    let lower = name.to_ascii_lowercase();
    if let Some((_, icon)) = BY_NAME.iter().find(|(n, _)| *n == lower) {
        return *icon;
    }
    // `.gitignore` has no extension as far as `Path` is concerned, and a
    // dotfile like `.zshrc` reports `zshrc` — neither is in the table, so both
    // fall through to the default rather than matching something wrong.
    Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| {
            BY_EXTENSION
                .iter()
                .find(|(k, _)| *k == ext)
                .map(|(_, icon)| *icon)
        })
        .unwrap_or(FILE)
}

/// A handful of glyphs for the greeting screen to show, so the reader can see
/// whether their font has them before turning them on.
pub fn sample() -> [char; 6] {
    [FOLDER, '\u{f013}', '\u{f120}', '\u{f121}', '\u{f0f6}', FILE]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_is_a_folder_whatever_it_is_called() {
        assert_eq!(for_entry("anything.toml", true), FOLDER);
        assert_eq!(for_entry("", true), FOLDER);
    }

    #[test]
    fn config_formats_share_one_glyph() {
        // By kind, not by language: at a glance "this configures something" is
        // more useful than fifteen logos you have to learn.
        let toml = for_entry("cozy.toml", false);
        for name in ["a.yaml", "b.yml", "c.json", "d.ini", "e.conf", "f.kdl"] {
            assert_eq!(for_entry(name, false), toml, "{name}");
        }
    }

    #[test]
    fn the_kinds_are_told_apart() {
        let kinds = [
            for_entry("a.toml", false),
            for_entry("b.fish", false),
            for_entry("c.rs", false),
            for_entry("d.md", false),
            for_entry("e.png", false),
            for_entry("f.zip", false),
        ];
        let mut unique = kinds.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), kinds.len(), "each kind needs its own glyph");
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(
            for_entry("Cargo.TOML", false),
            for_entry("cargo.toml", false)
        );
        assert_eq!(for_entry("Makefile", false), for_entry("makefile", false));
    }

    #[test]
    fn whole_names_win_over_extensions() {
        // `.gitignore` has no extension as far as `Path` is concerned, so
        // without the name table it would be indistinguishable from any other
        // dotfile.
        assert_ne!(for_entry(".gitignore", false), FILE);
        assert_ne!(for_entry("README.md", false), for_entry("notes.md", false));
    }

    #[test]
    fn an_unknown_file_gets_the_default_rather_than_a_wrong_guess() {
        assert_eq!(for_entry("mystery.zzzz", false), FILE);
        assert_eq!(for_entry("noextension", false), FILE);
        assert_eq!(for_entry(".zshrc", false), FILE);
    }

    #[test]
    fn every_glyph_is_in_the_font_awesome_block() {
        // The whole reason the table is small. Seti and Devicons moved
        // codepoints between Nerd Font v2 and v3; this block did not, and is in
        // every patched font there is. A glyph outside it is a box on somebody's
        // machine.
        let range = '\u{f000}'..='\u{f2ff}';
        let mut all: Vec<char> = vec![FOLDER, FILE];
        all.extend(BY_EXTENSION.iter().map(|(_, c)| *c));
        all.extend(BY_NAME.iter().map(|(_, c)| *c));
        all.extend(sample());
        for c in all {
            assert!(
                range.contains(&c),
                "U+{:04X} is outside Font Awesome; it will be a box somewhere",
                c as u32
            );
        }
    }

    #[test]
    fn the_table_has_no_duplicate_keys() {
        // A duplicate is a silently unreachable row.
        for table in [BY_EXTENSION, BY_NAME] {
            let mut keys: Vec<&str> = table.iter().map(|(k, _)| *k).collect();
            let before = keys.len();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), before, "duplicate key in the table");
        }
    }

    #[test]
    fn the_sample_shows_more_than_one_glyph() {
        // Its whole job is letting someone see whether their font has these.
        let mut s = sample().to_vec();
        s.sort_unstable();
        s.dedup();
        assert!(s.len() >= 5, "a sample of one proves little");
    }
}
