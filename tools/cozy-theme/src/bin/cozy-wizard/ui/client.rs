//! The client screen: the session-key chords, which configure minimal
//! rather than the loadout.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;
use super::{intro_paragraph, shorten_home};

/// The client page: the session-key chords, which live in minimal's own
/// config rather than in the loadout.
///
/// The page is emphatic about that boundary. Everything else the wizard
/// collects ends up in `build/`, reversible by deleting it; these four rows end
/// up in a file the user may have written by hand, and one of them decides
/// whether they can get out of a session at all.
pub fn draw_client(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();
    let intro = intro_paragraph(
        "How you get out of a session.",
        format!(
            "Detaching is a two-key gesture: the leader, then the detach key. \
             These are minimal's own settings, not the loadout's — they go in \
             {}, and they apply to every session, not just this one.",
            shorten_home(&client_config_path(&app.home), &app.home)
        ),
    );

    let [intro_area, body, footer] = Layout::vertical([
        Constraint::Length(THEME_INTRO_ROWS + 1),
        Constraint::Min(6),
        Constraint::Length(3),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    let b = &app.bindings;
    let rows: [(&str, String, &str); 4] = [
        (
            "Leader",
            b.leader.as_config_str(),
            "enters command mode; swallowed, never sent to the shell",
        ),
        (
            "Detach",
            b.detach.as_config_str(),
            "pressed after the leader, leaves the session running",
        ),
        (
            "Forward",
            b.forward.as_config_str(),
            "sends a literal leader down to a nested session",
        ),
        (
            "Bell on leader",
            if b.bell { "yes".into() } else { "no".into() },
            "ring the terminal bell when command mode opens",
        ),
    ];

    let mut lines: Vec<Line> = Vec::new();
    for (i, (label, value, about)) in rows.iter().enumerate() {
        let focused = i == app.client_row;
        let editing = focused && app.editing.is_some();
        let shown = if editing {
            // A caret, so an emptied field still shows where typing lands.
            format!("{}_", app.editing.as_deref().unwrap_or(""))
        } else {
            value.clone()
        };
        let accent = if editing {
            t.orange
        } else if focused {
            t.blue
        } else {
            t.fg
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} {label:<15}", if focused { "▸" } else { " " }),
                if focused {
                    Style::default().fg(accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.fg)
                },
            ),
            Span::styled(
                format!("{shown:<14}"),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled((*about).to_string(), Style::default().fg(t.comment)),
        ]));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), body);

    frame.render_widget(
        Paragraph::new(Text::from(client_notes(app, &t))).wrap(Wrap { trim: true }),
        footer,
    );
}

/// The strip under the client page: whichever of these is true — the reason an
/// edit cannot be accepted, or the warning that a plain leader is a poor
/// choice — and always the resulting gesture, spelled out.
pub fn client_notes(app: &App, t: &Theme) -> Vec<Line<'static>> {
    let mut notes: Vec<Line> = Vec::new();
    if let Some(buffer) = &app.editing {
        if let Err(e) = Key::parse(buffer.trim()).and_then(|k| {
            let mut next = app.bindings;
            match app.client_row {
                0 => next.leader = k,
                1 => next.detach = k,
                2 => next.forward = k,
                _ => {}
            }
            next.validate()
        }) {
            notes.push(Line::styled(e.to_string(), Style::default().fg(t.red)));
        }
    } else if app.bindings.leader.is_awkward_leader() {
        notes.push(Line::styled(
            format!(
                "`{}` is a plain key, so every press of it opens command mode. \
                 Allowed, but awkward.",
                app.bindings.leader.as_config_str()
            ),
            Style::default().fg(t.orange),
        ));
    }
    notes.push(Line::from(vec![
        Span::styled("Detach with ", Style::default().fg(t.comment)),
        Span::styled(
            app.bindings.hint(),
            Style::default().fg(t.green).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if app.bindings.is_default() {
                "  (the default — nothing will be written)"
            } else {
                "  (the greeting will say so too)"
            },
            Style::default().fg(t.comment),
        ),
    ]));
    notes
}

// --- keys -----------------------------------------------------------------

pub fn on_key_client(app: &mut App, key: KeyEvent) {
    if let Some(buffer) = app.editing.as_mut() {
        match key.code {
            KeyCode::Esc => app.editing = None,
            KeyCode::Enter => {
                let text = buffer.clone();
                let row = app.client_row;
                // A rejected chord keeps the buffer open with the text
                // still in it: the error names what is wrong, and the fix
                // is usually one character.
                if app.commit_client_field(row, &text).is_ok() {
                    app.editing = None;
                }
            }
            KeyCode::Backspace => {
                buffer.pop();
            }
            // `ctrl-]` is a chord the user may well want to type. Accept it
            // as the text it stands for rather than as a keystroke, so the
            // page can configure the very key it is being pressed with.
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                *buffer = format!("ctrl-{}", c.to_ascii_lowercase());
            }
            KeyCode::Char(c) => buffer.push(c),
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => app.screen = Screen::Patches,
        KeyCode::Up | KeyCode::Char('k') => {
            app.client_row = app.client_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.client_row = (app.client_row + 1).min(App::CLIENT_ROWS - 1);
        }
        KeyCode::Char('r') => app.bindings = Bindings::default(),
        KeyCode::Char(' ') if app.client_row == 3 => {
            app.bindings.bell = !app.bindings.bell;
        }
        KeyCode::Char(' ') => {
            app.editing = Some(app.client_field_text(app.client_row));
        }
        // Enter moves on, as it does on every other page. Space changes
        // the row under the cursor, as it does on the packages page.
        KeyCode::Enter => app.screen = Screen::Resources,
        _ => {}
    }
}
