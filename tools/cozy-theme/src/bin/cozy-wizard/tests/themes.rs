//! The theme browser.

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
fn theme_intro_fits_in_its_rows() {
    // The third hand-sized intro in this file, and the third chance to
    // silently eat the end of a sentence. Same guard as the other two.
    let a = on_themes();
    for w in [56u16, 60, 72, 80, 100, 120] {
        let rows = render_app(&a, w, 30);
        assert!(
            flatten(&rows).contains("enter picks."),
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

// -- adjustments -----------------------------------------------------------

fn on_knobs() -> App {
    let mut a = on_themes();
    a.on_key(press(KeyCode::Char('a')));
    assert!(a.adjusting);
    a
}

#[test]
fn the_adjusting_intro_fits_in_the_same_rows() {
    // The heading block is a fixed height and now carries two different
    // sentences. The second one gets the same guard as the first.
    let a = on_knobs();
    for w in [56u16, 60, 72, 80, 100, 120] {
        let rows = render_app(&a, w, 30);
        assert!(
            flatten(&rows).contains("what WCAG measures."),
            "adjust intro truncated at {w} columns:\n{}",
            rows[..7].join("\n")
        );
    }
}

#[test]
fn a_opens_the_knobs_and_a_or_esc_closes_them() {
    let mut a = on_knobs();
    let text = flatten(&render_app(&a, 110, 30));
    for label in [
        "contrast",
        "accents",
        "comments",
        "surfaces",
        "background",
        "warmth",
    ] {
        assert!(text.contains(label), "{label} missing:\n{text}");
    }
    a.on_key(press(KeyCode::Char('a')));
    assert!(!a.adjusting);
    assert_eq!(
        a.screen,
        Screen::Themes,
        "closing must not leave the screen"
    );

    a.on_key(press(KeyCode::Char('a')));
    a.on_key(press(KeyCode::Esc));
    assert!(!a.adjusting);
    assert_eq!(
        a.screen,
        Screen::Themes,
        "esc closes the panel, not the page"
    );
}

#[test]
fn esc_still_leaves_the_page_when_the_knobs_are_closed() {
    let mut a = on_themes();
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Schemes);
}

#[test]
fn arrows_pick_a_knob_and_move_only_that_one() {
    let mut a = on_knobs();
    a.on_key(press(KeyCode::Right));
    assert_eq!(a.adjust.contrast, 5, "right should raise the focused knob");
    assert_eq!(a.adjust.saturation, 0, "and leave the others alone");

    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Left));
    assert_eq!(a.adjust.saturation, -5);
    assert_eq!(a.adjust.contrast, 5, "the first knob keeps its value");
}

#[test]
fn a_knob_clamps_at_both_ends() {
    let mut a = on_knobs();
    for _ in 0..40 {
        a.on_key(press(KeyCode::Right));
    }
    assert_eq!(a.adjust.contrast, 100);
    for _ in 0..80 {
        a.on_key(press(KeyCode::Left));
    }
    assert_eq!(a.adjust.contrast, -100);
}

#[test]
fn home_and_end_run_a_knob_to_its_stop() {
    let mut a = on_knobs();
    a.on_key(press(KeyCode::End));
    assert_eq!(a.adjust.contrast, 100);
    a.on_key(press(KeyCode::Home));
    assert_eq!(a.adjust.contrast, -100);
}

#[test]
fn r_resets_every_knob() {
    let mut a = on_knobs();
    a.on_key(press(KeyCode::End));
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::End));
    assert!(!a.adjust.is_identity());
    a.on_key(press(KeyCode::Char('r')));
    assert!(a.adjust.is_identity());
}

#[test]
fn the_knobs_repaint_the_preview_as_they_move() {
    // The reason the panel replaces the list rather than sitting beside it:
    // the preview is the point, and it has to answer immediately.
    let mut a = on_knobs();
    let before = frame_bg(&a, 110, 30);
    for _ in 0..12 {
        a.on_key(press(KeyCode::Left)); // contrast down, background lifts
    }
    assert_ne!(
        frame_bg(&a, 110, 30),
        before,
        "moving a knob should repaint the interface"
    );
}

