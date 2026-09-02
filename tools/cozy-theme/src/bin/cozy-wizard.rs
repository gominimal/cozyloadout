//! cozy-wizard — interactive setup for the cozy loadout.
//!
//! Run it with `just wizard`. See AGENTS.md for the build pipeline.

use color_eyre::eyre::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

/// Which fish greeting the loadout should install.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Greeting {
    /// `▃🭕🭏🭕🭏 M I N I M A L` — the mark drawn with Symbols for Legacy
    /// Computing (the U+1FB00 block, Unicode 13). Sharper, but a font without
    /// those glyphs shows tofu, which is the whole reason this is a choice.
    Legacy,
    /// The block-element mark that ships today. Only U+2580–U+259F, which
    /// essentially every monospace font has had for decades.
    Blocks,
}

impl Greeting {
    const ALL: [Greeting; 2] = [Greeting::Legacy, Greeting::Blocks];

    fn label(self) -> &'static str {
        match self {
            Greeting::Legacy => " Newer symbols ",
            Greeting::Blocks => " Block elements ",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Greeting::Legacy => "Needs a font with Symbols for Legacy Computing.",
            Greeting::Blocks => "Works in any font with block-drawing characters.",
        }
    }

    /// The mark exactly as fish will print it, minus the colour.
    fn art(self) -> Vec<&'static str> {
        match self {
            // One line, because that is how it renders: a font missing the
            // glyphs shows tofu or blanks right here, which is the point.
            Greeting::Legacy => vec!["▃🭕🭏🭕🭏 M I N I M A L"],
            Greeting::Blocks => vec!["   ████  ████▄", "▄▄▄ ▀███▄ ▀███▄", "▀███  ▀███  ▀███"],
        }
    }

    /// Rows the art needs, plus the block's borders and `pad_y` above and
    /// below. The caption is not counted: it rides on the bottom border. Get
    /// this wrong and the box clips its own contents.
    fn height(self, pad_y: u16) -> u16 {
        // The cast is over a 1- or 3-element literal array; it cannot overflow.
        u16::try_from(self.art().len()).unwrap_or(u16::MAX) + 2 + pad_y * 2
    }
}

/// Columns of padding inside every bordered box, so text is not jammed against
/// the border.
///
const BOX_PADDING_X: u16 = 2;

/// Blank rows between one option box and the next.
const BOX_GAP: u16 = 1;

/// Rows of padding above and below the art inside an option box.
///
/// Rows are the scarce dimension. There is not room at 80x24 for this *and* a
/// caption line inside each box — the first attempt at it clipped the second
/// box. The captions pay for it by riding on the bottom border
/// (`title_bottom`), which costs no rows at all.
const BOX_PADDING_Y: u16 = 1;

/// Rows reserved for the welcome text.
///
/// A fixed height, not a measurement: ratatui only exposes
/// `Paragraph::line_count` behind an unstable feature, and taking an unstable
/// API for a layout nicety is not worth it. Five rows is the heading, a blank,
/// and up to three wrapped lines, which covers the intro from 50 columns up —
/// asserted by `intro_fits_in_its_rows`, because guessing it wrong silently
/// eats the end of the sentence. Used as a `Length` rather than a `Min` so the
/// slack lands in the trailing `Min(0)` instead of opening a gap above the
/// options.
const INTRO_ROWS: u16 = 5;

struct App {
    selected: usize,
    /// Set when the user presses enter. `None` means they quit without
    /// choosing, which has to stay distinguishable from picking the default.
    chosen: Option<Greeting>,
    done: bool,
}

impl App {
    fn new() -> Self {
        Self {
            selected: 0,
            chosen: None,
            done: false,
        }
    }

    fn current(&self) -> Greeting {
        Greeting::ALL[self.selected]
    }

