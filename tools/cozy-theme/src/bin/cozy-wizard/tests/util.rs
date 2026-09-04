//! Fixtures the screen tests share: the page-walk chain that puts an `App`
//! on a given screen, and the helpers that render one to text.

#[allow(clippy::wildcard_imports)]
use crate::*;
#[allow(unused_imports)]
use ratatui::style::Color;
#[allow(unused_imports)]
use std::process::Command;

/// Every `App` a test builds, with the paths that reach outside the repository
/// pointed somewhere disposable.
///
/// This exists because the suite once wrote four schemes into the developer's
/// own `~/.config/cozy/schemes`: the save target moved to the user's directory
/// and twenty call sites did not. One constructor is one place to get that
/// right. `no_test_can_write_into_a_real_home` checks it stays right.
pub fn app_with(schemes: PathBuf, saved: State) -> App {
    let mut a = App::with_state(schemes, saved);
    let scratch = std::env::temp_dir().join(format!("cozy-test-home-{}", std::process::id()));
    a.home.clone_from(&scratch);
    // Never reach minimal.dev from a test: a real request per fixture is slow,
    // flaky, and aimed at somebody's actual web server.
    a.fetch_registry = false;
    a.user_schemes = scratch.join("schemes");
    a
}

/// A path that cannot exist, so detection is deterministic in tests rather
/// than depending on whether the repo has been fetched.
pub fn app() -> App {
    app_with(
        PathBuf::from("target/does-not-exist-for-tests"),
        State::default(),
    )
}

/// Empty whatever text field is open.
///
/// Counted backspaces are a trap: the suggested value in these prompts is a
/// path, and a longer default silently leaves a remnant that the typed text is
/// then appended to. This presses more than any field can hold.
pub fn clear_input(app: &mut App) {
    for _ in 0..512 {
        app.on_key(press(KeyCode::Backspace));
    }
}

pub fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
}

/// A private directory per test; tests run in parallel in one process.
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cozy-wizard-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

pub fn render_app(app: &App, w: u16, h: u16) -> Vec<String> {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

pub fn render(w: u16, h: u16) -> Vec<String> {
    render_app(&app(), w, h)
}

/// The frame as one line of prose: borders dropped and whitespace
/// collapsed, so a phrase the wrapper split across two rows still matches.
/// Searching the raw rows for \"not a git checkout\" failed for exactly
/// that reason — the text was right and the assertion was wrong.
pub fn flatten(rows: &[String]) -> String {
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

pub fn on_schemes() -> App {
    let mut a = app();
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Schemes);
    a
}

/// The theme browser against the real collection in this repo.
pub fn on_themes() -> App {
    let mut a = app_with(PathBuf::from("../../schemes/vendor"), State::default());
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('n')));
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Themes);
    a
}

/// The background colour the frame is painted in.
pub fn frame_bg(app: &App, w: u16, h: u16) -> Color {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    // A cell inside the preview, away from borders and text.
    terminal.backend().buffer()[(w - 4, h - 4)].bg
}

/// Drive a fetch to completion without the network: point the wizard at a
/// local repo so `git clone` is real but instant.
pub fn app_after_fetch(tag: &str) -> App {
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
    let mut a = app_with(dest, State::default());
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

/// The package chooser, with the real templates/packages.toml.
pub fn on_packages() -> App {
    let mut a = on_themes();
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Packages);
    assert!(!a.packages.is_empty(), "package list should have loaded");
    a
}

pub fn typing(app: &mut App, text: &str) {
    for c in text.chars() {
        app.on_key(press(KeyCode::Char(c)));
    }
}

/// The patches page, with the picker rooted at a private tree rather
/// than the real $HOME, so the tests do not depend on this machine.
pub fn on_patches(tag: &str) -> (App, PathBuf) {
    let root = temp_dir(&format!("patches-{tag}"));
    std::fs::create_dir_all(root.join("dotfiles")).unwrap();
    std::fs::write(root.join("dotfiles/config.toml"), "x").unwrap();
    std::fs::write(root.join("notes.md"), "n").unwrap();
    let mut a = on_packages();
    crate::ui::patches::enter_patches(&mut a);
    a.picker = Some(Picker::new(&root));
    assert_eq!(a.screen, Screen::Patches);
    (a, root)
}

/// A patches page whose file picker sits in a fake `~/.config` holding a
/// file the loadout also installs.
pub fn on_patches_with_conflict(tag: &str) -> (App, PathBuf) {
    let home = temp_dir(&format!("conflict-{tag}"));
    std::fs::create_dir_all(home.join(".config/helix")).unwrap();
    std::fs::write(home.join(".config/helix/config.toml"), "mine").unwrap();
    std::fs::write(home.join(".config/starship.toml"), "mine").unwrap();
    let mut a = on_packages();
    // Destinations are computed against this, not against the real home.
    a.home.clone_from(&home);
    crate::ui::patches::enter_patches(&mut a);
    a.picker = Some(Picker::new(&home.join(".config")));
    (a, home)
}

/// Walk a run to the end and return what it would write.
pub fn completed_run(pick_blocks: bool, theme_steps: usize) -> App {
    let mut a = app_with(PathBuf::from("../../schemes/vendor"), State::default());
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

pub fn on_apply() -> App {
    let mut a = on_client();
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Resources);
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Apply);
    a
}

pub fn on_client() -> App {
    let mut a = on_packages();
    crate::ui::patches::enter_patches(&mut a);
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Client);
    a
}

pub fn on_resources() -> App {
    let mut a = on_client();
    a.on_key(press(KeyCode::Enter));
    a
}
