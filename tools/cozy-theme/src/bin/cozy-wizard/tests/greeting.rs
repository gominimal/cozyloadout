//! The greeting screen.

#[allow(unused_imports)]
use super::util::*;
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
fn the_unconfigured_chord_is_the_one_the_template_falls_back_to() {
    // The preview shows whatever the client page currently holds, which starts
    // at `Bindings::default()`. That default and the template's own fallback
    // both track minimal's `${MINIMAL_DETACH_HINT:-ctrl-] then d}`, and this is
    // what says so if either moves.
    let fallback = crate::keys::Bindings::default().hint();
    let template = include_str!("../../../../../../templates/fish/config.fish");
    assert!(
        template.contains(&format!("set -g __cozy_detach \"{fallback}\"")),
        "the template's fallback no longer matches Bindings::default(): {fallback:?}"
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
            // The *last* words, not the first: the end of the sentence is
            // what a row too few takes away, and this intro has grown twice.
            flatten(&rows).contains("and the icons too."),
            "intro truncated at {w} columns — INTRO_ROWS is too small:\n{}",
            rows[..8].join("\n")
        );
    }
}

#[test]
fn the_preview_shows_the_chord_the_client_page_currently_holds() {
    // The greeting screen comes first, but the chord it advertises is a real
    // setting the user can change five screens later — and change back to on a
    // second pass. Showing a hardcoded default there would make the preview
    // wrong for exactly the people who bothered to configure it.
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    clear_input(&mut a);
    typing(&mut a, "ctrl-a");
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.bindings.hint(), "ctrl-a then d");

    // Walk back to the greeting screen and look at the preview.
    a.screen = Screen::Greeting;
    let text = flatten(&render_app(&a, 110, 30));
    assert!(
        text.contains("ctrl-a then d to detach"),
        "the preview should follow the client page:\n{text}"
    );
    assert!(
        !text.contains("ctrl-] then d"),
        "and must not still show the default:\n{text}"
    );
}

// -- the icon question -----------------------------------------------------

#[test]
fn the_greeting_screen_asks_about_icons_by_showing_them() {
    // The same reason the marks are previewed rather than described: a font
    // either has these glyphs or draws boxes, and one glance settles it.
    let a = app();
    let text = flatten(&render_app(&a, 100, 30));
    assert!(text.contains("icons in the file lists"), "{text}");
    assert!(text.contains("space toggles"), "{text}");
    for glyph in crate::icons::sample() {
        assert!(
            text.contains(glyph),
            "the sample must actually draw {:?} (U+{:04X}):\n{text}",
            glyph,
            glyph as u32
        );
    }
}

#[test]
fn space_toggles_icons_without_leaving_the_page() {
    let mut a = app();
    assert!(
        a.icons,
        "on by default, matching the eza the loadout installs"
    );
    a.on_key(press(KeyCode::Char(' ')));
    assert!(!a.icons);
    assert_eq!(a.screen, Screen::Greeting, "space must not advance");
    assert!(
        a.greeting.is_none(),
        "and must not count as choosing a mark"
    );
    assert!(flatten(&render_app(&a, 100, 30)).contains("[ ] icons"));

    a.on_key(press(KeyCode::Char(' ')));
    assert!(a.icons);
    assert!(flatten(&render_app(&a, 100, 30)).contains("[x] icons"));
}

#[test]
fn enter_still_chooses_the_mark_and_moves_on() {
    let mut a = app();
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Schemes);
    assert!(a.greeting.is_some());
}

#[test]
fn the_answer_survives_to_the_next_run() {
    let mut a = app();
    a.on_key(press(KeyCode::Char(' ')));
    let saved = a.to_state();
    assert_eq!(saved.icons, Some(false));
    assert!(!app_with(a.schemes_dir.clone(), saved).icons);

    // Never asked takes the default rather than "off" — a settings file written
    // before this existed should not silently turn them off.
    let older = cozy_theme::Settings::default();
    assert_eq!(older.icons, None);
    assert!(app_with(a.schemes_dir.clone(), older).icons);
}

#[test]
#[ignore = "prints a frame to look at rather than asserting"]
fn show_the_greeting_screen() {
    let a = app();
    for line in render_app(&a, 96, 22) {
        println!("|{}|", line.trim_end());
    }
}
