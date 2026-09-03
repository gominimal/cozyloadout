//! cozy-wizard — interactive setup for the cozy loadout.
//!
//! Run it with `just wizard`. See AGENTS.md for the build pipeline.

mod picker;
mod state;

use picker::{Pick, Picker};
use state::State;

use clap::Parser;
use color_eyre::eyre::Result;
use cozy_theme::{
    discover, loadout_patches, mix, shadowed_by, user_patches, OptionalPackage, Options, Packages,
    Rgb, Scheme, SchemeEntry, SLOTS,
};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::ResetColor;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::fmt::Write as _;
use std::io;
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

/// Rows for the free-text package field: a rule, a blank, the field, and a
/// line echoing what was parsed.
const INPUT_ROWS: u16 = 4;

/// Rows for the package detail panel: a rule, a blank, the name-and-licence
/// line, and the description.
const DETAIL_ROWS: u16 = 4;

/// Rows for the theme screen's heading and guidance.
///
/// Deliberately tighter than the other screens: every row here is a row the
/// scheme list does not get, and the list is the screen. Five is the heading, a
/// blank, up to two wrapped lines, and one blank to separate the guidance from
/// the list — without that last row the text runs straight into the first
/// scheme name in a narrow terminal, where the guidance wraps to two lines.
/// `theme_intro_fits_in_its_rows` holds the sizing, the same way the other two
/// screens' intros are held.
const THEME_INTRO_ROWS: u16 = 5;

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

    /// Where to remember the answers between runs
    #[arg(long, default_value = state::FILE)]
    state: PathBuf,
}

// ---------------------------------------------------------------------------
// Greeting
// ---------------------------------------------------------------------------

/// Which fish greeting the loadout should install.
///
/// The three marked variants differ only in which Unicode block their glyphs
/// come from, and that is the whole point: a font either has them or draws
/// tofu, and no amount of describing it beats putting them on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Greeting {
    /// Symbols for Legacy Computing (the U+1FB00 block, Unicode 13). The
    /// sharpest mark and the least widely supported.
    Legacy,
    /// Geometric Shapes (U+25A0–U+25FF). Decades older than the block above,
    /// so a font that lacks those may well still have these.
    Geometric,
    /// Block Elements (U+2580–U+259F), which essentially every monospace font
    /// has had for decades.
    Blocks,
    /// No logo — just the line telling you how to detach.
    Text,
    /// Nothing at all: a silent shell.
    None,
}

impl Greeting {
    /// Order is best-looking first, then descending font support, then the two
    /// that have no mark at all. The sharpest mark leads even though its glyphs
    /// are the least widely available: this is the one screen where a font that
    /// cannot draw something says so plainly, and the next two options are
    /// right underneath for anyone whose font cannot.
    const ALL: [Greeting; 5] = [
        Greeting::Legacy,
        Greeting::Geometric,
        Greeting::Blocks,
        Greeting::Text,
        Greeting::None,
    ];

    /// Stable spelling for the state file and the renderer's `--greeting`.
    /// Not `Debug`, which is for programmers and free to change.
    fn key(self) -> &'static str {
        match self {
            Greeting::Legacy => "legacy",
            Greeting::Geometric => "geometric",
            Greeting::Blocks => "blocks",
            Greeting::Text => "text",
            Greeting::None => "none",
        }
    }

    /// The reverse. Anything unrecognised is `None` and the caller falls back
    /// to the default, which is what a hand-edited file should get.
    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.key() == key)
    }

    fn label(self) -> &'static str {
        match self {
            Greeting::Legacy => "Default",
            Greeting::Geometric => "Geometric shapes",
            Greeting::Blocks => "Block elements",
            Greeting::Text => "No logo",
            Greeting::None => "Nothing at all",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Greeting::Legacy => "Needs a font with Symbols for Legacy Computing.",
            Greeting::Geometric => "Geometric Shapes — older, and more widely available.",
            Greeting::Blocks => "Block-drawing characters, which any font has.",
            Greeting::Text => "Just the line telling you how to detach.",
            Greeting::None => "A silent shell.",
        }
    }

    /// The mark, exactly as fish will print it. Empty for the two variants
    /// that have none.
    fn art(self) -> Vec<&'static str> {
        match self {
            Greeting::Legacy => vec!["▃🭕🭏🭕🭏 M I N I M A L"],
            Greeting::Geometric => vec![".◥◣◥◣ M I N I M A L"],
            Greeting::Blocks => vec![
                "   ████  ████▄",
                "▄▄▄ ▀███▄ ▀███▄",
                "▀███  ▀███  ▀███",
                "  M I N I M A L",
            ],
            Greeting::Text | Greeting::None => Vec::new(),
        }
    }

    /// Whether the "ctrl-w to detach" line is printed under the mark.
    fn has_detach_line(self) -> bool {
        self != Greeting::None
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

/// The host home, which patch destinations are computed relative to.
fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Whether a typed token looks like a package name.
///
/// Deliberately conservative: lowercase letters, digits, and the punctuation
/// that appears in real registry names (`ca-certificates`, `procps-ng`,
/// `libstdc++`). Anything else is a typo, a shell fragment, or a paste
/// accident, and the page shows what it accepted so a rejection is visible.
fn is_package_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.+".contains(c))
        && s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

/// What to do with everything chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Generate,
    GenerateAndInstall,
    SaveOnly,
    Abort,
}

impl Action {
    // Install first: it is the whole point of running the wizard, so it is
    // what the cursor starts on rather than something to arrow down to.
    const ALL: [Action; 4] = [
        Action::GenerateAndInstall,
        Action::Generate,
        Action::SaveOnly,
        Action::Abort,
    ];

    fn label(self) -> &'static str {
        match self {
            Action::Generate => "Generate",
            Action::GenerateAndInstall => "Generate and install",
            Action::SaveOnly => "Save settings and exit",
            Action::Abort => "Abort",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Action::Generate => "Render the loadout into build/. Nothing outside this repo changes.",
            Action::GenerateAndInstall => {
                "Render, bundle, and unzip into ~/.config/minimal/loadouts/, replacing what is there."
            }
            Action::SaveOnly => "Remember these answers for next time without building anything.",
            Action::Abort => "Leave without building anything or remembering these answers.",
        }
    }

    /// Whether the answers are worth keeping. Abort is the only one that
    /// throws them away — that is what makes it different from the others.
    fn saves(self) -> bool {
        self != Action::Abort
    }
}

/// How an action turned out, for the line under the list.
enum Applied {
    Idle,
    Running(&'static str),
    Ok(String),
    Failed(String),
}

/// Render the loadout with everything this wizard collected.
///
/// A direct call into the library the `cozy-theme` binary is a thin CLI over,
/// so the wizard and `just theme` run literally the same code. It used to
/// spawn that binary instead, which meant finding it on disk and making every
/// path absolute because the child had its own working directory — two
/// problems that only existed because of the subprocess.
fn run_generate(app: &App, repo: &Path, install: bool) -> Result<String, String> {
    run_generate_to(app, repo, &repo.join("build"), install)
}

/// The same, with the output directory named — so a test can render somewhere
/// throwaway instead of over the checkout's `build/`.
fn run_generate_to(app: &App, repo: &Path, out: &Path, install: bool) -> Result<String, String> {
    let scheme = app
        .schemes
        .get(app.theme_row)
        .ok_or_else(|| "no scheme selected".to_string())?;

    let options = Options {
        home: app.home.clone(),
        scheme: scheme.path.clone(),
        templates: repo.join("templates"),
        out: out.to_path_buf(),
        greeting: app.greeting.unwrap_or(Greeting::Blocks).key().to_string(),
        // Always non-empty, even when nothing is chosen: an empty `with` means
        // "everything", which is the opposite of an empty checklist. A single
        // empty string is the explicit empty set.
        with: {
            let chosen = app.chosen_packages();
            if chosen.is_empty() {
                vec![String::new()]
            } else {
                chosen.into_iter().map(str::to_string).collect()
            }
        },
        patch_files: app
            .pickers
            .first()
            .map(|p| p.chosen.iter().cloned().collect())
            .unwrap_or_default(),
        patch_dirs: app
            .pickers
            .get(1)
            .map(|p| p.chosen.iter().cloned().collect())
            .unwrap_or_default(),
        ..Options::default()
    };
    cozy_theme::build(&options).map_err(|e| format!("{e}"))?;
    let mut report = format!("rendered {} into {}", scheme.name, out.display());

    if install {
        // In-process, like the render. Shelling out to `just install` printed
        // the recipe line and unzip's chatter straight through the alternate
        // screen, which is where the stray output in the wizard came from.
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_string())?;
        let dest = cozy_theme::install(out, "cozy", &cozy_theme::loadouts_dir(&home))
            .map_err(|e| format!("{e}"))?;
        let _ = write!(report, "\ninstalled into {}", dest.display());
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// What the package page's keys are aimed at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    List,
    /// The free-text field. While it has focus, printable keys are text —
    /// including `q`, which everywhere else quits.
    Input,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Greeting,
    Schemes,
    Themes,
    Packages,
    Patches,
    Apply,
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

    /// Every scheme on disk. Names only — parsing all of them at startup would
    /// read hundreds of files for a list that shows twenty.
    schemes: Vec<SchemeEntry>,
    theme_row: usize,
    /// First visible row, so the list scrolls rather than jumping.
    theme_top: usize,
    /// The selected scheme, parsed. `None` before the first load or when the
    /// file failed to parse — a broken scheme in the collection must not take
    /// the wizard down with it.
    loaded: Option<Scheme>,
    /// Rows the list had on the last frame. Key handling needs to know how far
    /// a page is, and it has no frame to ask — a `Cell` so `draw` can record
    /// it without taking `&mut App`.
    list_rows: std::cell::Cell<usize>,

    /// The optional packages, and whether each is wanted. Loaded from
    /// `templates/packages.toml`; empty if that file could not be read, which
    /// makes the page show its own explanation rather than an empty list.
    packages: Vec<OptionalPackage>,
    wanted: Vec<bool>,
    package_row: usize,
    package_top: usize,
    focus: Focus,
    /// Package names installed regardless of any choice on this page — used
    /// only to tell the user a typed name is already covered.
    always: Vec<String>,
    /// The two pickers on the patches page, and which one has the keys.
    /// Files and directories are separate because a loadout patches them
    /// differently: a file maps to one dest, a directory to a glob.
    pickers: Vec<Picker>,
    picker_focus: usize,

    /// What the last completed run chose. Consulted as each page opens rather
    /// than all at once, because the scheme list and the package list are only
    /// known once their page is entered.
    /// The summary page's cursor, and how the chosen action went.
    action_row: usize,
    applied: Applied,
    /// The repo the wizard is configuring — where `build/` and `templates/`
    /// live, and where `just` is run.
    repo: PathBuf,

    /// The host home, which patch destinations are computed relative to. A
    /// field rather than a call to `std::env` at the point of use: the tests
    /// need to point it at a fixture, and `set_var` is process-wide, so
    /// parallel tests doing that raced each other.
    home: PathBuf,

    saved: State,
    /// Set only by finishing the last page. Quitting leaves it false, so an
    /// abandoned run does not overwrite the answers from a finished one.
    completed: bool,

    /// Extra package names typed by hand, as raw text. Parsed on read rather
    /// than on every keystroke so the user can type freely — including the
    /// half-finished states that are not valid names yet.
    extra: String,

    done: bool,
}

impl App {
    fn with_state(schemes_dir: PathBuf, saved: State) -> Self {
        let fetch_kind = FetchKind::detect(&schemes_dir);
        // The greeting page is first, so its remembered answer is applied here
        // rather than on entry. An unrecognised value falls back to the
        // default, which is what a hand-edited file should get.
        let greeting_row = saved
            .greeting
            .as_deref()
            .and_then(Greeting::from_key)
            .and_then(|g| Greeting::ALL.iter().position(|x| *x == g))
            .unwrap_or(0);
        Self {
            screen: Screen::Greeting,
            schemes_dir,
            home: home(),
            greeting_row,
            greeting: None,
            fetch_kind,
            fetch_yes: fetch_kind != FetchKind::Blocked,
            fetch: Fetch::Idle,
            schemes: Vec::new(),
            theme_row: 0,
            theme_top: 0,
            loaded: None,
            list_rows: std::cell::Cell::new(10),
            packages: Vec::new(),
            wanted: Vec::new(),
            package_row: 0,
            package_top: 0,
            focus: Focus::List,
            always: Vec::new(),
            pickers: Vec::new(),
            picker_focus: 0,
            action_row: 0,
            applied: Applied::Idle,
            repo: PathBuf::from("."),
            saved,
            completed: false,
            extra: String::new(),
            done: false,
        }
    }

