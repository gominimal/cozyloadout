//! The scheme-collection screen: offer to clone or update, then report.

use super::intro_paragraph;
#[allow(clippy::wildcard_imports)]
use super::prelude::*;

pub fn draw_schemes(frame: &mut Frame, inner: Rect, app: &App) {
    // Once the fetch has settled the question has been answered, so stop asking
    // it. Leaving `fetch_kind.question()` up read as "Update ... now?" directly
    // above "Done.", with an explanation underneath describing something that
    // has already happened.
    let settled = matches!(app.fetch, Fetch::Done(_) | Fetch::Failed(_));
    let (heading, detail) = match &app.fetch {
        Fetch::Done(_) => ("The scheme collection is ready.", String::new()),
        Fetch::Failed(_) => ("The scheme collection was not fetched.", String::new()),
        _ => (
            app.fetch_kind.question(),
            app.fetch_kind.detail(&app.schemes_dir),
        ),
    };

    let [intro_area, _, status_area] = Layout::vertical([
        Constraint::Length(if settled { 2 } else { SCHEMES_INTRO_ROWS }),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    frame.render_widget(intro_paragraph(heading, detail), intro_area);

    let dim = Style::default().fg(Color::DarkGray);
    let status = match &app.fetch {
        Fetch::Idle if app.fetch_kind == FetchKind::Blocked => vec![Line::styled(
            "Nothing will be downloaded. Press enter to continue.",
            dim,
        )],
        Fetch::Idle => vec![choice_line(app.fetch_yes)],
        Fetch::Running(frame_no, _) => {
            const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
            let tick = SPINNER[(*frame_no as usize / 2) % SPINNER.len()];
            vec![Line::styled(
                format!("{tick} running git…"),
                Style::default().fg(Color::Cyan),
            )]
        }
        Fetch::Done(output) => {
            let mut lines = vec![Line::styled(
                "Done.",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )];
            lines.extend(
                output
                    .lines()
                    .take(4)
                    .map(|l| Line::styled(l.to_string(), dim)),
            );
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                CONTINUE_HINT,
                Style::default().fg(Color::Cyan),
            ));
            lines
        }
        Fetch::Failed(why) => {
            let mut lines = vec![Line::styled(
                "git failed.",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )];
            lines.extend(
                why.lines()
                    .take(4)
                    .map(|l| Line::styled(l.to_string(), dim)),
            );
            lines.push(Line::raw(""));
            // Failing is not fatal: the two schemes in this repo still work, so
            // the way forward is the same key.
            lines.push(Line::styled(
                "Press enter to continue without it.",
                Style::default().fg(Color::Yellow),
            ));
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(Text::from(status)).wrap(Wrap { trim: true }),
        status_area,
    );
}

/// Shown once the fetch has settled. The footer carries the same key, but a
/// full-screen UI that has just finished a long-running job should say what to
/// do next where the user is already looking.
pub const CONTINUE_HINT: &str = "Press enter to continue.";

/// The yes/no row, with the active choice picked out.
pub fn choice_line(yes: bool) -> Line<'static> {
    let on = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let off = Style::default().fg(Color::DarkGray);
    Line::from(vec![
        Span::styled("  Yes  ", if yes { on } else { off }),
        Span::raw("   "),
        Span::styled("  No  ", if yes { off } else { on }),
    ])
}

// --- keys -----------------------------------------------------------------

pub fn on_key_schemes(app: &mut App, key: KeyEvent) {
    let can_fetch = app.fetch_kind != FetchKind::Blocked;
    match key.code {
        // Back to the greeting, so a mis-press is not a dead end.
        KeyCode::Esc => {
            app.screen = Screen::Greeting;
            app.fetch = Fetch::Idle;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') | KeyCode::Tab if can_fetch => {
            app.fetch_yes = !app.fetch_yes;
        }
        KeyCode::Char('y' | 'Y') if can_fetch => app.fetch_yes = true,
        KeyCode::Char('n' | 'N') => app.fetch_yes = false,
        KeyCode::Enter | KeyCode::Char(' ') => match app.fetch {
            // A finished fetch: enter moves on rather than re-running it.
            Fetch::Done(_) | Fetch::Failed(_) => super::themes::enter_themes(app),
            _ if app.fetch_yes && can_fetch => {
                app.fetch = spawn_fetch(app.fetch_kind, &app.schemes_dir);
            }
            _ => super::themes::enter_themes(app),
        },
        _ => {}
    }
}

// --- footer ---------------------------------------------------------------

/// The keys this screen answers to, for the footer.
pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    // A finished fetch has nothing left to answer; the only thing to do
    // is read the result and move on.
    if matches!(app.fetch, Fetch::Done(_) | Fetch::Failed(_)) {
        return vec![("enter", "continue"), ("q", "quit")];
    }
    vec![
        ("←/→", "yes/no"),
        ("enter", "confirm"),
        ("esc", "back"),
        ("q", "quit"),
    ]
}
