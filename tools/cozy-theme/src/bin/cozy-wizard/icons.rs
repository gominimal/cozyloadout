//! Nerd Font glyphs and colours for the file listings, the way `eza` draws
//! them.
//!
//! **Font Awesome 4 codepoints only** (`U+F000`–`U+F2FF`). Nerd Fonts patch in
//! several icon sets, and the fashionable file-type icons come from Seti and
//! Devicons — but those sets moved codepoints between Nerd Font v2 and v3,
//! while the Font Awesome block has been stable and present in every patched
//! font since the beginning. A handful of glyphs that always render beats a
//! larger table that shows boxes on half the machines it meets.
//!
//! Everything is classified by **kind**, not by language: one gear for anything
//! that configures something, one terminal for every shell. At a glance "this
//! configures something" is more useful than fifteen logos you have to learn,
//! it keeps the table inside the codepoint block above, and it gives the colour
//! something meaningful to follow.

use crate::theme::Theme;
use ratatui::style::Color;
use std::path::Path;

/// What a listing entry is, as far as a glance is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dir,
    Config,
    Shell,
    Source,
    Prose,
    Data,
    Media,
    Archive,
    Secret,
    Build,
    Vcs,
    Doc,
    Plain,
}

impl Kind {
    /// The glyph, from the Font Awesome block.
    pub fn icon(self) -> char {
        match self {
            Kind::Dir => '\u{f07b}',
            Kind::Config => '\u{f013}',
            Kind::Shell => '\u{f120}',
            Kind::Source => '\u{f121}',
            Kind::Prose => '\u{f0f6}',
            Kind::Data => '\u{f1c0}',
            Kind::Media => '\u{f1c5}',
            Kind::Archive => '\u{f1c6}',
            Kind::Secret => '\u{f084}',
            Kind::Build => '\u{f085}',
            Kind::Vcs => '\u{f1d3}',
            Kind::Doc => '\u{f02d}',
            Kind::Plain => '\u{f016}',
        }
    }

    /// The colour, from the selected scheme — so the listing re-themes with
    /// everything else, exactly as the syntax preview does.
    ///
    /// Slots are chosen for contrast between neighbours in a listing rather
    /// than for any meaning in the colour itself: a directory has to stand out
    /// from the files under it, and a lock file from a config beside it.
    pub fn color(self, t: &Theme) -> Color {
        match self {
            Kind::Dir => t.blue,
            Kind::Config => t.yellow,
            Kind::Shell => t.green,
            Kind::Media => t.magenta,
            // Sharing a slot is fine where the glyphs already differ: there are
            // more kinds than a sixteen-colour scheme has distinct accents, and
            // a lock and a zip are told apart by their icon.
            Kind::Archive | Kind::Secret => t.red,
            Kind::Source | Kind::Vcs => t.orange,
            Kind::Data | Kind::Build => t.cyan,
            Kind::Prose | Kind::Doc => t.fg,
            Kind::Plain => t.comment,
        }
    }
}

/// Extension to kind.
const BY_EXTENSION: &[(&str, Kind)] = &[
    // Configuration — the overwhelming majority of what this page shows.
    ("toml", Kind::Config),
    ("yaml", Kind::Config),
    ("yml", Kind::Config),
    ("ini", Kind::Config),
    ("cfg", Kind::Config),
    ("conf", Kind::Config),
    ("kdl", Kind::Config),
    ("hjson", Kind::Config),
    ("json", Kind::Config),
    ("plist", Kind::Config),
    ("tmtheme", Kind::Media),
    // Shells and scripts.
    ("fish", Kind::Shell),
    ("sh", Kind::Shell),
    ("bash", Kind::Shell),
    ("zsh", Kind::Shell),
    ("nu", Kind::Shell),
    // Source.
    ("rs", Kind::Source),
    ("go", Kind::Source),
    ("py", Kind::Source),
    ("js", Kind::Source),
    ("ts", Kind::Source),
    ("c", Kind::Source),
    ("h", Kind::Source),
    ("lua", Kind::Source),
    ("vim", Kind::Source),
    ("nix", Kind::Source),
    // Prose and data.
    ("md", Kind::Prose),
    ("markdown", Kind::Prose),
    ("txt", Kind::Prose),
    ("rst", Kind::Prose),
    ("log", Kind::Prose),
    ("csv", Kind::Data),
    ("sql", Kind::Data),
    ("db", Kind::Data),
    // Media.
    ("png", Kind::Media),
    ("jpg", Kind::Media),
    ("jpeg", Kind::Media),
    ("gif", Kind::Media),
    ("svg", Kind::Media),
    ("webp", Kind::Media),
    ("pdf", Kind::Doc),
    ("mp3", Kind::Media),
    ("wav", Kind::Media),
    ("mp4", Kind::Media),
    // Archives and keys.
    ("zip", Kind::Archive),
    ("gz", Kind::Archive),
    ("xz", Kind::Archive),
    ("zst", Kind::Archive),
    ("tar", Kind::Archive),
    ("pem", Kind::Secret),
    ("key", Kind::Secret),
    ("pub", Kind::Secret),
    ("lock", Kind::Secret),
];

