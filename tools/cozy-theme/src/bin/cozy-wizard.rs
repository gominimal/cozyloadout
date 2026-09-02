//! cozy-wizard — interactive setup for the cozy loadout.
//!
//! Run it with `just wizard`. See AGENTS.md for the build pipeline.

use clap::Parser;
use color_eyre::eyre::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Upstream scheme collection. Kept identical to the `fetch-schemes` recipe in
/// the justfile, which `fetch_matches_the_justfile_recipe` asserts.
const SCHEMES_REPO: &str = "https://github.com/tinted-theming/schemes.git";

/// Columns of padding inside every bordered box, so text is not jammed against
/// the border.
const BOX_PADDING_X: u16 = 2;

/// Blank rows between one option box and the next.
const BOX_GAP: u16 = 1;

/// Rows of padding above and below the art inside an option box.
///
/// Rows are the scarce dimension. There is not room at 80x24 for this *and* a
/// caption line inside each box — the first attempt at it clipped the second
/// box. The captions pay for it by riding on the bottom border
/// (`title_bottom`), which costs no rows at all.
const BOX_PADDING_Y: u16 = 1;

/// Rows reserved for the welcome text.
///
/// A fixed height, not a measurement: ratatui only exposes
/// `Paragraph::line_count` behind an unstable feature, and taking an unstable
/// API for a layout nicety is not worth it. Five rows is the heading, a blank,
/// and up to three wrapped lines, which covers the intro from 50 columns up —
/// asserted by `intro_fits_in_its_rows`, because guessing it wrong silently
/// eats the end of the sentence.
const INTRO_ROWS: u16 = 5;

/// Rows reserved for the question and explanation on the schemes screen. It
/// runs longer than the greeting's intro and has no option boxes competing for
/// the space, so it gets more — `schemes_intro_fits_in_its_rows` holds it.
const SCHEMES_INTRO_ROWS: u16 = 8;

/// How long the event loop waits before redrawing while a fetch is running.
/// Only the spinner needs it; an idle wizard still blocks on a key and costs
/// nothing.
const TICK: Duration = Duration::from_millis(100);

#[derive(Parser)]
#[command(
    name = "cozy-wizard",
    version,
    about = "Interactive setup for the cozy loadout"
)]
struct Args {
    /// Where the upstream scheme collection lives
    #[arg(long, default_value = "schemes/vendor")]
    schemes: PathBuf,
}

// ---------------------------------------------------------------------------
// Greeting
// ---------------------------------------------------------------------------

/// Which fish greeting the loadout should install.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Greeting {
    /// `▃🭕🭏🭕🭏 M I N I M A L` — the mark drawn with Symbols for Legacy
    /// Computing (the U+1FB00 block, Unicode 13). Sharper, but a font without
    /// those glyphs shows tofu, which is the whole reason this is a choice.
    Legacy,
    /// The block-element mark that ships today. Only U+2580–U+259F, which
    /// essentially every monospace font has had for decades.
    Blocks,
}

impl Greeting {
    const ALL: [Greeting; 2] = [Greeting::Legacy, Greeting::Blocks];

    fn label(self) -> &'static str {
        match self {
            Greeting::Legacy => " Newer symbols ",
            Greeting::Blocks => " Block elements ",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Greeting::Legacy => "Needs a font with Symbols for Legacy Computing.",
            Greeting::Blocks => "Works in any font with block-drawing characters.",
        }
    }

    /// The mark exactly as fish will print it, minus the colour.
    fn art(self) -> Vec<&'static str> {
        match self {
            // One line, because that is how it renders: a font missing the
            // glyphs shows tofu or blanks right here, which is the point.
            Greeting::Legacy => vec!["▃🭕🭏🭕🭏 M I N I M A L"],
            Greeting::Blocks => vec!["   ████  ████▄", "▄▄▄ ▀███▄ ▀███▄", "▀███  ▀███  ▀███"],
        }
    }

    /// Rows the art needs, plus the block's borders and `pad_y` above and
    /// below. The caption is not counted: it rides on the bottom border. Get
    /// this wrong and the box clips its own contents.
    fn height(self, pad_y: u16) -> u16 {
        // The cast is over a 1- or 3-element literal array; it cannot overflow.
        u16::try_from(self.art().len()).unwrap_or(u16::MAX) + 2 + pad_y * 2
    }
}

