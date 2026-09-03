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

/// The key hints along the bottom, from whichever screen is up.
///
/// Each screen module answers for its own keys — the same reason it owns its
/// own `on_key`. A hint list that lived here would be a second place to
/// remember when a binding changes, which is exactly how a footer starts
/// advertising keys that no longer work.
pub fn footer_hints(app: &App) -> Vec<Span<'static>> {
    let pairs = if app.busy() {
        vec![("q", "cancel")]
    } else {
        match app.screen {
            Screen::Greeting => greeting::hints(app),
            Screen::Schemes => schemes::hints(app),
            Screen::Themes => themes::hints(app),
            Screen::Packages => packages::hints(app),
            Screen::Patches => patches::hints(app),
            Screen::Client => client::hints(app),
            Screen::Resources => vm::hints(app),
            Screen::Apply => apply::hints(app),
        }
    };
    pairs
        .into_iter()
        .flat_map(|(k, what)| {
            [
                Span::styled(k, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!(" {what}  ")),
            ]
        })
        .collect()
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
