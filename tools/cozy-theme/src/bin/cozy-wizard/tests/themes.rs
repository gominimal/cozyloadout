//! The theme browser.

#[allow(unused_imports)]
use super::util::*;
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
    assert_eq!(app_with(a.schemes_dir.clone(), saved).adjust.contrast, 5);

    let bad = State {
        contrast: Some(120),
        warmth: Some(-30),
        ..State::default()
    };
    let back = app_with(a.schemes_dir.clone(), bad);
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
        // Only the unconditional keys: `e` appears when there is something
        // under the cursor, which this fixture has no picker for.
        // `e_is_advertised_before_anything_is_chosen` covers that one.
        (
            Screen::Patches,
            &["move", "in/out", "choose", "back", "done"],
        ),
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

// -- adjustments belong to the scheme they were made against ---------------

#[test]
fn moving_to_another_scheme_clears_the_adjustments() {
    // +40 comments rescues one palette and ruins the next, so an adjustment
    // does not follow the cursor onto a scheme it was never tuned against.
    let mut a = on_knobs();
    a.on_key(press(KeyCode::End)); // contrast to +100
    assert!(!a.adjust.is_identity());

    a.on_key(press(KeyCode::Esc)); // back to the list
    assert!(
        !a.adjust.is_identity(),
        "closing the panel must not clear them"
    );
    a.on_key(press(KeyCode::Down)); // a different scheme
    assert!(
        a.adjust.is_identity(),
        "a new scheme starts from what its author published: {:?}",
        a.adjust
    );
}

#[test]
fn a_move_that_does_not_change_the_scheme_keeps_them() {
    // Holding ↑ at the top of the list is not a way to lose your work.
    let mut a = on_knobs();
    a.on_key(press(KeyCode::End));
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.theme_row, 0, "this fixture starts at the top");
    for _ in 0..5 {
        a.on_key(press(KeyCode::Up));
    }
    assert!(!a.adjust.is_identity(), "a clamped move changes nothing");

    // And coming back to the same scheme the long way round keeps them too.
    a.on_key(press(KeyCode::Down));
    assert!(a.adjust.is_identity(), "but a real move does clear them");
}

#[test]
fn the_list_says_whether_the_scheme_is_adjusted() {
    // The knobs are off screen while browsing, so without this the reset above
    // would be a silent loss.
    let mut a = on_themes();
    assert!(flatten(&render_app(&a, 110, 30)).contains("a to adjust"));

    a.on_key(press(KeyCode::Char('a')));
    a.on_key(press(KeyCode::End));
    a.on_key(press(KeyCode::Esc));
    let text = flatten(&render_app(&a, 110, 30));
    assert!(
        text.contains("contrast +100"),
        "the list should say what is set:\n{text}"
    );
    assert!(!text.contains("a to adjust"), "and drop the hint:\n{text}");

    a.on_key(press(KeyCode::Down));
    let text = flatten(&render_app(&a, 110, 30));
    assert!(
        text.contains("a to adjust") && !text.contains("contrast +100"),
        "the line should empty as the reset happens:\n{text}"
    );
}

#[test]
fn revisiting_the_page_keeps_this_runs_adjustments() {
    // Going forward and coming back is not "moving to another scheme", so the
    // same rule that preserves the chosen theme preserves its adjustments.
    let mut a = on_knobs();
    a.on_key(press(KeyCode::End));
    let knobs = a.adjust;
    a.on_key(press(KeyCode::Enter)); // -> packages
    a.on_key(press(KeyCode::Esc)); // back to themes
    assert_eq!(a.screen, Screen::Themes);
    assert_eq!(a.adjust, knobs, "coming back must not reset them");
}

