//! The greeting screen.

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
        text.contains("ctrl-] then d to detach"),
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
        !text.contains("ctrl-] then d to detach"),
        "nothing is printed at all"
    );
}

#[test]
fn the_previewed_detach_chord_is_the_one_the_template_falls_back_to() {
    // The preview shows a chord the session has not negotiated yet, so the
    // only thing keeping it honest is that it equals the template's own
    // fallback. Both track minimal's `${MINIMAL_DETACH_HINT:-ctrl-] then
    // d}`; if that default moves again, this is what says so.
    let template = include_str!("../../../../../../templates/fish/config.fish");
    assert!(
        template.contains(&format!("set -g __cozy_detach \"{DETACH_FALLBACK}\"")),
        "the template's fallback no longer matches the preview's {DETACH_FALLBACK:?}"
    );
    assert!(
        template.contains("set -g __cozy_detach $MINIMAL_DETACH_HINT"),
        "the greeting should read the negotiated chord, not hardcode one"
    );
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