/// Whole filenames that deserve their own kind regardless of extension.
///
/// Matched case-insensitively on the full name, so `.gitignore` and `Makefile`
/// — neither of which has a usable extension — are reachable at all.
const BY_NAME: &[(&str, Kind)] = &[
    (".gitignore", Kind::Vcs),
    (".gitconfig", Kind::Vcs),
    (".gitmodules", Kind::Vcs),
    (".gitattributes", Kind::Vcs),
    ("makefile", Kind::Build),
    ("justfile", Kind::Build),
    ("dockerfile", Kind::Build),
    ("license", Kind::Doc),
    ("readme", Kind::Doc),
    ("readme.md", Kind::Doc),
];

/// The kind of one listing entry.
pub fn kind_of(name: &str, is_dir: bool) -> Kind {
    if is_dir {
        return Kind::Dir;
    }
    let lower = name.to_ascii_lowercase();
    if let Some((_, kind)) = BY_NAME.iter().find(|(n, _)| *n == lower) {
        return *kind;
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
                .map(|(_, kind)| *kind)
        })
        .unwrap_or(Kind::Plain)
}

/// A handful of glyphs for the greeting screen to show, so the reader can see
/// whether their font has them before turning them on.
pub fn sample() -> [char; 6] {
    [
        Kind::Dir,
        Kind::Config,
        Kind::Shell,
        Kind::Source,
        Kind::Prose,
        Kind::Plain,
    ]
    .map(Kind::icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_is_a_folder_whatever_it_is_called() {
        assert_eq!(kind_of("anything.toml", true), Kind::Dir);
        assert_eq!(kind_of("", true), Kind::Dir);
    }

    #[test]
    fn config_formats_share_one_glyph() {
        // By kind, not by language: at a glance "this configures something" is
        // more useful than fifteen logos you have to learn.
        let toml = kind_of("cozy.toml", false);
        for name in ["a.yaml", "b.yml", "c.json", "d.ini", "e.conf", "f.kdl"] {
            assert_eq!(kind_of(name, false), toml, "{name}");
        }
    }

    #[test]
    fn the_kinds_are_told_apart() {
        let kinds = [
            kind_of("a.toml", false).icon(),
            kind_of("b.fish", false).icon(),
            kind_of("c.rs", false).icon(),
            kind_of("d.md", false).icon(),
            kind_of("e.png", false).icon(),
            kind_of("f.zip", false).icon(),
        ];
        let mut unique = kinds.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), kinds.len(), "each kind needs its own glyph");
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(kind_of("Cargo.TOML", false), kind_of("cargo.toml", false));
        assert_eq!(kind_of("Makefile", false), kind_of("makefile", false));
    }

    #[test]
    fn whole_names_win_over_extensions() {
        // `.gitignore` has no extension as far as `Path` is concerned, so
        // without the name table it would be indistinguishable from any other
        // dotfile.
        assert_eq!(kind_of(".gitignore", false), Kind::Vcs);
        assert_ne!(kind_of("README.md", false), kind_of("notes.md", false));
    }

    #[test]
    fn an_unknown_file_gets_the_default_rather_than_a_wrong_guess() {
        assert_eq!(kind_of("mystery.zzzz", false), Kind::Plain);
        assert_eq!(kind_of("noextension", false), Kind::Plain);
        assert_eq!(kind_of(".zshrc", false), Kind::Plain);
    }

    #[test]
    fn every_glyph_is_in_the_font_awesome_block() {
        // The whole reason the table is small. Seti and Devicons moved
        // codepoints between Nerd Font v2 and v3; this block did not, and is in
        // every patched font there is. A glyph outside it is a box on somebody's
        // machine.
        let range = '\u{f000}'..='\u{f2ff}';
        let mut all: Vec<char> = BY_EXTENSION.iter().map(|(_, k)| k.icon()).collect();
        all.extend(BY_NAME.iter().map(|(_, k)| k.icon()));
        all.extend(sample());
        all.push(Kind::Dir.icon());
        all.push(Kind::Plain.icon());
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