    /// Everything worth remembering from this run.
    ///
    /// The scheme-collection question is deliberately absent: whether to clone
    /// or pull is about the state of the disk right now, not a preference, and
    /// answering it once should not answer it forever.
    fn to_state(&self) -> State {
        State {
            greeting: self.greeting.map(|g| g.key().to_string()),
            theme: self.schemes.get(self.theme_row).map(|e| e.name.clone()),
            packages: self
                .packages
                .iter()
                .zip(&self.wanted)
                .map(|(o, keep)| (o.name.clone(), *keep))
                .collect(),
            extra: self.extra.clone(),
            files: self
                .pickers
                .first()
                .map(|p| p.chosen.iter().cloned().collect())
                .unwrap_or_default(),
            dirs: self
                .pickers
                .get(1)
                .map(|p| p.chosen.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }

    /// Colours to draw with: the selected scheme's, or the wizard's own before
    /// one is loaded.
    fn theme(&self) -> Theme {
        self.loaded
            .as_ref()
            .map_or_else(Theme::fallback, Theme::from_scheme)
    }

    /// The window of the list that fits on screen, with its true indices.
    fn visible_schemes(&self, rows: usize) -> impl Iterator<Item = (usize, &SchemeEntry)> {
        self.schemes
            .iter()
            .enumerate()
            .skip(self.theme_top)
            .take(rows)
    }

    /// Load the selected scheme so the UI can re-paint in it. A scheme that
    /// fails to parse leaves the previous colours up rather than blanking the
    /// screen; the list still moves.
    fn load_selected(&mut self) {
        if let Some(entry) = self.schemes.get(self.theme_row) {
            if let Ok(scheme) = Scheme::load(&entry.path) {
                self.loaded = Some(scheme);
            }
        }
    }

    /// Move the cursor and keep `theme_top` in step, so the selection stays on
    /// screen without the list jumping a page at a time.
    fn move_theme(&mut self, delta: isize, rows: usize) {
        if self.schemes.is_empty() {
            return;
        }
        let last = self.schemes.len() - 1;
        let row = isize::try_from(self.theme_row)
            .unwrap_or(0)
            .saturating_add(delta);
        let clamped = row.clamp(0, isize::try_from(last).unwrap_or(isize::MAX));
        self.theme_row = usize::try_from(clamped).unwrap_or(0);
        if self.theme_row < self.theme_top {
            self.theme_top = self.theme_row;
        } else if rows > 0 && self.theme_row >= self.theme_top + rows {
            self.theme_top = self.theme_row + 1 - rows;
        }
        self.load_selected();
    }

    /// Rows the list can show. The draw code derives this from the real area;
    /// key handling has no frame, so it uses the last one drawn.
    fn list_rows(&self) -> usize {
        self.list_rows.get().max(1)
    }

    /// The repository the wizard is configuring, derived from the schemes
    /// directory: `<repo>/schemes/vendor`.
    ///
    /// The empty check is load-bearing. `Path::new("schemes/vendor")` climbs to
    /// `"schemes"` and then to `""`, and an empty path is not the current
    /// directory — it is nothing. Passing it to `Command::current_dir` fails
    /// with "No such file or directory", which is how generating from a
    /// default `--schemes` broke.
    fn repo_root(&self) -> PathBuf {
        let root = self
            .schemes_dir
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."));
        if root.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            root.to_path_buf()
        }
    }

    /// `templates/`, found relative to the schemes directory — the same way
    /// `enter_themes` locates the repo the wizard is running inside.
    fn templates_dir(&self) -> PathBuf {
        self.repo_root().join("templates")
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
        // `q` quits everywhere except inside the text field, where it is just a
        // letter — a wizard that exits when you type `qt5` would be absurd.
        // Ctrl-C still gets you out from anywhere.
        let typing = self.screen == Screen::Packages && self.focus == Focus::Input;
        if ctrl_c || (key.code == KeyCode::Char('q') && !typing) {
            self.done = true;
            return;
        }
        if self.busy() {
            return;
        }
        match self.screen {
            Screen::Greeting => self.on_key_greeting(key),
            Screen::Schemes => self.on_key_schemes(key),
            Screen::Themes => self.on_key_themes(key),
            Screen::Packages => self.on_key_packages(key),
            Screen::Patches => self.on_key_patches(key),
            Screen::Apply => self.on_key_apply(key),
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

    /// Move to the theme browser, discovering what is on disk on the way in —
    /// after the fetch, so a scheme collection downloaded a moment ago is in
    /// the list.
    fn enter_themes(&mut self) {
        // The schemes directory is `<root>/vendor`, so its parent is what
        // `discover` walks.
        let root = self
            .schemes_dir
            .parent()
            .unwrap_or(&self.schemes_dir)
            .to_path_buf();
        // What to land on: this run's choice if there is one, otherwise the
        // remembered one. Re-applying the file on a second visit would undo a
        // change made this run, which is the whole reason for the distinction.
        // Either way it is looked up *by name*, so a re-cloned collection or a
        // freshly fetched one does not move the cursor somewhere arbitrary.
        let want = self
            .schemes
            .get(self.theme_row)
            .map(|e| e.name.clone())
            .or_else(|| self.saved.theme.clone());

        // Re-discovered every time rather than once: the user can go back,
        // fetch the collection, and return, and the new schemes should be here.
        self.schemes = discover(&root);
        self.theme_row = want
            .and_then(|name| self.schemes.iter().position(|s| s.name == name))
            .unwrap_or(0);
        self.theme_top = self.theme_row;
        self.load_selected();
        self.screen = Screen::Themes;
    }

    /// Move to the package chooser, loading the list on the way in.
    ///
    /// `templates` is found relative to the schemes directory, which is how the
    /// rest of the wizard already locates the repo it is running inside.
    fn enter_packages(&mut self, templates: &Path) {
        // Only on the first visit. Coming back to this page must show what the
        // user did this run, not what the sticky file remembers.
        if !self.packages.is_empty() {
            self.screen = Screen::Packages;
            return;
        }
        if let Ok(p) = Packages::load(&templates.join("packages.toml")) {
            self.always = p
                .base
                .packages
                .iter()
                .chain(&p.cozy.packages)
                .cloned()
                .collect();
            // Each package's remembered answer, falling back to its own
            // default when this file has never seen it.
            self.wanted = p
                .optional
                .iter()
                .map(|o| self.saved.wants_package(&o.name, o.default))
                .collect();
            self.packages = p.optional;
        }
        self.extra.clone_from(&self.saved.extra);
        self.package_row = 0;
        self.package_top = 0;
        self.screen = Screen::Packages;
    }

    fn current_package(&self) -> Option<&OptionalPackage> {
        self.packages.get(self.package_row)
    }

    /// Names the user has chosen to install, in list order.
    fn chosen_packages(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .packages
            .iter()
            .zip(&self.wanted)
            .filter(|(_, keep)| **keep)
            .map(|(o, _)| o.name.as_str())
            .collect();
        names.extend(self.extra_packages());
        names.dedup();
        names
    }

    fn move_package(&mut self, delta: isize, rows: usize) {
        if self.packages.is_empty() {
            return;
        }
        let last = self.packages.len() - 1;
        let row = isize::try_from(self.package_row)
            .unwrap_or(0)
            .saturating_add(delta);
        let clamped = row.clamp(0, isize::try_from(last).unwrap_or(isize::MAX));
        self.package_row = usize::try_from(clamped).unwrap_or(0);
        if self.package_row < self.package_top {
            self.package_top = self.package_row;
        } else if rows > 0 && self.package_row >= self.package_top + rows {
            self.package_top = self.package_row + 1 - rows;
        }
    }

    /// Names typed into the free-text field, split on whitespace and commas.
    ///
    /// Anything that is not a plausible package name is dropped rather than
    /// carried forward — the field shows what it parsed, so a rejected token is
    /// visible rather than silently installed as something odd.
    fn extra_packages(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .extra
            .split([' ', ',', '\t'])
            .map(str::trim)
            .filter(|n| !n.is_empty() && is_package_name(n))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Typed names that the loadout already installs, so the page can say so
    /// rather than letting the user think they added something.
    fn redundant_extras(&self) -> Vec<&str> {
        self.extra_packages()
            .into_iter()
            .filter(|n| {
                self.packages.iter().any(|o| o.name == *n) || self.always.iter().any(|a| a == n)
            })
            .collect()
    }

    fn on_key_input(&mut self, key: KeyEvent) {
        match key.code {
            // All three leave the field; none of them is a "cancel", because
            // the text is already the value — there is nothing to revert to.
            KeyCode::Esc | KeyCode::Enter | KeyCode::Tab => self.focus = Focus::List,
            KeyCode::Backspace => {
                self.extra.pop();
            }
            KeyCode::Char(c) => self.extra.push(c),
            _ => {}
        }
    }

    /// Move to the patches page, starting both pickers at $HOME — where the
    /// dotfiles a loadout patches in actually live.
    fn enter_patches(&mut self) {
        if self.pickers.is_empty() {
            let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
            let mut files = Picker::new(Pick::Files, &home);
            let mut dirs = Picker::new(Pick::Dirs, &home);
            // Anything the user has since deleted is simply not restored: it
            // is gone, which is not an error, so the page opens without it.
            files.chosen = State::existing(&self.saved.files).into_iter().collect();
            dirs.chosen = State::existing(&self.saved.dirs).into_iter().collect();
            self.pickers = vec![files, dirs];
        }
        self.picker_focus = 0;
        self.screen = Screen::Patches;
    }

    fn picker(&self) -> &Picker {
        &self.pickers[self.picker_focus]
    }

    /// Everything chosen across both pickers, files first.
    /// The loadout's own config files that the user's picks would displace.
    ///
    /// Not an error — the user asked for theirs specifically, so theirs wins —
    /// but it has to be said out loud. Silently dropping a config the loadout
    /// exists to install is the kind of thing you discover three sessions
    /// later wondering why your theme is half applied.
    fn displaced_configs(&self) -> Vec<String> {
        let Some(scheme) = &self.loaded else {
            return Vec::new();
        };
        let collect = |i: usize| -> Vec<PathBuf> {
            self.pickers
                .get(i)
                .map(|p| p.chosen.iter().cloned().collect())
                .unwrap_or_default()
        };
        let picks = user_patches(&collect(0), &collect(1), &self.home);
        if picks.is_empty() {
            return Vec::new();
        }
        let Ok(ours) = loadout_patches(&self.templates_dir(), "cozy", &scheme.slug) else {
            return Vec::new();
        };
        let chosen = self.chosen_packages();
        ours.into_iter()
            .filter(|p| {
                // A declined package's config is not written anyway, so
                // warning that it was displaced would be noise.
                p.package
                    .as_ref()
                    .is_none_or(|pkg| chosen.contains(&pkg.as_str()))
            })
            .filter(|p| shadowed_by(&p.dest, &picks).is_some())
            .map(|p| p.dest)
            .collect()
    }

    fn chosen_paths(&self) -> Vec<&PathBuf> {
        self.pickers.iter().flat_map(|p| p.chosen.iter()).collect()
    }

    fn enter_apply(&mut self) {
        self.repo = self.repo_root();
        self.applied = Applied::Idle;
        self.screen = Screen::Apply;
    }

    fn action(&self) -> Action {
        Action::ALL[self.action_row]
    }

    fn on_key_apply(&mut self, key: KeyEvent) {
        // Once the action has run, any key leaves. Naming two specific keys
        // made people hunt for them, and there is nothing else to do here:
        // re-running from the same screen would be a second build nobody asked
        // for, and a failure's message is printed on the way out so it is
        // still readable after the alternate screen is gone.
        if matches!(self.applied, Applied::Ok(_) | Applied::Failed(_)) {
            self.done = true;
            return;
        }
        match key.code {
            KeyCode::Esc => self.screen = Screen::Patches,
            KeyCode::Up | KeyCode::Char('k') => {
                self.action_row = self.action_row.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.action_row = (self.action_row + 1).min(Action::ALL.len() - 1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.apply(),
            _ => {}
        }
    }

    /// Carry out the highlighted action.
    fn apply(&mut self) {
        let action = self.action();
        // Abort is the one action that does not keep the answers. Everything
        // else — including generating without installing — counts as having
        // finished, so `main` writes the state file.
        self.completed = action.saves();
        match action {
            Action::Abort | Action::SaveOnly => self.done = true,
            Action::Generate | Action::GenerateAndInstall => {
                let install = action == Action::GenerateAndInstall;
                self.applied = Applied::Running(action.label());
                let repo = self.repo.clone();
                self.applied = match run_generate(self, &repo, install) {
                    Ok(report) => Applied::Ok(report),
                    Err(why) => Applied::Failed(why),
                };
            }
        }
    }

    fn on_key_patches(&mut self, key: KeyEvent) {
        let page = self.list_rows();
        let focus = self.picker_focus;
        match key.code {
            KeyCode::Esc => self.screen = Screen::Packages,
            // Tab rather than left/right: those walk the tree, which is the
            // more frequent action and wants the arrow keys.
            KeyCode::Tab | KeyCode::BackTab => {
                self.picker_focus = (focus + 1) % self.pickers.len();
            }
            KeyCode::Up | KeyCode::Char('k') => self.pickers[focus].move_cursor(-1, page),
            KeyCode::Down | KeyCode::Char('j') => self.pickers[focus].move_cursor(1, page),
            KeyCode::PageUp => {
                self.pickers[focus].move_cursor(-(isize::try_from(page).unwrap_or(10)), page);
            }
            KeyCode::PageDown => {
                self.pickers[focus].move_cursor(isize::try_from(page).unwrap_or(10), page);
            }
            KeyCode::Right | KeyCode::Char('l') => self.pickers[focus].descend(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                self.pickers[focus].ascend();
            }
            KeyCode::Char(' ') => {
                self.pickers[focus].toggle();
            }
            // Enter finishes rather than descending: descending is on the
            // arrow that points into the tree, which leaves enter free to mean
            // the same thing it means on every other page.
            KeyCode::Enter => self.enter_apply(),
            _ => {}
        }
    }

    fn on_key_packages(&mut self, key: KeyEvent) {
        if self.focus == Focus::Input {
            self.on_key_input(key);
            return;
        }
        let page = self.list_rows();
        match key.code {
            KeyCode::Esc => self.screen = Screen::Themes,
            KeyCode::Up | KeyCode::Char('k') => self.move_package(-1, page),
            KeyCode::Down | KeyCode::Char('j') => self.move_package(1, page),
            KeyCode::PageUp => self.move_package(-(isize::try_from(page).unwrap_or(10)), page),
            KeyCode::PageDown => self.move_package(isize::try_from(page).unwrap_or(10), page),
            KeyCode::Home => self.move_package(isize::MIN / 2, page),
            KeyCode::End => self.move_package(isize::MAX / 2, page),
            KeyCode::Char(' ') => {
                if let Some(w) = self.wanted.get_mut(self.package_row) {
                    *w = !*w;
                }
            }
            // Bulk toggles, because turning fifteen things off one at a time to
            // get a minimal session is a chore the keyboard can absorb.
            KeyCode::Char('a') => self.wanted.iter_mut().for_each(|w| *w = true),
            KeyCode::Char('n') => self.wanted.iter_mut().for_each(|w| *w = false),
            KeyCode::Tab | KeyCode::Char('i') => self.focus = Focus::Input,
            KeyCode::Enter => self.enter_patches(),
            _ => {}
        }
    }

    fn on_key_themes(&mut self, key: KeyEvent) {
        let page = self.list_rows();
        match key.code {
            KeyCode::Esc => self.screen = Screen::Schemes,
            KeyCode::Up | KeyCode::Char('k') => self.move_theme(-1, page),
            KeyCode::Down | KeyCode::Char('j') => self.move_theme(1, page),
            KeyCode::PageUp => self.move_theme(-(isize::try_from(page).unwrap_or(10)), page),
            KeyCode::PageDown => self.move_theme(isize::try_from(page).unwrap_or(10), page),
            KeyCode::Home => self.move_theme(isize::MIN / 2, page),
            KeyCode::End => self.move_theme(isize::MAX / 2, page),
            KeyCode::Enter => {
                let templates = self.templates_dir();
                self.enter_packages(&templates);
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
                Fetch::Done(_) | Fetch::Failed(_) => self.enter_themes(),
                _ if self.fetch_yes && can_fetch => {
                    self.fetch = spawn_fetch(self.fetch_kind, &self.schemes_dir);
                }
                _ => self.enter_themes(),
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

    let saved = State::load(&args.state);

    let terminal = ratatui::init();
    let result = run(terminal, args.schemes, saved);
    // Unconditional, and before `?`: a run that ends in an error still has to
    // hand the terminal back before the report is printed, or the report lands
    // on the alternate screen and vanishes with it.
    //
    // `ResetColor` first, and explicitly. Leaving the alternate screen restores
    // what was on the primary one, but the SGR state the last frame left set is
    // the terminal's, not the screen's — quitting from the theme pages, which
    // paint a background over everything, otherwise hands back a shell still
    // wearing someone else's colours. The scheme is for the wizard; it does not
    // outlive it.
    drop(execute!(io::stdout(), ResetColor));
    ratatui::restore();

    let app = result?;
    match app.greeting {
        Some(choice) => println!("greeting: {choice:?}"),
        None => println!("cancelled"),
    }
    match &app.fetch {
        Fetch::Done(_) => println!("schemes: fetched"),
        Fetch::Failed(why) => println!("schemes: failed — {why}"),
        _ => println!("schemes: skipped"),
    }
    if let Some(entry) = app.schemes.get(app.theme_row) {
        println!("theme: {}", entry.name);
    }
    if !app.packages.is_empty() {
        println!("packages: {}", app.chosen_packages().join(" "));
    }
    // Only a finished run is an answer. Quitting part-way leaves whatever the
    // last completed run chose, rather than half-overwriting it.
    if app.completed {
        match app.to_state().save(&args.state) {
            Ok(()) => println!("saved: {}", args.state.display()),
            // Not fatal: the run happened, the answers just will not persist.
            Err(why) => eprintln!("could not save {}: {why}", args.state.display()),
        }
    }

    // The outcome, after the alternate screen is gone. Without this a failure
    // vanishes with the screen it was drawn on, which is the one message the
    // user most needs to keep.
    match &app.applied {
        Applied::Ok(report) => println!("{report}"),
        Applied::Failed(why) => eprintln!("failed: {why}"),
        _ => {}
    }

    let paths = app.chosen_paths();
    if !paths.is_empty() {
        println!(
            "patches: {}",
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    Ok(())
}

fn run(mut terminal: DefaultTerminal, schemes_dir: PathBuf, saved: State) -> Result<App> {
    let mut app = App::with_state(schemes_dir, saved);
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
        Screen::Themes => draw_themes(frame, inner, app),
        Screen::Packages => draw_packages(frame, inner, app),
        Screen::Patches => draw_patches(frame, inner, app),
        Screen::Apply => draw_apply(frame, inner, app),
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
        "Pick a greeting. The preview below is exactly what fish prints — if a \
         mark shows boxes, this font lacks those glyphs."
            .into(),
    );

    // A list plus one preview, rather than a box per option. With five options
    // and a four-line mark among them, stacking a box each does not fit in a
    // 24-row terminal — and only the highlighted one is being judged anyway.
    let [intro_area, list_area, preview_area] = Layout::vertical([
        Constraint::Length(INTRO_ROWS),
        Constraint::Length(u16::try_from(Greeting::ALL.len()).unwrap_or(5)),
        Constraint::Min(3),
    ])
    .areas(inner);
    frame.render_widget(intro, intro_area);

    let items: Vec<ListItem> = Greeting::ALL
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let selected = i == app.greeting_row;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:<18}", g.label()), style),
                Span::styled(g.note(), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items), list_area);

    draw_greeting_preview(frame, preview_area, app.current_greeting());
}

/// The selected greeting as fish will print it.
///
/// Unstyled on purpose: this is the user judging their own font, so it renders
/// in the terminal's own foreground rather than in colours chosen here.
fn draw_greeting_preview(frame: &mut Frame, area: Rect, greeting: Greeting) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::symmetric(BOX_PADDING_X, BOX_PADDING_Y))
        .title(Span::styled(
            " what fish will print ",
            Style::default().fg(Color::DarkGray),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = greeting.art().into_iter().map(Line::raw).collect();
    if greeting.has_detach_line() {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::from(vec![
            Span::raw("Welcome to minimal! "),
            Span::styled("ctrl-w", Style::default().fg(Color::Cyan)),
            Span::raw(" to detach"),
        ]));
    } else {
        lines.push(Line::styled(
            "(a silent shell)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_schemes(frame: &mut Frame, inner: Rect, app: &App) {
    // Once the fetch has settled the question has been answered, so stop asking
    // it. Leaving `fetch_kind.question()` up read as "Update ... now?" directly
    // above "Done.", with an explanation underneath describing something that
    // has already happened.
    let settled = matches!(app.fetch, Fetch::Done(_) | Fetch::Failed(_));
    let (heading, detail) = match &app.fetch {
        Fetch::Done(_) => ("The scheme collection is ready.", String::new()),
        Fetch::Failed(_) => ("The scheme collection was not fetched.", String::new()),
        _ => (
            app.fetch_kind.question(),
            app.fetch_kind.detail(&app.schemes_dir),
        ),
    };

    let [intro_area, _, status_area] = Layout::vertical([
        Constraint::Length(if settled { 2 } else { SCHEMES_INTRO_ROWS }),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    frame.render_widget(intro_paragraph(heading, detail), intro_area);

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
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                CONTINUE_HINT,
                Style::default().fg(Color::Cyan),
            ));
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
            lines.push(Line::raw(""));
            // Failing is not fatal: the two schemes in this repo still work, so
            // the way forward is the same key.
            lines.push(Line::styled(
                "Press enter to continue without it.",
                Style::default().fg(Color::Yellow),
            ));
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(Text::from(status)).wrap(Wrap { trim: true }),
        status_area,
    );
}

/// Shown once the fetch has settled. The footer carries the same key, but a
/// full-screen UI that has just finished a long-running job should say what to
/// do next where the user is already looking.
const CONTINUE_HINT: &str = "Press enter to continue.";

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

// ---------------------------------------------------------------------------
// Theme preview
// ---------------------------------------------------------------------------

/// A loaded scheme mapped onto the roles this UI paints with.
///
/// base16 assigns meaning to the slots, so this is a mapping rather than a
/// choice: base00–base03 are the greyscale surface from background up, base04–
/// base07 the foreground ramp, base08–base0F the accents. Everything the
/// preview draws comes from here, which is what makes the preview honest — if
/// a scheme has unreadable comments, they are unreadable here too.
struct Theme {
    bg: Color,
    surface: Color,
    selection: Color,
    comment: Color,
    fg: Color,
    bright: Color,
    red: Color,
    orange: Color,
    yellow: Color,
    green: Color,
    cyan: Color,
    blue: Color,
    magenta: Color,
    /// delta's diff backgrounds: the accent blended into base00, exactly as
    /// `templates/delta/delta.gitconfig` computes them.
    minus_bg: Color,
    plus_bg: Color,
}

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

impl Theme {
    fn from_scheme(scheme: &Scheme) -> Self {
        let slot = |name: &str| scheme.palette[name];
        Self {
            bg: rgb(slot("base00")),
            surface: rgb(slot("base01")),
            selection: rgb(slot("base02")),
            comment: rgb(slot("base03")),
            fg: rgb(slot("base05")),
            bright: rgb(slot("base07")),
            red: rgb(slot("base08")),
            orange: rgb(slot("base09")),
            yellow: rgb(slot("base0A")),
            green: rgb(slot("base0B")),
            cyan: rgb(slot("base0C")),
            blue: rgb(slot("base0D")),
            magenta: rgb(slot("base0E")),
            minus_bg: rgb(mix(slot("base08"), slot("base00"), 15.0)),
            plus_bg: rgb(mix(slot("base0B"), slot("base00"), 15.0)),
        }
    }

    /// The wizard's own colours when no scheme is loaded yet, so the chrome
    /// does not flash on the first frame.
    fn fallback() -> Self {
        let g = Color::DarkGray;
        Self {
            bg: Color::Reset,
            surface: Color::Reset,
            selection: g,
            comment: g,
            fg: Color::Reset,
            bright: Color::White,
            red: Color::Red,
            orange: Color::Yellow,
            yellow: Color::Yellow,
            green: Color::Green,
            cyan: Color::Cyan,
            blue: Color::Blue,
            magenta: Color::Magenta,
            minus_bg: Color::Reset,
            plus_bg: Color::Reset,
        }
    }
}

/// One span of the sample code, tagged with the role that colours it.
///
/// Real syntax highlighting would mean syntect and a grammar; this is a fixed
/// snippet, so the spans are written out by hand. That keeps the preview
/// dependency-free and, more usefully, exercises the same slots the helix and
/// bat themes actually assign — see AGENTS.md's per-tool notes.
enum Tok {
    Kw,
    Fn,
    Str,
    Num,
    Comment,
    Type,
    Plain,
}

const SAMPLE_CODE: &[&[(Tok, &str)]] = &[
    &[(Tok::Comment, "// Blend an accent into the surface.")],
    &[
        (Tok::Kw, "pub fn "),
        (Tok::Fn, "mix"),
        (Tok::Plain, "("),
        (Tok::Plain, "fg"),
        (Tok::Plain, ": "),
        (Tok::Type, "Rgb"),
        (Tok::Plain, ", pct: "),
        (Tok::Type, "f64"),
        (Tok::Plain, ") -> "),
        (Tok::Type, "Rgb"),
        (Tok::Plain, " {"),
    ],
    &[
        (Tok::Plain, "    "),
        (Tok::Kw, "let "),
        (Tok::Plain, "f = pct / "),
        (Tok::Num, "100.0"),
        (Tok::Plain, ";"),
    ],
    &[
        (Tok::Plain, "    "),
        (Tok::Kw, "if "),
        (Tok::Plain, "name == "),
        (Tok::Str, "\"base0D\""),
        (Tok::Plain, " { "),
        (Tok::Kw, "return "),
        (Tok::Plain, "fg; }"),
    ],
    &[(Tok::Plain, "}")],
];

impl Tok {
    fn color(&self, t: &Theme) -> Color {
        match self {
            Tok::Kw => t.magenta,
            Tok::Fn => t.blue,
            Tok::Str => t.green,
            Tok::Num => t.orange,
            Tok::Comment => t.comment,
            Tok::Type => t.yellow,
            Tok::Plain => t.fg,
        }
    }
}

/// The scheme list, and the preview that re-paints as it moves.
fn draw_themes(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();

    // Paint the whole area in the scheme's background first. Widgets below
    // only set foregrounds, so without this the preview would sit on the
    // terminal's own background and the scheme would look wrong.
    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), inner);

    let [intro_area, columns] =
        Layout::vertical([Constraint::Length(THEME_INTRO_ROWS), Constraint::Min(0)]).areas(inner);

    // The heading is styled from the scheme too, so it re-paints with
    // everything else rather than sitting there in the wizard's own colours.
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled(
                "Pick a colour scheme.",
                Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::styled(
                "Everything re-paints as you move, so this is how the loadout will \
                 look. Enter picks it.",
                Style::default().fg(t.fg),
            ),
        ]))
        .wrap(Wrap { trim: true }),
        intro_area,
    );

    // A narrow terminal cannot show both columns; the list is the one you
    // cannot do without, so the preview is what goes.
    let show_preview = columns.width >= 72;
    let [list_area, preview_area] = if show_preview {
        Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).areas(columns)
    } else {
        [columns, Rect::ZERO]
    };

    draw_theme_list(frame, list_area, app, &t, show_preview);
    if show_preview {
        draw_preview(frame, preview_area, app, &t);
    }
}

fn draw_theme_list(frame: &mut Frame, area: Rect, app: &App, t: &Theme, divider: bool) {
    // The right border is the divider between the two columns, so it only
    // makes sense when there is a second column; drawn regardless it reads as
    // a stray line down the edge of a narrow terminal.
    let block = Block::default()
        .borders(if divider {
            Borders::RIGHT
        } else {
            Borders::NONE
        })
        .border_style(Style::default().fg(t.selection))
        // base01 is the raised-surface slot, so the list column sits on it and
        // the two panes read as separate surfaces — the same relationship the
        // themed tools' own UIs use.
        .style(Style::default().bg(t.surface))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = inner.height.saturating_sub(1) as usize;
    // Key handling has no frame to measure, so record what this one had.
    app.list_rows.set(rows);
    let items: Vec<ListItem> = app
        .visible_schemes(rows)
        .map(|(i, entry)| {
            let selected = i == app.theme_row;
            let style = if selected {
                Style::default()
                    .fg(t.bg)
                    .bg(t.blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.fg)
            };
            let source = if selected {
                Style::default().fg(t.bg).bg(t.blue)
            } else {
                Style::default().fg(t.comment)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<20}", truncate(&entry.name, 20)), style),
                Span::styled(format!("{:>7}", entry.source), source),
            ]))
        })
        .collect();
    frame.render_widget(List::new(items), inner);

