//! cozy-wizard — interactive setup for the cozy loadout.
//!
//! Run it with `just wizard`. See AGENTS.md for the build pipeline.

mod fetch;
mod greeting;
mod hostcfg;
mod keys;
mod picker;
mod resources;
mod state;
mod theme;
mod ui;

use fetch::{Fetch, FetchKind};
use greeting::Greeting;
use hostcfg::{apply_client, apply_resources, client_config_path};
use keys::{Bindings, Key};
use picker::Picker;
use resources::Resources;
use state::State;
use theme::Theme;

use clap::Parser;
use color_eyre::eyre::Result;
use cozy_theme::{
    loadout_patches, shadowed_by, user_patches, Adjust, OptionalPackage, Options, Scheme,
    SchemeEntry,
};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::ResetColor;
use ratatui::DefaultTerminal;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self};
use std::time::Duration;

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
pub struct Args {
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
            Action::Generate => {
                "Render the loadout into build/. Nothing outside this repo changes."
            }
            Action::GenerateAndInstall => {
                "Render into ~/.config/minimal/loadouts/, replacing what is there, and apply any \
                 detach or VM changes."
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
        adjust: app.adjust,
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
    Client,
    Resources,
    Apply,
}

// A state bag, not an API: these are seven independent yes/no facts about one
// screen each, and folding them into an enum or a flags struct would put
// distance between a field and the page that owns it for no reader's benefit.
#[allow(clippy::struct_excessive_bools)]
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
    /// The six scheme adjustments, the knob under the cursor, and whether the
    /// themes page has handed the arrow keys to them.
    adjust: Adjust,
    knob_row: usize,
    adjusting: bool,

    /// The session-key bindings from the client page, and its cursor. These
    /// configure minimal itself rather than the loadout — see `apply_client`.
    bindings: Bindings,
    client_row: usize,
    /// `Some(buffer)` while a chord is being retyped. Editing is modal because
    /// the field takes arbitrary text, including the letters the page's own
    /// keys would otherwise swallow.
    editing: Option<String>,

