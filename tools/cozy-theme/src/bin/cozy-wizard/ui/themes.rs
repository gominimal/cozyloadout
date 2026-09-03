//! The theme browser: every scheme on disk, with the whole interface
//! re-painting in the one under the cursor.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;
use super::truncate;

/// The scheme list, and the preview that re-paints as it moves.
pub fn draw_themes(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();

    // Paint the whole area in the scheme's background first. Widgets below
    // only set foregrounds, so without this the preview would sit on the
    // terminal's own background and the scheme would look wrong.
    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), inner);

    let [intro_area, columns] =
        Layout::vertical([Constraint::Length(THEME_INTRO_ROWS), Constraint::Min(0)]).areas(inner);

    // The heading is styled from the scheme too, so it re-paints with
    // everything else rather than sitting there in the wizard's own colours.
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled(
                "Pick a colour scheme.",
                Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::styled(
                "Everything re-paints as you move, so this is how the loadout will \
                 look. Enter picks it.",
                Style::default().fg(t.fg),
            ),
        ]))
        .wrap(Wrap { trim: true }),
        intro_area,
    );

    // A narrow terminal cannot show both columns; the list is the one you
    // cannot do without, so the preview is what goes.
    let show_preview = columns.width >= 72;
    let [list_area, preview_area] = if show_preview {
        Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).areas(columns)
    } else {
        [columns, Rect::ZERO]
    };

    draw_theme_list(frame, list_area, app, &t, show_preview);
    if show_preview {
        draw_preview(frame, preview_area, app, &t);
    }
}

pub fn draw_theme_list(frame: &mut Frame, area: Rect, app: &App, t: &Theme, divider: bool) {
    // The right border is the divider between the two columns, so it only
    // makes sense when there is a second column; drawn regardless it reads as
    // a stray line down the edge of a narrow terminal.
    let block = Block::default()
        .borders(if divider {
            Borders::RIGHT
        } else {
            Borders::NONE
        })
        .border_style(Style::default().fg(t.selection))
        // base01 is the raised-surface slot, so the list column sits on it and
        // the two panes read as separate surfaces — the same relationship the
        // themed tools' own UIs use.
        .style(Style::default().bg(t.surface))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = inner.height.saturating_sub(1) as usize;
    // Key handling has no frame to measure, so record what this one had.
    app.list_rows.set(rows);
    let items: Vec<ListItem> = app
        .visible_schemes(rows)
        .map(|(i, entry)| {
            let selected = i == app.theme_row;
            let style = if selected {
                Style::default()
                    .fg(t.bg)
                    .bg(t.blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.fg)
            };
            let source = if selected {
                Style::default().fg(t.bg).bg(t.blue)
            } else {
                Style::default().fg(t.comment)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<20}", truncate(&entry.name, 20)), style),
                Span::styled(format!("{:>7}", entry.source), source),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items), inner);

    // Position, so a long list does not feel bottomless.
    let counter = format!("{}/{}", app.theme_row + 1, app.schemes.len());
    let y = inner.y + inner.height.saturating_sub(1);
    frame.render_widget(
        Paragraph::new(Line::styled(counter, Style::default().fg(t.comment))),
        Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        },
    );
}