// ---------------------------------------------------------------------------
// Fetching the upstream schemes
// ---------------------------------------------------------------------------

/// What fetching would do, decided by what is already on disk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FetchKind {
    /// Nothing there yet: `git clone --depth 1`.
    Clone,
    /// Already a checkout: `git pull --ff-only`.
    Update,
    /// The directory exists but is not a git checkout. `just fetch-schemes`
    /// deletes it and re-clones; the wizard refuses instead. Deleting a
    /// directory the user did not name, from a full-screen UI that has just
    /// covered the scrollback, is not something to do on a keypress.
    Blocked,
}

impl FetchKind {
    fn detect(dir: &Path) -> Self {
        if dir.join(".git").is_dir() {
            Self::Update
        } else if dir.exists() {
            Self::Blocked
        } else {
            Self::Clone
        }
    }

    fn question(self) -> &'static str {
        match self {
            Self::Clone => "Download the upstream scheme collection now?",
            Self::Update => "Update the upstream scheme collection now?",
            Self::Blocked => "The scheme directory is in the way.",
        }
    }

    fn detail(self, dir: &Path) -> String {
        let dir = dir.display();
        match self {
            Self::Clone => format!(
                "Shallow-clones tinted-theming into {dir}: hundreds of base16 and \
                 base24 schemes to build the loadout from. Skipping is fine — the \
                 two schemes in this repo work without it."
            ),
            Self::Update => {
                format!("Fast-forwards the checkout in {dir} to pick up new schemes.")
            }
            Self::Blocked => format!(
                "{dir} exists but is not a git checkout, so it cannot be updated \
                 and will not be deleted from here. Remove it yourself, or run \
                 `just fetch-schemes`, which replaces it."
            ),
        }
    }
}

/// Progress of the fetch, as far as the UI is concerned.
enum Fetch {
    Idle,
    /// The `u8` is the spinner frame; the receiver carries the outcome.
    Running(u8, Receiver<Result<String, String>>),
    Done(String),
    Failed(String),
}

/// Build the command that fetches the schemes. Mirrors `just fetch-schemes`.
fn fetch_command(kind: FetchKind, dir: &Path) -> Option<Command> {
    let mut cmd = Command::new("git");
    match kind {
        FetchKind::Update => {
            cmd.arg("-C").arg(dir).args(["pull", "--ff-only"]);
        }
        FetchKind::Clone => {
            cmd.args(["clone", "--depth", "1", SCHEMES_REPO]).arg(dir);
        }
        FetchKind::Blocked => return None,
    }
    Some(cmd)
}

/// Run the fetch on a thread so the UI keeps redrawing and stays quittable.
/// Doing it inline would freeze the terminal for the length of a clone, with no
/// way out of a stalled network but killing the process.
fn spawn_fetch(kind: FetchKind, dir: &Path) -> Fetch {
    let Some(mut cmd) = fetch_command(kind, dir) else {
        return Fetch::Failed("nothing to run".into());
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = match cmd.output() {
            Ok(out) if out.status.success() => {
                // git says everything useful on stderr, including progress and
                // "Already up to date.", so prefer it and fall back to stdout.
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let out = String::from_utf8_lossy(&out.stdout).trim().to_string();
                Ok(if err.is_empty() { out } else { err })
            }
            Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            Err(e) => Err(format!("could not run git: {e}")),
        };
        // The receiver is gone if the user quit mid-fetch; nothing to do.
        drop(tx.send(outcome));
    });
    Fetch::Running(0, rx)
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Greeting,
    Schemes,
}

struct App {
    screen: Screen,
    schemes_dir: PathBuf,

    greeting_row: usize,
    /// Set when the user confirms on the greeting screen. `None` means they
    /// quit without choosing, which has to stay distinguishable from picking
    /// the default.
    greeting: Option<Greeting>,

    fetch_kind: FetchKind,
    /// `true` = yes, the default when a fetch is actually possible.
    fetch_yes: bool,
    fetch: Fetch,

    done: bool,
}

impl App {
    fn new(schemes_dir: PathBuf) -> Self {
        let fetch_kind = FetchKind::detect(&schemes_dir);
        Self {
            screen: Screen::Greeting,
            schemes_dir,
            greeting_row: 0,
            greeting: None,
            fetch_kind,
            fetch_yes: fetch_kind != FetchKind::Blocked,
            fetch: Fetch::Idle,
            done: false,
        }
    }

