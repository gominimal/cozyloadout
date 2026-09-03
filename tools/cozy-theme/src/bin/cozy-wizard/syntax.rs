//! Syntax highlighting for the patches preview.
//!
//! Lit from the `.tmTheme` this repository already generates for the selected
//! scheme, not from a palette invented here — so a previewed file is coloured
//! exactly as `bat` will colour it in the session. That is the same honesty the
//! theme browser has: what you see is what you get, rather than an
//! approximation of it.
//!
//! Everything expensive is loaded once. syntect's bundled syntax set is a few
//! megabytes of deserialised state, and the theme has to be parsed out of XML;
//! doing either on a cursor move would be felt.

use ratatui::style::Color;
use std::path::Path;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// The grammars, deserialised once for the process.
///
/// `bat`'s set rather than syntect's own: the bundled 75 have no TOML grammar,
/// and TOML is most of what this page shows.
fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// A parsed theme, and the scheme it came from.
pub struct Highlighter {
    /// The slug the theme was built for, so a scheme change is detectable
    /// without re-parsing XML on every frame.
    pub slug: String,
    theme: Theme,
}

impl Highlighter {
    /// Build one from a scheme, by rendering the loadout's own `.tmTheme`.
    ///
    /// Returns `None` when the template cannot be rendered or the result cannot
    /// be parsed: highlighting is a nicety, and losing it should cost the
    /// colour, not the preview.
    pub fn new(scheme: &cozy_theme::Scheme, templates: &Path) -> Option<Self> {
        let xml = scheme
            .render_template(templates, "bat/themes/theme.tmTheme")
            .ok()?;
        let theme = ThemeSet::load_from_reader(&mut std::io::Cursor::new(xml.as_bytes())).ok()?;
        Some(Self {
            slug: scheme.slug.clone(),
            theme,
        })
    }

    /// Colour `lines`, choosing a grammar from the file's name.
    ///
    /// Lines are highlighted as a block rather than individually because the
    /// parser carries state across them — a string opened on one line stays
    /// open on the next, and per-line parsing would lose that.
    pub fn lines(&self, lines: &[String], name: &str) -> Vec<Vec<(Color, String)>> {
        let set = syntaxes();
        let Some(syntax) = grammar_for(set, name) else {
            return plain(lines);
        };
        let mut h = HighlightLines::new(syntax, &self.theme);
        let joined = lines.join("\n") + "\n";
        let mut out = Vec::with_capacity(lines.len());
        for line in LinesWithEndings::from(&joined) {
            let Ok(spans) = h.highlight_line(line, set) else {
                return plain(lines);
            };
            out.push(
                spans
                    .into_iter()
                    .map(|(style, text)| {
                        let c = style.foreground;
                        (
                            Color::Rgb(c.r, c.g, c.b),
                            text.trim_end_matches('\n').to_string(),
                        )
                    })
                    .filter(|(_, text)| !text.is_empty())
                    .collect(),
            );
        }
        out
    }
}

/// The grammar for a filename, by extension and then by the whole name.
///
/// The name matters as much as the extension here: the files this pane shows
/// are dotfiles, and `.zshrc`, `Makefile` and `config.fish` are not reached by
/// an extension lookup alone.
fn grammar_for<'a>(
    set: &'a SyntaxSet,
    name: &str,
) -> Option<&'a syntect::parsing::SyntaxReference> {
    let ext = Path::new(name).extension().and_then(|e| e.to_str());
    ext.and_then(|e| set.find_syntax_by_extension(e))
        .or_else(|| set.find_syntax_by_extension(name))
        .or_else(|| set.find_syntax_by_token(name.trim_start_matches('.')))
}

