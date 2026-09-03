//! Drawing. One module per screen, plus the frame around all of them.
//!
//! Split by screen rather than by widget kind: each file holds everything that
//! decides what one page looks like, so changing a page means opening one file
//! and reading it top to bottom. The shared pieces here are the ones every
//! screen genuinely uses — the outer frame, the footer, the heading block, and
//! two text helpers.

pub mod prelude;

pub mod apply;
pub mod client;
pub mod greeting;
pub mod packages;
pub mod patches;
pub mod schemes;
pub mod themes;
pub mod vm;

#[allow(clippy::wildcard_imports)]
use prelude::*;

pub fn draw(frame: &mut Frame, app: &App) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(Padding::horizontal(BOX_PADDING_X))
        .title(" cozy wizard ");
    let inner = outer.inner(body);
    frame.render_widget(outer, body);

    match app.screen {
        Screen::Greeting => greeting::draw_greeting(frame, inner, app),
        Screen::Schemes => schemes::draw_schemes(frame, inner, app),
        Screen::Themes => themes::draw_themes(frame, inner, app),
        Screen::Packages => packages::draw_packages(frame, inner, app),
        Screen::Patches => patches::draw_patches(frame, inner, app),
        Screen::Client => client::draw_client(frame, inner, app),
        Screen::Resources => vm::draw_resources(frame, inner, app),
        Screen::Apply => apply::draw_apply(frame, inner, app),
    }
    frame.render_widget(
        Paragraph::new(Line::from(footer_hints(app))).alignment(Alignment::Center),
        footer,
    );
}

pub fn footer_hints(app: &App) -> Vec<Span<'static>> {
    let hint = |k: &'static str, what: &'static str| {
        vec![
            Span::styled(k, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!(" {what}  ")),
        ]
    };
    if app.busy() {
        return hint("q", "cancel");
    }
    match app.screen {
        Screen::Greeting => [
            hint("↑/↓", "move"),
            hint("enter", "choose"),
            hint("q", "quit"),
        ]
        .concat(),
        Screen::Schemes => match app.fetch {
            Fetch::Done(_) | Fetch::Failed(_) => {
                [hint("enter", "continue"), hint("q", "quit")].concat()
            }
            _ => [
                hint("←/→", "yes/no"),
                hint("enter", "confirm"),
                hint("esc", "back"),
                hint("q", "quit"),
            ]
            .concat(),
        },
        Screen::Themes => [
            hint("↑/↓", "browse"),
            hint("pgup/pgdn", "page"),
            hint("enter", "choose"),
            hint("esc", "back"),
            hint("q", "quit"),
        ]
        .concat(),
        Screen::Packages if app.focus == Focus::Input => [
            hint("type", "package names"),
            hint("enter/esc", "back to the list"),
        ]
        .concat(),
        Screen::Apply if matches!(app.applied, Applied::Ok(_) | Applied::Failed(_)) => {
            [hint("any key", "exit")].concat()
        }
        Screen::Apply => [
            hint("↑/↓", "move"),
            hint("enter", "do it"),
            hint("esc", "back"),
        ]
        .concat(),
        Screen::Patches => [
            hint("↑/↓", "move"),
            hint("←/→", "in/out"),
            hint("space", "choose"),
            hint("tab", "files/dirs"),
            hint("esc", "back"),
            hint("enter", "done"),
        ]
        .concat(),
        Screen::Packages => [
            hint("↑/↓", "move"),
            hint("space", "toggle"),
            hint("a/n", "all/none"),
            hint("i", "add by name"),
            hint("esc", "back"),
            hint("enter", "done"),
        ]
        .concat(),
        Screen::Client if app.editing.is_some() => [
            hint("type", "the chord"),
            hint("enter", "accept"),
            hint("esc", "cancel"),
        ]
        .concat(),
        Screen::Client => [
            hint("↑/↓", "move"),
            hint("space", "change"),
            hint("r", "reset"),
            hint("esc", "back"),
            hint("enter", "done"),
        ]
        .concat(),
        Screen::Resources => [
            hint("↑/↓", "cores/memory"),
            hint("←/→", "adjust"),
            hint("r", "reset"),
            hint("esc", "back"),
            hint("enter", "done"),
        ]
        .concat(),
    }
}

pub fn intro_paragraph(heading: &str, body: String) -> Paragraph<'static> {
    Paragraph::new(Text::from(vec![
        Line::styled(
            heading.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::raw(body),
    ]))
    .wrap(Wrap { trim: true })
}

pub fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else {
        s.chars().take(width.saturating_sub(1)).collect::<String>() + "…"
    }
}

/// `~` for the home directory, because absolute paths are mostly prefix and
/// the summary line has no room to spare.
pub fn shorten_home(path: &Path, home: &Path) -> String {
    let full = path.display().to_string();
    let home = home.display().to_string();
    if home.is_empty() || !full.starts_with(&home) {
        return full;
    }
    format!("~{}", &full[home.len()..])
}
