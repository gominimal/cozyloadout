//! The scheme-collection screen.

#[allow(unused_imports)]
use super::util::*;
#[allow(unused_imports)]
use crate::picker::Pick;
#[allow(clippy::wildcard_imports)]
use crate::*;
#[allow(unused_imports)]
use ratatui::style::{Color, Modifier};
#[allow(unused_imports)]
use std::process::Command;

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
fn blocked_offers_no_yes_and_runs_nothing() {
    let dir = temp_dir("blocked");
    std::fs::create_dir_all(&dir).unwrap();
    let mut a = app_with(dir.clone(), State::default());
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
    let mut a = app_with(dir.clone(), State::default());
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
        text.contains(ui::schemes::CONTINUE_HINT),
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