#[test]
fn a_remembered_scheme_that_is_gone_takes_its_adjustments_with_it() {
    // They were tuned against a palette this checkout does not have.
    let dir = temp_dir("gone-scheme");
    let saved = State {
        theme: Some("a scheme that is not here".into()),
        contrast: Some(60),
        ..State::default()
    };
    let mut a = app_with(dir.clone(), saved);
    assert_eq!(a.adjust.contrast, 60, "restored before the page is entered");
    crate::ui::themes::enter_themes(&mut a);
    assert!(
        a.adjust.is_identity(),
        "a scheme that is gone should not leave its adjustments behind"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_remembered_scheme_that_is_still_here_keeps_its_adjustments() {
    // The other half: this is what makes the knobs sticky across runs.
    let mut a = on_knobs();
    a.on_key(press(KeyCode::End));
    let saved = a.to_state();
    assert_eq!(saved.contrast, Some(100));

    let mut back = app_with(a.schemes_dir.clone(), saved);
    crate::ui::themes::enter_themes(&mut back);
    assert_eq!(
        back.adjust.contrast, 100,
        "the remembered scheme is still on disk, so its adjustments stand"
    );
    assert_eq!(
        back.schemes[back.theme_row].name, a.schemes[a.theme_row].name,
        "and it is the same scheme"
    );
}

#[test]
#[ignore = "prints frames to look at rather than asserting"]
fn show_the_list_footer() {
    let mut a = on_themes();
    for (label, keys) in [
        ("untouched", vec![]),
        (
            "adjusted",
            vec![KeyCode::Char('a'), KeyCode::End, KeyCode::Esc],
        ),
        ("after moving on", vec![KeyCode::Down]),
    ] {
        for k in keys {
            a.on_key(press(k));
        }
        let rows = render_app(&a, 100, 14);
        println!("\n=== {label}");
        for line in &rows[rows.len() - 3..] {
            println!("|{}|", line.trim_end());
        }
    }
}

// -- save as ---------------------------------------------------------------

/// A themes page over a private scheme tree, so a save lands in a temporary
/// directory rather than in the repository's own `schemes/`.
fn on_private_themes(tag: &str) -> (App, PathBuf) {
    let root = temp_dir(&format!("save-{tag}"));
    let vendor = root.join("vendor/base16");
    std::fs::create_dir_all(&vendor).unwrap();
    let src = std::fs::read_to_string("../../schemes/minimal-dark.yaml").unwrap();
    std::fs::write(vendor.join("minimal-dark.yaml"), &src).unwrap();
    std::fs::write(vendor.join("zzz-other.yaml"), &src).unwrap();

    let mut a = app_with(root.join("vendor"), State::default());
    // Saved schemes go to the user's own directory; point that at the fixture
    // so a test cannot write into a real home.
    a.user_schemes.clone_from(&root);
    a.home.clone_from(&root);
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('n')));
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Themes);
    (a, root)
}

/// Adjust, then open the save prompt.
fn at_save_prompt(tag: &str) -> (App, PathBuf) {
    let (mut a, root) = on_private_themes(tag);
    a.on_key(press(KeyCode::Char('a')));
    a.on_key(press(KeyCode::End)); // contrast +100
    a.on_key(press(KeyCode::Char('s')));
    assert!(a.saving.is_some(), "s should open the prompt");
    (a, root)
}

