//! Going back, and coming forward again.

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
fn revisiting_a_page_keeps_what_you_changed_this_run() {
    // Every page restores from the sticky file when it opens. Re-applying
    // that on a *second* visit would silently undo everything the user did
    // this run — go back to check something, come forward, and your work
    // is gone.
    let saved = State {
        theme: Some("3024".into()),
        extra: "from-the-file".into(),
        ..State::default()
    };
    let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), saved);
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('n')));
    a.on_key(press(KeyCode::Enter)); // -> themes, restored to "3024"
    assert_eq!(a.schemes[a.theme_row].name, "3024");

    // Change it, then go back and forward again.
    for _ in 0..5 {
        a.on_key(press(KeyCode::Down));
    }
    let chosen = a.schemes[a.theme_row].name.clone();
    assert_ne!(chosen, "3024", "the test needs to have actually moved");
    a.on_key(press(KeyCode::Esc)); // -> schemes
    a.on_key(press(KeyCode::Enter)); // -> themes again
    assert_eq!(
        a.schemes[a.theme_row].name, chosen,
        "coming back should show this run's choice, not the file's"
    );

    // Same for the package page's toggles and its text field.
    a.on_key(press(KeyCode::Enter)); // -> packages
    assert_eq!(a.extra, "from-the-file");
    a.on_key(press(KeyCode::Char(' ')));
    let dropped = a.packages[0].name.clone();
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "-typed-now");
    a.on_key(press(KeyCode::Esc)); // leave the field
    a.on_key(press(KeyCode::Esc)); // -> themes
    a.on_key(press(KeyCode::Enter)); // -> packages again
    assert_eq!(
        a.extra, "from-the-file-typed-now",
        "the field should keep what was typed"
    );
    assert!(
        !a.chosen_packages().contains(&dropped.as_str()),
        "{dropped} was switched off this run and should have stayed off"
    );
}

#[test]
fn revisiting_the_patches_page_keeps_this_run_s_choices() {
    // The pickers guard on `is_empty()` rather than a visit count, so this
    // checks the guard actually holds — including that walking somewhere
    // else and coming back does not reset the browser to $HOME either.
    let (mut a, root) = on_patches("revisit");
    a.on_key(press(KeyCode::Tab));
    a.on_key(press(KeyCode::Char(' ')));
    let chosen: Vec<PathBuf> = a.chosen_paths().into_iter().cloned().collect();
    assert_eq!(chosen.len(), 1);

    a.on_key(press(KeyCode::Esc)); // -> packages
    a.on_key(press(KeyCode::Enter)); // -> patches again
    assert_eq!(
        a.chosen_paths().into_iter().cloned().collect::<Vec<_>>(),
        chosen,
        "coming back should keep what was picked this run"
    );
    assert_eq!(
        a.picker().cwd,
        root,
        "and should not have jumped back to $HOME"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn every_page_after_the_first_offers_a_way_back() {
    // Esc steps back everywhere it can, but a key nobody mentions is a key
    // nobody presses.
    let mut a = App::with_state(PathBuf::from("../../schemes/vendor"), State::default());
    a.on_key(press(KeyCode::Enter));
    for expect_back in [
        Screen::Schemes,
        Screen::Themes,
        Screen::Packages,
        Screen::Patches,
    ] {
        assert_eq!(a.screen, expect_back);
        let footer: String = ui::footer_hints(&a)
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(
            footer.contains("esc"),
            "{expect_back:?} does not advertise a way back: {footer:?}"
        );
        if expect_back == Screen::Schemes {
            // Decline the fetch: enter would otherwise start one, and a
            // test must not reach the network.
            a.on_key(press(KeyCode::Char('n')));
        }
        if expect_back != Screen::Patches {
            a.on_key(press(KeyCode::Enter));
        }
    }
}
