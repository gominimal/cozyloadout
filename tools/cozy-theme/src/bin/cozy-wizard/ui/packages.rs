//! The package screen: the optional list, its detail pane, and the
//! free-text field for names the list does not carry.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;
use super::truncate;

/// Optional packages: a checklist, with what each one is and what it is
/// licensed under for the row under the cursor.
pub fn draw_packages(frame: &mut Frame, inner: Rect, app: &App) {
    // Still wearing the scheme picked on the previous page — the choice is
    // meant to persist through the rest of the wizard, not just be previewed.
    let t = app.theme();
    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), inner);

    let intro = Paragraph::new(Text::from(vec![
        Line::styled(
            "Choose the optional packages.",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            "Space toggles the one under the cursor. Everything here is on by \
             default; the shell, editor and search tools are installed either way.",
            Style::default().fg(t.fg),
        ),
    ]))
    .wrap(Wrap { trim: true });

    // Detail sits at the bottom rather than in a side column: the descriptions
    // are a sentence, and the list wants the width for names and checkboxes.
    let [intro_area, list_area, input_area, detail_area] = Layout::vertical([
        Constraint::Length(THEME_INTRO_ROWS),
        Constraint::Min(3),
        Constraint::Length(INPUT_ROWS),
        Constraint::Length(DETAIL_ROWS),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    if app.packages.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No package list found — templates/packages.toml could not be read.",
                Style::default().fg(t.orange),
            ))
            .wrap(Wrap { trim: true }),
            list_area,
        );
        return;
    }

    draw_package_list(frame, list_area, app, &t);
    draw_extra_input(frame, input_area, app, &t);
    draw_package_detail(frame, detail_area, app, &t);
}

