//! The patches screen: one filesystem picker, a preview of what is under the
//! cursor, and the warning about configs the user's own picks displace.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;
use super::{shorten_home, truncate, truncate_start};
use crate::picker::Entry;

pub fn draw_patches(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();

    let intro = Paragraph::new(Text::from(vec![
        Line::styled(
            "Patch in your own files.",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            "Anything chosen here is copied into the session alongside the loadout's \
             own config. Arrows walk the tree, space chooses a file or a whole \
             folder, e says where it lands.",
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

    // Side by side while there is room. The listing is what you cannot do
    // without, so the preview is what a narrow terminal loses.
    if body.width >= PREVIEW_MIN_WIDTH {
        let [list, pane] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(body);
        draw_picker(frame, list, app.picker(), true, &app.home, app.icons, &t);
        draw_preview_pane(frame, pane, app, &t);
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
            " Files and folders ",
            if focused {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(accent)
            },
        ))
        // The path is on the bottom border: it is context you glance at, and
        // it costs no rows there.
        .title_bottom(Span::styled(
            // The filter takes the bottom border while it is on: it is what
            // explains why the listing is short, which matters more than the
            // path you already walked to.
            if p.query.is_empty() {
                format!(" {} ", shorten_home(&p.cwd, home))
            } else {
                format!(" /{} ", p.query)
            },
            Style::default().fg(if p.query.is_empty() {
                t.comment
            } else {
                t.blue
            }),
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
            let chosen = p.is_chosen(e);
            // Everything in the listing has a box now: a directory is as
            // patchable as a file, it just becomes a glob instead of a single
            // dest.
            let mark = if chosen { "[x] " } else { "[ ] " };
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
            } else {
                Style::default().fg(t.fg)
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
    if app.searching.is_some() {
        on_key_search(app, key);
        return;
    }
    if app.editing_dest.is_some() {
        on_key_dest(app, key);
        return;
    }
    let page = app.list_rows();
    match key.code {
        KeyCode::Esc => app.screen = Screen::Packages,
        KeyCode::Up | KeyCode::Char('k') => app.picker_mut().move_cursor(-1, page),
        KeyCode::Down | KeyCode::Char('j') => app.picker_mut().move_cursor(1, page),
        KeyCode::PageUp => {
            app.picker_mut()
                .move_cursor(-(isize::try_from(page).unwrap_or(10)), page);
        }
        KeyCode::PageDown => {
            app.picker_mut()
                .move_cursor(isize::try_from(page).unwrap_or(10), page);
        }
        KeyCode::Right | KeyCode::Char('l') => app.picker_mut().descend(),
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
            app.picker_mut().ascend();
        }
        KeyCode::Char(' ') => {
            app.picker_mut().toggle();
        }
        KeyCode::Char('/') => app.searching = Some(app.picker().query.clone()),
        // Choosing it first if it is not chosen already: saying where a file
        // should land is an unambiguous way of saying you want it. Requiring
        // space beforehand made this key look dead, because pressing it on a
        // highlighted entry is the obvious thing to try.
        KeyCode::Char('e') => {
            if let Some((path, is_dir)) = app.current_target() {
                if !app.picker().chosen.contains_key(&path) {
                    app.picker_mut().toggle();
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

/// Move to the patches page, starting the picker at $HOME — where the
/// dotfiles a loadout patches in actually live.
pub fn enter_patches(app: &mut App) {
    if app.picker.is_none() {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let mut picker = Picker::new(&home);
        // Anything the user has since deleted is simply not restored: it is
        // gone, which is not an error, so the page opens without it. The
        // remembered lists stay separate because that is the shape a loadout
        // wants them in; the picker holds the flag per path.
        for path in State::existing(&app.saved.files) {
            picker.chosen.insert(path, false);
        }
        for path in State::existing(&app.saved.dirs) {
            picker.chosen.insert(path, true);
        }
        app.picker = Some(picker);
    }
    app.screen = Screen::Patches;
}

// --- footer ---------------------------------------------------------------

/// The keys this screen answers to, for the footer.
pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.searching.is_some() {
        return vec![
            ("type", "to filter"),
            ("↑/↓", "move"),
            ("enter", "keep it"),
            ("esc", "clear"),
        ];
    }
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
        ("/", "filter"),
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

/// Below this the preview pane costs more than it gives, leaving both columns
/// too narrow to read a path in. Lower than it was: with one listing instead of
/// two, the preview fits in a much smaller terminal.
const PREVIEW_MIN_WIDTH: u16 = 84;

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

    let mut lines = heading_lines(app, entry, inner.width as usize, t);
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
        preview::Preview::Image {
            width,
            height,
            bytes,
        } => {
            draw_image(frame, inner, app, &path, lines, (width, height, bytes), dim);
            return;
        }
        preview::Preview::Ansi { lines: art, more } => {
            lines.extend(ansi_lines(art));
            if more {
                lines.push(Line::styled("…", dim));
            }
        }
        preview::Preview::Archive {
            entries,
            bytes,
            sample,
            capped,
        } => lines.extend(archive_lines(
            inner.width as usize,
            rows,
            (entries, bytes, capped),
            sample,
            t,
        )),
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
        } => lines.extend(dir_lines(
            app,
            inner.width as usize,
            rows,
            (files, dirs, bytes, capped),
            sample,
            t,
        )),
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

/// The fixed rows above a preview's body: what the entry is called, where it
/// points if it is a link, and where it lands if it has been chosen.
fn heading_lines(app: &App, entry: &Entry, width: usize, t: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(
        truncate(&entry.name, width),
        Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
    )];

    // Where a link points, and — for a linked *directory* — that the patch
    // walker will not go through it. A dotfile tree is very often a symlink
    // farm, and this is the difference between patching a folder in and
    // patching nothing in.
    if let Some(target) = &entry.link {
        lines.push(Line::from(vec![
            Span::styled("→ ", Style::default().fg(t.comment)),
            Span::styled(
                // From the left: the tail of a path is what identifies it.
                // The first forty characters of a nix store path say nothing.
                truncate_start(
                    &shorten_home(Path::new(target), &app.home),
                    width.saturating_sub(2),
                ),
                Style::default().fg(t.cyan),
            ),
        ]));
        if entry.is_dir {
            // Two short lines rather than one long one: this paragraph is
            // drawn unwrapped, because wrapping code would be worse, so
            // anything that must be read has to fit on its own row.
            lines.push(Line::styled(
                "a linked folder — minimal will not",
                Style::default().fg(t.orange),
            ));
            lines.push(Line::styled(
                "walk it unless follow_symlinks is on",
                Style::default().fg(t.orange),
            ));
        }
    }

    // Where it lands, for anything chosen — the one fact about a pick that is
    // not visible anywhere else, and the thing `e` edits.
    if let Some((path, is_dir)) = app.current_pick() {
        let overridden = app.dest_overrides.contains_key(&path);
        lines.push(Line::from(vec![
            Span::styled("→ ~/", Style::default().fg(t.comment)),
            Span::styled(
                truncate(&app.dest_of(&path, is_dir), width),
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
    lines
}

/// ANSI art in its own colours, not the scheme's: this file *is* a picture,
/// and repainting it in the theme would be repainting the subject.
fn ansi_lines(art: Vec<Vec<preview::Span>>) -> Vec<Line<'static>> {
    art.into_iter()
        .map(|spans| {
            Line::from(
                spans
                    .into_iter()
                    .map(|s| {
                        let mut style = Style::default();
                        if let Some((r, g, b)) = s.fg {
                            style = style.fg(Color::Rgb(r, g, b));
                        }
                        if let Some((r, g, b)) = s.bg {
                            style = style.bg(Color::Rgb(r, g, b));
                        }
                        Span::styled(s.text, style)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// What is inside an archive: the count and size first, then as much of the
/// member list as the pane has rows for.
fn archive_lines(
    width: usize,
    rows: usize,
    (entries, bytes, capped): (usize, u64, bool),
    sample: Vec<String>,
    t: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(
            format!(
                "{}{entries} file{}, {}",
                if capped { "at least " } else { "" },
                if entries == 1 { "" } else { "s" },
                preview::format_bytes(bytes)
            ),
            Style::default().fg(t.green),
        ),
        Line::raw(""),
    ];
    lines.extend(
        sample
            .into_iter()
            .take(rows.saturating_sub(2))
            .map(|p| Line::styled(truncate(&p, width), Style::default().fg(t.comment))),
    );
    lines
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

/// The `/` filter's keys, over the listing.
///
/// The same shape the theme browser uses: arrows keep working, `enter` closes
/// the field and keeps the filter, `esc` clears it. Space types a space here
/// rather than choosing — a filter you cannot put a space in is a filter that
/// cannot match half the filenames in a home directory.
fn on_key_search(app: &mut App, key: KeyEvent) {
    let Some(mut query) = app.searching.clone() else {
        return;
    };
    let page = app.list_rows();
    match key.code {
        KeyCode::Esc => {
            app.searching = None;
            app.picker_mut().set_query(String::new());
        }
        KeyCode::Enter => app.searching = None,
        KeyCode::Backspace => {
            query.pop();
            app.picker_mut().set_query(query.clone());
            app.searching = Some(query);
        }
        KeyCode::Char(c) => {
            query.push(c);
            app.picker_mut().set_query(query.clone());
            app.searching = Some(query);
        }
        KeyCode::Up => app.picker_mut().move_cursor(-1, page),
        KeyCode::Down => app.picker_mut().move_cursor(1, page),
        _ => {}
    }
}

/// The heading, the image's dimensions, and the picture itself.
///
/// Half-blocks are ordinary coloured cells, so this composes with the rest of
/// the frame — and a test can read the picture straight back out of the buffer,
/// which is the whole reason for choosing them over the kitty protocol.
fn draw_image(
    frame: &mut Frame,
    inner: Rect,
    app: &App,
    path: &Path,
    mut lines: Vec<Line<'static>>,
    (width, height, bytes): (u32, u32, u64),
    dim: Style,
) {
    lines.push(Line::styled(
        format!("{width}×{height}  ·  {}", preview::format_bytes(bytes)),
        dim,
    ));
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);

    // Everything under the heading goes to the picture.
    let below = Rect {
        x: inner.x,
        y: inner.y.saturating_add(3),
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    app.with_image(path, below, |drawable| {
        if let Some(d) = drawable {
            frame.render_widget(ratatui_image::Image::new(d), below);
        }
    });
}

/// What patching a folder in would copy: the counts, then a sample of the
/// paths, lit the same way the listing lights them.
fn dir_lines(
    app: &App,
    width: usize,
    rows: usize,
    (files, dirs, bytes, capped): (usize, usize, u64, bool),
    sample: Vec<String>,
    t: &Theme,
) -> Vec<Line<'static>> {
    let dim = Style::default().fg(t.comment);
    let mut out = vec![
        // The recursive count, not the folder's own listing: the patch source
        // becomes `<dir>/**/*`, so the whole tree comes with it.
        Line::styled(
            format!(
                "{}{files} file{}, {dirs} folder{}, {}",
                if capped { "at least " } else { "" },
                if files == 1 { "" } else { "s" },
                if dirs == 1 { "" } else { "s" },
                preview::format_bytes(bytes)
            ),
            Style::default().fg(t.green),
        ),
        Line::raw(""),
    ];
    out.extend(sample.into_iter().take(rows.saturating_sub(2)).map(|p| {
        // The kind comes from the basename; the path shown is relative to the
        // folder, so `themes/dark.toml` is a config, not a directory.
        let base = p.rsplit('/').next().unwrap_or(&p);
        let kind = icons::kind_of(base, false);
        let mut spans = Vec::new();
        let mut room = width;
        if app.icons {
            spans.push(Span::styled(
                format!("{} ", kind.icon()),
                Style::default().fg(kind.color(t)),
            ));
            room = room.saturating_sub(2);
        }
        spans.push(Span::styled(truncate(&p, room), dim));
        Line::from(spans)
    }));
    out
}