    fn current_greeting(&self) -> Greeting {
        Greeting::ALL[self.greeting_row]
    }

    /// True while the fetch owns the screen: keys other than quit are ignored,
    /// because there is nothing sensible to do until it lands.
    fn busy(&self) -> bool {
        matches!(self.fetch, Fetch::Running(..))
    }

    fn on_key(&mut self, key: KeyEvent) {
        // Windows reports press *and* release; acting on both moves twice per
        // keystroke.
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        if ctrl_c || key.code == KeyCode::Char('q') {
            self.done = true;
            return;
        }
        if self.busy() {
            return;
        }
        match self.screen {
            Screen::Greeting => self.on_key_greeting(key),
            Screen::Schemes => self.on_key_schemes(key),
        }
    }

    fn on_key_greeting(&mut self, key: KeyEvent) {
        match key.code {
            // Esc on the first screen has nowhere to go back to.
            KeyCode::Esc => self.done = true,
            KeyCode::Up | KeyCode::Char('k') => {
                self.greeting_row = self.greeting_row.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.greeting_row = (self.greeting_row + 1).min(Greeting::ALL.len() - 1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.greeting = Some(self.current_greeting());
                self.screen = Screen::Schemes;
            }
            _ => {}
        }
    }

    fn on_key_schemes(&mut self, key: KeyEvent) {
        let can_fetch = self.fetch_kind != FetchKind::Blocked;
        match key.code {
            // Back to the greeting, so a mis-press is not a dead end.
            KeyCode::Esc => {
                self.screen = Screen::Greeting;
                self.fetch = Fetch::Idle;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') | KeyCode::Tab
                if can_fetch =>
            {
                self.fetch_yes = !self.fetch_yes;
            }
            KeyCode::Char('y' | 'Y') if can_fetch => self.fetch_yes = true,
            KeyCode::Char('n' | 'N') => self.fetch_yes = false,
            KeyCode::Enter | KeyCode::Char(' ') => match self.fetch {
                // A finished fetch: enter moves on rather than re-running it.
                Fetch::Done(_) | Fetch::Failed(_) => self.done = true,
                _ if self.fetch_yes && can_fetch => {
                    self.fetch = spawn_fetch(self.fetch_kind, &self.schemes_dir);
                }
                _ => self.done = true,
            },
            _ => {}
        }
    }

    /// Advance the spinner and collect the fetch result if it has landed.
    fn tick(&mut self) {
        let Fetch::Running(frame, rx) = &mut self.fetch else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(output)) => {
                self.fetch_kind = FetchKind::detect(&self.schemes_dir);
                self.fetch = Fetch::Done(output);
            }
            Ok(Err(why)) => self.fetch = Fetch::Failed(why),
            Err(mpsc::TryRecvError::Empty) => *frame = frame.wrapping_add(1),
            // The thread died without sending; do not spin forever.
            Err(mpsc::TryRecvError::Disconnected) => {
                self.fetch = Fetch::Failed("git exited without a result".into());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    // Order matters: `ratatui::init` installs a panic hook that restores the
    // terminal, and its own docs say it has to go on *after* any other hook, so
    // color_eyre's has to be installed first. Get this backwards and a panic
    // leaves the terminal in raw mode on the alternate screen.
    color_eyre::install()?;
    let args = Args::parse();

    let terminal = ratatui::init();
    let result = run(terminal, args.schemes);
    // Unconditional, and before `?`: a run that ends in an error still has to
    // hand the terminal back before the report is printed, or the report lands
    // on the alternate screen and vanishes with it.
    ratatui::restore();

    let app = result?;
    match app.greeting {
        Some(choice) => println!("greeting: {choice:?}"),
        None => println!("cancelled"),
    }
    match app.fetch {
        Fetch::Done(_) => println!("schemes: fetched"),
        Fetch::Failed(why) => println!("schemes: failed — {why}"),
        _ => println!("schemes: skipped"),
    }
    Ok(())
}

fn run(mut terminal: DefaultTerminal, schemes_dir: PathBuf) -> Result<App> {
    let mut app = App::new(schemes_dir);
    while !app.done {
        terminal.draw(|frame| draw(frame, &app))?;
        // Poll only while something is moving. With no fetch running there is
        // nothing to animate, so blocking on a key keeps an idle wizard off the
        // CPU entirely.
        if app.busy() {
            if event::poll(TICK)? {
                if let Event::Key(key) = event::read()? {
                    app.on_key(key);
                }
            }
            app.tick();
        } else if let Event::Key(key) = event::read()? {
            app.on_key(key);
        }
    }
    Ok(app)
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn draw(frame: &mut Frame, app: &App) {
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
        Screen::Greeting => draw_greeting(frame, inner, app),
        Screen::Schemes => draw_schemes(frame, inner, app),
    }
    frame.render_widget(
        Paragraph::new(Line::from(footer_hints(app))).alignment(Alignment::Center),
        footer,
    );
}

fn footer_hints(app: &App) -> Vec<Span<'static>> {
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
                [hint("enter", "finish"), hint("q", "quit")].concat()
            }
            _ => [
                hint("←/→", "yes/no"),
                hint("enter", "confirm"),
                hint("esc", "back"),
                hint("q", "quit"),
            ]
            .concat(),
        },
    }
}

