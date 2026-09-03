//! The patches screen.

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
fn tab_swaps_between_the_two_pickers() {
    let (mut a, root) = on_patches("tab");
    assert_eq!(a.picker().kind, Pick::Files);
    a.on_key(press(KeyCode::Tab));
    assert_eq!(
        a.picker().kind,
        Pick::Dirs,
        "tab should reach the directory picker"
    );
    a.on_key(press(KeyCode::Tab));
    assert_eq!(a.picker().kind, Pick::Files, "and wrap back round");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn each_picker_only_takes_its_own_kind() {
    let (mut a, root) = on_patches("kinds");
    // Row 0 is the `dotfiles` directory in both.
    a.on_key(press(KeyCode::Char(' ')));
    assert!(
        a.chosen_paths().is_empty(),
        "the file picker must not take a directory"
    );
    a.on_key(press(KeyCode::Tab));
    a.on_key(press(KeyCode::Char(' ')));
    assert_eq!(
        a.chosen_paths()
            .iter()
            .map(|p| p.as_path())
            .collect::<Vec<_>>(),
        vec![root.join("dotfiles").as_path()],
        "the directory picker must take it"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn arrows_walk_the_tree_and_enter_moves_on() {
    // Enter is `done` on every other page, so descending is on the arrow
    // that points into the tree rather than stealing enter.
    let (mut a, root) = on_patches("walk");
    a.on_key(press(KeyCode::Right));
    assert_eq!(
        a.picker().cwd,
        root.join("dotfiles"),
        "right should descend"
    );
    a.on_key(press(KeyCode::Char(' ')));
    assert_eq!(
        a.chosen_paths().len(),
        1,
        "and the file inside is selectable"
    );
    a.on_key(press(KeyCode::Left));
    assert_eq!(a.picker().cwd, root, "left should climb back out");
    assert_eq!(a.chosen_paths().len(), 1, "without losing the selection");

    assert!(!a.done);
    a.on_key(press(KeyCode::Enter));
    assert_eq!(
        a.screen,
        Screen::Client,
        "enter should move on rather than descend"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn both_pickers_selections_are_reported_together() {
    let (mut a, root) = on_patches("both");
    a.on_key(press(KeyCode::Right));
    a.on_key(press(KeyCode::Char(' ')));
    a.on_key(press(KeyCode::Left));
    a.on_key(press(KeyCode::Tab));
    a.on_key(press(KeyCode::Char(' ')));
    let chosen: Vec<String> = a
        .chosen_paths()
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert_eq!(chosen.len(), 2, "{chosen:?}");
    assert!(
        chosen.iter().any(|p| p.ends_with("config.toml")),
        "{chosen:?}"
    );
    assert!(chosen.iter().any(|p| p.ends_with("dotfiles")), "{chosen:?}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn picking_your_own_config_displaces_the_loadouts_and_says_so() {
    let (mut a, home) = on_patches_with_conflict("file");

    while a.picker().current().unwrap().name != "starship.toml" {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Char(' ')));

    let displaced = a.displaced_configs();
    assert!(
        displaced.iter().any(|d| d == ".config/starship.toml"),
        "the loadout's own starship config should be displaced: {displaced:?}"
    );
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("using yours instead of"), "{text}");
    assert!(
        text.contains(".config/starship.toml"),
        "and should name it:\n{text}"
    );

    // And the summary repeats it, being the last screen before anything is
    // written.
    a.on_key(press(KeyCode::Enter)); // -> client
    a.on_key(press(KeyCode::Enter)); // -> resources
    a.on_key(press(KeyCode::Enter)); // -> apply
    assert_eq!(a.screen, Screen::Apply);
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("replaces"), "{text}");
    assert!(text.contains(".config/starship.toml"), "{text}");
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_long_list_of_picks_cannot_push_the_warning_off_the_screen() {
    // The summary strip was a fixed two rows and the chosen-path line
    // wraps, so a couple of long paths ate both and the warning — the one
    // thing on it that is not repeated anywhere else — was clipped away.
    let (mut a, home) = on_patches_with_conflict("crowded");
    while a.picker().current().unwrap().name != "starship.toml" {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Char(' ')));
    // Enough paths to wrap the chosen line well past one row.
    for i in 0..6 {
        a.pickers[0]
            .chosen
            .insert(home.join(format!(".config/a-fairly-long-config-name-{i}.toml")));
    }

    let text = flatten(&render_app(&a, 90, 26));
    assert!(
        text.contains("using yours instead of") && text.contains(".config/starship.toml"),
        "the warning has to survive a crowded summary line:\n{text}"
    );
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn picking_a_directory_displaces_everything_the_loadout_puts_in_it() {
    let (mut a, home) = on_patches_with_conflict("dir");

    a.on_key(press(KeyCode::Tab));
    while a.picker().current().unwrap().name != "helix" {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Char(' ')));

    let displaced = a.displaced_configs();
    assert!(
        displaced.len() >= 3,
        "config, languages and the theme: {displaced:?}"
    );
    assert!(
        displaced.iter().all(|d| d.starts_with(".config/helix/")),
        "{displaced:?}"
    );
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn nothing_picked_means_nothing_displaced() {
    let (a, home) = on_patches_with_conflict("none");
    assert!(a.displaced_configs().is_empty());
    let text = flatten(&render_app(&a, 110, 30));
    assert!(
        !text.contains("using yours instead of"),
        "no warning without a conflict"
    );
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn esc_steps_back_to_the_packages() {
    let (mut a, root) = on_patches("esc");
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Packages);
    assert!(!a.done);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_patches_page_keeps_the_chosen_theme() {
    let (a, root) = on_patches("theme");
    let want = a
        .loaded
        .as_ref()
        .expect("a scheme should be loaded")
        .palette["base00"];
    assert_eq!(frame_bg(&a, 100, 30), Color::Rgb(want.r, want.g, want.b));
    std::fs::remove_dir_all(&root).unwrap();
}
