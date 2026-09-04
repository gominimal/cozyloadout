//! The client and VM screens.

#[allow(unused_imports)]
use super::util::*;
#[allow(clippy::wildcard_imports)]
use crate::*;
#[allow(unused_imports)]
use ratatui::style::{Color, Modifier};
#[allow(unused_imports)]
use std::process::Command;

#[test]
fn the_client_page_explains_that_it_is_not_the_loadout() {
    // The one page that writes outside `build/`. If that stops being said
    // plainly, someone will change their leader expecting it to be
    // undone by deleting the repo.
    let a = on_client();
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("minimal's own settings"), "{text}");
    assert!(text.contains("config.toml"), "{text}");
    assert!(text.contains("every session"), "{text}");
}

#[test]
fn the_client_page_starts_on_minimals_shipped_defaults() {
    let a = on_client();
    assert!(a.bindings.is_default());
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("ctrl-] then d"), "{text}");
    assert!(
        text.contains("nothing will be written"),
        "an untouched page should promise to write nothing:\n{text}"
    );
}

#[test]
fn retyping_a_chord_changes_the_detach_gesture() {
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' '))); // edit the leader
    assert!(a.editing.is_some());
    clear_input(&mut a);
    typing(&mut a, "ctrl-a");
    a.on_key(press(KeyCode::Enter));
    assert!(a.editing.is_none(), "a valid chord should commit");
    assert_eq!(a.bindings.hint(), "ctrl-a then d");
    assert!(!a.bindings.is_default());
}

#[test]
fn a_chord_minimal_would_refuse_is_refused_here_with_the_reason() {
    // ctrl-w is the one that matters: it is why the greeting's advice
    // changed, and it is exactly what someone reaching for the old key
    // would type.
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    clear_input(&mut a);
    typing(&mut a, "ctrl-w");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(
        text.contains("VWERASE"),
        "the reason, not just a refusal:\n{text}"
    );

    a.on_key(press(KeyCode::Enter));
    assert!(a.editing.is_some(), "a refused chord keeps the field open");
    assert!(a.bindings.is_default(), "and changes nothing");
}

#[test]
fn a_detach_key_that_shadows_the_leader_is_refused_on_screen() {
    let mut a = on_client();
    a.on_key(press(KeyCode::Down)); // the detach row
    a.on_key(press(KeyCode::Char(' ')));
    for _ in 0..4 {
        a.on_key(press(KeyCode::Backspace));
    }
    // The default forward key *is* the leader, so this shadows both.
    typing(&mut a, "ctrl-]");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("shadows"), "{text}");
    a.on_key(press(KeyCode::Enter));
    assert!(a.bindings.is_default(), "nothing should have changed");
}

#[test]
fn typing_the_leader_chord_itself_is_captured_as_text() {
    // The page has to be able to configure the very key you press to get
    // out of a session, so ctrl-<x> is read as the chord it names rather
    // than as a keystroke to act on.
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    a.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert_eq!(a.editing.as_deref(), Some("ctrl-a"));
}

#[test]
fn q_is_a_letter_while_typing_a_chord() {
    // The same trap the packages page had: `q` quits everywhere else.
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    a.on_key(press(KeyCode::Char('q')));
    assert!(!a.done, "q should be text here, not the quit key");
    assert!(a.editing.as_deref().unwrap_or("").ends_with('q'));
}

#[test]
fn r_puts_the_chords_back_to_the_defaults() {
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    clear_input(&mut a);
    typing(&mut a, "ctrl-a");
    a.on_key(press(KeyCode::Enter));
    assert!(!a.bindings.is_default());
    a.on_key(press(KeyCode::Char('r')));
    assert!(a.bindings.is_default(), "r should reset");
}

#[test]
fn the_bell_row_toggles_rather_than_opening_an_editor() {
    let mut a = on_client();
    for _ in 0..3 {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Char(' ')));
    assert!(a.editing.is_none(), "the bell is a flag, not a chord");
    assert!(a.bindings.bell);
    a.on_key(press(KeyCode::Char(' ')));
    assert!(!a.bindings.bell);
}

#[test]
fn enter_moves_on_from_the_client_page_like_everywhere_else() {
    // Enter finishing a page is the one convention that holds across the
    // whole wizard. The client page briefly used it to open an editor,
    // which made this the only screen where finishing was a different key.
    let mut a = on_client();
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Resources);
    assert!(a.editing.is_none(), "enter must not open an editor");
}

#[test]
fn space_is_what_edits_a_chord() {
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    assert_eq!(a.screen, Screen::Client, "space must not leave the page");
    assert_eq!(a.editing.as_deref(), Some("ctrl-]"));
}

#[test]
fn neither_host_page_binds_tab() {
    // Space changes the row under the cursor and enter finishes, as on the
    // packages page. Tab has no third job to do here.
    let mut a = on_client();
    a.on_key(press(KeyCode::Tab));
    assert_eq!(a.screen, Screen::Client);
    assert!(a.editing.is_none(), "tab should do nothing here");

    let mut a = on_resources();
    let before = a.resources.field;
    a.on_key(press(KeyCode::Tab));
    assert_eq!(a.screen, Screen::Resources);
    assert_eq!(a.resources.field, before, "tab should do nothing here");
}

#[test]
fn the_client_footer_advertises_the_keys_it_actually_uses() {
    let a = on_client();
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("space change"), "{text}");
    assert!(text.contains("enter done"), "{text}");
}

#[test]
fn the_vm_page_shows_the_host_and_its_ceilings() {
    let a = on_resources();
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("This machine"), "{text}");
    assert!(text.contains("Allocatable"), "{text}");
    assert!(
        text.contains("CPU cores") && text.contains("Memory"),
        "{text}"
    );
    assert!(
        text.contains("minvmd"),
        "it should name what applies this:\n{text}"
    );
}

#[test]
fn the_vm_page_adjusts_the_focused_field_only() {
    let mut a = on_resources();
    let before = a.resources.allocation();
    a.on_key(press(KeyCode::Right));
    let after = a.resources.allocation();
    assert_eq!(after.vcpus, before.vcpus + 1, "cores have the focus first");
    assert_eq!(after.ram_mib, before.ram_mib, "memory should be untouched");

    a.on_key(press(KeyCode::Down)); // switch fields
    a.on_key(press(KeyCode::Right));
    assert!(a.resources.allocation().ram_mib > before.ram_mib);
}

/// Not a check — a way to eyeball the two new pages.
/// `cargo test --bin cozy-wizard -- --ignored show_ --nocapture`
#[test]
#[ignore = "prints frames to look at rather than asserting"]
fn show_the_host_pages() {
    for (name, app) in [("client", on_client()), ("resources", on_resources())] {
        println!("\n===== {name} =====");
        for line in render_app(&app, 100, 24) {
            println!("|{}|", line.trim_end());
        }
    }
}
