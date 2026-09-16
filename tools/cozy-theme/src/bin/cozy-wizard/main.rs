//! cozy-wizard — interactive setup for the cozy loadout.
//!
//! Run it with `just wizard`. See AGENTS.md for the build pipeline.

mod fetch;
mod fuzzy;
mod greeting;
mod hostcfg;
mod icons;
mod images;
mod keys;
mod picker;
mod preview;
mod registry;
mod resources;
mod syntax;
mod theme;
mod ui;

use cozy_theme::Settings as State;
use fetch::{Fetch, FetchKind};
use greeting::Greeting;
use hostcfg::{apply_client, apply_resources, client_config_path};
use keys::{Bindings, Key};
use picker::Picker;
use resources::Resources;
use theme::Theme;

use clap::Parser;
use color_eyre::eyre::Result;
use cozy_theme::{
    is_package_name, loadout_patches, shadowed_by, user_patches, Adjust, OptionalPackage, Options,
    Scheme, SchemeEntry,
};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::ResetColor;
use ratatui::style::Color;
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

    /// The settings file to start from and write back to. Defaults to
    /// `~/.config/cozy/settings.toml`; point it at your own to keep a named set
    /// of answers, which `cozy-theme --settings` can then render directly.
    #[arg(long)]
    settings: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Greeting
// ---------------------------------------------------------------------------

/// Where the wizard reads its answers from, and where it writes them back.
///
/// Normally the same file: `~/.config/cozy/settings.toml`, not one in the
/// checkout, because the answers are yours and should survive re-cloning it.
///
/// They differ exactly once. A `.cozy-wizard.toml` left in the working
/// directory by an older run is still *read* when there is no new file yet, so
/// nobody loses their answers to the move — but the run writes the new
/// location, which migrates them. Reading and writing the old path would have
/// meant the file never moved at all.
fn settings_paths(explicit: Option<PathBuf>) -> (PathBuf, PathBuf) {
    if let Some(path) = explicit {
        return (path.clone(), path);
    }
    settings_paths_in(
        &cozy_theme::user_settings_path(&home()),
        Path::new(cozy_theme::settings::FILE),
    )
}

/// The read/write decision, against two given paths. Pure, so it can be checked
/// without a real home.
fn settings_paths_in(modern: &Path, legacy: &Path) -> (PathBuf, PathBuf) {
    if !modern.exists() && legacy.exists() {
        return (legacy.to_path_buf(), modern.to_path_buf());
    }
    (modern.to_path_buf(), modern.to_path_buf())
}

/// The path offered when saving settings to a file of their own.
///
/// Beside the automatic one, in the user's own directory — the point of a named
/// settings file is that it outlives the checkout.
fn suggested_settings_name(home: &Path) -> String {
    cozy_theme::config_dir(home)
        .join("my-loadout.toml")
        .display()
        .to_string()
}

/// The host home, which patch destinations are computed relative to.
fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

/// What to do with everything chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Generate,
    SaveAs,
    SaveOnly,
    Abort,
}

impl Action {
    // Generate first: it is the whole point of running the wizard, so it is
    // what the cursor starts on rather than something to arrow down to.
    const ALL: [Action; 4] = [
        Action::Generate,
        Action::SaveAs,
        Action::SaveOnly,
        Action::Abort,
    ];

    fn label(self, install: bool) -> &'static str {
        match self {
            Action::Generate if install => "Generate and install",
            Action::Generate => "Generate",
            Action::SaveAs => "Save these settings to a file",
            Action::SaveOnly => "Save settings and exit",
            Action::Abort => "Abort",
        }
    }

    fn about(self, install: bool) -> &'static str {
        // One line each: the list draws these unwrapped, so a longer sentence
        // is a clipped sentence.
        match self {
            Action::Generate if install => {
                "Render, install into ~/.config/minimal/loadouts/, apply detach and VM changes."
            }
            Action::Generate => "Render into build/. Nothing outside this repo changes.",
            Action::SaveAs => "Write these answers to a file `cozy-theme --settings` can rebuild.",
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

/// What the wizard is already doing about a package the registry search found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PackageState {
    /// In `base` or `cozy` — installed whatever anyone chooses.
    Always,
    /// Ticked in the optional list, or already typed into the field.
    Added,
    /// In the optional list, switched off.
    Declined,
    /// Not something this page knows about.
    New,
}