    // Position, so a long list does not feel bottomless.
    let counter = format!("{}/{}", app.theme_row + 1, app.schemes.len());
    let y = inner.y + inner.height.saturating_sub(1);
    frame.render_widget(
        Paragraph::new(Line::styled(counter, Style::default().fg(t.comment))),
        Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        },
    );
}

fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else {
        s.chars().take(width.saturating_sub(1)).collect::<String>() + "…"
    }
}

/// Optional packages: a checklist, with what each one is and what it is
/// licensed under for the row under the cursor.
fn draw_packages(frame: &mut Frame, inner: Rect, app: &App) {
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
fn draw_extra_input(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
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

fn draw_package_list(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
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

fn draw_package_detail(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
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

/// Two pickers side by side: files on the left, directories on the right.
///
/// Separate rather than one browser with a mode, because a loadout patches the
/// two differently — a file maps to a single `dest`, a directory to a glob —
/// and because seeing both sets of choices at once is the point.
/// The four lines of "here is what you chose".
///
/// The scheme-collection answer is deliberately absent: it was about the state
/// of the disk a moment ago, not a choice worth reviewing.
fn summary_paragraph(app: &App, t: &Theme) -> Paragraph<'static> {
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
fn draw_apply(frame: &mut Frame, inner: Rect, app: &App) {
    let t = app.theme();
    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), inner);

    let [intro_area, summary_area, list_area, status_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(8),
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

fn draw_patches(frame: &mut Frame, inner: Rect, app: &App) {
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

    // Side by side while there is room; stacked would halve an already short
    // listing, so a narrow terminal shows only the focused picker instead.
    if body.width >= 72 {
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
                &t,
            );
        }
    } else {
        draw_picker(frame, body, app.picker(), true, &app.home, &t);
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

/// `~` for the home directory, because absolute paths are mostly prefix and
/// the summary line has no room to spare.
fn shorten_home(path: &Path, home: &Path) -> String {
    let full = path.display().to_string();
    let home = home.display().to_string();
    if home.is_empty() || !full.starts_with(&home) {
        return full;
    }
    format!("~{}", &full[home.len()..])
}

fn draw_picker(frame: &mut Frame, area: Rect, p: &Picker, focused: bool, home: &Path, t: &Theme) {
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

fn draw_preview(frame: &mut Frame, area: Rect, app: &App, t: &Theme) {
    let block = Block::default().padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(scheme) = &app.loaded else {
        frame.render_widget(
            Paragraph::new(Line::styled("…", Style::default().fg(t.comment))),
            inner,
        );
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    // Heading: the scheme's own name and which way round it reads.
    lines.push(Line::from(vec![
        Span::styled(
            scheme.name.clone(),
            Style::default().fg(t.bright).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", scheme.variant()),
            Style::default().fg(t.comment),
        ),
    ]));
    lines.push(Line::raw(""));

    // The palette itself, so the accents are visible even where the samples
    // below happen not to use one.
    let mut swatches: Vec<Span> = Vec::new();
    for slot in SLOTS {
        let c = scheme.palette[slot];
        swatches.push(Span::styled("██", Style::default().fg(rgb(c))));
    }
    lines.push(Line::from(swatches));
    lines.push(Line::raw(""));

    // A prompt, as starship draws it.
    lines.push(Line::from(vec![
        Span::styled("~/dev/cozyloadout", Style::default().fg(t.cyan)),
        Span::styled(" on ", Style::default().fg(t.comment)),
        Span::styled(" main", Style::default().fg(t.magenta)),
        Span::styled(" [!] ", Style::default().fg(t.orange)),
        Span::styled("via ", Style::default().fg(t.comment)),
        Span::styled("🦀 v1.97.1", Style::default().fg(t.red)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "❯ ",
            Style::default().fg(t.green).add_modifier(Modifier::BOLD),
        ),
        Span::styled("just theme ", Style::default().fg(t.fg)),
        Span::styled(
            &app.schemes[app.theme_row].name,
            Style::default().fg(t.yellow),
        ),
    ]));
    lines.push(Line::raw(""));

    // Syntax highlighting, as helix and bat draw it.
    for row in SAMPLE_CODE {
        lines.push(Line::from(
            row.iter()
                .map(|(tok, text)| Span::styled(*text, Style::default().fg(tok.color(t))))
                .collect::<Vec<_>>(),
        ));
    }
    lines.push(Line::raw(""));

    // A diff, as delta draws it — including the blended backgrounds, which are
    // the one part of the loadout's colour that is computed rather than picked.
    lines.push(Line::styled(
        "modified  templates/fish/config.fish",
        Style::default().fg(t.yellow),
    ));
    lines.push(Line::from(Span::styled(
        "-    set -g fish_greeting \"\"",
        Style::default().fg(t.red).bg(t.minus_bg),
    )));
    lines.push(Line::from(Span::styled(
        "+    set -g fish_greeting $mark",
        Style::default().fg(t.green).bg(t.plus_bg),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path that cannot exist, so detection is deterministic in tests rather
    /// than depending on whether the repo has been fetched.
    fn app() -> App {
        App::with_state(
            PathBuf::from("target/does-not-exist-for-tests"),
            State::default(),
        )
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
        // Written against ALL rather than named variants: the list has grown
        // from two to five once already, and a test that hard-codes which
        // variant sits where breaks every time it does.
        let mut a = app();
        let last = *Greeting::ALL.last().unwrap();
        assert_eq!(a.current_greeting(), Greeting::ALL[0]);
        a.on_key(press(KeyCode::Up));
        assert_eq!(
            a.current_greeting(),
            Greeting::ALL[0],
            "up at the top stays put"
        );
        a.on_key(press(KeyCode::Down));
        assert_eq!(a.current_greeting(), Greeting::ALL[1]);
        for _ in 0..Greeting::ALL.len() + 3 {
            a.on_key(press(KeyCode::Down));
        }
        assert_eq!(a.current_greeting(), last, "down at the end stays put");
        a.on_key(press(KeyCode::Char('k')));
        assert_eq!(a.current_greeting(), Greeting::ALL[Greeting::ALL.len() - 2]);
    }

    #[test]
    fn the_preview_shows_only_the_selected_greeting() {
        // Five options and one preview: moving the cursor has to change what is
        // drawn, or the page is claiming something it does not do.
        let mut a = app();
        for expected in Greeting::ALL {
            assert_eq!(a.current_greeting(), expected);
            let text = flatten(&render_app(&a, 90, 30));
            for line in expected.art() {
                // `flatten` collapses runs of spaces, so the expected line has
                // to be collapsed the same way — the block mark has a double
                // space inside it.
                let want = line.split_whitespace().collect::<Vec<_>>().join(" ");
                assert!(text.contains(&want), "{expected:?}: {line:?} not previewed");
            }
            // Nobody else's mark is on screen at the same time.
            for other in Greeting::ALL {
                if other == expected {
                    continue;
                }
                if let Some(first) = other.art().first() {
                    let head: String = first.chars().take(4).collect();
                    assert!(
                        !text.contains(&head) || expected.art().iter().any(|l| l.contains(&head)),
                        "{other:?}'s mark leaked into {expected:?}'s preview"
                    );
                }
            }
            a.on_key(press(KeyCode::Down));
        }
    }

    #[test]
    fn the_markless_greetings_preview_honestly() {
        let mut a = app();
        while a.current_greeting() != Greeting::Text {
            a.on_key(press(KeyCode::Down));
        }
        let text = flatten(&render_app(&a, 90, 30));
        assert!(
            text.contains("ctrl-w to detach"),
            "the detach line is the whole option"
        );
        assert!(!text.contains("████"), "no mark should be drawn");

        a.on_key(press(KeyCode::Down));
        assert_eq!(a.current_greeting(), Greeting::None);
        let text = flatten(&render_app(&a, 90, 30));
        assert!(
            text.contains("(a silent shell)"),
            "an empty preview must say it is empty"
        );
        assert!(
            !text.contains("ctrl-w to detach"),
            "nothing is printed at all"
        );
    }

    #[test]
    fn the_blocky_mark_carries_the_wordmark_too() {
        let art = Greeting::Blocks.art();
        assert!(art.iter().any(|l| l.contains("████")), "the mark itself");
        assert_eq!(
            art.last().map(|l| l.trim()),
            Some("M I N I M A L"),
            "and the wordmark under it"
        );
    }

    #[test]
    fn every_greeting_matches_what_the_template_will_print() {
        // The preview is a promise about the generated config. If the template
        // stops carrying one of these marks, the promise is a lie.
        let template = include_str!("../../../../../templates/fish/config.fish");
        for g in Greeting::ALL {
            for line in g.art() {
                assert!(
                    template.contains(line),
                    "{g:?}: templates/fish/config.fish no longer contains {line:?}"
                );
            }
            assert!(
                template.contains(&format!("greeting == \"{}\"", g.key())) || g == Greeting::Blocks,
                "{g:?}: the template has no branch for {:?}",
                g.key()
            );
        }
    }

    #[test]
    fn the_geometric_mark_avoids_the_glyphs_the_legacy_one_needs() {
        // The point of offering it: Geometric Shapes (U+25A0-U+25FF) are
        // decades older than Symbols for Legacy Computing, so a font without
        // the latter may well still have these.
        let art = Greeting::Geometric.art()[0];
        for c in art.chars() {
            assert!(
                !('\u{1FB00}'..='\u{1FBFF}').contains(&c),
                "{c:?} is a Legacy Computing glyph, which this option exists to avoid"
            );
        }
        assert!(art.chars().any(|c| ('\u{25A0}'..='\u{25FF}').contains(&c)));
    }

    #[test]
    fn enter_records_the_choice_and_advances() {
        let mut a = app();
        a.on_key(press(KeyCode::Down));
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.greeting, Some(Greeting::ALL[1]));
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
                Greeting::ALL[0],
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
    fn declining_advances_without_running_git() {
        let mut a = on_schemes();
        a.on_key(press(KeyCode::Char('n')));
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Themes, "declining still moves on");
        assert!(!a.done, "declining is not quitting");
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
        let mut a = App::with_state(dir.clone(), State::default());
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.fetch_kind, FetchKind::Blocked);
        assert!(
            !a.fetch_yes,
            "yes must not be preselected when it cannot run"
        );
        a.on_key(press(KeyCode::Char('y')));
        assert!(!a.fetch_yes, "y must not enable a fetch that cannot run");
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Themes, "a blocked fetch still moves on");
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
        let justfile = include_str!("../../../../../justfile");
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
                flatten(&rows).contains("those glyphs."),
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
        let mut a = App::with_state(dir.clone(), State::default());
        a.on_key(press(KeyCode::Enter));
        let text = flatten(&render_app(&a, 100, 24));
        assert!(text.contains("not a git checkout"), "{text}");
        assert!(
            text.contains("just fetch-schemes"),
            "should name the recipe that can fix it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The theme browser against the real collection in this repo.
    fn on_themes() -> App {
        let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), State::default());
        a.on_key(press(KeyCode::Enter));
        a.on_key(press(KeyCode::Char('n')));
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Themes);
        a
    }

    /// The background colour the frame is painted in.
    fn frame_bg(app: &App, w: u16, h: u16) -> Color {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        // A cell inside the preview, away from borders and text.
        terminal.backend().buffer()[(w - 4, h - 4)].bg
    }

    /// Drive a fetch to completion without the network: point the wizard at a
    /// local repo so `git clone` is real but instant.
    fn app_after_fetch(tag: &str) -> App {
        let origin = temp_dir(&format!("{tag}-origin"));
        std::fs::create_dir_all(&origin).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
        ] {
            Command::new("git")
                .arg("-C")
                .arg(&origin)
                .args(args)
                .output()
                .unwrap();
        }
        std::fs::write(origin.join("scheme.yaml"), "x\n").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["add", "-A"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["commit", "-qm", "init"])
            .output()
            .unwrap();

        let dest = temp_dir(&format!("{tag}-dest"));
        let mut a = App::with_state(dest, State::default());
        a.on_key(press(KeyCode::Enter));
        // Clone from the local origin rather than over the network.
        a.fetch = {
            let (tx, rx) = mpsc::channel();
            let out = Command::new("git")
                .args(["clone", "--depth", "1"])
                .arg(&origin)
                .arg(&a.schemes_dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            tx.send(Ok(String::from_utf8_lossy(&out.stderr).trim().to_string()))
                .unwrap();
            Fetch::Running(0, rx)
        };
        a.tick();
        let _ = std::fs::remove_dir_all(&origin);
        a
    }

    #[test]
    fn a_finished_fetch_says_how_to_continue() {
        // Without this the screen just sits there showing "Done." and the user
        // has to guess, or find the key in the footer strip.
        let mut a = app_after_fetch("done-hint");
        assert!(
            matches!(a.fetch, Fetch::Done(_)),
            "fetch should have landed"
        );

        let text = flatten(&render_app(&a, 80, 24));
        assert!(text.contains("Done."), "{text}");
        assert!(
            text.contains(CONTINUE_HINT),
            "no continue instruction:\n{text}"
        );
        assert!(
            text.contains("enter continue"),
            "footer should agree with the screen"
        );

        // And the key it names actually works.
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Themes, "enter should continue, not quit");
        assert!(!a.done);
        let _ = std::fs::remove_dir_all(&a.schemes_dir);
    }

    #[test]
    fn a_failed_fetch_also_says_how_to_continue() {
        let mut a = on_schemes();
        a.fetch = Fetch::Failed("fatal: could not read from remote".into());
        let text = flatten(&render_app(&a, 80, 24));
        assert!(text.contains("git failed."), "{text}");
        assert!(
            text.contains("Press enter to continue without it."),
            "a failure must not look like a dead end:\n{text}"
        );
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Themes, "a failed fetch still moves on");
    }

    #[test]
    fn theme_intro_fits_in_its_rows() {
        // The third hand-sized intro in this file, and the third chance to
        // silently eat the end of a sentence. Same guard as the other two.
        let a = on_themes();
        for w in [56u16, 60, 72, 80, 100, 120] {
            let rows = render_app(&a, w, 30);
            assert!(
                flatten(&rows).contains("Enter picks it."),
                "theme intro truncated at {w} columns — THEME_INTRO_ROWS is too small:\n{}",
                rows[..7].join("\n")
            );
        }
    }

    #[test]
    fn the_intro_does_not_run_into_the_list() {
        // A blank row has to survive between the guidance and the first scheme
        // name, including at the narrow widths where the guidance wraps to two
        // lines and used up the whole block.
        for w in [60u16, 80, 100] {
            let rows = render_app(&on_themes(), w, 24);
            let first_item = rows
                .iter()
                .position(|r| r.contains("0x96f"))
                .expect("list should be drawn");
            let above = &rows[first_item - 1];
            assert!(
                above.trim_matches(|c| c == '│' || c == ' ').is_empty(),
                "no separator above the list at {w} columns: {above:?}"
            );
        }
    }

    #[test]
    fn scrolling_repaints_the_ui_in_the_selected_scheme() {
        // The whole point of the screen. Assert against the schemes' own
        // base00 rather than "the colour changed", so a bug that repaints in
        // some *other* scheme's colours still fails.
        let mut a = on_themes();
        assert!(a.schemes.len() > 50, "expected the vendored collection");

        let first = a.loaded.as_ref().expect("first scheme should load");
        let want = first.palette["base00"];
        assert_eq!(
            frame_bg(&a, 100, 30),
            Color::Rgb(want.r, want.g, want.b),
            "frame should be painted in {}'s base00",
            first.name
        );

        // Move somewhere with a definitely different background.
        let start = a.loaded.as_ref().unwrap().palette["base00"];
        let mut moved = false;
        for _ in 0..80 {
            a.on_key(press(KeyCode::Down));
            if a.loaded.as_ref().unwrap().palette["base00"] != start {
                moved = true;
                break;
            }
        }
        assert!(moved, "no scheme in 80 rows had a different background");

        let now = a.loaded.as_ref().unwrap().palette["base00"];
        assert_eq!(
            frame_bg(&a, 100, 30),
            Color::Rgb(now.r, now.g, now.b),
            "frame should have repainted in {}'s base00",
            a.schemes[a.theme_row].name
        );
    }

    #[test]
    fn every_discovered_scheme_is_selectable() {
        // A scheme that fails to parse must not blank the preview or wedge the
        // list; `load_selected` keeps the previous colours. Walk the whole
        // collection and assert something stays loaded the entire way.
        let mut a = on_themes();
        let total = a.schemes.len();
        for i in 0..total {
            a.on_key(press(KeyCode::Down));
            assert!(a.loaded.is_some(), "preview went blank at row {i}");
        }
        assert_eq!(a.theme_row, total - 1, "end of the list should clamp");
    }

    #[test]
    fn the_list_scrolls_to_follow_the_cursor() {
        let mut a = on_themes();
        a.list_rows.set(10);
        for _ in 0..25 {
            a.on_key(press(KeyCode::Down));
        }
        assert!(a.theme_top > 0, "the window should have scrolled");
        assert!(
            a.theme_row >= a.theme_top && a.theme_row < a.theme_top + 10,
            "cursor {} outside the window {}..{}",
            a.theme_row,
            a.theme_top,
            a.theme_top + 10
        );
        // Home returns to the top and takes the window with it.
        a.on_key(press(KeyCode::Home));
        assert_eq!((a.theme_row, a.theme_top), (0, 0));
    }

    /// The package chooser, with the real templates/packages.toml.
    fn on_packages() -> App {
        let mut a = on_themes();
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Packages);
        assert!(!a.packages.is_empty(), "package list should have loaded");
        a
    }

    #[test]
    fn packages_start_from_their_declared_defaults() {
        let a = on_packages();
        let p = cozy_theme::Packages::load(Path::new("../../templates/packages.toml")).unwrap();
        let want: Vec<bool> = p.optional.iter().map(|o| o.default).collect();
        assert_eq!(
            a.wanted, want,
            "the page must open on packages.toml's defaults"
        );
        assert_eq!(
            a.chosen_packages().len(),
            want.iter().filter(|w| **w).count()
        );
    }

    #[test]
    fn space_toggles_only_the_row_under_the_cursor() {
        let mut a = on_packages();
        let before = a.chosen_packages().len();
        let name = a.current_package().unwrap().name.clone();
        a.on_key(press(KeyCode::Char(' ')));
        assert_eq!(
            a.chosen_packages().len(),
            before - 1,
            "should have dropped one"
        );
        assert!(
            !a.chosen_packages().contains(&name.as_str()),
            "{name} should be off"
        );
        a.on_key(press(KeyCode::Char(' ')));
        assert_eq!(
            a.chosen_packages().len(),
            before,
            "toggling back restores it"
        );
    }

    #[test]
    fn bulk_toggles_cover_the_whole_list() {
        let mut a = on_packages();
        a.on_key(press(KeyCode::Char('n')));
        assert!(a.chosen_packages().is_empty(), "n should clear everything");
        a.on_key(press(KeyCode::Char('a')));
        assert_eq!(
            a.chosen_packages().len(),
            a.packages.len(),
            "a should select everything"
        );
    }

    #[test]
    fn the_detail_panel_shows_description_and_licence() {
        let mut a = on_packages();
        // Walk the list; every row must describe itself and name a licence.
        for _ in 0..a.packages.len() {
            let pkg = a.current_package().unwrap();
            let (name, about, licence) = (pkg.name.clone(), pkg.about.clone(), pkg.license.clone());
            let text = flatten(&render_app(&a, 100, 30));
            assert!(text.contains(&name), "{name} not shown");
            assert!(
                text.contains(&licence),
                "{name}: licence {licence} not shown"
            );
            let head: String = about.split(['.', '(']).next().unwrap().into();
            assert!(text.contains(head.trim()), "{name}: description not shown");
            a.on_key(press(KeyCode::Down));
        }
    }

    #[test]
    fn the_proprietary_licence_is_called_out() {
        // Every other package is permissive; this is the one a reader must not
        // skim past, so it must not render in the same grey as the rest.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut a = on_packages();
        while a.current_package().unwrap().name != "claude-code" {
            a.on_key(press(KeyCode::Down));
        }
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| draw(f, &a)).unwrap();
        let buf = term.backend().buffer();
        let row = (0..30)
            .find(|y| {
                (0..100)
                    .map(|x| buf[(x, *y)].symbol())
                    .collect::<String>()
                    .contains("LicenseRef")
            })
            .expect("licence line should be drawn");
        let x = (0..100)
            .find(|x| buf[(*x, row)].symbol() == "L")
            .expect("licence text should start somewhere");
        let cell = &buf[(x, row)];
        let dim = a.theme().comment;
        assert_ne!(
            cell.fg, dim,
            "a proprietary licence must not render as ordinary grey"
        );
        assert!(
            cell.modifier.contains(Modifier::BOLD),
            "and should be emphasised"
        );
    }

    #[test]
    fn the_chosen_theme_persists_onto_the_package_page() {
        // The scheme picked on the previous page is the wizard's colours from
        // then on, not a preview that ends when the page does.
        let mut a = on_themes();
        for _ in 0..7 {
            a.on_key(press(KeyCode::Down));
        }
        let want = a.loaded.as_ref().unwrap().palette["base00"];
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Packages);
        assert_eq!(
            frame_bg(&a, 100, 30),
            Color::Rgb(want.r, want.g, want.b),
            "the package page should still be wearing {}",
            a.schemes[a.theme_row].name
        );
    }

    fn typing(app: &mut App, text: &str) {
        for c in text.chars() {
            app.on_key(press(KeyCode::Char(c)));
        }
    }

    #[test]
    fn typing_q_does_not_quit_the_wizard() {
        // The hazard this whole focus mode exists for: `q` quits everywhere
        // else, and a package field where typing `qt5` exits would be absurd.
        let mut a = on_packages();
        a.on_key(press(KeyCode::Char('i')));
        assert_eq!(a.focus, Focus::Input);
        typing(&mut a, "qt5");
        assert!(!a.done, "q must be a letter while typing");
        assert_eq!(a.extra, "qt5");
        // Ctrl-C still works, because there has to be a way out from anywhere.
        a.on_key(KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        ));
        assert!(a.done, "ctrl-c must still quit from the text field");
    }

    #[test]
    fn focus_moves_into_the_field_and_back() {
        let mut a = on_packages();
        assert_eq!(a.focus, Focus::List);
        a.on_key(press(KeyCode::Tab));
        assert_eq!(a.focus, Focus::Input);
        a.on_key(press(KeyCode::Esc));
        assert_eq!(a.focus, Focus::List, "esc leaves the field");
        assert_eq!(a.screen, Screen::Packages, "and does not leave the page");
        a.on_key(press(KeyCode::Char('i')));
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.focus, Focus::List, "enter leaves the field");
        assert!(!a.done, "enter in the field must not finish the wizard");
    }

    #[test]
    fn list_keys_do_not_leak_into_the_field() {
        // `space` toggles in the list and is a space in the field; `j` moves in
        // the list and is a letter in the field.
        let mut a = on_packages();
        let chosen = a.chosen_packages().len();
        let row = a.package_row;
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "a b");
        a.on_key(press(KeyCode::Char('j')));
        assert_eq!(a.extra, "a bj");
        assert_eq!(a.package_row, row, "the list must not have moved");
        assert_eq!(
            a.chosen_packages().len() - a.extra_packages().len(),
            chosen,
            "the checklist must not have been toggled"
        );
    }

    #[test]
    fn typed_names_are_installed_and_junk_is_dropped() {
        let mut a = on_packages();
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "emacs, tmux  RUBBISH!! neovim");
        assert_eq!(
            a.extra_packages(),
            vec!["emacs", "neovim", "tmux"],
            "should keep the plausible names and drop the rest"
        );
        let chosen = a.chosen_packages();
        for name in ["emacs", "neovim", "tmux"] {
            assert!(chosen.contains(&name), "{name} should be installed");
        }
        assert!(
            !chosen.contains(&"RUBBISH!!"),
            "junk must not reach the package list"
        );
    }

    #[test]
    fn backspace_edits_the_field() {
        let mut a = on_packages();
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "emacsx");
        a.on_key(press(KeyCode::Backspace));
        assert_eq!(a.extra, "emacs");
        assert_eq!(a.extra_packages(), vec!["emacs"]);
    }

    #[test]
    fn a_name_the_loadout_already_has_is_flagged() {
        // Typing `fish` should not silently look like it did something.
        let mut a = on_packages();
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "fish emacs");
        assert_eq!(a.redundant_extras(), vec!["fish"]);
        let text = flatten(&render_app(&a, 100, 30));
        assert!(text.contains("already installed: fish"), "{text}");
        assert!(
            text.contains("adding emacs fish"),
            "and should still echo what it parsed"
        );
    }

    #[test]
    fn the_field_says_when_it_parsed_nothing() {
        let mut a = on_packages();
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "!!!");
        let text = flatten(&render_app(&a, 100, 30));
        assert!(
            text.contains("Nothing usable yet"),
            "silently ignoring what someone typed is the worst version of this:\n{text}"
        );
    }

    #[test]
    fn esc_steps_back_to_the_themes() {
        let mut a = on_packages();
        a.on_key(press(KeyCode::Esc));
        assert_eq!(a.screen, Screen::Themes);
        assert!(!a.done);
    }

    /// The patches page, with both pickers rooted at a private tree rather
    /// than the real $HOME, so the tests do not depend on this machine.
    fn on_patches(tag: &str) -> (App, PathBuf) {
        let root = temp_dir(&format!("patches-{tag}"));
        std::fs::create_dir_all(root.join("dotfiles")).unwrap();
        std::fs::write(root.join("dotfiles/config.toml"), "x").unwrap();
        std::fs::write(root.join("notes.md"), "n").unwrap();
        let mut a = on_packages();
        a.enter_patches();
        a.pickers = vec![
            Picker::new(Pick::Files, &root),
            Picker::new(Pick::Dirs, &root),
        ];
        assert_eq!(a.screen, Screen::Patches);
        (a, root)
    }

    #[test]
    fn tab_swaps_between_the_two_pickers() {
        let (mut a, root) = on_patches("tab");
        assert_eq!(a.picker().kind, Pick::Files);
        a.on_key(press(KeyCode::Tab));
        assert_eq!(
            a.picker().kind,
            Pick::Dirs,
            "tab should reach the directory picker"
        );
        a.on_key(press(KeyCode::Tab));
        assert_eq!(a.picker().kind, Pick::Files, "and wrap back round");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn each_picker_only_takes_its_own_kind() {
        let (mut a, root) = on_patches("kinds");
        // Row 0 is the `dotfiles` directory in both.
        a.on_key(press(KeyCode::Char(' ')));
        assert!(
            a.chosen_paths().is_empty(),
            "the file picker must not take a directory"
        );
        a.on_key(press(KeyCode::Tab));
        a.on_key(press(KeyCode::Char(' ')));
        assert_eq!(
            a.chosen_paths()
                .iter()
                .map(|p| p.as_path())
                .collect::<Vec<_>>(),
            vec![root.join("dotfiles").as_path()],
            "the directory picker must take it"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn arrows_walk_the_tree_and_enter_moves_on() {
        // Enter is `done` on every other page, so descending is on the arrow
        // that points into the tree rather than stealing enter.
        let (mut a, root) = on_patches("walk");
        a.on_key(press(KeyCode::Right));
        assert_eq!(
            a.picker().cwd,
            root.join("dotfiles"),
            "right should descend"
        );
        a.on_key(press(KeyCode::Char(' ')));
        assert_eq!(
            a.chosen_paths().len(),
            1,
            "and the file inside is selectable"
        );
        a.on_key(press(KeyCode::Left));
        assert_eq!(a.picker().cwd, root, "left should climb back out");
        assert_eq!(a.chosen_paths().len(), 1, "without losing the selection");

        assert!(!a.done);
        a.on_key(press(KeyCode::Enter));
        assert_eq!(
            a.screen,
            Screen::Apply,
            "enter should move on rather than descend"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn both_pickers_selections_are_reported_together() {
        let (mut a, root) = on_patches("both");
        a.on_key(press(KeyCode::Right));
        a.on_key(press(KeyCode::Char(' ')));
        a.on_key(press(KeyCode::Left));
        a.on_key(press(KeyCode::Tab));
        a.on_key(press(KeyCode::Char(' ')));
        let chosen: Vec<String> = a
            .chosen_paths()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(chosen.len(), 2, "{chosen:?}");
        assert!(
            chosen.iter().any(|p| p.ends_with("config.toml")),
            "{chosen:?}"
        );
        assert!(chosen.iter().any(|p| p.ends_with("dotfiles")), "{chosen:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A patches page whose file picker sits in a fake `~/.config` holding a
    /// file the loadout also installs.
    fn on_patches_with_conflict(tag: &str) -> (App, PathBuf) {
        let home = temp_dir(&format!("conflict-{tag}"));
        std::fs::create_dir_all(home.join(".config/helix")).unwrap();
        std::fs::write(home.join(".config/helix/config.toml"), "mine").unwrap();
        std::fs::write(home.join(".config/starship.toml"), "mine").unwrap();
        let mut a = on_packages();
        // Destinations are computed against this, not against the real home.
        a.home.clone_from(&home);
        a.enter_patches();
        a.pickers = vec![
            Picker::new(Pick::Files, &home.join(".config")),
            Picker::new(Pick::Dirs, &home.join(".config")),
        ];
        (a, home)
    }

    #[test]
    fn picking_your_own_config_displaces_the_loadouts_and_says_so() {
        let (mut a, home) = on_patches_with_conflict("file");

        while a.picker().current().unwrap().name != "starship.toml" {
            a.on_key(press(KeyCode::Down));
        }
        a.on_key(press(KeyCode::Char(' ')));

        let displaced = a.displaced_configs();
        assert!(
            displaced.iter().any(|d| d == ".config/starship.toml"),
            "the loadout's own starship config should be displaced: {displaced:?}"
        );
        let text = flatten(&render_app(&a, 110, 30));
        assert!(text.contains("using yours instead of"), "{text}");
        assert!(
            text.contains(".config/starship.toml"),
            "and should name it:\n{text}"
        );

        // And the summary repeats it, being the last screen before anything is
        // written.
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Apply);
        let text = flatten(&render_app(&a, 110, 30));
        assert!(text.contains("replaces"), "{text}");
        assert!(text.contains(".config/starship.toml"), "{text}");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_long_list_of_picks_cannot_push_the_warning_off_the_screen() {
        // The summary strip was a fixed two rows and the chosen-path line
        // wraps, so a couple of long paths ate both and the warning — the one
        // thing on it that is not repeated anywhere else — was clipped away.
        let (mut a, home) = on_patches_with_conflict("crowded");
        while a.picker().current().unwrap().name != "starship.toml" {
            a.on_key(press(KeyCode::Down));
        }
        a.on_key(press(KeyCode::Char(' ')));
        // Enough paths to wrap the chosen line well past one row.
        for i in 0..6 {
            a.pickers[0]
                .chosen
                .insert(home.join(format!(".config/a-fairly-long-config-name-{i}.toml")));
        }

        let text = flatten(&render_app(&a, 90, 26));
        assert!(
            text.contains("using yours instead of") && text.contains(".config/starship.toml"),
            "the warning has to survive a crowded summary line:\n{text}"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn picking_a_directory_displaces_everything_the_loadout_puts_in_it() {
        let (mut a, home) = on_patches_with_conflict("dir");

        a.on_key(press(KeyCode::Tab));
        while a.picker().current().unwrap().name != "helix" {
            a.on_key(press(KeyCode::Down));
        }
        a.on_key(press(KeyCode::Char(' ')));

        let displaced = a.displaced_configs();
        assert!(
            displaced.len() >= 3,
            "config, languages and the theme: {displaced:?}"
        );
        assert!(
            displaced.iter().all(|d| d.starts_with(".config/helix/")),
            "{displaced:?}"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn nothing_picked_means_nothing_displaced() {
        let (a, home) = on_patches_with_conflict("none");
        assert!(a.displaced_configs().is_empty());
        let text = flatten(&render_app(&a, 110, 30));
        assert!(
            !text.contains("using yours instead of"),
            "no warning without a conflict"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn esc_steps_back_to_the_packages() {
        let (mut a, root) = on_patches("esc");
        a.on_key(press(KeyCode::Esc));
        assert_eq!(a.screen, Screen::Packages);
        assert!(!a.done);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_patches_page_keeps_the_chosen_theme() {
        let (a, root) = on_patches("theme");
        let want = a
            .loaded
            .as_ref()
            .expect("a scheme should be loaded")
            .palette["base00"];
        assert_eq!(frame_bg(&a, 100, 30), Color::Rgb(want.r, want.g, want.b));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_summary_names_what_was_chosen() {
        let (mut a, root) = on_patches("summary");
        a.on_key(press(KeyCode::Tab));
        a.on_key(press(KeyCode::Char(' ')));
        let text = flatten(&render_app(&a, 110, 30));
        assert!(text.contains("1 chosen"), "{text}");
        assert!(
            text.contains("dotfiles"),
            "the summary should name the path:\n{text}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    // -- remembering between runs ------------------------------------------

    /// Walk a run to the end and return what it would write.
    fn completed_run(pick_blocks: bool, theme_steps: usize) -> App {
        let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), State::default());
        if pick_blocks {
            a.on_key(press(KeyCode::Down));
        }
        a.on_key(press(KeyCode::Enter)); // greeting -> schemes
        a.on_key(press(KeyCode::Char('n')));
        a.on_key(press(KeyCode::Enter)); // schemes -> themes
        for _ in 0..theme_steps {
            a.on_key(press(KeyCode::Down));
        }
        a.on_key(press(KeyCode::Enter)); // themes -> packages
        a
    }

    #[test]
    fn a_finished_run_records_every_page() {
        let mut a = completed_run(true, 4);
        a.on_key(press(KeyCode::Char(' '))); // drop the first optional package
        let dropped = a.packages[0].name.clone();
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "emacs");
        a.on_key(press(KeyCode::Esc));
        let theme = a.schemes[a.theme_row].name.clone();
        a.on_key(press(KeyCode::Enter)); // packages -> patches
        a.on_key(press(KeyCode::Enter)); // patches -> apply
        assert_eq!(a.screen, Screen::Apply);
        for _ in 0..2 {
            a.on_key(press(KeyCode::Down)); // "save settings and exit"
        }
        a.on_key(press(KeyCode::Enter));

        assert!(a.completed, "choosing anything but abort completes the run");
        let st = a.to_state();
        assert_eq!(st.greeting.as_deref(), Some(Greeting::ALL[1].key()));
        assert_eq!(st.theme.as_deref(), Some(theme.as_str()));
        assert_eq!(st.extra, "emacs");
        assert_eq!(
            st.packages.get(&dropped),
            Some(&false),
            "the dropped package is recorded off"
        );
        assert!(st.packages.len() > 1, "and the rest are recorded too");
    }

    #[test]
    fn quitting_early_does_not_count_as_an_answer() {
        // The rule that keeps a half-answered wizard from overwriting a
        // finished one. `main` only writes when `completed` is set.
        let mut a = completed_run(false, 2);
        a.on_key(press(KeyCode::Char('q')));
        assert!(a.done, "q should end the run");
        assert!(!a.completed, "but quitting is not completing");

        let mut b = completed_run(false, 2);
        b.on_key(press(KeyCode::Enter)); // -> patches
        b.on_key(press(KeyCode::Esc)); // back to packages
        b.on_key(press(KeyCode::Char('q')));
        assert!(
            !b.completed,
            "backing out and quitting is still not completing"
        );
    }

    #[test]
    fn a_saved_run_comes_back_selected() {
        let first = {
            let mut a = completed_run(true, 6);
            a.on_key(press(KeyCode::Char(' ')));
            a.on_key(press(KeyCode::Char('i')));
            typing(&mut a, "emacs tmux");
            a.on_key(press(KeyCode::Esc));
            a.to_state()
        };
        let theme = first.theme.clone().unwrap();
        let off: Vec<String> = first
            .packages
            .iter()
            .filter(|(_, v)| !**v)
            .map(|(k, _)| k.clone())
            .collect();

        // A second run, opened with what the first one saved.
        let mut b = App::with_state(PathBuf::from("../../schemes/vendor"), first);
        assert_eq!(
            b.current_greeting(),
            Greeting::ALL[1],
            "greeting should be restored"
        );
        b.on_key(press(KeyCode::Enter));
        b.on_key(press(KeyCode::Char('n')));
        b.on_key(press(KeyCode::Enter));
        assert_eq!(
            b.schemes[b.theme_row].name, theme,
            "theme should be restored"
        );
        b.on_key(press(KeyCode::Enter));
        assert_eq!(
            b.extra, "emacs tmux",
            "the typed packages should come back editable"
        );
        for name in &off {
            assert!(
                !b.chosen_packages().contains(&name.as_str()),
                "{name} should still be off"
            );
        }
    }

    #[test]
    fn a_theme_that_no_longer_exists_falls_back_to_the_default() {
        let saved = State {
            theme: Some("a-scheme-nobody-has".into()),
            ..State::default()
        };
        let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), saved);
        a.on_key(press(KeyCode::Enter));
        a.on_key(press(KeyCode::Char('n')));
        a.on_key(press(KeyCode::Enter));
        assert_eq!(
            a.theme_row, 0,
            "a deleted scheme should leave the cursor at the default"
        );
        assert!(
            a.loaded.is_some(),
            "and something must still be loaded to paint with"
        );
    }

    #[test]
    fn a_deleted_path_does_not_come_back() {
        let dir = temp_dir("restore-paths");
        std::fs::create_dir_all(dir.join("kept")).unwrap();
        std::fs::write(dir.join("kept.txt"), "x").unwrap();
        let saved = State {
            files: vec![dir.join("kept.txt"), dir.join("gone.txt")],
            dirs: vec![dir.join("kept"), dir.join("gone")],
            ..State::default()
        };
        let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), saved);
        a.enter_patches();
        let chosen: Vec<&PathBuf> = a.chosen_paths();
        assert_eq!(chosen.len(), 2, "only the two that still exist: {chosen:?}");
        assert!(chosen.iter().all(|p| p.exists()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_scheme_fetch_answer_is_never_recorded() {
        // Whether to clone or pull is about the disk right now, not a
        // preference — answering it once must not answer it forever.
        let a = completed_run(false, 1);
        let toml = toml::to_string_pretty(&a.to_state()).unwrap();
        for word in ["fetch", "clone", "update", "schemes_dir"] {
            assert!(
                !toml.contains(word),
                "state file should not mention {word}:\n{toml}"
            );
        }
    }

    // -- going back and forward again --------------------------------------

    #[test]
    fn revisiting_a_page_keeps_what_you_changed_this_run() {
        // Every page restores from the sticky file when it opens. Re-applying
        // that on a *second* visit would silently undo everything the user did
        // this run — go back to check something, come forward, and your work
        // is gone.
        let saved = State {
            theme: Some("3024".into()),
            extra: "from-the-file".into(),
            ..State::default()
        };
        let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), saved);
        a.on_key(press(KeyCode::Enter));
        a.on_key(press(KeyCode::Char('n')));
        a.on_key(press(KeyCode::Enter)); // -> themes, restored to "3024"
        assert_eq!(a.schemes[a.theme_row].name, "3024");

        // Change it, then go back and forward again.
        for _ in 0..5 {
            a.on_key(press(KeyCode::Down));
        }
        let chosen = a.schemes[a.theme_row].name.clone();
        assert_ne!(chosen, "3024", "the test needs to have actually moved");
        a.on_key(press(KeyCode::Esc)); // -> schemes
        a.on_key(press(KeyCode::Enter)); // -> themes again
        assert_eq!(
            a.schemes[a.theme_row].name, chosen,
            "coming back should show this run's choice, not the file's"
        );

        // Same for the package page's toggles and its text field.
        a.on_key(press(KeyCode::Enter)); // -> packages
        assert_eq!(a.extra, "from-the-file");
        a.on_key(press(KeyCode::Char(' ')));
        let dropped = a.packages[0].name.clone();
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "-typed-now");
        a.on_key(press(KeyCode::Esc)); // leave the field
        a.on_key(press(KeyCode::Esc)); // -> themes
        a.on_key(press(KeyCode::Enter)); // -> packages again
        assert_eq!(
            a.extra, "from-the-file-typed-now",
            "the field should keep what was typed"
        );
        assert!(
            !a.chosen_packages().contains(&dropped.as_str()),
            "{dropped} was switched off this run and should have stayed off"
        );
    }

    #[test]
    fn revisiting_the_patches_page_keeps_this_run_s_choices() {
        // The pickers guard on `is_empty()` rather than a visit count, so this
        // checks the guard actually holds — including that walking somewhere
        // else and coming back does not reset the browser to $HOME either.
        let (mut a, root) = on_patches("revisit");
        a.on_key(press(KeyCode::Tab));
        a.on_key(press(KeyCode::Char(' ')));
        let chosen: Vec<PathBuf> = a.chosen_paths().into_iter().cloned().collect();
        assert_eq!(chosen.len(), 1);

        a.on_key(press(KeyCode::Esc)); // -> packages
        a.on_key(press(KeyCode::Enter)); // -> patches again
        assert_eq!(
            a.chosen_paths().into_iter().cloned().collect::<Vec<_>>(),
            chosen,
            "coming back should keep what was picked this run"
        );
        assert_eq!(
            a.picker().cwd,
            root,
            "and should not have jumped back to $HOME"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn every_page_after_the_first_offers_a_way_back() {
        // Esc steps back everywhere it can, but a key nobody mentions is a key
        // nobody presses.
        let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), State::default());
        a.on_key(press(KeyCode::Enter));
        for expect_back in [
            Screen::Schemes,
            Screen::Themes,
            Screen::Packages,
            Screen::Patches,
        ] {
            assert_eq!(a.screen, expect_back);
            let footer: String = footer_hints(&a)
                .iter()
                .map(|s| s.content.to_string())
                .collect();
            assert!(
                footer.contains("esc"),
                "{expect_back:?} does not advertise a way back: {footer:?}"
            );
            if expect_back == Screen::Schemes {
                // Decline the fetch: enter would otherwise start one, and a
                // test must not reach the network.
                a.on_key(press(KeyCode::Char('n')));
            }
            if expect_back != Screen::Patches {
                a.on_key(press(KeyCode::Enter));
            }
        }
    }

    // -- the apply page ----------------------------------------------------

    fn on_apply() -> App {
        let mut a = on_packages();
        a.enter_patches();
        a.on_key(press(KeyCode::Enter));
        assert_eq!(a.screen, Screen::Apply);
        a
    }

    #[test]
    fn the_repo_root_is_never_an_empty_path() {
        // `Path::new("schemes/vendor")` climbs to "schemes" and then to "",
        // and an empty path is not the current directory — it is nothing.
        // Passing it to `Command::current_dir` fails, which is exactly how
        // generating from a default `--schemes` broke.
        for dir in ["schemes/vendor", "./schemes/vendor", "/abs/schemes/vendor"] {
            let a = App::with_state(PathBuf::from(dir), State::default());
            let root = a.repo_root();
            assert!(
                !root.as_os_str().is_empty(),
                "{dir} produced an empty repo root"
            );
            assert!(
                a.templates_dir().starts_with(&root),
                "templates should sit under the repo root"
            );
        }
    }

    #[test]
    fn the_summary_reports_every_choice_but_the_fetch() {
        let mut a = on_apply();
        let text = flatten(&render_app(&a, 100, 30));
        for field in ["greeting", "theme", "packages", "patches"] {
            assert!(
                text.contains(field),
                "{field} missing from the summary:\n{text}"
            );
        }
        assert!(
            text.contains(&a.schemes[a.theme_row].name),
            "the scheme should be named"
        );
        // The scheme-collection question was about the disk a moment ago, not
        // a choice worth reviewing.
        for word in ["clone", "Download", "Update the upstream"] {
            assert!(
                !text.contains(word),
                "the summary should not mention {word}"
            );
        }
        a.on_key(press(KeyCode::Esc));
        assert_eq!(a.screen, Screen::Patches, "esc should still step back");
    }

    #[test]
    fn abort_is_the_only_action_that_discards_the_answers() {
        for (steps, action, saves) in [
            (0, Action::GenerateAndInstall, true),
            (1, Action::Generate, true),
            (2, Action::SaveOnly, true),
            (3, Action::Abort, false),
        ] {
            let mut a = on_apply();
            for _ in 0..steps {
                a.on_key(press(KeyCode::Down));
            }
            assert_eq!(a.action(), action);
            assert_eq!(action.saves(), saves, "{action:?}");
        }
        // Abort really does neither.
        let mut a = on_apply();
        for _ in 0..3 {
            a.on_key(press(KeyCode::Down));
        }
        a.on_key(press(KeyCode::Enter));
        assert!(a.done);
        assert!(!a.completed, "abort must not write the state file");

        // Save-only finishes without building.
        let mut b = on_apply();
        for _ in 0..2 {
            b.on_key(press(KeyCode::Down));
        }
        b.on_key(press(KeyCode::Enter));
        assert!(
            b.done && b.completed,
            "save-only should still be remembered"
        );
        assert!(
            matches!(b.applied, Applied::Idle),
            "and must not have built anything"
        );
    }

    #[test]
    fn any_key_exits_once_the_action_has_run() {
        // Naming two specific keys made people hunt for them, and there is
        // nothing else to do on this screen afterwards.
        for key in [
            KeyCode::Char('x'),
            KeyCode::Char(' '),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Down,
            KeyCode::Tab,
        ] {
            let mut a = on_apply();
            a.applied = Applied::Ok("rendered".into());
            a.on_key(press(key));
            assert!(a.done, "{key:?} should have exited");
        }
        // A failure ends the same way rather than sitting there re-running.
        for key in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Esc] {
            let mut a = on_apply();
            a.applied = Applied::Failed("git exploded".into());
            a.on_key(press(key));
            assert!(a.done, "{key:?} should have exited after a failure");
        }
    }

    #[test]
    fn the_settled_screen_says_how_to_leave() {
        for applied in [
            Applied::Ok("rendered something".into()),
            Applied::Failed("something broke".into()),
        ] {
            let mut a = on_apply();
            a.applied = applied;
            let text = flatten(&render_app(&a, 110, 30));
            assert!(text.contains("press any key to exit"), "{text}");
        }
    }

    #[test]
    fn install_is_the_first_thing_offered() {
        // It is the point of running the wizard; making it the second option
        // means everyone arrows past the one they wanted.
        let a = on_apply();
        assert_eq!(a.action(), Action::GenerateAndInstall);
        let rows = render_app(&a, 100, 30);
        let install = rows
            .iter()
            .position(|r| r.contains("Generate and install"))
            .unwrap();
        let generate = rows
            .iter()
            .position(|r| r.contains("Generate") && !r.contains("install"))
            .unwrap();
        assert!(install < generate, "install should be listed first");
    }

    #[test]
    fn the_cursor_clamps_at_both_ends_of_the_action_list() {
        let mut a = on_apply();
        a.on_key(press(KeyCode::Up));
        assert_eq!(
            a.action(),
            Action::GenerateAndInstall,
            "the cursor starts on install and up should stay there"
        );
        for _ in 0..10 {
            a.on_key(press(KeyCode::Down));
        }
        assert_eq!(a.action(), Action::Abort, "down should stop at the end");
    }

    #[test]
    fn generate_actually_renders_what_was_chosen() {
        // The whole point of the page. Runs the real renderer against a
        // throwaway output directory and reads what came out.
        let out = temp_dir("apply-generate");
        std::fs::create_dir_all(&out).unwrap();

        let mut a = on_apply();
        // A repo whose `build/` is the temp dir: the renderer is told where to
        // write, so this does not disturb the checkout.
        a.repo = PathBuf::from("../..");
        a.greeting = Some(Greeting::Legacy);
        // Turn everything optional off, so the effect is visible in the output.
        a.wanted.iter_mut().for_each(|w| *w = false);

        let scheme = a.schemes[a.theme_row].name.clone();
        let report = run_generate_to(&a, Path::new("../.."), &out, false)
            .unwrap_or_else(|e| panic!("generate failed: {e}"));
        assert!(
            report.contains(&scheme),
            "the renderer should name the scheme: {report}"
        );

        let manifest = std::fs::read_to_string(out.join("cozy.toml")).unwrap();
        assert!(
            manifest.contains("\"fish\""),
            "required packages must survive"
        );
        assert!(
            !manifest.contains("\"tealdeer\""),
            "declined packages must be gone"
        );
        assert!(
            !out.join("cozy/atuin").exists(),
            "a declined package's config must not be rendered"
        );
        let fish = std::fs::read_to_string(out.join("cozy/fish/config.fish")).unwrap();
        assert!(
            fish.contains("🭕"),
            "the legacy greeting should have been chosen"
        );
        std::fs::remove_dir_all(&out).unwrap();
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
        let done = app_after_fetch("dump");
        println!("\n=== schemes after a successful fetch 80x24 ===");
        for row in render_app(&done, 80, 24) {
            println!("|{}|", row.trim_end());
        }
        let _ = std::fs::remove_dir_all(&done.schemes_dir);
        let mut a = on_themes();
        for (w, h) in [(100u16, 30u16), (80, 24), (60, 20)] {
            println!("\n=== themes {w}x{h} ({} schemes) ===", a.schemes.len());
            for row in render_app(&a, w, h) {
                println!("|{}|", row.trim_end());
            }
        }
        for _ in 0..5 {
            a.on_key(press(KeyCode::Down));
        }
        println!(
            "\n=== themes after 5 down: {} ===",
            a.schemes[a.theme_row].name
        );

        let mut p = on_packages();
        for _ in 0..7 {
            p.on_key(press(KeyCode::Down));
        }
        p.on_key(press(KeyCode::Char(' ')));
        p.on_key(press(KeyCode::Char('i')));
        for c in "emacs tmux fzf".chars() {
            p.on_key(press(KeyCode::Char(c)));
        }
        for (w, h) in [(80u16, 24u16), (60, 20)] {
            println!("\n=== packages {w}x{h} ===");
            for row in render_app(&p, w, h) {
                println!("|{}|", row.trim_end());
            }
        }

        let mut r = on_packages();
        r.enter_patches();
        r.enter_apply();
        println!("\n=== apply 90x28 ===");
        for row in render_app(&r, 90, 28) {
            println!("|{}|", row.trim_end());
        }

        let mut q = on_packages();
        q.enter_patches();
        q.pickers[0].move_cursor(2, 10);
        q.pickers[0].toggle();
        for (w, h) in [(100u16, 30u16), (70, 22)] {
            println!("\n=== patches {w}x{h} ===");
            for row in render_app(&q, w, h) {
                println!("|{}|", row.trim_end());
            }
        }
    }
}
