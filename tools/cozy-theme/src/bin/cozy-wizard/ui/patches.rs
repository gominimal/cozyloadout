//! The patches screen: two filesystem pickers, and the warning about
//! configs the user's own picks displace.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;
use super::shorten_home;

pub fn draw_patches(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();
    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), inner);

    let intro = Paragraph::new(Text::from(vec![
        Line::styled(
            "Patch in your own files.",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            "Anything chosen here is copied into the session alongside the loadout's \
             own config. Arrows walk the tree, space chooses, tab swaps between files \
             and directories.",
            Style::default().fg(t.fg),
        ),
    ]))
    .wrap(Wrap { trim: true });

    let displaced = app.displaced_configs();
    // Two rows for the chosen-path line, which wraps, plus one for the warning
    // when there is one. Sizing this for the common case clipped the warning
    // off the bottom exactly when it had something to say.
    let summary_rows = if displaced.is_empty() { 2 } else { 3 };
    let [intro_area, body, summary] = Layout::vertical([
        Constraint::Length(THEME_INTRO_ROWS),
        Constraint::Min(3),
        Constraint::Length(summary_rows),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    // Side by side while there is room; stacked would halve an already short
    // listing, so a narrow terminal shows only the focused picker instead.
    if body.width >= 72 {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(body);
        for (i, area) in [left, right].into_iter().enumerate() {
            draw_picker(
                frame,
                area,
                &app.pickers[i],
                i == app.picker_focus,
                &app.home,
                &t,
            );
        }
    } else {
        draw_picker(frame, body, app.picker(), true, &app.home, &t);
    }

    let chosen = app.chosen_paths();
    let line = if chosen.is_empty() {
        Line::styled(
            "Nothing chosen — this page is optional.",
            Style::default().fg(t.comment),
        )
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} chosen: ", chosen.len()),
                Style::default().fg(t.green),
            ),
            Span::styled(
                chosen
                    .iter()
                    .map(|p| shorten_home(p, &app.home))
                    .collect::<Vec<_>>()
                    .join("  "),
                Style::default().fg(t.comment),
            ),
        ])
    };
    // Say which of the loadout's own configs a pick has displaced. Theirs wins
    // — they chose it — but a config the loadout exists to install quietly
    // going missing is something you find out three sessions later.
    let lines = if displaced.is_empty() {
        vec![line]
    } else {
        vec![
            Line::from(vec![
                Span::styled("using yours instead of ", Style::default().fg(t.orange)),
                Span::styled(displaced.join("  "), Style::default().fg(t.comment)),
            ]),
            line,
        ]
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        summary,
    );
}

pub fn draw_picker(
    frame: &mut Frame,
    area: Rect,
    p: &Picker,
    focused: bool,
    home: &Path,
    t: &Theme,
) {
    let accent = if focused { t.blue } else { t.selection };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            p.kind.title(),
            if focused {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(accent)
            },
        ))
        // The path is on the bottom border: it is context you glance at, and
        // it costs no rows there.
        .title_bottom(Span::styled(
            format!(" {} ", shorten_home(&p.cwd, home)),
            Style::default().fg(t.comment),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(err) = &p.error {
        frame.render_widget(
            Paragraph::new(Line::styled(err.clone(), Style::default().fg(t.orange)))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    if p.entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled("empty", Style::default().fg(t.comment))),
            inner,
        );
        return;
    }

    let rows = inner.height as usize;
    let items: Vec<ListItem> = p
        .entries
        .iter()
        .enumerate()
        .skip(p.top)
        .take(rows)
        .map(|(i, e)| {
            let selectable = p.kind.accepts(e.is_dir);
            let chosen = p.is_chosen(e);
            let mark = if chosen {
                "[x] "
            } else if selectable {
                "[ ] "
            } else {
                // A directory in the file picker is scenery you walk through,
                // not something space can take; no empty box to imply otherwise.
                "    "
            };
            let name = if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            let style = if i == p.row && focused {
                Style::default()
                    .fg(t.bg)
                    .bg(t.blue)
                    .add_modifier(Modifier::BOLD)
            } else if chosen {
                Style::default().fg(t.green)
            } else if selectable {
                Style::default().fg(t.fg)
            } else {
                Style::default().fg(t.comment)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    mark,
                    Style::default().fg(if chosen { t.green } else { t.comment }),
                ),
                Span::styled(name, style),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

// --- keys -----------------------------------------------------------------

pub fn on_key_patches(app: &mut App, key: KeyEvent) {
    let page = app.list_rows();
    let focus = app.picker_focus;
    match key.code {
        KeyCode::Esc => app.screen = Screen::Packages,
        // Tab rather than left/right: those walk the tree, which is the
        // more frequent action and wants the arrow keys.
        KeyCode::Tab | KeyCode::BackTab => {
            app.picker_focus = (focus + 1) % app.pickers.len();
        }
        KeyCode::Up | KeyCode::Char('k') => app.pickers[focus].move_cursor(-1, page),
        KeyCode::Down | KeyCode::Char('j') => app.pickers[focus].move_cursor(1, page),
        KeyCode::PageUp => {
            app.pickers[focus].move_cursor(-(isize::try_from(page).unwrap_or(10)), page);
        }
        KeyCode::PageDown => {
            app.pickers[focus].move_cursor(isize::try_from(page).unwrap_or(10), page);
        }
        KeyCode::Right | KeyCode::Char('l') => app.pickers[focus].descend(),
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
            app.pickers[focus].ascend();
        }
        KeyCode::Char(' ') => {
            app.pickers[focus].toggle();
        }
        // Enter finishes rather than descending: descending is on the
        // arrow that points into the tree, which leaves enter free to mean
        // the same thing it means on every other page.
        KeyCode::Enter => app.screen = Screen::Client,
        _ => {}
    }
}

/// Move to the patches page, starting both pickers at $HOME — where the
/// dotfiles a loadout patches in actually live.
pub fn enter_patches(app: &mut App) {
    if app.pickers.is_empty() {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let mut files = Picker::new(Pick::Files, &home);
        let mut dirs = Picker::new(Pick::Dirs, &home);
        // Anything the user has since deleted is simply not restored: it
        // is gone, which is not an error, so the page opens without it.
        files.chosen = State::existing(&app.saved.files).into_iter().collect();
        dirs.chosen = State::existing(&app.saved.dirs).into_iter().collect();
        app.pickers = vec![files, dirs];
    }
    app.picker_focus = 0;
    app.screen = Screen::Patches;
}