fn intro_paragraph(heading: &str, body: String) -> Paragraph<'static> {
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

fn draw_greeting(frame: &mut Frame, inner: Rect, app: &App) {
    let intro = intro_paragraph(
        "Welcome to the minimal cozy loadout wizard.",
        "Pick the greeting that renders correctly here. If one shows boxes or gaps, \
         this font lacks those glyphs."
            .into(),
    );

    // Both the padding inside the boxes and the gap between them are comforts
    // that give way when rows run short — in that order, since the padding is
    // worth more than the gap. What never gives way is the art: at 50x18 an
    // unconditional padding clipped the block mark to a single line, and the
    // mark is the thing this screen exists to show. There is no spacer row
    // above the first box either; each box carries its own top padding.
    //
    // Ordered most- to least-generous; the first that fits wins.
    let options = u16::try_from(Greeting::ALL.len()).unwrap_or(1);
    let needed = |pad_y: u16, gap: u16| {
        INTRO_ROWS
            + Greeting::ALL.iter().map(|g| g.height(pad_y)).sum::<u16>()
            + gap * (options - 1)
    };
    let (pad_y, gap) = [
        (BOX_PADDING_Y, BOX_GAP),
        (BOX_PADDING_Y, 0),
        (0, BOX_GAP),
        (0, 0),
    ]
    .into_iter()
    .find(|&(pad_y, gap)| needed(pad_y, gap) <= inner.height)
    .unwrap_or((0, 0));

    let mut constraints = vec![Constraint::Length(INTRO_ROWS)];
    let mut option_rows = Vec::with_capacity(Greeting::ALL.len());
    for (i, greeting) in Greeting::ALL.iter().enumerate() {
        if i > 0 && gap > 0 {
            constraints.push(Constraint::Length(gap));
        }
        option_rows.push(constraints.len());
        constraints.push(Constraint::Length(greeting.height(pad_y)));
    }
    constraints.push(Constraint::Min(0));
    let areas = Layout::vertical(constraints).split(inner);

    frame.render_widget(intro, areas[0]);
    for (i, greeting) in Greeting::ALL.iter().enumerate() {
        draw_option(
            frame,
            areas[option_rows[i]],
            *greeting,
            pad_y,
            i == app.greeting_row,
        );
    }
}

