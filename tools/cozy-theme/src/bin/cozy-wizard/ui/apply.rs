//! The summary and the four things that can be done with it.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;

/// The summary of every choice, and what can be done with it.
///
/// Separate rather than one browser with a mode, because a loadout patches the
/// two differently — a file maps to a single `dest`, a directory to a glob —
/// and because seeing both sets of choices at once is the point.
/// The four lines of "here is what you chose".
///
/// The scheme-collection answer is deliberately absent: it was about the state
/// of the disk a moment ago, not a choice worth reviewing.
pub fn summary_paragraph(app: &App, t: &Theme) -> Paragraph<'static> {
    let row = |k: &'static str, v: String| {
        Line::from(vec![
            Span::styled(format!("  {k:<10}"), Style::default().fg(t.comment)),
            Span::styled(v, Style::default().fg(t.fg)),
        ])
    };
    let files = app.picker.as_ref().map_or(0, |p| p.chosen_of(false).len());
    let dirs = app.picker.as_ref().map_or(0, |p| p.chosen_of(true).len());
    let extras = app.extra_packages();
    let displaced = app.displaced_configs();
    Paragraph::new(Text::from(vec![
        row(
            "greeting",
            app.greeting
                .unwrap_or(Greeting::Blocks)
                .label()
                .trim()
                .to_string(),
        ),
        row(
            "theme",
            app.schemes
                .get(app.theme_row)
                .map_or_else(|| "—".to_string(), |e| e.name.clone()),
        ),
        row(
            "adjusted",
            if app.adjust.is_identity() {
                "no — the scheme as published".to_string()
            } else {
                // Named by the same function the theme page's footer uses, so
                // the two cannot describe one adjustment differently.
                format!(
                    "{}  → {}",
                    super::themes::knob_summary(app.adjust),
                    app.scheme().map_or_else(String::new, |s| s.slug)
                )
            },
        ),
        row(
            "packages",
            if extras.is_empty() {
                format!("{} optional", app.chosen_packages().len())
            } else {
                format!(
                    "{} optional, including {}",
                    app.chosen_packages().len(),
                    extras.join(" ")
                )
            },
        ),
        row(
            "patches",
            if files + dirs == 0 {
                "none".to_string()
            } else {
                format!("{files} file(s), {dirs} director(ies)")
            },
        ),
        // These two configure minimal itself, not the loadout, so they are
        // marked as such: the rows above are undone by deleting `build/`,
        // these are not.
        row(
            "detach",
            if app.bindings.is_default() {
                format!("{} (default, unchanged)", app.bindings.hint())
            } else {
                format!("{} — writes minimal's config", app.bindings.hint())
            },
        ),
        row("vm", {
            let a = app.resources.allocation();
            let what = format!("{} cores, {}", a.vcpus, resources::format_mib(a.ram_mib));
            if app.resources.is_default() {
                format!("{what} (default, unchanged)")
            } else if resources::minvmd_on_path().is_none() {
                format!("{what} — minvmd not on PATH, will not be applied")
            } else {
                format!("{what} — runs minvmd config set")
            }
        }),
        // The same warning the patches page shows, repeated here because this
        // is the last screen before anything is written.
        if displaced.is_empty() {
            Line::raw("")
        } else {
            Line::from(vec![
                Span::styled("  replaces ", Style::default().fg(t.orange)),
                Span::styled(displaced.join("  "), Style::default().fg(t.comment)),
            ])
        },
    ]))
    .wrap(Wrap { trim: false })
}

/// The summary and the four things that can be done with it.
pub fn draw_apply(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();

    let [intro_area, summary_area, tick_area, list_area, status_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(9),
        Constraint::Length(2),
        Constraint::Length(9),
        Constraint::Min(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::styled(
            "Ready.",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        )),
        intro_area,
    );

    frame.render_widget(summary_paragraph(app, &t), summary_area);

    // The tick sits above the actions, not among them: the list is a list of
    // actions, and a row where enter toggles rather than acts would be the one
    // place on the screen where enter means something else.
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if app.install { "  [x] " } else { "  [ ] " },
                Style::default()
                    .fg(if app.install { t.green } else { t.comment })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "install into ~/.config/minimal/loadouts/",
                Style::default().fg(if app.install { t.fg } else { t.comment }),
            ),
            Span::styled("   space toggles", Style::default().fg(t.comment)),
        ])),
        tick_area,
    );

    let items: Vec<ListItem> = Action::ALL
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let selected = i == app.action_row;
            let style = if selected {
                Style::default()
                    .fg(t.bg)
                    .bg(t.blue)
                    .add_modifier(Modifier::BOLD)
            } else if *a == Action::Abort {
                Style::default().fg(t.red)
            } else {
                Style::default().fg(t.fg)
            };
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("  {:<28}", a.label(app.install)),
                    style,
                )),
                Line::from(Span::styled(
                    format!("    {}", a.about(app.install)),
                    Style::default().fg(t.comment),
                )),
            ])
        })
        .collect();
    if app.saving_settings.is_some() {
        draw_settings_prompt(frame, list_area, app, &t);
    } else {
        frame.render_widget(List::new(items), list_area);
    }

    let status = match &app.applied {
        Applied::Idle => Line::raw(""),
        Applied::Running(what) => Line::styled(format!("  {what}…"), Style::default().fg(t.cyan)),
        Applied::Ok(report) => Line::from(vec![
            Span::styled(
                "  done  ",
                Style::default().fg(t.green).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                report.lines().last().unwrap_or_default().to_string(),
                Style::default().fg(t.comment),
            ),
            Span::styled("   press any key to exit", Style::default().fg(t.cyan)),
        ]),
        Applied::Failed(why) => Line::from(vec![
            Span::styled(
                "  failed  ",
                Style::default().fg(t.red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                why.lines().next().unwrap_or_default().to_string(),
                Style::default().fg(t.orange),
            ),
            Span::styled("   press any key to exit", Style::default().fg(t.cyan)),
        ]),
    };
    frame.render_widget(
        Paragraph::new(status).wrap(Wrap { trim: true }),
        status_area,
    );
}

