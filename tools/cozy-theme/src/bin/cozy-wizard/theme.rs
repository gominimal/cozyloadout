//! The palette the wizard paints itself with, and the syntax sample it shows.
//!
//! Every colour the interface uses comes from the scheme currently under the
//! cursor, which is what makes the theme preview honest: if a scheme has
//! unreadable comments, they are unreadable *here*, in the wizard, and not only
//! later in a session.

use cozy_theme::{mix, Rgb, Scheme};
use ratatui::style::Color;

/// A loaded scheme mapped onto the roles this UI paints with.
///
/// base16 assigns meaning to the slots, so this is a mapping rather than a
/// choice: base00–base03 are the greyscale surface from background up, base04–
/// base07 the foreground ramp, base08–base0F the accents. Everything the
/// preview draws comes from here, which is what makes the preview honest — if
/// a scheme has unreadable comments, they are unreadable here too.
pub struct Theme {
    pub bg: Color,
    pub surface: Color,
    pub selection: Color,
    pub comment: Color,
    pub fg: Color,
    pub bright: Color,
    pub red: Color,
    pub orange: Color,
    pub yellow: Color,
    pub green: Color,
    pub cyan: Color,
    pub blue: Color,
    pub magenta: Color,
    /// delta's diff backgrounds: the accent blended into base00, exactly as
    /// `templates/delta/delta.gitconfig` computes them.
    pub minus_bg: Color,
    pub plus_bg: Color,
}

pub fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

impl Theme {
    pub fn from_scheme(scheme: &Scheme) -> Self {
        let slot = |name: &str| scheme.palette[name];
        Self {
            bg: rgb(slot("base00")),
            surface: rgb(slot("base01")),
            selection: rgb(slot("base02")),
            comment: rgb(slot("base03")),
            fg: rgb(slot("base05")),
            bright: rgb(slot("base07")),
            red: rgb(slot("base08")),
            orange: rgb(slot("base09")),
            yellow: rgb(slot("base0A")),
            green: rgb(slot("base0B")),
            cyan: rgb(slot("base0C")),
            blue: rgb(slot("base0D")),
            magenta: rgb(slot("base0E")),
            minus_bg: rgb(mix(slot("base08"), slot("base00"), 15.0)),
            plus_bg: rgb(mix(slot("base0B"), slot("base00"), 15.0)),
        }
    }

    /// The wizard's own colours when no scheme is loaded yet, so the chrome
    /// does not flash on the first frame.
    pub fn fallback() -> Self {
        let g = Color::DarkGray;
        Self {
            bg: Color::Reset,
            surface: Color::Reset,
            selection: g,
            comment: g,
            fg: Color::Reset,
            bright: Color::White,
            red: Color::Red,
            orange: Color::Yellow,
            yellow: Color::Yellow,
            green: Color::Green,
            cyan: Color::Cyan,
            blue: Color::Blue,
            magenta: Color::Magenta,
            minus_bg: Color::Reset,
            plus_bg: Color::Reset,
        }
    }
}

/// One span of the sample code, tagged with the role that colours it.
///
/// Real syntax highlighting would mean syntect and a grammar; this is a fixed
/// snippet, so the spans are written out by hand. That keeps the preview
/// dependency-free and, more usefully, exercises the same slots the helix and
/// bat themes actually assign — see AGENTS.md's per-tool notes.
pub enum Tok {
    Kw,
    Fn,
    Str,
    Num,
    Comment,
    Type,
    Plain,
}

pub const SAMPLE_CODE: &[&[(Tok, &str)]] = &[
    &[(Tok::Comment, "// Blend an accent into the surface.")],
    &[
        (Tok::Kw, "pub fn "),
        (Tok::Fn, "mix"),
        (Tok::Plain, "("),
        (Tok::Plain, "fg"),
        (Tok::Plain, ": "),
        (Tok::Type, "Rgb"),
        (Tok::Plain, ", pct: "),
        (Tok::Type, "f64"),
        (Tok::Plain, ") -> "),
        (Tok::Type, "Rgb"),
        (Tok::Plain, " {"),
    ],
    &[
        (Tok::Plain, "    "),
        (Tok::Kw, "let "),
        (Tok::Plain, "f = pct / "),
        (Tok::Num, "100.0"),
        (Tok::Plain, ";"),
    ],
    &[
        (Tok::Plain, "    "),
        (Tok::Kw, "if "),
        (Tok::Plain, "name == "),
        (Tok::Str, "\"base0D\""),
        (Tok::Plain, " { "),
        (Tok::Kw, "return "),
        (Tok::Plain, "fg; }"),
    ],
    &[(Tok::Plain, "}")],
];

impl Tok {
    pub fn color(&self, t: &Theme) -> Color {
        match self {
            Tok::Kw => t.magenta,
            Tok::Fn => t.blue,
            Tok::Str => t.green,
            Tok::Num => t.orange,
            Tok::Comment => t.comment,
            Tok::Type => t.yellow,
            Tok::Plain => t.fg,
        }
    }
}