fn draw_option(frame: &mut Frame, area: Rect, greeting: Greeting, pad_y: u16, selected: bool) {
    let accent = if selected {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .padding(Padding::symmetric(BOX_PADDING_X, pad_y))
        .title_bottom(Span::styled(
            format!(" {} ", greeting.note()),
            Style::default().fg(Color::DarkGray),
        ))
        .title(Span::styled(
            greeting.label(),
            if selected {
                Style::default().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(accent)
            },
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The art is deliberately unstyled: it renders in the terminal's own
    // foreground, which is what the user is being asked to judge.
    let lines: Vec<Line> = greeting.art().into_iter().map(Line::raw).collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_schemes(frame: &mut Frame, inner: Rect, app: &App) {
    let intro = intro_paragraph(
        app.fetch_kind.question(),
        app.fetch_kind.detail(&app.schemes_dir),
    );
    let [intro_area, _, status_area] = Layout::vertical([
        Constraint::Length(SCHEMES_INTRO_ROWS),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    let dim = Style::default().fg(Color::DarkGray);
    let status = match &app.fetch {
        Fetch::Idle if app.fetch_kind == FetchKind::Blocked => vec![Line::styled(
            "Nothing will be downloaded. Press enter to continue.",
            dim,
        )],
        Fetch::Idle => vec![choice_line(app.fetch_yes)],
        Fetch::Running(frame_no, _) => {
            const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
            let tick = SPINNER[(*frame_no as usize / 2) % SPINNER.len()];
            vec![Line::styled(
                format!("{tick} running git…"),
                Style::default().fg(Color::Cyan),
            )]
        }
        Fetch::Done(output) => {
            let mut lines = vec![Line::styled(
                "Done.",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )];
            lines.extend(
                output
                    .lines()
                    .take(4)
                    .map(|l| Line::styled(l.to_string(), dim)),
            );
            lines
        }
        Fetch::Failed(why) => {
            let mut lines = vec![Line::styled(
                "git failed.",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )];
            lines.extend(
                why.lines()
                    .take(4)
                    .map(|l| Line::styled(l.to_string(), dim)),
            );
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(Text::from(status)).wrap(Wrap { trim: true }),
        status_area,
    );
}

/// The yes/no row, with the active choice picked out.
fn choice_line(yes: bool) -> Line<'static> {
    let on = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let off = Style::default().fg(Color::DarkGray);
    Line::from(vec![
        Span::styled("  Yes  ", if yes { on } else { off }),
        Span::raw("   "),
        Span::styled("  No  ", if yes { off } else { on }),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path that cannot exist, so detection is deterministic in tests rather
    /// than depending on whether the repo has been fetched.
    fn app() -> App {
        App::new(PathBuf::from("target/does-not-exist-for-tests"))
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    /// A private directory per test; tests run in parallel in one process.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cozy-wizard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn render_app(app: &App, w: u16, h: u16) -> Vec<String> {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buf = terminal.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    fn render(w: u16, h: u16) -> Vec<String> {
        render_app(&app(), w, h)
    }

    /// The frame as one line of prose: borders dropped and whitespace
    /// collapsed, so a phrase the wrapper split across two rows still matches.
    /// Searching the raw rows for \"not a git checkout\" failed for exactly
    /// that reason — the text was right and the assertion was wrong.
    fn flatten(rows: &[String]) -> String {
        let stripped: String = rows
            .join(" ")
            .chars()
            .map(|c| {
                if "│─╭╮╰╯".contains(c) {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        stripped.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    // -- greeting screen ---------------------------------------------------

    #[test]
    fn quits_on_q_esc_and_ctrl_c() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let mut a = app();
            a.on_key(press(code));
            assert!(a.done, "{code:?} should end the loop");
            assert_eq!(a.greeting, None, "quitting is not a choice");
        }
        let mut a = app();
        a.on_key(KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        ));
        assert!(a.done);
    }

    #[test]
    fn selection_moves_and_clamps() {
        let mut a = app();
        assert_eq!(a.current_greeting(), Greeting::Legacy);
        a.on_key(press(KeyCode::Up));
        assert_eq!(
            a.current_greeting(),
            Greeting::Legacy,
            "up at the top stays put"
        );
        a.on_key(press(KeyCode::Down));
        assert_eq!(a.current_greeting(), Greeting::Blocks);
        a.on_key(press(KeyCode::Down));
        assert_eq!(
            a.current_greeting(),
            Greeting::Blocks,
            "down at the end stays put"
        );
        a.on_key(press(KeyCode::Char('k')));
        assert_eq!(a.current_greeting(), Greeting::Legacy);
    }

    #[test]
    fn enter_records_the_choice_and_advances() {
        let mut a = app();
        a.on_key(press(KeyCode::Down));
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.greeting, Some(Greeting::Blocks));
        assert_eq!(a.screen, Screen::Schemes, "enter moves to the next screen");
        assert!(!a.done, "choosing is not quitting");
    }

    #[test]
    fn release_and_repeat_are_ignored() {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let mut a = app();
            a.on_key(KeyEvent::new_with_kind(
                KeyCode::Down,
                KeyModifiers::NONE,
                kind,
            ));
            assert_eq!(
                a.current_greeting(),
                Greeting::Legacy,
                "{kind:?} should not move"
            );
        }
    }

    // -- schemes screen ----------------------------------------------------

    fn on_schemes() -> App {
        let mut a = app();
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Schemes);
        a
    }

    #[test]
    fn esc_goes_back_rather_than_quitting() {
        let mut a = on_schemes();
        a.on_key(press(KeyCode::Esc));
        assert_eq!(a.screen, Screen::Greeting, "esc should step back");
        assert!(!a.done, "esc on the second screen is not a quit");
    }

    #[test]
    fn yes_no_toggles() {
        let mut a = on_schemes();
        assert!(a.fetch_yes, "yes is the default when a fetch is possible");
        a.on_key(press(KeyCode::Char('n')));
        assert!(!a.fetch_yes);
        a.on_key(press(KeyCode::Char('y')));
        assert!(a.fetch_yes);
        a.on_key(press(KeyCode::Right));
        assert!(!a.fetch_yes, "arrows toggle too");
    }

    #[test]
    fn declining_finishes_without_running_git() {
        let mut a = on_schemes();
        a.on_key(press(KeyCode::Char('n')));
        a.on_key(press(KeyCode::Enter));
        assert!(a.done);
        assert!(
            matches!(a.fetch, Fetch::Idle),
            "no fetch should have started"
        );
    }

    #[test]
    fn detection_distinguishes_clone_update_and_blocked() {
        let dir = temp_dir("detect");
        assert_eq!(
            FetchKind::detect(&dir),
            FetchKind::Clone,
            "missing means clone"
        );

        std::fs::create_dir_all(dir.join(".git")).unwrap();
        assert_eq!(
            FetchKind::detect(&dir),
            FetchKind::Update,
            "a checkout means update"
        );

        std::fs::remove_dir_all(dir.join(".git")).unwrap();
        assert_eq!(
            FetchKind::detect(&dir),
            FetchKind::Blocked,
            "a non-checkout directory must not be silently deleted"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn blocked_offers_no_yes_and_runs_nothing() {
        let dir = temp_dir("blocked");
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = App::new(dir.clone());
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.fetch_kind, FetchKind::Blocked);
        assert!(
            !a.fetch_yes,
            "yes must not be preselected when it cannot run"
        );
        a.on_key(press(KeyCode::Char('y')));
        assert!(!a.fetch_yes, "y must not enable a fetch that cannot run");
        a.on_key(press(KeyCode::Enter));
        assert!(a.done);
        assert!(matches!(a.fetch, Fetch::Idle));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fetch_command_matches_the_kind() {
        let dir = Path::new("schemes/vendor");
        let clone = fetch_command(FetchKind::Clone, dir).unwrap();
        assert_eq!(clone.get_program(), "git");
        let args: Vec<_> = clone
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"clone".to_string()), "{args:?}");
        assert!(args.contains(&"--depth".to_string()), "{args:?}");
        assert!(args.contains(&SCHEMES_REPO.to_string()), "{args:?}");

        let update = fetch_command(FetchKind::Update, dir).unwrap();
        let args: Vec<_> = update
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"pull".to_string()), "{args:?}");
        assert!(args.contains(&"--ff-only".to_string()), "{args:?}");

        assert!(fetch_command(FetchKind::Blocked, dir).is_none());
    }

    #[test]
    fn fetch_matches_the_justfile_recipe() {
        // Two places now know how to fetch the schemes, and they must not
        // drift: a wizard that clones a different repo, or into a different
        // directory, than `just fetch-schemes` would be worse than no wizard.
        let justfile = include_str!("../../../../justfile");
        assert!(
            justfile.contains(SCHEMES_REPO),
            "justfile no longer clones {SCHEMES_REPO}"
        );
        assert!(
            justfile.contains("--depth 1") && justfile.contains("--ff-only"),
            "justfile no longer uses a shallow clone and a fast-forward pull"
        );
        assert!(
            justfile.contains("schemes/vendor") || justfile.contains("{{SCHEMES}}/vendor"),
            "justfile no longer uses schemes/vendor"
        );
        assert_eq!(
            Args::parse_from(["cozy-wizard"]).schemes,
            PathBuf::from("schemes/vendor")
        );
    }

    // -- layout ------------------------------------------------------------

    #[test]
    fn intro_fits_in_its_rows() {
        // INTRO_ROWS is a hand-picked constant, so the failure it guards
        // against is silent: the intro wraps to one line more than fits and the
        // end of the sentence simply vanishes. Two earlier guesses did that.
        for w in [50u16, 60, 72, 80, 100, 120] {
            let rows = render(w, 30);
            assert!(
                flatten(&rows).contains("glyphs."),
                "intro truncated at {w} columns — INTRO_ROWS is too small:\n{}",
                rows[..8].join("\n")
            );
        }
    }

    #[test]
    fn schemes_intro_fits_in_its_rows() {
        // The same hazard on the second screen, whose text is longer.
        let a = on_schemes();
        for w in [60u16, 72, 80, 100, 120] {
            let rows = render_app(&a, w, 30);
            assert!(
                flatten(&rows).contains("without it."),
                "schemes intro truncated at {w} columns:\n{}",
                rows[..9].join("\n")
            );
        }
    }

    #[test]
    fn both_greetings_are_shown_together() {
        let text = render(80, 24).join(" ");
        assert!(text.contains("▃"), "legacy mark missing");
        assert!(text.contains("🭕"), "legacy symbols missing");
        for line in Greeting::Blocks.art() {
            assert!(text.contains(line.trim()), "blocks mark missing {line:?}");
        }
    }

    #[test]
    fn art_survives_a_small_terminal() {
        // The padding is allowed to disappear when rows run short; the art is
        // not. Adding vertical padding without a fallback clipped the block
        // mark to one line at 50x18.
        for (w, h) in [(50u16, 18u16), (60, 20), (80, 24), (120, 40)] {
            let text = render(w, h).join(" ");
            for line in Greeting::Blocks.art() {
                assert!(
                    text.contains(line.trim()),
                    "block mark line {line:?} missing at {w}x{h}"
                );
            }
            assert!(text.contains("🭕"), "legacy mark missing at {w}x{h}");
        }
    }

    fn box_top(rows: &[String], title: &str) -> usize {
        rows.iter()
            .position(|r| r.contains(title))
            .expect("box not drawn")
    }

    #[test]
    fn gap_separates_the_options_when_there_is_room() {
        let rows = render(80, 24);
        let first_bottom = box_top(&rows, "Needs a font");
        let second_top = box_top(&rows, "Block elements");
        assert_eq!(
            second_top - first_bottom,
            2,
            "expected one blank row between the boxes:\n{}",
            rows[first_bottom..=second_top].join("\n")
        );
    }

    #[test]
    fn padding_is_present_when_there_is_room() {
        let rows = render(80, 24);
        let top = box_top(&rows, "Newer symbols");
        assert!(
            rows[top + 1]
                .trim_matches(|c| c == '│' || c == ' ')
                .is_empty(),
            "expected a blank padding row under the box title:\n{}",
            rows[top + 1]
        );
    }

    #[test]
    fn schemes_screen_asks_and_offers_both_answers() {
        let a = on_schemes();
        let text = flatten(&render_app(&a, 80, 24));
        assert!(
            text.contains("Download the upstream scheme collection now?"),
            "{text}"
        );
        assert!(
            text.contains("Yes") && text.contains("No"),
            "both answers must be visible"
        );
        assert!(
            text.contains("tinted-theming"),
            "should name what it downloads"
        );
    }

    #[test]
    fn blocked_screen_explains_itself() {
        let dir = temp_dir("blocked-ui");
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = App::new(dir.clone());
        a.on_key(press(KeyCode::Enter));
        let text = flatten(&render_app(&a, 100, 24));
        assert!(text.contains("not a git checkout"), "{text}");
        assert!(
            text.contains("just fetch-schemes"),
            "should name the recipe that can fix it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore = "prints frames for eyeballing; run with --ignored --nocapture"]
    fn dump_frames() {
        for (w, h) in [(50u16, 18u16), (60, 20), (80, 24)] {
            println!("\n=== greeting {w}x{h} ===");
            for row in render(w, h) {
                println!("|{}|", row.trim_end());
            }
        }
        let a = on_schemes();
        for (w, h) in [(60u16, 20u16), (80, 24)] {
            println!("\n=== schemes {w}x{h} ===");
            for row in render_app(&a, w, h) {
                println!("|{}|", row.trim_end());
            }
        }
    }
}
