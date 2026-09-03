//! The VM screen: how much of this machine a session gets.

use super::intro_paragraph;
#[allow(clippy::wildcard_imports)]
use super::prelude::*;

/// The VM page: how much of this machine a session gets.
pub fn draw_resources(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();
    let r = &app.resources;
    let intro = intro_paragraph(
        "How much of this machine the VM gets.",
        "Minimal runs sessions in a microVM. These are minvmd's settings, applied with \
         `minvmd config set` and picked up at the next boot — the limits below are its \
         own, so nothing here can be set to something that will not start."
            .to_string(),
    );

    let [intro_area, body, footer] = Layout::vertical([
        Constraint::Length(THEME_INTRO_ROWS + 1),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    let known = r.host.total_mib > 0;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("  {:<15}", "This machine"),
                Style::default().fg(t.comment),
            ),
            Span::styled(
                if known {
                    format!(
                        "{} cores · {}",
                        r.host.logical_cores,
                        resources::format_mib(r.host.total_mib)
                    )
                } else {
                    format!("{} cores · memory unknown", r.host.logical_cores)
                },
                Style::default().fg(t.fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {:<15}", "Allocatable"),
                Style::default().fg(t.comment),
            ),
            Span::styled(
                format!(
                    "up to {} cores · {}",
                    r.max_vcpus,
                    resources::format_mib(r.max_ram_mib())
                ),
                Style::default().fg(t.fg),
            ),
        ]),
        Line::raw(""),
    ];

    lines.extend(resource_rows(r, &t));
    frame.render_widget(Paragraph::new(Text::from(lines)), body);

    frame.render_widget(
        Paragraph::new(Text::from(resource_notes(r, known, &t))).wrap(Wrap { trim: true }),
        footer,
    );
}

/// The two adjustable rows: `▸ label  ◂ value ▸  max …`, lit when focused.
pub fn resource_rows(r: &Resources, t: &Theme) -> Vec<Line<'static>> {
    let alloc = r.allocation();
    [
        (
            resources::Field::Cpu,
            "CPU cores",
            alloc.vcpus.to_string(),
            format!("max {}", r.max_vcpus),
        ),
        (
            resources::Field::Memory,
            "Memory",
            resources::format_mib(alloc.ram_mib),
            format!("max {}", resources::format_mib(r.max_ram_mib())),
        ),
    ]
    .into_iter()
    .map(|(field, label, value, max)| {
        let focused = r.field == field;
        // Arrows only on the focused row: an affordance for the keys that would
        // act now, not decoration for every row.
        let (left, right) = if focused {
            ("◂ ", " ▸")
        } else {
            ("  ", "  ")
        };
        let accent = if focused { t.blue } else { t.fg };
        Line::from(vec![
            Span::styled(
                format!("{} {label:<13}", if focused { "▸" } else { " " }),
                if focused {
                    Style::default().fg(accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.fg)
                },
            ),
            Span::styled(
                format!("{left}{value:^9}{right}  "),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(max, Style::default().fg(t.comment)),
        ])
    })
    .collect()
}

/// The strip under the VM page: why a limit is what it is, or why nothing will
/// be applied.
pub fn resource_notes(r: &Resources, host_known: bool, t: &Theme) -> Vec<Line<'static>> {
    let mut notes: Vec<Line> = Vec::new();
    if !host_known {
        notes.push(Line::styled(
            "Could not read this machine's memory, so the ceiling is the floor. \
             Pick a size you know it has.",
            Style::default().fg(t.orange),
        ));
    }
    if resources::minvmd_on_path().is_none() {
        notes.push(Line::styled(
            "minvmd is not on PATH — the choice will be remembered, but nothing will be applied.",
            Style::default().fg(t.orange),
        ));
    } else if r.is_default() {
        notes.push(Line::styled(
            "These are the defaults, so nothing will be applied.",
            Style::default().fg(t.comment),
        ));
    }
    notes
}

// --- keys -----------------------------------------------------------------

pub fn on_key_resources(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.screen = Screen::Client,
        // Up/down pick the field and left/right change it — the same shape
        // minimal's own resource screen uses.
        KeyCode::Up | KeyCode::Down | KeyCode::Char('k' | 'j') => {
            app.resources.toggle_field();
        }
        KeyCode::Left | KeyCode::Char('h') => app.resources.adjust(Step::Down),
        KeyCode::Right | KeyCode::Char('l') => app.resources.adjust(Step::Up),
        KeyCode::Char('r') => {
            let host = app.resources.host;
            app.resources = Resources::for_host(host);
        }
        KeyCode::Enter => super::apply::enter_apply(app),
        _ => {}
    }
}
