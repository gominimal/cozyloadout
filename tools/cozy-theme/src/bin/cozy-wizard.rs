//! cozy-wizard — interactive setup for the cozy loadout.
//!
//! Scaffolding only: it starts, draws one frame, and quits. The scheme picker
//! and the rest go on top of this loop.
//!
//! Run it with `just wizard`. See AGENTS.md for the build pipeline.

use color_eyre::eyre::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};

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
    result
}

fn run(mut terminal: DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(draw)?;
        // Blocking read. There is no animation and nothing polls, so waking on
        // a timer would only burn power; add a poll when something needs one.
        if let Event::Key(key) = event::read()? {
            if should_quit(key) {
                return Ok(());
            }
        }
    }
}

/// `q`, `Esc`, or `Ctrl-C`. `KeyEventKind` is checked because Windows reports
/// press *and* release, so an unfiltered match fires twice per keystroke.
fn should_quit(key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press {
        return false;
    }
    matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
}

fn draw(frame: &mut Frame) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    frame.render_widget(
        Paragraph::new("Nothing here yet.")
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" cozy wizard "),
            ),
        body,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" quit"),
        ]))
        .alignment(Alignment::Center),
        footer,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, mods, kind)
    }

    #[test]
    fn quits_on_q_esc_and_ctrl_c() {
        let press = KeyEventKind::Press;
        assert!(should_quit(key(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            press
        )));
        assert!(should_quit(key(KeyCode::Esc, KeyModifiers::NONE, press)));
        assert!(should_quit(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            press
        )));
    }

    #[test]
    fn ignores_other_keys() {
        let press = KeyEventKind::Press;
        assert!(!should_quit(key(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            press
        )));
        assert!(!should_quit(key(KeyCode::Enter, KeyModifiers::NONE, press)));
        // Plain `c` is not a quit; only Ctrl-C is.
        assert!(!should_quit(key(
            KeyCode::Char('c'),
            KeyModifiers::NONE,
            press
        )));
    }

    #[test]
    fn ignores_release_and_repeat() {
        // Windows reports press *and* release. Acting on both would quit twice
        // per keystroke — harmless here, but not once a keypress does work.
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            assert!(
                !should_quit(key(KeyCode::Char('q'), KeyModifiers::NONE, kind)),
                "{kind:?} should not quit"
            );
        }
    }
}