// --- keys -----------------------------------------------------------------

pub fn on_key_apply(app: &mut App, key: KeyEvent) {
    if app.saving_settings.is_some() {
        on_key_saving_settings(app, key);
        return;
    }
    // Once the action has run, any key leaves. Naming two specific keys
    // made people hunt for them, and there is nothing else to do here:
    // re-running from the same screen would be a second build nobody asked
    // for, and a failure's message is printed on the way out so it is
    // still readable after the alternate screen is gone.
    if matches!(app.applied, Applied::Ok(_) | Applied::Failed(_)) {
        app.done = true;
        return;
    }
    match key.code {
        KeyCode::Esc => app.screen = Screen::Resources,
        KeyCode::Up | KeyCode::Char('k') => {
            app.action_row = app.action_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.action_row = (app.action_row + 1).min(Action::ALL.len() - 1);
        }
        // Space toggles the tick from anywhere on the page rather than making
        // it a row: the list is a list of *actions*, and a row where enter
        // toggles instead of acting would be the one place enter means
        // something else.
        KeyCode::Char(' ') => app.install = !app.install,
        KeyCode::Enter => app.apply(),
        _ => {}
    }
}

/// The prompt for where to write the settings.
fn on_key_saving_settings(app: &mut App, key: KeyEvent) {
    let Some(buffer) = app.saving_settings.as_mut() else {
        return;
    };
    match key.code {
        KeyCode::Esc => {
            app.saving_settings = None;
            app.settings_note = None;
        }
        KeyCode::Backspace => {
            buffer.pop();
        }
        KeyCode::Char(c) => buffer.push(c),
        KeyCode::Enter => {
            let path = PathBuf::from(buffer.trim());
            match save_settings_to(app, &path) {
                Ok(note) => {
                    app.saving_settings = None;
                    // Written, so this run counts as finished: the automatic
                    // file should record it too.
                    app.completed = true;
                    app.applied = Applied::Ok(note);
                }
                // A refused path keeps the prompt open with the text in it.
                Err(why) => app.settings_note = Some(Err(why)),
            }
        }
        _ => {}
    }
}

/// Write the answers to a file of the user's choosing.
fn save_settings_to(app: &App, path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty() {
        return Err("give it a name".to_string());
    }
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    app.to_state().save(path)?;
    Ok(format!(
        "saved {}\nrebuild it with: cozy-theme --settings {}",
        path.display(),
        path.display()
    ))
}

pub fn enter_apply(app: &mut App) {
    app.repo = app.repo_root();
    app.applied = Applied::Idle;
    app.screen = Screen::Apply;
}

// --- footer ---------------------------------------------------------------

/// The keys this screen answers to, for the footer.
pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.saving_settings.is_some() {
        return vec![("type", "a path"), ("enter", "save"), ("esc", "cancel")];
    }
    // Once it has run there is nothing left to choose; the result is on screen
    // and any key takes you out.
    if matches!(app.applied, Applied::Ok(_) | Applied::Failed(_)) {
        return vec![("any key", "exit")];
    }
    vec![
        ("↑/↓", "move"),
        ("space", "install on/off"),
        ("enter", "do it"),
        ("esc", "back"),
    ]
}

/// Where to write the settings, and what that file is for.
fn draw_settings_prompt(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let path = app.saving_settings.clone().unwrap_or_default();
    let mut lines = vec![
        Line::styled(
            "  Save these settings to",
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            format!("  {path}_"),
            Style::default().fg(t.green).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            "  Every answer on this page, as a file. `cozy-theme --settings <file>` \
             rebuilds this loadout from it on any machine.",
            Style::default().fg(t.fg),
        ),
    ];
    if let Some(Err(why)) = &app.settings_note {
        lines.push(Line::raw(""));
        lines.push(Line::styled(format!("  {why}"), Style::default().fg(t.red)));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        area,
    );
}