/// One uncoloured span per line, for anything with no grammar. `Color::Reset`
/// so the caller's own foreground shows through rather than a guess.
fn plain(lines: &[String]) -> Vec<Vec<(Color, String)>> {
    lines
        .iter()
        .map(|l| vec![(Color::Reset, l.clone())])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheme() -> cozy_theme::Scheme {
        cozy_theme::Scheme::load(Path::new("../../schemes/minimal-dark.yaml")).unwrap()
    }

    fn highlighter() -> Highlighter {
        Highlighter::new(&scheme(), Path::new("../../templates"))
            .expect("the loadout's own tmTheme should build a theme")
    }

    #[test]
    fn a_theme_is_built_from_the_loadouts_own_tm_theme() {
        // The property that makes the preview honest: these are the colours
        // `bat` gets, because it is the same file.
        let h = highlighter();
        assert_eq!(h.slug, "minimal-dark");
    }

    #[test]
    fn toml_is_coloured_and_a_comment_differs_from_a_value() {
        let h = highlighter();
        let out = h.lines(
            &["# a comment".to_string(), "key = \"value\"".to_string()],
            "config.toml",
        );
        assert_eq!(out.len(), 2);
        let comment = out[0][0].0;
        let colours: Vec<Color> = out[1].iter().map(|(c, _)| *c).collect();
        assert!(
            colours.iter().any(|c| *c != comment),
            "a comment and a value should not be the same colour: {colours:?}"
        );
    }

    #[test]
    fn the_text_survives_highlighting_exactly() {
        // Spans are drawn in order; losing or reordering a character would be
        // a lie about the file's contents.
        let h = highlighter();
        let src = vec![
            "[table]".to_string(),
            "n = 42 # trailing".to_string(),
            String::new(),
            "  indented = true".to_string(),
        ];
        let out = h.lines(&src, "x.toml");
        assert_eq!(out.len(), src.len());
        for (spans, original) in out.iter().zip(&src) {
            let joined: String = spans.iter().map(|(_, t)| t.as_str()).collect();
            assert_eq!(&joined, original);
        }
    }

    #[test]
    fn a_dotfile_with_no_extension_still_finds_a_grammar() {
        // `.zshrc` and friends are the common case on this page; an
        // extension-only lookup would leave them all plain.
        let set = syntaxes();
        for name in [".zshrc", ".bashrc", "Makefile"] {
            assert!(grammar_for(set, name).is_some(), "{name}");
        }
    }

    #[test]
    fn fish_and_json_and_yaml_are_all_known() {
        let set = syntaxes();
        for name in ["config.fish", "a.json", "b.yaml", "c.yml", "d.rs", "e.sh"] {
            assert!(grammar_for(set, name).is_some(), "{name}");
        }
    }

    #[test]
    fn an_unknown_extension_comes_back_plain_rather_than_empty() {
        let h = highlighter();
        let out = h.lines(&["some text".to_string()], "mystery.zzzz");
        assert_eq!(out.len(), 1);
        let joined: String = out[0].iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(joined, "some text");
    }

    #[test]
    fn state_carries_across_lines() {
        // A string opened on one line is still open on the next. Highlighting
        // line by line in isolation would lose that and mis-colour the rest.
        let h = highlighter();
        let out = h.lines(
            &[
                "x = \"\"\"".to_string(),
                "still inside the string".to_string(),
                "\"\"\"".to_string(),
            ],
            "a.toml",
        );
        assert_eq!(out.len(), 3);
        let inside = out[1][0].0;
        let opener_last = out[0].last().unwrap().0;
        assert_eq!(
            inside, opener_last,
            "the second line should still be string-coloured"
        );
    }

    #[test]
    fn an_empty_line_does_not_vanish() {
        // The pane draws one row per returned line; dropping the empty ones
        // would shift everything below them up.
        let h = highlighter();
        let out = h.lines(&["a = 1".into(), String::new(), "b = 2".into()], "a.toml");
        assert_eq!(out.len(), 3);
    }

    #[test]
    #[ignore = "prints the bundled grammar list"]
    fn list_bundled_syntaxes() {
        let set = syntaxes();
        let mut names: Vec<String> = set
            .syntaxes()
            .iter()
            .map(|s| format!("{} [{}]", s.name, s.file_extensions.join(" ")))
            .collect();
        names.sort();
        println!("{} grammars:\n{}", names.len(), names.join("\n"));
    }

    #[test]
    #[ignore = "measures rather than asserts"]
    fn time_the_expensive_parts() {
        use std::time::Instant;
        let t = Instant::now();
        let n = syntaxes().syntaxes().len();
        println!("grammar set: {n} grammars in {:?}", t.elapsed());

        let t = Instant::now();
        let h = highlighter();
        println!("theme from the loadout's tmTheme: {:?}", t.elapsed());

        let lines: Vec<String> = (0..15).map(|i| format!("key{i} = {i}")).collect();
        let t = Instant::now();
        for _ in 0..100 {
            let _ = h.lines(&lines, "a.toml");
        }
        println!("highlighting 15 lines: {:?} each", t.elapsed() / 100);
    }
}