pub fn draw_preview(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let block = Block::default().padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(scheme) = &app.loaded else {
        frame.render_widget(
            Paragraph::new(Line::styled("…", Style::default().fg(t.comment))),
            inner,
        );
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    // Heading: the scheme's own name and which way round it reads.
    lines.push(Line::from(vec![
        Span::styled(
            scheme.name.clone(),
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", scheme.variant()),
            Style::default().fg(t.comment),
        ),
    ]));
    lines.push(Line::raw(""));

    // The palette itself, so the accents are visible even where the samples
    // below happen not to use one.
    let mut swatches: Vec<Span> = Vec::new();
    for slot in SLOTS {
        let c = scheme.palette[slot];
        swatches.push(Span::styled("██", Style::default().fg(rgb(c))));
    }
    lines.push(Line::from(swatches));
    lines.push(Line::raw(""));

    // A prompt, as starship draws it.
    lines.push(Line::from(vec![
        Span::styled("~/dev/cozyloadout", Style::default().fg(t.cyan)),
        Span::styled(" on ", Style::default().fg(t.comment)),
        Span::styled(" main", Style::default().fg(t.magenta)),
        Span::styled(" [!] ", Style::default().fg(t.orange)),
        Span::styled("via ", Style::default().fg(t.comment)),
        Span::styled("🦀 v1.97.1", Style::default().fg(t.red)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "❯ ",
            Style::default().fg(t.green).add_modifier(Modifier::BOLD),
        ),
        Span::styled("just theme ", Style::default().fg(t.fg)),
        Span::styled(
            &app.schemes[app.theme_row].name,
            Style::default().fg(t.yellow),
        ),
    ]));
    lines.push(Line::raw(""));

    // Syntax highlighting, as helix and bat draw it.
    for row in SAMPLE_CODE {
        lines.push(Line::from(
            row.iter()
                .map(|(tok, text)| Span::styled(*text, Style::default().fg(tok.color(t))))
                .collect::<Vec<_>>(),
        ));
    }
    lines.push(Line::raw(""));

    // A diff, as delta draws it — including the blended backgrounds, which are
    // the one part of the loadout's colour that is computed rather than picked.
    lines.push(Line::styled(
        "modified  templates/fish/config.fish",
        Style::default().fg(t.yellow),
    ));
    lines.push(Line::from(Span::styled(
        "-    set -g fish_greeting \"\"",
        Style::default().fg(t.red).bg(t.minus_bg),
    )));
    lines.push(Line::from(Span::styled(
        "+    set -g fish_greeting $mark",
        Style::default().fg(t.green).bg(t.plus_bg),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

// --- keys -----------------------------------------------------------------

pub fn on_key_themes(app: &mut App, key: KeyEvent) {
    let page = app.list_rows();
    match key.code {
        KeyCode::Esc => app.screen = Screen::Schemes,
        KeyCode::Up | KeyCode::Char('k') => app.move_theme(-1, page),
        KeyCode::Down | KeyCode::Char('j') => app.move_theme(1, page),
        KeyCode::PageUp => app.move_theme(-(isize::try_from(page).unwrap_or(10)), page),
        KeyCode::PageDown => app.move_theme(isize::try_from(page).unwrap_or(10), page),
        KeyCode::Home => app.move_theme(isize::MIN / 2, page),
        KeyCode::End => app.move_theme(isize::MAX / 2, page),
        KeyCode::Enter => {
            let templates = app.templates_dir();
            super::packages::enter_packages(app, &templates);
        }
        _ => {}
    }
}

/// Move to the theme browser, discovering what is on disk on the way in —
/// after the fetch, so a scheme collection downloaded a moment ago is in
/// the list.
pub fn enter_themes(app: &mut App) {
    // The schemes directory is `<root>/vendor`, so its parent is what
    // `discover` walks.
    let root = app
        .schemes_dir
        .parent()
        .unwrap_or(&app.schemes_dir)
        .to_path_buf();
    // What to land on: this run's choice if there is one, otherwise the
    // remembered one. Re-applying the file on a second visit would undo a
    // change made this run, which is the whole reason for the distinction.
    // Either way it is looked up *by name*, so a re-cloned collection or a
    // freshly fetched one does not move the cursor somewhere arbitrary.
    let want = app
        .schemes
        .get(app.theme_row)
        .map(|e| e.name.clone())
        .or_else(|| app.saved.theme.clone());

    // Re-discovered every time rather than once: the user can go back,
    // fetch the collection, and return, and the new schemes should be here.
    app.schemes = discover(&root);
    app.theme_row = want
        .and_then(|name| app.schemes.iter().position(|s| s.name == name))
        .unwrap_or(0);
    app.theme_top = app.theme_row;
    app.load_selected();
    app.screen = Screen::Themes;
}