/// The free-text field for packages that are not on the list.
///
/// The registry has far more than the fifteen offered above, and a loadout is
/// personal — there is no reason to make someone edit a TOML file to add `emacs`.
pub fn draw_extra_input(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let focused = app.focus == Focus::Input;
    let accent = if focused { t.blue } else { t.selection };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            " anything else to install ",
            Style::default().fg(if focused { t.blue } else { t.comment }),
        ))
        .padding(Padding::new(0, 0, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [field, note] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    // A block cursor only while focused: a caret sitting in an unfocused field
    // is an invitation to type into something that is not listening.
    let mut spans = vec![
        Span::styled("› ", Style::default().fg(accent)),
        Span::styled(app.extra.clone(), Style::default().fg(t.fg)),
    ];
    if focused {
        spans.push(Span::styled(" ", Style::default().bg(t.fg)));
    } else if app.extra.is_empty() {
        spans.push(Span::styled(
            "press i or tab to add packages by name",
            Style::default().fg(t.comment),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), field);

    // Echo what was actually parsed. Splitting on spaces and dropping tokens
    // that are not names is invisible otherwise, and silently ignoring half of
    // what someone typed is the worst version of this widget.
    let parsed = app.extra_packages();
    let redundant = app.redundant_extras();
    let line = if app.extra.trim().is_empty() {
        Line::styled(
            "Space-separated names from the Minimal registry, e.g. emacs vim tmux.",
            Style::default().fg(t.comment),
        )
    } else if parsed.is_empty() {
        Line::styled(
            "Nothing usable yet — names are lowercase, digits, - _ . +",
            Style::default().fg(t.orange),
        )
    } else if redundant.is_empty() {
        Line::styled(
            format!("adding {}", parsed.join(" ")),
            Style::default().fg(t.green),
        )
    } else {
        Line::from(vec![
            Span::styled(
                format!("adding {}", parsed.join(" ")),
                Style::default().fg(t.green),
            ),
            Span::styled(
                format!("  ·  already installed: {}", redundant.join(" ")),
                Style::default().fg(t.orange),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: true }), note);
}

pub fn draw_package_list(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let rows = area.height as usize;
    app.list_rows.set(rows);

    let items: Vec<ListItem> = app
        .packages
        .iter()
        .zip(&app.wanted)
        .enumerate()
        .skip(app.package_top)
        .take(rows)
        .map(|(i, (pkg, wanted))| {
            let selected = i == app.package_row;
            let mark = if *wanted { "[x] " } else { "[ ] " };
            // The checkbox is coloured by state, the name by the cursor, so
            // "which one am I on" and "is it on" stay separate questions.
            let mark_style = if *wanted {
                Style::default().fg(t.green)
            } else {
                Style::default().fg(t.comment)
            };
            let name_style = if selected {
                Style::default()
                    .fg(t.bg)
                    .bg(t.blue)
                    .add_modifier(Modifier::BOLD)
            } else if *wanted {
                Style::default().fg(t.fg)
            } else {
                Style::default().fg(t.comment)
            };
            ListItem::new(Line::from(vec![
                Span::styled(mark, mark_style),
                Span::styled(format!("{:<18}", truncate(&pkg.name, 18)), name_style),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items), area);
}

pub fn draw_package_detail(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let Some(pkg) = app.current_package() else {
        return;
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(t.selection))
        .padding(Padding::new(0, 0, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // A proprietary licence is the one a reader must not skim past, so it is
    // called out rather than sitting in the same grey as everything else.
    let permissive = pkg.license.starts_with("MIT")
        || pkg.license.starts_with("Apache")
        || pkg.license.starts_with("BSD")
        || pkg.license.starts_with("ISC");
    let licence_style = if permissive {
        Style::default().fg(t.comment)
    } else {
        Style::default().fg(t.orange).add_modifier(Modifier::BOLD)
    };

    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    pkg.name.clone(),
                    Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(pkg.license.clone(), licence_style),
                // A checklist without a tally makes you count the boxes.
                Span::styled(
                    format!(
                        "   ·  {} of {} chosen",
                        app.wanted.iter().filter(|w| **w).count(),
                        app.packages.len()
                    ),
                    Style::default().fg(t.comment),
                ),
            ]),
            Line::styled(pkg.about.clone(), Style::default().fg(t.fg)),
        ]))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

// --- keys -----------------------------------------------------------------

pub fn on_key_packages(app: &mut App, key: KeyEvent) {
    if app.focus == Focus::Input {
        on_key_input(app, key);
        return;
    }
    let page = app.list_rows();
    match key.code {
        KeyCode::Esc => app.screen = Screen::Themes,
        KeyCode::Up | KeyCode::Char('k') => app.move_package(-1, page),
        KeyCode::Down | KeyCode::Char('j') => app.move_package(1, page),
        KeyCode::PageUp => app.move_package(-(isize::try_from(page).unwrap_or(10)), page),
        KeyCode::PageDown => app.move_package(isize::try_from(page).unwrap_or(10), page),
        KeyCode::Home => app.move_package(isize::MIN / 2, page),
        KeyCode::End => app.move_package(isize::MAX / 2, page),
        KeyCode::Char(' ') => {
            if let Some(w) = app.wanted.get_mut(app.package_row) {
                *w = !*w;
            }
        }
        // Bulk toggles, because turning fifteen things off one at a time to
        // get a minimal session is a chore the keyboard can absorb.
        KeyCode::Char('a') => app.wanted.iter_mut().for_each(|w| *w = true),
        KeyCode::Char('n') => app.wanted.iter_mut().for_each(|w| *w = false),
        KeyCode::Tab | KeyCode::Char('i') => app.focus = Focus::Input,
        KeyCode::Enter => super::patches::enter_patches(app),
        _ => {}
    }
}

pub fn on_key_input(app: &mut App, key: KeyEvent) {
    match key.code {
        // All three leave the field; none of them is a "cancel", because
        // the text is already the value — there is nothing to revert to.
        KeyCode::Esc | KeyCode::Enter | KeyCode::Tab => app.focus = Focus::List,
        KeyCode::Backspace => {
            app.extra.pop();
        }
        KeyCode::Char(c) => app.extra.push(c),
        _ => {}
    }
}

/// Move to the package chooser, loading the list on the way in.
///
/// `templates` is found relative to the schemes directory, which is how the
/// rest of the wizard already locates the repo it is running inside.
pub fn enter_packages(app: &mut App, templates: &Path) {
    // Only on the first visit. Coming back to this page must show what the
    // user did this run, not what the sticky file remembers.
    if !app.packages.is_empty() {
        app.screen = Screen::Packages;
        return;
    }
    if let Ok(p) = Packages::load(&templates.join("packages.toml")) {
        app.always = p
            .base
            .packages
            .iter()
            .chain(&p.cozy.packages)
            .cloned()
            .collect();
        // Each package's remembered answer, falling back to its own
        // default when this file has never seen it.
        app.wanted = p
            .optional
            .iter()
            .map(|o| app.saved.wants_package(&o.name, o.default))
            .collect();
        app.packages = p.optional;
    }
    app.extra.clone_from(&app.saved.extra);
    app.package_row = 0;
    app.package_top = 0;
    app.screen = Screen::Packages;
}
