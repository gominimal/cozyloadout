//! The git screen: who you are to git inside a session, and whether GitHub's
//! SSH remotes are fetched over HTTPS.

use super::intro_paragraph;
#[allow(clippy::wildcard_imports)]
use super::prelude::*;

/// The rows, in the order they are drawn: two values, then three ticks.
const NAME: usize = 0;
const EMAIL: usize = 1;
const SET_NAME: usize = 2;
const SET_EMAIL: usize = 3;
const HTTPS: usize = 4;
pub const GIT_ROWS: usize = 5;

/// The git page.
///
/// Unlike the client and VM pages this configures the *loadout*: what is chosen
/// here becomes lines in the activation hook, under `build/`, and nothing on
/// this machine changes. The session has a home of its own, so its git starts
/// with no identity whatever this machine's has.
pub fn draw_git(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();
    let intro = intro_paragraph(
        "Who you are to git, inside a session.",
        "A session has its own home, so its git starts with no name or email. \
         Ticked values are set by the loadout's activation hook each time a \
         session is created; this machine's git config is not touched."
            .to_string(),
    );

    let [intro_area, body, notes] = Layout::vertical([
        Constraint::Length(THEME_INTRO_ROWS + 1),
        Constraint::Length(6),
        Constraint::Min(0),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    let mut lines: Vec<Line> = Vec::new();
    for row in 0..GIT_ROWS {
        let focused = row == app.git_row;
        let editing = focused && app.git_editing.is_some();
        let accent = if editing {
            t.orange
        } else if focused {
            t.blue
        } else {
            t.fg
        };
        let marker = Span::styled(
            if focused { "▸ " } else { "  " },
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        );
        let label_style = if focused {
            Style::default().fg(accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.fg)
        };
        if let Some((label, value)) = match row {
            NAME => Some(("Name", &app.git_name)),
            EMAIL => Some(("Email", &app.git_email)),
            _ => None,
        } {
            let shown = if editing {
                // A caret, so an emptied field still shows where typing lands.
                format!("{}_", app.git_editing.as_deref().unwrap_or(""))
            } else if value.is_empty() {
                "—".to_string()
            } else {
                value.clone()
            };
            lines.push(Line::from(vec![
                marker,
                Span::styled(format!("{label:<8}"), label_style),
                Span::styled(
                    shown,
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }
        let (on, about) = match row {
            SET_NAME => (app.git_set_name, "set user.name in every new session"),
            SET_EMAIL => (app.git_set_email, "set user.email in every new session"),
            _ => (
                app.git_https,
                // Short enough for sixty columns; the commands under the
                // list spell out both URLs.
                "fetch GitHub's SSH remotes over HTTPS",
            ),
        };
        lines.push(Line::from(vec![
            marker,
            Span::styled(
                if on { "[x] " } else { "[ ] " },
                Style::default()
                    .fg(if on { t.green } else { t.comment })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(about, label_style),
        ]));
        if row == SET_NAME {
            // Separates the values from the ticks without a heading.
            lines.insert(lines.len() - 1, Line::raw(""));
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), body);

    frame.render_widget(
        Paragraph::new(Text::from(git_notes(app, &t))).wrap(Wrap { trim: true }),
        notes,
    );
}

/// The strip under the page: anything that will not do what it looks like,
/// then the exact commands the hook will run — the page's answer, stated as
/// what it is rather than described.
pub fn git_notes(app: &App, t: &Theme) -> Vec<Line<'static>> {
    let warn = |s: String| Line::styled(s, Style::default().fg(t.orange));
    // A blank first row, so the notes do not read as a sixth option.
    let mut notes: Vec<Line> = vec![Line::raw("")];
    if app.git_set_name && app.git_name.trim().is_empty() {
        notes.push(warn(
            "user.name is ticked but empty, so it will not be set.".into(),
        ));
    }
    if app.git_set_email && app.git_email.trim().is_empty() {
        notes.push(warn(
            "user.email is ticked but empty, so it will not be set.".into(),
        ));
    } else if !app.git_email.trim().is_empty() && !app.git_email.contains('@') {
        // Warned, not refused: git takes any string, and it is the user's.
        notes.push(warn(format!(
            "`{}` does not look like an email address.",
            app.git_email.trim()
        )));
    }
    let script = app.git_setup().script();
    if script.is_empty() {
        notes.push(Line::styled(
            "Nothing ticked — the hook leaves git's identity alone.",
            Style::default().fg(t.comment),
        ));
    } else {
        notes.push(Line::styled(
            "The hook will run:",
            Style::default().fg(t.comment),
        ));
        for line in script.lines() {
            notes.push(Line::styled(
                line.trim().trim_end_matches(" || true").to_string(),
                Style::default().fg(t.green),
            ));
        }
    }
    notes
}

// --- keys -----------------------------------------------------------------

pub fn on_key_git(app: &mut App, key: KeyEvent) {
    if let Some(buffer) = app.git_editing.as_mut() {
        match key.code {
            KeyCode::Esc => app.git_editing = None,
            KeyCode::Enter => {
                let text = buffer.trim().to_string();
                // Typing a value is an unambiguous way of saying you want it
                // set, the same reasoning that has `e` on the patches page
                // choose the entry it edits. Emptying it is the reverse.
                let wanted = !text.is_empty();
                if app.git_row == NAME {
                    app.git_name = text;
                    app.git_set_name = wanted;
                } else {
                    app.git_email = text;
                    app.git_set_email = wanted;
                }
                app.git_editing = None;
            }
            KeyCode::Backspace => {
                buffer.pop();
            }
            // Control characters cannot be typed in: a name with a newline in
            // it is not a name, and `check_git_value` would refuse it anyway.
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() =>
            {
                buffer.push(c);
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => app.screen = Screen::Patches,
        KeyCode::Up | KeyCode::Char('k') => app.git_row = app.git_row.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            app.git_row = (app.git_row + 1).min(GIT_ROWS - 1);
        }
        KeyCode::Char(' ') => match app.git_row {
            NAME => app.git_editing = Some(app.git_name.clone()),
            EMAIL => app.git_editing = Some(app.git_email.clone()),
            SET_NAME => app.git_set_name = !app.git_set_name,
            SET_EMAIL => app.git_set_email = !app.git_set_email,
            HTTPS => app.git_https = !app.git_https,
            _ => {}
        },
        // Enter moves on, as on every other page; space changes the row.
        KeyCode::Enter => app.screen = Screen::Client,
        _ => {}
    }
}

// --- footer ---------------------------------------------------------------

/// The keys this screen answers to, for the footer.
pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.git_editing.is_some() {
        return vec![
            ("type", "the value"),
            ("enter", "accept"),
            ("esc", "cancel"),
        ];
    }
    vec![
        ("↑/↓", "move"),
        ("space", "change"),
        ("esc", "back"),
        ("enter", "done"),
    ]
}