    /// The VM's share of this machine, probed on the way into the page.
    resources: Resources,

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
        let mut app = Self {
            screen: Screen::Greeting,
            schemes_dir,
            home: home(),
            adjust: Adjust::default(),
            knob_row: 0,
            adjusting: false,
            bindings: Bindings::default(),
            client_row: 0,
            editing: None,
            // Probing reads /proc or runs sysctl, so it happens once here
            // rather than on every frame.
            resources: Resources::probe(),
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
        };
        // Both host-settings pages restore here rather than on entry: neither
        // depends on anything discovered later (unlike the scheme list), and
        // the summary reads them even if the user never opens either page.
        app.restore_host_settings();
        app
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
            contrast: Some(self.adjust.contrast),
            saturation: Some(self.adjust.saturation),
            comments: Some(self.adjust.comments),
            separation: Some(self.adjust.separation),
            background: Some(self.adjust.background),
            warmth: Some(self.adjust.warmth),
            leader: Some(self.bindings.leader.as_config_str()),
            detach: Some(self.bindings.detach.as_config_str()),
            forward: Some(self.bindings.forward.as_config_str()),
            bell_on_leader: Some(self.bindings.bell),
            vcpus: Some(self.resources.allocation().vcpus),
            ram_mib: Some(self.resources.allocation().ram_mib),
        }
    }

    /// Restore the two host-settings pages from the file.
    ///
    /// Both are restored *validated*: a chord set that no longer passes — the
    /// file was hand-edited, or minimal tightened a rule — falls back to the
    /// defaults whole rather than leaving a half-applied set, and a VM size
    /// this host cannot offer is dropped per field by `Resources::restore`.
    fn restore_host_settings(&mut self) {
        // Out-of-range values are dropped per field rather than failing: this
        // file is hand-editable, and a bad number should cost that knob, not
        // the run.
        let knob = |v: Option<i8>| v.filter(|v| (-100..=100).contains(v)).unwrap_or(0);
        self.adjust = Adjust {
            contrast: knob(self.saved.contrast),
            saturation: knob(self.saved.saturation),
            comments: knob(self.saved.comments),
            separation: knob(self.saved.separation),
            background: knob(self.saved.background),
            warmth: knob(self.saved.warmth),
        };
        let parse = |s: &Option<String>, fallback: Key| {
            s.as_deref()
                .and_then(|t| Key::parse(t).ok())
                .unwrap_or(fallback)
        };
        let d = Bindings::default();
        let restored = Bindings {
            leader: parse(&self.saved.leader, d.leader),
            detach: parse(&self.saved.detach, d.detach),
            forward: parse(&self.saved.forward, d.forward),
            bell: self.saved.bell_on_leader.unwrap_or(d.bell),
        };
        self.bindings = if restored.validate().is_ok() {
            restored
        } else {
            d
        };
        self.resources.restore(self.saved.vcpus, self.saved.ram_mib);
    }

    /// The scheme as it will actually be rendered — adjustments applied.
    ///
    /// Everything that reads a scheme goes through here rather than touching
    /// `loaded`, so the preview, the swatches, the displaced-config list and
    /// the generated files cannot disagree about which scheme this is. The
    /// adjustment is recomputed rather than cached: it is sixteen colours, and
    /// a cache is a second thing to keep in step with the knobs.
    fn scheme(&self) -> Option<Scheme> {
        self.loaded.as_ref().map(|s| s.adjusted(self.adjust))
    }

    /// Colours to draw with: the selected scheme's, or the wizard's own before
    /// one is loaded.
    fn theme(&self) -> Theme {
        self.scheme()
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
        let typing = (self.screen == Screen::Packages && self.focus == Focus::Input)
            || (self.screen == Screen::Client && self.editing.is_some());
        if ctrl_c || (key.code == KeyCode::Char('q') && !typing) {
            self.done = true;
            return;
        }
        if self.busy() {
            return;
        }
        match self.screen {
            Screen::Greeting => crate::ui::greeting::on_key_greeting(self, key),
            Screen::Schemes => crate::ui::schemes::on_key_schemes(self, key),
            Screen::Themes => crate::ui::themes::on_key_themes(self, key),
            Screen::Packages => crate::ui::packages::on_key_packages(self, key),
            Screen::Patches => crate::ui::patches::on_key_patches(self, key),
            Screen::Client => crate::ui::client::on_key_client(self, key),
            Screen::Resources => crate::ui::vm::on_key_resources(self, key),
            Screen::Apply => crate::ui::apply::on_key_apply(self, key),
        }
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
        let Some(scheme) = self.scheme() else {
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

    fn action(&self) -> Action {
        Action::ALL[self.action_row]
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
                    // The host settings ride with `install`, not with
                    // `Generate`: Generate promises that nothing outside this
                    // repo changes, and minimal's config and minvmd's state
                    // are both outside it.
                    Ok(report) if install => Applied::Ok(self.apply_host_settings(report)),
                    Ok(report) => Applied::Ok(report),
                    Err(why) => Applied::Failed(why),
                };
            }
        }
    }

    /// Apply the two settings that live outside the loadout, appending what
    /// happened to the install report.
    ///
    /// Neither failure is fatal. The loadout is already written by this point,
    /// and "your detach chord could not be saved" is not a reason to tell
    /// someone their install failed — so both are reported as extra lines
    /// rather than by turning the whole run into a failure.
    fn apply_host_settings(&self, mut report: String) -> String {
        if !self.bindings.is_default() {
            match apply_client(&client_config_path(&self.home), &self.bindings) {
                Ok(what) => {
                    let _ = write!(report, "\n{what}");
                }
                Err(why) => {
                    let _ = write!(report, "\ndetach keys NOT saved: {why}");
                }
            }
        }
        if !self.resources.is_default() {
            match apply_resources(self.resources.allocation()) {
                Ok(what) => {
                    let _ = write!(report, "\n{what}");
                }
                Err(why) => {
                    let _ = write!(report, "\nVM settings NOT applied: {why}");
                }
            }
        }
        report
    }

    /// The four rows on the client page, in the order they are drawn.
    const CLIENT_ROWS: usize = 4;

    /// Read the row's current value back as text, for the edit buffer to start
    /// from — retyping a chord usually means changing one character of it.
    fn client_field_text(&self, row: usize) -> String {
        match row {
            0 => self.bindings.leader.as_config_str(),
            1 => self.bindings.detach.as_config_str(),
            2 => self.bindings.forward.as_config_str(),
            _ => String::new(),
        }
    }

    /// Commit an edited chord, or report why it cannot be committed.
    ///
    /// The whole binding set is re-validated rather than just the new key,
    /// because the rules that matter here are about the set: a detach key is
    /// only wrong *relative to* the leader and the forward key.
    fn commit_client_field(&mut self, row: usize, text: &str) -> Result<(), keys::KeyError> {
        let key = Key::parse(text.trim())?;
        let mut next = self.bindings;
        match row {
            0 => next.leader = key,
            1 => next.detach = key,
            2 => next.forward = key,
            _ => return Ok(()),
        }
        next.validate()?;
        self.bindings = next;
        Ok(())
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
        terminal.draw(|frame| ui::draw(frame, &app))?;
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

// ---------------------------------------------------------------------------
// Theme preview
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
