//! Which fish greeting the loadout installs, and how the wizard previews it.
//!
//! The marks live here rather than beside the drawing code because they are a
//! promise about the *generated config*, not about this screen — the guard test
//! that checks each one still appears in `templates/fish/config.fish` is what
//! keeps that promise, and it belongs next to what it guards.

/// Which fish greeting the loadout should install.
///
/// The three marked variants differ only in which Unicode block their glyphs
/// come from, and that is the whole point: a font either has them or draws
/// tofu, and no amount of describing it beats putting them on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Greeting {
    /// Symbols for Legacy Computing (the U+1FB00 block, Unicode 13). The
    /// sharpest mark and the least widely supported.
    Legacy,
    /// Geometric Shapes (U+25A0–U+25FF). Decades older than the block above,
    /// so a font that lacks those may well still have these.
    Geometric,
    /// Block Elements (U+2580–U+259F), which essentially every monospace font
    /// has had for decades.
    Blocks,
    /// No logo — just the line telling you how to detach.
    Text,
    /// Nothing at all: a silent shell.
    None,
}

impl Greeting {
    /// Order is best-looking first, then descending font support, then the two
    /// that have no mark at all. The sharpest mark leads even though its glyphs
    /// are the least widely available: this is the one screen where a font that
    /// cannot draw something says so plainly, and the next two options are
    /// right underneath for anyone whose font cannot.
    pub const ALL: [Greeting; 5] = [
        Greeting::Legacy,
        Greeting::Geometric,
        Greeting::Blocks,
        Greeting::Text,
        Greeting::None,
    ];

    /// Stable spelling for the state file and the renderer's `--greeting`.
    /// Not `Debug`, which is for programmers and free to change.
    pub fn key(self) -> &'static str {
        match self {
            Greeting::Legacy => "legacy",
            Greeting::Geometric => "geometric",
            Greeting::Blocks => "blocks",
            Greeting::Text => "text",
            Greeting::None => "none",
        }
    }

    /// The reverse. Anything unrecognised is `None` and the caller falls back
    /// to the default, which is what a hand-edited file should get.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Greeting::Legacy => "Default",
            Greeting::Geometric => "Geometric shapes",
            Greeting::Blocks => "Block elements",
            Greeting::Text => "No logo",
            Greeting::None => "Nothing at all",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Greeting::Legacy => "Needs a font with Symbols for Legacy Computing.",
            Greeting::Geometric => "Geometric Shapes — older, and more widely available.",
            Greeting::Blocks => "Block-drawing characters, which any font has.",
            Greeting::Text => "Just the line telling you how to detach.",
            Greeting::None => "A silent shell.",
        }
    }

    /// The mark, exactly as fish will print it. Empty for the two variants
    /// that have none.
    pub fn art(self) -> Vec<&'static str> {
        match self {
            Greeting::Legacy => vec!["▃🭕🭏🭕🭏 M I N I M A L"],
            Greeting::Geometric => vec![".◥◣◥◣ M I N I M A L"],
            Greeting::Blocks => vec![
                "   ████  ████▄",
                "▄▄▄ ▀███▄ ▀███▄",
                "▀███  ▀███  ▀███",
                "  M I N I M A L",
            ],
            Greeting::Text | Greeting::None => Vec::new(),
        }
    }

    /// Whether the "… to detach" line is printed under the mark.
    pub fn has_detach_line(self) -> bool {
        self != Greeting::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocky_mark_carries_the_wordmark_too() {
        let art = Greeting::Blocks.art();
        assert!(art.iter().any(|l| l.contains("████")), "the mark itself");
        assert_eq!(
            art.last().map(|l| l.trim()),
            Some("M I N I M A L"),
            "and the wordmark under it"
        );
    }

    #[test]
    fn every_greeting_matches_what_the_template_will_print() {
        // The preview is a promise about the generated config. If the template
        // stops carrying one of these marks, the promise is a lie.
        let template = include_str!("../../../../../templates/fish/config.fish");
        for g in Greeting::ALL {
            for line in g.art() {
                assert!(
                    template.contains(line),
                    "{g:?}: templates/fish/config.fish no longer contains {line:?}"
                );
            }
            assert!(
                template.contains(&format!("greeting == \"{}\"", g.key())) || g == Greeting::Blocks,
                "{g:?}: the template has no branch for {:?}",
                g.key()
            );
        }
    }

    #[test]
    fn the_geometric_mark_avoids_the_glyphs_the_legacy_one_needs() {
        // The point of offering it: Geometric Shapes (U+25A0-U+25FF) are
        // decades older than Symbols for Legacy Computing, so a font without
        // the latter may well still have these.
        let art = Greeting::Geometric.art()[0];
        for c in art.chars() {
            assert!(
                !('\u{1FB00}'..='\u{1FBFF}').contains(&c),
                "{c:?} is a Legacy Computing glyph, which this option exists to avoid"
            );
        }
        assert!(art.chars().any(|c| ('\u{25A0}'..='\u{25FF}').contains(&c)));
    }
}
