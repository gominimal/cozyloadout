//! Going back, and coming forward again.

#[allow(unused_imports)]
use super::util::*;
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
    let mut a = app_with(PathBuf::from("../../schemes/vendor"), saved);
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
    let mut a = app_with(PathBuf::from("../../schemes/vendor"), State::default());
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

/// The scheme has to reach every screen, not only the ones after it is picked.
///
/// Reported as "the current theme doesn't appear to apply to all pages". Two
/// separate causes: the chrome — the frame and the key hints, drawn in
/// `ui::draw` — was never styled at all, and the greeting and scheme-source
/// pages painted with raw `Color` constants rather than the loaded scheme.
/// Going back to either of those from the theme browser left them looking like
/// a different program.
#[test]
fn every_screen_paints_in_the_loaded_scheme() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let base = on_themes();
    let want = base.theme().selection;
    let base_bg = base.theme().bg;
    assert!(
        matches!(base_bg, Color::Rgb(..)),
        "fixture should have a real background, got {base_bg:?}"
    );
    assert!(
        !matches!(want, Color::DarkGray | Color::Reset),
        "fixture should have a real scheme loaded, got {want:?}"
    );

    // Every screen the wizard has, reached the way a user reaches it. The
    // greeting and schemes pages are entered by walking *back* from the theme
    // browser, which is the case that was broken.
    let mut back = on_themes();
    back.on_key(press(KeyCode::Esc));
    let mut back_more = on_themes();
    back_more.on_key(press(KeyCode::Esc));
    back_more.on_key(press(KeyCode::Esc));
    let (patches, _root) = on_patches("all-screens-themed");

    // The overlays draw as much as the pages under them and are easy to miss
    // when a palette changes, so they are walked too.
    let mut searching = on_themes();
    searching.on_key(press(KeyCode::Char('/')));
    let mut adjusting = on_themes();
    adjusting.on_key(press(KeyCode::Char('a')));
    let mut pkg_search = on_packages();
    pkg_search.on_key(press(KeyCode::Char('/')));

    // A `Vec`, not an array: `App` is large enough that eleven of them on the
    // stack trips clippy's local-array size limit.
    let screens = vec![
        ("greeting", back_more),
        ("schemes", back),
        ("themes", base),
        ("packages", on_packages()),
        ("patches", patches),
        ("client", on_client()),
        ("resources", on_resources()),
        ("apply", on_apply()),
        ("theme search", searching),
        ("theme adjust", adjusting),
        ("package search", pkg_search),
    ];

    for (name, app) in screens {
        let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
        let buf = terminal.backend().buffer();
        // The top-left corner of the outer frame: the one cell every screen
        // has in common.
        assert_eq!(
            buf[(0, 0)].fg,
            want,
            "the frame on the {name} screen is not in the scheme"
        );

        // The background, on the two rows that used to miss it. Four screens
        // painted their own onto the area *inside* the border, which left the
        // border row and the footer showing the terminal through; the other
        // four painted none at all.
        let bg = base_bg;
        assert_eq!(
            buf[(0, 0)].bg,
            bg,
            "the border row on the {name} screen has no background"
        );
        assert_eq!(
            buf[(0, 29)].bg,
            bg,
            "the footer on the {name} screen has no background"
        );

        // And every other cell too. A scheme's colours are `Color::Rgb`, so
        // any *named* colour left in the buffer is a hardcoded constant that
        // will not move when the scheme does. `Reset` is allowed: the greeting
        // mark is drawn in the terminal's own foreground on purpose, because
        // the user is judging their font rather than the palette.
        for y in 0..30 {
            for x in 0..90 {
                let cell = &buf[(x, y)];
                for (what, colour) in [("fg", cell.fg), ("bg", cell.bg)] {
                    assert!(
                        matches!(colour, Color::Rgb(..) | Color::Reset),
                        "{name} screen, cell ({x},{y}) {what} is {colour:?}, \
                         a hardcoded colour rather than the scheme's"
                    );
                }
            }
        }
    }
}