#[test]
fn the_panel_reports_the_wcag_ratio_and_whether_it_passes() {
    // What makes this a measurement rather than a matter of taste.
    let mut a = on_knobs();
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("text on bg"), "{text}");
    assert!(text.contains(":1"), "{text}");
    assert!(text.contains("WCAG AA"), "{text}");

    // Crushing the contrast has to move it the wrong way and say so.
    for _ in 0..40 {
        a.on_key(press(KeyCode::Left));
    }
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("below WCAG AA"), "{text}");
}

#[test]
fn adjustments_reach_the_summary_and_the_slug() {
    let mut a = on_knobs();
    a.on_key(press(KeyCode::Right));
    a.on_key(press(KeyCode::Right));
    let adjusted_slug = a.scheme().unwrap().slug;
    a.on_key(press(KeyCode::Enter)); // -> packages
    for _ in 0..4 {
        a.on_key(press(KeyCode::Enter)); // patches, client, resources, apply
    }
    assert_eq!(a.screen, Screen::Apply);

    let text = flatten(&render_app(&a, 120, 40));
    assert!(text.contains("contrast +10"), "{text}");
    assert!(
        text.contains(&adjusted_slug),
        "the summary should name the slug the files will use:\n{text}"
    );
}

#[test]
fn an_untouched_scheme_says_so_on_the_summary() {
    let a = on_apply();
    let text = flatten(&render_app(&a, 120, 40));
    assert!(text.contains("the scheme as published"), "{text}");
}

#[test]
fn the_knobs_survive_to_the_next_run_and_a_bad_value_does_not() {
    let mut a = on_knobs();
    a.on_key(press(KeyCode::Right));
    let saved = a.to_state();
    assert_eq!(saved.contrast, Some(5));
    assert_eq!(
        App::with_state(a.schemes_dir.clone(), saved)
            .adjust
            .contrast,
        5
    );

    let bad = State {
        contrast: Some(120),
        warmth: Some(-30),
        ..State::default()
    };
    let back = App::with_state(a.schemes_dir.clone(), bad);
    assert_eq!(back.adjust.contrast, 0, "an out-of-range knob falls back");
    assert_eq!(back.adjust.warmth, -30, "without taking the others with it");
}

#[test]
#[ignore = "prints a frame to look at rather than asserting"]
fn show_the_knobs() {
    let mut a = on_knobs();
    a.on_key(press(KeyCode::Right));
    a.on_key(press(KeyCode::Right));
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Left));
    for line in render_app(&a, 100, 24) {
        println!("|{}|", line.trim_end());
    }
}

#[test]
fn every_screen_still_advertises_its_own_keys() {
    // The footer moved out of one big match into the screen modules; this is
    // what says none of them was dropped or truncated on the way.
    let expected: &[(Screen, &[&str])] = &[
        (Screen::Greeting, &["move", "choose", "quit"]),
        (
            Screen::Themes,
            &["browse", "page", "adjust", "choose", "back"],
        ),
        (
            Screen::Packages,
            &["move", "toggle", "all/none", "add by name", "done"],
        ),
        (Screen::Patches, &["in/out", "choose", "files/dirs", "done"]),
        (Screen::Client, &["move", "change", "reset", "back", "done"]),
        (
            Screen::Resources,
            &["cores/memory", "adjust", "reset", "done"],
        ),
        (Screen::Apply, &["move", "do it", "back"]),
    ];
    let mut a = on_themes();
    for (screen, words) in expected {
        a.screen = *screen;
        let footer: String = crate::ui::footer_hints(&a)
            .into_iter()
            .map(|s| s.content.to_string())
            .collect();
        for w in *words {
            assert!(
                footer.contains(w),
                "{screen:?} footer is missing {w:?}: {footer:?}"
            );
        }
    }
}