    fn on_key(&mut self, key: KeyEvent) {
        // Windows reports press *and* release; acting on both moves twice per
        // keystroke.
        if key.kind != KeyEventKind::Press {
            return;
        }
        let quit = matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'));
        if quit {
            self.done = true;
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(Greeting::ALL.len() - 1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.chosen = Some(self.current());
                self.done = true;
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    // Order matters: `ratatui::init` installs a panic hook that restores the
    // terminal, and its own docs say it has to go on *after* any other hook, so
    // color_eyre's has to be installed first. Get this backwards and a panic
    // leaves the terminal in raw mode on the alternate screen.
    color_eyre::install()?;

    let terminal = ratatui::init();
    let result = run(terminal);
    // Unconditional, and before `?`: a run that ends in an error still has to
    // hand the terminal back before the report is printed, or the report lands
    // on the alternate screen and vanishes with it.
    ratatui::restore();

    match result? {
        Some(choice) => println!("greeting: {choice:?}"),
        None => println!("cancelled"),
    }
    Ok(())
}

fn run(mut terminal: DefaultTerminal) -> Result<Option<Greeting>> {
    let mut app = App::new();
    while !app.done {
        terminal.draw(|frame| draw(frame, &app))?;
        // Blocking read. Nothing animates, so waking on a timer would only burn
        // power; add a poll when something needs one.
        if let Event::Key(key) = event::read()? {
            app.on_key(key);
        }
    }
    Ok(app.chosen)
}

fn draw(frame: &mut Frame, app: &App) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(Padding::horizontal(BOX_PADDING_X))
        .title(" cozy wizard ");
    let inner = outer.inner(body);
    frame.render_widget(outer, body);

    let intro = Paragraph::new(Text::from(vec![
        Line::styled(
            "Welcome to the minimal cozy loadout wizard.",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::raw(
            "Pick the greeting that renders correctly here. If one shows \
             boxes or gaps, this font lacks those glyphs.",
        ),
    ]))
    .wrap(Wrap { trim: true });

    // Both the padding inside the boxes and the gap between them are comforts
    // that give way when rows run short — in that order, since the padding is
    // worth more than the gap. What never gives way is the art: at 50x18 an
    // unconditional padding clipped the block mark to a single line, and the
    // mark is the thing this screen exists to show. There is no spacer row
    // above the first box either; each box carries its own top padding.
    //
    // Ordered most- to least-generous; the first that fits wins.
    let options = u16::try_from(Greeting::ALL.len()).unwrap_or(1);
    let needed = |pad_y: u16, gap: u16| {
        INTRO_ROWS
            + Greeting::ALL.iter().map(|g| g.height(pad_y)).sum::<u16>()
            + gap * (options - 1)
    };
    let (pad_y, gap) = [
        (BOX_PADDING_Y, BOX_GAP),
        (BOX_PADDING_Y, 0),
        (0, BOX_GAP),
        (0, 0),
    ]
    .into_iter()
    .find(|&(pad_y, gap)| needed(pad_y, gap) <= inner.height)
    .unwrap_or((0, 0));

    let mut constraints = vec![Constraint::Length(INTRO_ROWS)];
    let mut option_rows = Vec::with_capacity(Greeting::ALL.len());
    for (i, greeting) in Greeting::ALL.iter().enumerate() {
        if i > 0 && gap > 0 {
            constraints.push(Constraint::Length(gap));
        }
        option_rows.push(constraints.len());
        constraints.push(Constraint::Length(greeting.height(pad_y)));
    }
    constraints.push(Constraint::Min(0));
    let areas = Layout::vertical(constraints).split(inner);

    frame.render_widget(intro, areas[0]);
    for (i, greeting) in Greeting::ALL.iter().enumerate() {
        draw_option(
            frame,
            areas[option_rows[i]],
            *greeting,
            pad_y,
            i == app.selected,
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            key_hint("↑/↓"),
            Span::raw(" move  "),
            key_hint("enter"),
            Span::raw(" choose  "),
            key_hint("q"),
            Span::raw(" quit"),
        ]))
        .alignment(Alignment::Center),
        footer,
    );
}

fn draw_option(frame: &mut Frame, area: Rect, greeting: Greeting, pad_y: u16, selected: bool) {
    let accent = if selected {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .padding(Padding::symmetric(BOX_PADDING_X, pad_y))
        .title_bottom(Span::styled(
            format!(" {} ", greeting.note()),
            Style::default().fg(Color::DarkGray),
        ))
        .title(Span::styled(
            greeting.label(),
            if selected {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(accent)
            },
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The art is deliberately unstyled: it renders in the terminal's own
    // foreground, which is what the user is being asked to judge.
    let mut lines: Vec<Line> = greeting.art().into_iter().map(Line::raw).collect();
    lines.push(Line::styled(
        greeting.note(),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn key_hint(key: &str) -> Span<'_> {
    Span::styled(key, Style::default().add_modifier(Modifier::BOLD))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    #[test]
    fn quits_on_q_esc_and_ctrl_c() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let mut app = App::new();
            app.on_key(press(code));
            assert!(app.done, "{code:?} should end the loop");
            assert_eq!(app.chosen, None, "quitting is not a choice");
        }
        let mut app = App::new();
        app.on_key(KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        ));
        assert!(app.done);
        assert_eq!(app.chosen, None);
    }

    #[test]
    fn selection_moves_and_clamps() {
        let mut app = App::new();
        assert_eq!(app.current(), Greeting::Legacy);
        // Up at the top stays put rather than wrapping or underflowing.
        app.on_key(press(KeyCode::Up));
        assert_eq!(app.current(), Greeting::Legacy);
        app.on_key(press(KeyCode::Down));
        assert_eq!(app.current(), Greeting::Blocks);
        // Down at the bottom stays put rather than running off the end.
        app.on_key(press(KeyCode::Down));
        assert_eq!(app.current(), Greeting::Blocks);
        app.on_key(press(KeyCode::Char('k')));
        assert_eq!(app.current(), Greeting::Legacy);
    }

    #[test]
    fn enter_records_the_highlighted_option() {
        let mut app = App::new();
        app.on_key(press(KeyCode::Down));
        app.on_key(press(KeyCode::Enter));
        assert!(app.done);
        assert_eq!(app.chosen, Some(Greeting::Blocks));
    }

    #[test]
    fn release_and_repeat_are_ignored() {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let mut app = App::new();
            app.on_key(KeyEvent::new_with_kind(
                KeyCode::Down,
                KeyModifiers::NONE,
                kind,
            ));
            assert_eq!(app.current(), Greeting::Legacy, "{kind:?} should not move");
        }
    }

    #[test]
    fn blocks_art_uses_only_widely_supported_characters() {
        // The point of offering a choice at all: the block-element greeting has
        // to stay inside U+2580–U+259F, which any monospace font has. If a
        // newer symbol creeps in, the "safe" option stops being safe.
        for line in Greeting::Blocks.art() {
            for c in line.chars() {
                assert!(
                    c == ' ' || ('\u{2580}'..='\u{259F}').contains(&c),
                    "{c:?} (U+{:04X}) is outside Block Elements",
                    c as u32
                );
            }
        }
    }

    /// Render a frame and return it as plain rows of text.
    fn render(w: u16, h: u16) -> Vec<String> {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let app = App::new();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let buf = terminal.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    #[test]
    fn intro_fits_in_its_rows() {
        // INTRO_ROWS is a hand-picked constant, so the failure it guards
        // against is silent: the intro wraps to one line more than fits and the
        // end of the sentence simply vanishes. Two earlier guesses did exactly
        // that. Assert the last word survives across the widths a terminal is
        // plausibly at.
        for w in [50u16, 60, 72, 80, 100, 120] {
            let rows = render(w, 30);
            let text = rows.join(" ");
            assert!(
                text.contains("glyphs."),
                "intro truncated at {w} columns — INTRO_ROWS is too small:\n{}",
                rows[..8].join("\n")
            );
        }
    }

    #[test]
    fn both_greetings_are_shown_together() {
        // The user is comparing them, so neither may be scrolled off or
        // clipped away at an ordinary terminal size.
        let text = render(80, 24).join(" ");
        assert!(text.contains("▃"), "legacy mark missing");
        assert!(text.contains("🭕"), "legacy symbols missing");
        for line in Greeting::Blocks.art() {
            assert!(text.contains(line.trim()), "blocks mark missing {line:?}");
        }
    }

    #[test]
    fn art_survives_a_small_terminal() {
        // The padding is allowed to disappear when rows run short; the art is
        // not. Adding vertical padding without this fallback clipped the block
        // mark to one line at 50x18.
        for (w, h) in [(50u16, 18u16), (60, 20), (80, 24), (120, 40)] {
            let text = render(w, h).join(" ");
            for line in Greeting::Blocks.art() {
                assert!(
                    text.contains(line.trim()),
                    "block mark line {line:?} missing at {w}x{h}"
                );
            }
            assert!(text.contains("🭕"), "legacy mark missing at {w}x{h}");
        }
    }

    /// Index of the row carrying a box's top border.
    fn box_top(rows: &[String], title: &str) -> usize {
        rows.iter()
            .position(|r| r.contains(title))
            .expect("box not drawn")
    }

    #[test]
    fn gap_separates_the_options_when_there_is_room() {
        let rows = render(80, 24);
        let first_bottom = box_top(&rows, "Needs a font");
        let second_top = box_top(&rows, "Block elements");
        assert_eq!(
            second_top - first_bottom,
            2,
            "expected one blank row between the boxes:\n{}",
            rows[first_bottom..=second_top].join("\n")
        );
    }

    #[test]
    fn comforts_yield_before_the_art_does() {
        // The ladder: both at 80x24, gap dropped before padding at 60x20, and
        // at 50x18 whatever it takes to keep three lines of block mark. The
        // exact sizes matter less than the ordering — the art is last to go,
        // which `art_survives_a_small_terminal` pins from the other side.
        let wide = render(80, 24).join(" ");
        assert!(wide.contains("Works in any font"), "caption lost at 80x24");
        for (w, h) in [(50u16, 18u16), (60, 20), (80, 24)] {
            let text = render(w, h).join(" ");
            for line in Greeting::Blocks.art() {
                assert!(text.contains(line.trim()), "art lost at {w}x{h}");
            }
        }
    }

    #[test]
    fn padding_is_present_when_there_is_room() {
        // ...and it really is there at a normal size, so the fallback above
        // cannot quietly become the only path.
        let rows = render(80, 24);
        let top = rows
            .iter()
            .position(|r| r.contains("Newer symbols"))
            .unwrap();
        assert!(
            rows[top + 1]
                .trim_matches(|c| c == '│' || c == ' ')
                .is_empty(),
            "expected a blank padding row under the box title:\n{}",
            rows[top + 1]
        );
    }

    #[test]
    fn blocks_art_matches_the_shipped_fish_greeting() {
        // The wizard is asking the user to judge the *real* greeting. If the
        // template's mark is edited and this copy is not, the preview becomes a
        // lie — so pin it to the template.
        let template = include_str!("../../../../templates/fish/config.fish");
        for line in Greeting::Blocks.art() {
            assert!(
                template.contains(line),
                "templates/fish/config.fish no longer contains {line:?}"
            );
        }
    }

    #[test]
    #[ignore = "prints frames for eyeballing; run with --ignored --nocapture"]
    fn dump_frames() {
        for (w, h) in [(50u16, 18u16), (60, 20), (80, 24), (100, 30)] {
            println!("\n=== {w}x{h} ===");
            for row in render(w, h) {
                println!("|{}|", row.trim_end());
            }
        }
    }
}
