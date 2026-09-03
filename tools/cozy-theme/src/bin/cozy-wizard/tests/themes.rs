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
