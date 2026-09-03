//! The summary and the four things that can be done with it.

#[allow(clippy::wildcard_imports)]
use super::prelude::*;

/// Two pickers side by side: files on the left, directories on the right.
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
    let files = app.pickers.first().map_or(0, |p| p.chosen.len());
    let dirs = app.pickers.get(1).map_or(0, |p| p.chosen.len());
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
    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), inner);

    let [intro_area, summary_area, list_area, status_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(10),
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
                Line::from(Span::styled(format!("  {:<24}", a.label()), style)),
                Line::from(Span::styled(
                    format!("    {}", a.about()),
                    Style::default().fg(t.comment),
                )),
            ])
        })
        .collect();
    frame.render_widget(List::new(items), list_area);

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
        KeyCode::Enter | KeyCode::Char(' ') => app.apply(),
        _ => {}
    }
}

pub fn enter_apply(app: &mut App) {
    app.repo = app.repo_root();
    app.applied = Applied::Idle;
    app.screen = Screen::Apply;
}