impl PackageState {
    /// What to show beside the package in the search results.
    fn note(self) -> &'static str {
        match self {
            PackageState::Always => "installed anyway",
            PackageState::Added => "already added",
            PackageState::Declined => "turned off above",
            PackageState::New => "",
        }
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
            .picker
            .as_ref()
            .map(|p| p.chosen_of(false))
            .unwrap_or_default(),
        adjust: app.adjust,
        patch_dests: app.dest_overrides.clone(),
        patch_dirs: app
            .picker
            .as_ref()
            .map(|p| p.chosen_of(true))
            .unwrap_or_default(),
        ..Options::default()
    };
    // The summary comes back rather than going to stdout, so it can be shown
    // on the final frame instead of underneath it.
    let mut report = cozy_theme::build(&options).map_err(|e| format!("{e}"))?;

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

    /// Whether the file pickers draw Nerd Font icons.
    ///
    /// Asked on the greeting screen because it is the same question that screen
    /// already exists to answer — does this font have these glyphs — and asking
    /// it twice, on two screens, would be asking the reader to judge their font
    /// twice over.
    icons: bool,

    fetch_kind: FetchKind,
    /// `true` = yes, the default when a fetch is actually possible.
    fetch_yes: bool,
    fetch: Fetch,

    /// Every scheme on disk. Names only — parsing all of them at startup would
    /// read hundreds of files for a list that shows twenty.
    ///
    /// `schemes` is `all_schemes` filtered by the `/` query; the full list is
    /// kept so clearing the filter costs nothing.
    all_schemes: Vec<SchemeEntry>,
    schemes: Vec<SchemeEntry>,
    /// `Some(text)` while the `/` filter is open. Closing it keeps the filter;
    /// clearing it is a separate act, so `esc` out of a search does not throw
    /// away the narrowing you just did.
    searching: Option<String>,
    /// The filter currently applied, kept when the field closes so the list
    /// stays narrowed and reopening `/` resumes where it left off.
    search_query: String,
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

    /// The Minimal registry, read from the index `min` keeps on disk. Loaded
    /// once on the way into the packages page.
    registry: registry::Registry,
    /// The row highlighted in the registry search results.
    registry_row: usize,
    /// A fetch of minimal.dev's bundle, in flight. The local index answers
    /// immediately; this replaces it when it lands, because it is current and
    /// carries categories and advisories the local one has no idea about.
    registry_fetch: Option<std::sync::mpsc::Receiver<Result<registry::Registry, String>>>,
    /// Why the last fetch did not happen, if it did not. Not an error worth
    /// stopping for — there is a working registry either way.
    registry_note: Option<String>,
    /// Whether to reach minimal.dev at all.
    ///
    /// Off in tests. Without it the suite makes a real request per fixture that
    /// opens the packages page — slow, flaky, and pointed at somebody's actual
    /// web server. `no_test_reaches_the_network` holds it off.
    fetch_registry: bool,
    /// The six scheme adjustments, the knob under the cursor, and whether the
    /// themes page has handed the arrow keys to them.
    adjust: Adjust,
    knob_row: usize,
    adjusting: bool,
    /// `Some(name)` while the save-as prompt is open, and whatever the last
    /// save said — kept after the prompt closes so the confirmation survives
    /// long enough to read.
    saving: Option<String>,
    saved_note: Option<Result<String, String>>,

    /// `Some(path)` while the apply page's "save these settings" prompt is
    /// open, and whatever the last attempt said.
    saving_settings: Option<String>,
    settings_note: Option<Result<String, String>>,

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

    /// The preview of whatever the patches page has under its cursor, keyed by
    /// the path it was read from.
    ///
    /// A `RefCell` filled during drawing, like `list_rows`: the alternative is
    /// re-reading it in every key handler that can move a cursor — five of
    /// them, and the one you forget is a pane showing the wrong file. Keyed by
    /// path so it re-reads exactly when the answer would differ, rather than
    /// hitting the disk on all ten frames a second.
    preview: std::cell::RefCell<Option<(PathBuf, preview::Preview)>>,

    /// The decoded image under the patches cursor, keyed by the path *and* the
    /// area it was fitted to.
    ///
    /// Both parts matter: decoding on every frame would put a JPEG decoder in
    /// the draw loop, and keying on the path alone would keep drawing an image
    /// fitted to the wrong size after the terminal is resized.
    image: std::cell::RefCell<Option<(PathBuf, ratatui::layout::Rect, images::Drawable)>>,

    /// The syntax highlighter, rebuilt when the scheme changes. Cached for the
    /// same reason the preview is, and more so: building one renders the
    /// loadout's `.tmTheme` and parses the XML back.
    highlighter: std::cell::RefCell<Option<syntax::Highlighter>>,

    /// Destinations typed by hand on the patches page, keyed by source path.
    /// Empty until someone changes one; everything else takes the computed
    /// default.
    dest_overrides: std::collections::BTreeMap<PathBuf, String>,
    /// `Some(text)` while a destination is being typed, and why the last
    /// attempt was refused.
    editing_dest: Option<String>,
    dest_note: Option<String>,

    /// The filesystem picker on the patches page. One list taking either kind
    /// — what a pick *is* decides the shape of the patch, not which pane you
    /// were standing in.
    picker: Option<Picker>,

    /// What the last completed run chose. Consulted as each page opens rather
    /// than all at once, because the scheme list and the package list are only
    /// known once their page is entered.
    /// The summary page's cursor, whether installing is ticked, and how the
    /// chosen action went.
    action_row: usize,
    install: bool,
    applied: Applied,
    /// The repo the wizard is configuring — where `build/` and `templates/`
    /// live, and where `just` is run.
    repo: PathBuf,

    /// The host home, which patch destinations are computed relative to. A
    /// field rather than a call to `std::env` at the point of use: the tests
    /// need to point it at a fixture, and `set_var` is process-wide, so
    /// parallel tests doing that raced each other.
    home: PathBuf,

    /// Where schemes saved here go, and where saved ones are read back from.
    /// Resolved once for the same reason `home` is — it depends on
    /// `$XDG_CONFIG_HOME`, which a test cannot change without racing every
    /// other test in the process.
    user_schemes: PathBuf,

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
            user_schemes: cozy_theme::user_schemes_dir(&home()),
            adjust: Adjust::default(),
            knob_row: 0,
            adjusting: false,
            saving: None,
            saved_note: None,
            saving_settings: None,
            settings_note: None,
            bindings: Bindings::default(),
            client_row: 0,
            editing: None,
            // Probing reads /proc or runs sysctl, so it happens once here
            // rather than on every frame.
            resources: Resources::probe(),
            greeting_row,
            greeting: None,
            // On by default, matching the `eza --icons` the loadout installs.
            // Anyone whose font lacks them sees boxes in the sample right there
            // and presses one key.
            icons: saved.icons.unwrap_or(true),
            fetch_kind,
            fetch_yes: fetch_kind != FetchKind::Blocked,
            fetch: Fetch::Idle,
            all_schemes: Vec::new(),
            schemes: Vec::new(),
            searching: None,
            search_query: String::new(),
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
            registry: registry::Registry::default(),
            registry_row: 0,
            registry_fetch: None,
            registry_note: None,
            fetch_registry: true,
            preview: std::cell::RefCell::new(None),
            image: std::cell::RefCell::new(None),
            highlighter: std::cell::RefCell::new(None),
            dest_overrides: saved.patch_dests.clone(),
            editing_dest: None,
            dest_note: None,
            picker: None,
            action_row: 0,
            // On by default: installing is what running the wizard is for, and
            // an untouched run should produce a usable session.
            install: true,
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
        // A remembered scheme is loaded *now*, not when the theme page is first
        // opened, so a resumed run is in its own colours from the first frame.
        // Guarded rather than unconditional: with nothing remembered there is
        // no scheme to prefer, and walking the collection to land on whatever
        // sorts first would be a directory walk to answer a question nobody
        // asked.
        if app.saved.theme.is_some() {
            let _ = crate::ui::themes::select_theme(&mut app);
        }
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
            patch_dests: self.dest_overrides.clone(),
            files: self
                .picker
                .as_ref()
                .map(|p| p.chosen_of(false))
                .unwrap_or_default(),
            dirs: self
                .picker
                .as_ref()
                .map(|p| p.chosen_of(true))
                .unwrap_or_default(),
            contrast: Some(self.adjust.contrast),
            saturation: Some(self.adjust.saturation),
            comments: Some(self.adjust.comments),
            separation: Some(self.adjust.separation),
            background: Some(self.adjust.background),
            warmth: Some(self.adjust.warmth),
            icons: Some(self.icons),
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

    /// The path under the patches cursor, if this picker can take it — chosen
    /// or not.
    ///
    /// Distinct from [`Self::current_pick`]: a directory highlighted in the
    /// *file* picker is scenery you walk through, and giving it a destination
    /// would be answering for a patch that cannot exist.
    fn current_target(&self) -> Option<(PathBuf, bool)> {
        let picker = self.picker.as_ref()?;
        let entry = picker.current()?;
        Some((picker.cwd.join(&entry.name), entry.is_dir))
    }

    /// The path under the patches cursor, if it is one this run has chosen.
    ///
    /// What the preview reports on: "where this lands" is a statement about a
    /// patch that is actually going to happen.
    fn current_pick(&self) -> Option<(PathBuf, bool)> {
        let picker = self.picker.as_ref()?;
        let entry = picker.current()?;
        let path = picker.cwd.join(&entry.name);
        picker
            .chosen
            .contains_key(&path)
            .then_some((path, entry.is_dir))
    }

    /// Where a chosen path will land — the typed destination if there is one,
    /// the computed one otherwise.
    fn dest_of(&self, path: &Path, is_dir: bool) -> String {
        self.dest_overrides
            .get(path)
            .cloned()
            .unwrap_or_else(|| cozy_theme::patch_dest(path, &self.home, is_dir))
    }

    /// The preview for the entry under the patches cursor, read at most once
    /// per path.
    ///
    /// `rows` bounds how much is read, so the pane's height decides the cost.
    fn preview_of(&self, path: &Path, is_dir: bool, rows: usize) -> preview::Preview {
        let mut slot = self.preview.borrow_mut();
        if let Some((cached, value)) = slot.as_ref() {
            if cached == path {
                return value.clone();
            }
        }
        let value = preview::Preview::read(path, is_dir, rows);
        *slot = Some((path.to_path_buf(), value.clone()));
        value
    }

    /// Run `f` with the drawable image for `path` at `area`, decoding it at most
    /// once per path and size.
    ///
    /// A closure rather than a returned handle because the value lives inside a
    /// `RefCell`: handing out a reference would keep the borrow alive across
    /// the caller's own use of `self`, which is exactly where a second borrow
    /// panics.
    fn with_image<R>(
        &self,
        path: &Path,
        area: ratatui::layout::Rect,
        f: impl FnOnce(Option<&images::Drawable>) -> R,
    ) -> R {
        let mut slot = self.image.borrow_mut();
        let stale = slot
            .as_ref()
            .is_none_or(|(p, a, _)| p != path || *a != area);
        if stale {
            *slot = images::protocol_for(path, area).map(|d| (path.to_path_buf(), area, d));
        }
        f(slot.as_ref().map(|(_, _, d)| d))
    }

    /// Colour `lines` as the selected scheme would.
    ///
    /// Falls back to one uncoloured span per line when there is no scheme yet,
    /// or when the theme cannot be built: highlighting is a nicety, and losing
    /// it should cost the colour rather than the preview.
    fn highlight(&self, lines: &[String], name: &str) -> Vec<Vec<(Color, String)>> {
        let plain = || {
            lines
                .iter()
                .map(|l| vec![(Color::Reset, l.clone())])
                .collect::<Vec<_>>()
        };
        let Some(scheme) = self.scheme() else {
            return plain();
        };
        let mut slot = self.highlighter.borrow_mut();
        if slot.as_ref().is_none_or(|h| h.slug != scheme.slug) {
            *slot = syntax::Highlighter::new(&scheme, &self.templates_dir());
        }
        slot.as_ref().map_or_else(plain, |h| h.lines(lines, name))
    }

    /// The text the `/` field opens with — whatever filter is already applied,
    /// so reopening it lets you edit rather than start again.
    fn searching_text(&self) -> String {
        if self.schemes.len() == self.all_schemes.len() {
            String::new()
        } else {
            self.search_query.clone()
        }
    }

    /// Re-derive the visible scheme list from the full one and the query,
    /// keeping the cursor on the same *scheme* where it survives the filter.
    ///
    /// Following the scheme rather than the row index is what makes typing feel
    /// like narrowing around what you were looking at, rather than being dumped
    /// back at the top on every keystroke.
    fn filter_schemes(&mut self, query: &str) {
        self.search_query = query.to_string();
        let under_cursor = self.schemes.get(self.theme_row).map(|s| s.name.clone());
        self.schemes = fuzzy::filter(&self.all_schemes, query, |s| s.name.as_str())
            .into_iter()
            .map(|i| self.all_schemes[i].clone())
            .collect();
        self.theme_row = under_cursor
            .and_then(|name| self.schemes.iter().position(|s| s.name == name))
            .unwrap_or(0);
        self.theme_top = self.theme_top.min(self.theme_row);
        self.load_selected();
    }

    /// Take the fetched bundle if it has arrived.
    ///
    /// Polled from the event loop's tick rather than waited on: the page is
    /// usable the moment it opens, and the better registry arrives when it
    /// arrives.
    fn poll_registry(&mut self) {
        let Some(rx) = &self.registry_fetch else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(fetched)) => {
                self.registry = fetched;
                self.registry_fetch = None;
                self.registry_note = None;
            }
            Ok(Err(why)) => {
                self.registry_fetch = None;
                // Only worth saying when there is nothing else to fall back on.
                self.registry_note = (!self.registry.is_available()).then_some(why);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.registry_fetch = None,
        }
    }

    /// What the wizard is already doing about a package, for the registry
    /// search to report.
    ///
    /// The bug this replaces looked only at the free-text field and the
    /// always-installed set, and so said nothing about the optional list on the
    /// very same page: `bottom` (in `cozy`, hence always) was reported and
    /// `atuin` (optional, ticked on) was not.
    fn package_state(&self, name: &str) -> PackageState {
        if self.always.iter().any(|a| a == name) {
            return PackageState::Always;
        }
        if self.chosen_packages().contains(&name) {
            return PackageState::Added;
        }
        // In the list above but switched off. Worth its own answer: adding it
        // as free text here would install it while the list still shows it
        // unticked, which reads as a contradiction.
        if self.packages.iter().any(|p| p.name == name) {
            return PackageState::Declined;
        }
        PackageState::New
    }

    /// Typed package names the registry does not have.
    ///
    /// Empty when there is no index: a name cannot be checked without one, and
    /// "not in the registry" would then be a claim rather than a finding.
    fn unknown_extras(&self) -> Vec<&str> {
        if !self.registry.is_available() {
            return Vec::new();
        }
        self.extra_packages()
            .into_iter()
            .filter(|name| !self.registry.knows(name))
            .collect()
    }

    /// The registry search results for what is currently typed.
    fn registry_hits(&self) -> Vec<&registry::Package> {
        self.registry
            .search(self.searching.as_deref().unwrap_or(""))
    }

    /// The repository's scheme root — `schemes/`, the parent of the vendored
    /// collection this was pointed at.
    fn repo_schemes(&self) -> PathBuf {
        self.schemes_dir
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| self.schemes_dir.clone(), Path::to_path_buf)
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
        let before = self.theme_row;
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
        // An adjustment belongs to the scheme it was made against: +40 comments
        // rescues one palette and ruins the next. Landing on a different scheme
        // therefore starts from what its author published.
        //
        // Guarded on the row actually changing, so holding ↑ at the top of the
        // list — or any other clamped move — is not a way to lose your work.
        if self.theme_row != before {
            self.adjust = Adjust::default();
            // The note is about the scheme that was under the cursor; carrying
            // it onto another one would claim a save that did not happen here.
            self.saved_note = None;
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
            || (self.screen == Screen::Client && self.editing.is_some())
            || (self.screen == Screen::Themes && self.saving.is_some())
            || (self.screen == Screen::Apply && self.saving_settings.is_some())
            || (self.screen == Screen::Patches && self.editing_dest.is_some())
            // `/` puts both list screens into a text field, where `q` is a
            // letter rather than the quit key.
            || (matches!(
                self.screen,
                Screen::Themes | Screen::Patches | Screen::Packages
            ) && self.searching.is_some());
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
        let mut unique: Vec<&str> = Vec::with_capacity(names.len());
        for name in names {
            if !unique.contains(&name) {
                unique.push(name);
            }
        }
        unique
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
        self.picker
            .as_ref()
            .expect("the patches page loads its picker on entry")
    }

    fn picker_mut(&mut self) -> &mut Picker {
        self.picker
            .as_mut()
            .expect("the patches page loads its picker on entry")
    }

    /// Everything chosen, in sorted order.
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
        let collect = |want_dir: bool| -> Vec<PathBuf> {
            self.picker
                .as_ref()
                .map(|p| p.chosen_of(want_dir))
                .unwrap_or_default()
        };
        let picks = user_patches(
            &collect(false),
            &collect(true),
            &self.home,
            &self.dest_overrides,
        );
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
        self.picker
            .as_ref()
            .map(|p| p.chosen.keys().collect())
            .unwrap_or_default()
    }

    fn action(&self) -> Action {
        Action::ALL[self.action_row]
    }

    /// Carry out the highlighted action.
    fn apply(&mut self) {
        let action = self.action();
        // Abort is the one action that does not keep the answers. Everything
        // else — including generating without installing — counts as having
        // finished, so `main` writes the settings file.
        self.completed = action.saves();
        match action {
            Action::Abort | Action::SaveOnly => self.done = true,
            Action::SaveAs => {
                self.completed = false;
                self.saving_settings = Some(suggested_settings_name(&self.home));
            }
            Action::Generate => {
                let install = self.install;
                self.applied = Applied::Running(action.label(install));
                let repo = self.repo.clone();
                self.applied = match run_generate(self, &repo, install) {
                    // The host settings ride with the install tick, not with a
                    // bare render: generating promises that nothing outside
                    // this repo changes, and minimal's config and minvmd's
                    // state are both outside it.
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
        self.poll_registry();
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

    // Resolved once, as a pair: a run that reads a legacy file writes the new
    // location, which is what moves it.
    let (read_from, settings) = settings_paths(args.settings);
    let saved = State::load(&read_from);

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
        match app.to_state().save(&settings) {
            Ok(()) => println!("saved: {}", settings.display()),
            // Not fatal: the run happened, the answers just will not persist.
            Err(why) => eprintln!("could not save {}: {why}", settings.display()),
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
