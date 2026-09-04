//! The greeting screen: five marks, previewed in the reader's own font.

use super::intro_paragraph;
#[allow(clippy::wildcard_imports)]
use super::prelude::*;

pub fn draw_greeting(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();
    let intro = intro_paragraph(
        "Welcome to the minimal cozy loadout wizard.",
        "Pick a greeting — the preview is what fish prints. Boxes mean this \
         font lacks those glyphs, and the icons too."
            .into(),
    );

    // A list plus one preview, rather than a box per option. With five options
    // and a four-line mark among them, stacking a box each does not fit in a
    // 24-row terminal — and only the highlighted one is being judged anyway.
    let [intro_area, list_area, icon_area, preview_area] = Layout::vertical([
        Constraint::Length(INTRO_ROWS),
        Constraint::Length(u16::try_from(Greeting::ALL.len()).unwrap_or(5)),
        Constraint::Length(2),
        Constraint::Min(3),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    let items: Vec<ListItem> = Greeting::ALL
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let selected = i == app.greeting_row;
            let style = if selected {
                Style::default()
                    .fg(t.bg)
                    .bg(t.cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:<18}", g.label()), style),
                Span::styled(g.note(), Style::default().fg(t.comment)),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items), list_area);

    // The icons themselves, not a description of them — the same reason the
    // marks above are previewed rather than named. A font either draws these or
    // shows boxes, and one glance settles it.
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if app.icons { "  [x] " } else { "  [ ] " },
                Style::default()
                    .fg(if app.icons { t.green } else { t.comment })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "icons in the file lists  ",
                Style::default().fg(if app.icons { t.fg } else { t.comment }),
            ),
            Span::styled(
                icons::sample().iter().collect::<String>(),
                Style::default().fg(t.cyan),
            ),
            Span::styled("   space toggles", Style::default().fg(t.comment)),
        ])),
        icon_area,
    );

    draw_greeting_preview(
        frame,
        preview_area,
        app.current_greeting(),
        &app.bindings.hint(),
        &t,
    );
}

/// The selected greeting as fish will print it.
///
/// **The mark itself is unstyled on purpose**: this is the user judging their
/// own font, so it renders in the terminal's own foreground rather than in
/// colours chosen here. The box around it is the scheme's, like every other box
/// in the wizard — the frame is chrome, the mark is the thing being judged.
pub fn draw_greeting_preview(
    frame: &mut Frame,
    area: Rect,
    greeting: Greeting,
    detach: &str,
    t: &Theme,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.selection))
        .padding(Padding::symmetric(BOX_PADDING_X, BOX_PADDING_Y))
        .title(Span::styled(
            " what fish will print ",
            Style::default().fg(t.comment),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = greeting.art().into_iter().map(Line::raw).collect();
    if greeting.has_detach_line() {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::from(vec![
            Span::raw("Welcome to minimal! "),
            Span::styled(detach.to_string(), Style::default().fg(t.cyan)),
            Span::raw(" to detach"),
        ]));
    } else {
        lines.push(Line::styled(
            "(a silent shell)",
            Style::default().fg(t.comment),
        ));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

// --- keys -----------------------------------------------------------------

pub fn on_key_greeting(app: &mut App, key: KeyEvent) {
    match key.code {
        // Esc on the first screen has nowhere to go back to.
        KeyCode::Esc => app.done = true,
        KeyCode::Up | KeyCode::Char('k') => {
            app.greeting_row = app.greeting_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.greeting_row = (app.greeting_row + 1).min(Greeting::ALL.len() - 1);
        }
        // Space is the tick, not a second confirm: the apply page already
        // spends it that way, and enter is what finishes a page everywhere.
        KeyCode::Char(' ') => app.icons = !app.icons,
        KeyCode::Enter => {
            app.greeting = Some(app.current_greeting());
            app.screen = Screen::Schemes;
        }
        _ => {}
    }
}

// --- footer ---------------------------------------------------------------

/// The keys this screen answers to, for the footer.
pub fn hints(_app: &App) -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑/↓", "move"),
        ("space", "icons on/off"),
        ("enter", "choose"),
        ("q", "quit"),
    ]
}
