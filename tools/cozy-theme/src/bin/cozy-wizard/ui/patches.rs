//! The patches screen: two filesystem pickers, and the warning about
//! configs the user's own picks displace.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;
use super::{shorten_home, truncate};

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

    // A ladder, widest first: both pickers plus a preview, then both pickers,
    // then only the focused one. Stacking would halve an already short listing,
    // so a narrow terminal drops panes rather than shrinking them.
    if body.width >= PREVIEW_MIN_WIDTH {
        let [left, right, pane] = Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(40),
        ])
        .areas(body);
        for (i, area) in [left, right].into_iter().enumerate() {
            draw_picker(
                frame,
                area,
                &app.pickers[i],
                i == app.picker_focus,
                &app.home,
                app.icons,
                &t,
            );
        }
        draw_preview_pane(frame, pane, app, &t);
    } else if body.width >= 72 {
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
                app.icons,
                &t,
            );
        }
    } else {
        draw_picker(frame, body, app.picker(), true, &app.home, app.icons, &t);
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
    with_icons: bool,
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
            // The icon sits between the checkbox and the name, where `eza`
            // puts it. A trailing space of its own, because a Nerd Font glyph
            // is drawn double-width in most terminals and would otherwise touch
            // the name.
            let icon = if with_icons {
                format!("{} ", icons::for_entry(&e.name, e.is_dir))
            } else {
                String::new()
            };
            let name = if e.is_dir {
                format!("{icon}{}/", e.name)
            } else {
                format!("{icon}{}", e.name)
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

// --- footer ---------------------------------------------------------------

/// The keys this screen answers to, for the footer.
pub fn hints(_app: &App) -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑/↓", "move"),
        ("←/→", "in/out"),
        ("space", "choose"),
        ("tab", "files/dirs"),
        ("esc", "back"),
        ("enter", "done"),
    ]
}

/// Below this the preview pane costs more than it gives: three columns in
/// ninety cells leaves each too narrow to read a path in.
const PREVIEW_MIN_WIDTH: u16 = 104;

/// What the entry under the cursor holds — file contents, or what patching a
/// directory in would actually copy.
fn draw_preview_pane(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let picker = app.picker();
    // The same box the pickers get: three panes in a row, one of them a bare
    // divider, reads as an unfinished layout rather than a deliberate one.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.selection))
        .padding(Padding::horizontal(1))
        .title(Span::styled(" Preview ", Style::default().fg(t.comment)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(entry) = picker.current() else {
        return;
    };
    let path = picker.cwd.join(&entry.name);
    // The name and the blank line under it are not content, so only the rest
    // is worth reading.
    let rows = usize::from(inner.height).saturating_sub(2);
    let value = app.preview_of(&path, entry.is_dir, rows.max(1));

    let mut lines = vec![
        Line::styled(
            truncate(&entry.name, inner.width as usize),
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
    ];
    let dim = Style::default().fg(t.comment);
    let body = Style::default().fg(t.fg);
    match value {
        preview::Preview::Text { lines: text, more } => {
            // Highlighted as the chosen scheme lights it — the colours come
            // from the same `.tmTheme` the loadout installs for `bat`.
            let width = inner.width as usize;
            lines.extend(highlighted_lines(
                &app.highlight(&text, &entry.name),
                width,
                body,
            ));
            if more {
                lines.push(Line::styled("…", dim));
            }
        }
        preview::Preview::Binary { bytes } => {
            lines.push(Line::styled(
                format!("binary, {}", preview::format_bytes(bytes)),
                dim,
            ));
        }
        preview::Preview::Empty => lines.push(Line::styled("empty", dim)),
        preview::Preview::Error(why) => {
            lines.push(Line::styled(why, Style::default().fg(t.orange)));
        }
        preview::Preview::Dir {
            files,
            dirs,
            bytes,
            sample,
            capped,
        } => {
            // What the patch copies, not what the folder shows: the source
            // becomes `<dir>/**/*`, so the whole tree comes with it.
            lines.push(Line::styled(
                format!(
                    "{}{files} file{}, {dirs} folder{}, {}",
                    if capped { "at least " } else { "" },
                    if files == 1 { "" } else { "s" },
                    if dirs == 1 { "" } else { "s" },
                    preview::format_bytes(bytes)
                ),
                Style::default().fg(t.green),
            ));
            lines.push(Line::raw(""));
            lines.extend(
                sample
                    .into_iter()
                    .take(rows.saturating_sub(2))
                    .map(|p| Line::styled(truncate(&p, inner.width as usize), dim)),
            );
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

/// Highlighted spans as drawable lines, clipped to the pane.
///
/// Clipping is by span rather than by finished line: a `truncate` over the
/// joined text would have to be re-split to keep the colours, and cutting mid
/// span is exactly where that goes wrong.
fn highlighted_lines(
    rows: &[Vec<(Color, String)>],
    width: usize,
    fallback: Style,
) -> Vec<Line<'static>> {
    rows.iter()
        .map(|spans| {
            let mut used = 0usize;
            let mut out: Vec<Span> = Vec::new();
            for (colour, text) in spans {
                if used >= width {
                    break;
                }
                let room = width - used;
                let clipped: String = text.chars().take(room).collect();
                used += clipped.chars().count();
                // `Color::Reset` means "no grammar matched"; the pane's own
                // foreground is a better answer than the terminal's default.
                let style = if *colour == Color::Reset {
                    fallback
                } else {
                    Style::default().fg(*colour)
                };
                out.push(Span::styled(clipped, style));
            }
            Line::from(out)
        })
        .collect()
}