#[test]
fn saving_is_offered_only_once_something_is_adjusted() {
    // Saving an untouched scheme under a second name is a copy, not a save.
    let (mut a, root) = on_private_themes("gated");
    a.on_key(press(KeyCode::Char('s')));
    assert!(a.saving.is_none(), "nothing to save yet");

    a.on_key(press(KeyCode::Char('a')));
    a.on_key(press(KeyCode::End));
    a.on_key(press(KeyCode::Esc)); // back to the list
    a.on_key(press(KeyCode::Char('s')));
    assert!(a.saving.is_some(), "and offered from the list too");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_prompt_suggests_a_name_and_shows_the_file_it_will_write() {
    let (a, root) = at_save_prompt("prompt");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("Save this scheme as"), "{text}");
    assert!(
        text.contains("Minimal Dark custom"),
        "a suggestion:\n{text}"
    );
    assert!(
        text.contains("minimal-dark-custom.yaml"),
        "and the filename, so the slugifying is not a surprise:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn saving_writes_a_scheme_that_loads_back() {
    let (mut a, root) = at_save_prompt("writes");
    clear_input(&mut a);
    typing(&mut a, "My Theme");
    a.on_key(press(KeyCode::Enter));
    assert!(a.saving.is_none(), "a good name should close the prompt");

    let path = root.join("my-theme.yaml");
    assert!(path.exists(), "expected {}", path.display());
    let back = cozy_theme::Scheme::load(&path).unwrap();
    assert_eq!(back.name, "My Theme");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_saved_scheme_has_the_adjustments_baked_in_and_the_knobs_reset() {
    // The knobs go to zero because the edits are now *in* the scheme. Leaving
    // them set would apply every one of them a second time.
    let (mut a, root) = at_save_prompt("baked");
    let adjusted = a.scheme().unwrap().palette.clone();
    clear_input(&mut a);
    typing(&mut a, "Baked");
    a.on_key(press(KeyCode::Enter));

    assert!(a.adjust.is_identity(), "the knobs should be back to zero");
    assert_eq!(
        a.scheme().unwrap().palette,
        adjusted,
        "and the scheme now on screen is the adjusted one"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn saving_selects_the_new_scheme_in_the_list() {
    let (mut a, root) = at_save_prompt("select");
    clear_input(&mut a);
    typing(&mut a, "Aaa Mine");
    a.on_key(press(KeyCode::Enter));

    assert_eq!(
        a.schemes[a.theme_row].name, "aaa-mine",
        "the cursor should land on what was just saved"
    );
    assert!(
        a.schemes.iter().any(|s| s.name == "aaa-mine"),
        "and it should be in the list"
    );
    let text = flatten(&render_app(&a, 110, 30));
    assert!(
        text.contains("saved as aaa-mine"),
        "with a confirmation:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_name_that_is_taken_keeps_the_prompt_open_and_says_why() {
    let (mut a, root) = at_save_prompt("taken");
    clear_input(&mut a);
    typing(&mut a, "minimal-dark");
    a.on_key(press(KeyCode::Enter));

    assert!(a.saving.is_some(), "a refused name keeps the prompt open");
    assert!(!a.adjust.is_identity(), "and must not clear the work");
    let text = flatten(&render_app(&a, 70, 30));
    assert!(text.contains("already exists"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_name_with_nothing_in_it_is_refused_rather_than_writing_yaml() {
    let (mut a, root) = at_save_prompt("empty");
    clear_input(&mut a);
    typing(&mut a, "!!!");
    a.on_key(press(KeyCode::Enter));
    assert!(a.saving.is_some());
    // Narrow, so the preview column is hidden: `flatten` reads across the
    // whole row, and with two columns the message interleaves with the sample
    // code beside it.
    let text = flatten(&render_app(&a, 70, 30));
    assert!(text.contains("no letters or digits"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn esc_cancels_the_prompt_without_writing_anything() {
    let (mut a, root) = at_save_prompt("cancel");
    a.on_key(press(KeyCode::Esc));
    assert!(a.saving.is_none());
    assert!(!a.adjust.is_identity(), "cancelling keeps the adjustments");
    assert!(
        std::fs::read_dir(&root)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "yaml")
            })
            .count()
            == 0,
        "nothing should have been written"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn q_is_a_letter_while_naming_a_scheme() {
    // The same trap the packages and client pages have.
    let (mut a, root) = at_save_prompt("q-key");
    a.on_key(press(KeyCode::Char('q')));
    assert!(!a.done, "q should be text here, not the quit key");
    assert!(a.saving.as_deref().unwrap_or("").ends_with('q'));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn moving_on_clears_a_stale_save_confirmation() {
    let (mut a, root) = at_save_prompt("stale");
    clear_input(&mut a);
    typing(&mut a, "Zzz Last");
    a.on_key(press(KeyCode::Enter));
    assert!(a.saved_note.is_some());

    a.on_key(press(KeyCode::Up));
    assert!(
        a.saved_note.is_none(),
        "the note belongs to the scheme it was saved from"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "prints frames to look at rather than asserting"]
fn show_the_save_flow() {
    let (mut a, root) = at_save_prompt("show");
    println!("\n=== the prompt");
    for line in render_app(&a, 96, 16) {
        println!("|{}|", line.trim_end());
    }
    clear_input(&mut a);
    typing(&mut a, "Warm Dark");
    a.on_key(press(KeyCode::Enter));
    println!("\n=== after saving");
    for line in render_app(&a, 96, 16) {
        println!("|{}|", line.trim_end());
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_preview_survives_the_scheme_list_emptying_underneath_it() {
    let root = temp_dir("vanishing");
    let vendor = root.join("vendor/base16");
    std::fs::create_dir_all(&vendor).unwrap();
    let src = std::fs::read_to_string("../../schemes/minimal-dark.yaml").unwrap();
    std::fs::write(vendor.join("minimal-dark.yaml"), &src).unwrap();

    let mut a = app_with(root.join("vendor"), State::default());
    a.user_schemes = root.join("mine");
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('n')));
    a.on_key(press(KeyCode::Enter));
    let _ = render_app(&a, 110, 30);

    // The collection goes away while the wizard is on another page.
    std::fs::remove_dir_all(&vendor).unwrap();
    a.on_key(press(KeyCode::Enter)); // -> packages
    crate::ui::themes::enter_themes(&mut a); // and back
    assert!(a.schemes.is_empty());
    let _ = render_app(&a, 110, 30);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn no_test_can_write_into_a_real_home() {
    // This suite once wrote four schemes into the developer's own
    // ~/.config/cozy/schemes, because the save target moved to the user's
    // directory before the fixtures did. Every App a test builds must point
    // somewhere disposable.
    let real =
        std::env::var_os("HOME").map(|h| cozy_theme::user_schemes_dir(std::path::Path::new(&h)));
    for (name, app) in [
        ("on_themes", on_themes()),
        ("on_private_themes", on_private_themes("guard").0),
    ] {
        if let Some(real) = &real {
            assert_ne!(
                &app.user_schemes, real,
                "{name} would save into a real home directory"
            );
        }
    }
}
