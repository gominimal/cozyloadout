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
    //
    // The destination editor lives here too, and gets its own row, because this
    // strip is the only part of the page that is always drawn — the preview
    // column is the first thing a narrow terminal loses, and an editor you
    // cannot see is broken.
    let summary_rows = if app.editing_dest.is_some() {
        3
    } else if displaced.is_empty() {
        2
    } else {
        3
    };
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

    draw_summary_strip(frame, summary, app, &displaced, &t);
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
            let kind = icons::kind_of(&e.name, e.is_dir);
            // The icon sits between the checkbox and the name, where `eza` puts
            // it, and takes the kind's colour. The *name* keeps the colour it
            // already had, because on this page a colour already means
            // something — green is chosen, dim is not selectable — and letting
            // the file type fight that would cost more than it gives.
            //
            // A trailing space of its own: a Nerd Font glyph is drawn
            // double-width in most terminals and would otherwise touch the name.
            let mut spans = vec![Span::styled(mark, style)];
            if with_icons {
                spans.push(Span::styled(
                    format!("{} ", kind.icon()),
                    if i == p.row && focused {
                        style
                    } else {
                        Style::default().fg(kind.color(t))
                    },
                ));
            }
            spans.push(Span::styled(name, style));
            ListItem::new(Line::from(spans))
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

// --- keys -----------------------------------------------------------------

pub fn on_key_patches(app: &mut App, key: KeyEvent) {
    if app.editing_dest.is_some() {
        on_key_dest(app, key);
        return;
    }
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
        // Choosing it first if it is not chosen already: saying where a file
        // should land is an unambiguous way of saying you want it. Requiring
        // space beforehand made this key look dead, because pressing it on a
        // highlighted entry is the obvious thing to try.
        KeyCode::Char('e') => {
            if let Some((path, is_dir)) = app.current_target() {
                if !app.pickers[focus].chosen.contains(&path) {
                    app.pickers[focus].toggle();
                }
                app.editing_dest = Some(app.dest_of(&path, is_dir));
                app.dest_note = None;
            }
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
pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.editing_dest.is_some() {
        return vec![
            ("type", "a path under ~"),
            ("enter", "accept"),
            ("esc", "cancel"),
        ];
    }
    let mut keys = vec![
        ("↑/↓", "move"),
        ("←/→", "in/out"),
        ("space", "choose"),
        ("tab", "files/dirs"),
    ];
    // Offered whenever it would do something — which is any entry this picker
    // can take, not only one already chosen.
    if app.current_target().is_some() {
        keys.push(("e", "where it lands"));
    }
    keys.push(("esc", "back"));
    keys.push(("enter", "done"));
    keys
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
    // The heading, the destination lines when there are any, and the blank row
    // under them are not content, so only the rest is worth reading.
    let header = if app.current_pick().is_some() { 4 } else { 2 };
    let rows = usize::from(inner.height).saturating_sub(header);
    let value = app.preview_of(&path, entry.is_dir, rows.max(1));

    let mut lines = vec![Line::styled(
        truncate(&entry.name, inner.width as usize),
        Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
    )];

    // Where it lands, for anything chosen — the one fact about a pick that is
    // not visible anywhere else, and the thing `e` edits.
    if let Some((path, is_dir)) = app.current_pick() {
        let overridden = app.dest_overrides.contains_key(&path);
        lines.push(Line::from(vec![
            Span::styled("→ ~/", Style::default().fg(t.comment)),
            Span::styled(
                truncate(&app.dest_of(&path, is_dir), inner.width as usize),
                Style::default().fg(if overridden { t.orange } else { t.green }),
            ),
        ]));
        lines.push(Line::styled(
            if overridden {
                "changed — e to edit"
            } else {
                "e to change"
            },
            Style::default().fg(t.comment),
        ));
    }
    lines.push(Line::raw(""));
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
            // The same icons and colours the listing uses, so a glance at what
            // a folder would bring in reads the same way as the folder itself.
            lines.extend(sample.into_iter().take(rows.saturating_sub(2)).map(|p| {
                // The kind comes from the basename; the path shown is relative
                // to the folder, so `themes/dark.toml` is a config, not a
                // directory.
                let base = p.rsplit('/').next().unwrap_or(&p);
                let kind = icons::kind_of(base, false);
                let mut spans = Vec::new();
                let mut width = inner.width as usize;
                if app.icons {
                    spans.push(Span::styled(
                        format!("{} ", kind.icon()),
                        Style::default().fg(kind.color(t)),
                    ));
                    width = width.saturating_sub(2);
                }
                spans.push(Span::styled(truncate(&p, width), dim));
                Line::from(spans)
            }));
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

/// The destination editor.
fn on_key_dest(app: &mut App, key: KeyEvent) {
    let Some(buffer) = app.editing_dest.as_mut() else {
        return;
    };
    match key.code {
        KeyCode::Esc => {
            app.editing_dest = None;
            app.dest_note = None;
        }
        KeyCode::Backspace => {
            buffer.pop();
        }
        KeyCode::Char(c) => buffer.push(c),
        KeyCode::Enter => {
            let typed = buffer.clone();
            let Some((path, is_dir)) = app.current_pick() else {
                app.editing_dest = None;
                return;
            };
            match cozy_theme::check_dest(&typed) {
                Ok(()) => {
                    let cleaned = cozy_theme::clean_dest(&typed, is_dir);
                    // Storing the computed answer as an override would be a
                    // silent promise to keep it even if the rule changed.
                    if cleaned == cozy_theme::patch_dest(&path, &app.home, is_dir) {
                        app.dest_overrides.remove(&path);
                    } else {
                        app.dest_overrides.insert(path, cleaned);
                    }
                    app.editing_dest = None;
                    app.dest_note = None;
                }
                // A refused destination keeps the field open with the text in
                // it: the fix is usually a character.
                Err(why) => app.dest_note = Some(why),
            }
        }
        _ => {}
    }
}

/// The strip along the bottom: what is chosen, what that displaces, and the
/// destination editor when it is open.
///
/// The one part of this page that is always drawn — the preview column is the
/// first thing a narrow terminal loses — which is why the editor lives here
/// rather than in the pane beside it.
fn draw_summary_strip(
    frame: &mut Frame,
    summary: Rect,
    app: &App,
    displaced: &[String],
    t: &Theme,
) {
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
    let lines = if let Some(typed) = &app.editing_dest {
        // The `~/` is an affordance, not part of the text — so it is dropped
        // when the typed path already starts with one, rather than rendering
        // `~//etc` and looking like a mistake the reader made.
        let prefix = if typed.starts_with('/') || typed.starts_with('~') {
            "lands at  "
        } else {
            "lands at  ~/"
        };
        // Truncated rather than wrapped: this strip is three rows, and a field
        // that wrapped to two would push the explanation of *why* a path was
        // refused off the bottom — which is the whole reason it is here.
        let room = (summary.width as usize).saturating_sub(prefix.len() + 1);
        vec![
            Line::from(vec![
                Span::styled(prefix, Style::default().fg(t.comment)),
                Span::styled(
                    format!("{}_", truncate(typed, room)),
                    Style::default().fg(t.green).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::styled(
                app.dest_note
                    .clone()
                    .unwrap_or_else(|| "Always relative to the session's home.".to_string()),
                Style::default().fg(if app.dest_note.is_some() {
                    t.red
                } else {
                    t.comment
                }),
            ),
        ]
    } else if displaced.is_empty() {
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
