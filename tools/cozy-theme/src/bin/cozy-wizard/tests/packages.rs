//! The package screen.

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
fn packages_start_from_their_declared_defaults() {
    let a = on_packages();
    let p = cozy_theme::Packages::load(Path::new("../../templates/packages.toml")).unwrap();
    let want: Vec<bool> = p.optional.iter().map(|o| o.default).collect();
    assert_eq!(
        a.wanted, want,
        "the page must open on packages.toml's defaults"
    );
    assert_eq!(
        a.chosen_packages().len(),
        want.iter().filter(|w| **w).count()
    );
}

#[test]
fn space_toggles_only_the_row_under_the_cursor() {
    let mut a = on_packages();
    let before = a.chosen_packages().len();
    let name = a.current_package().unwrap().name.clone();
    a.on_key(press(KeyCode::Char(' ')));
    assert_eq!(
        a.chosen_packages().len(),
        before - 1,
        "should have dropped one"
    );
    assert!(
        !a.chosen_packages().contains(&name.as_str()),
        "{name} should be off"
    );
    a.on_key(press(KeyCode::Char(' ')));
    assert_eq!(
        a.chosen_packages().len(),
        before,
        "toggling back restores it"
    );
}

#[test]
fn bulk_toggles_cover_the_whole_list() {
    let mut a = on_packages();
    a.on_key(press(KeyCode::Char('n')));
    assert!(a.chosen_packages().is_empty(), "n should clear everything");
    a.on_key(press(KeyCode::Char('a')));
    assert_eq!(
        a.chosen_packages().len(),
        a.packages.len(),
        "a should select everything"
    );
}

#[test]
fn the_detail_panel_shows_description_and_licence() {
    let mut a = on_packages();
    // Walk the list; every row must describe itself and name a licence.
    for _ in 0..a.packages.len() {
        let pkg = a.current_package().unwrap();
        let (name, about, licence) = (pkg.name.clone(), pkg.about.clone(), pkg.license.clone());
        let text = flatten(&render_app(&a, 100, 30));
        assert!(text.contains(&name), "{name} not shown");
        assert!(
            text.contains(&licence),
            "{name}: licence {licence} not shown"
        );
        let head: String = about.split(['.', '(']).next().unwrap().into();
        assert!(text.contains(head.trim()), "{name}: description not shown");
        a.on_key(press(KeyCode::Down));
    }
}

#[test]
fn the_proprietary_licence_is_called_out() {
    // Every other package is permissive; this is the one a reader must not
    // skim past, so it must not render in the same grey as the rest.
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut a = on_packages();
    while a.current_package().unwrap().name != "claude-code" {
        a.on_key(press(KeyCode::Down));
    }
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| ui::draw(f, &a)).unwrap();
    let buf = term.backend().buffer();
    let row = (0..30)
        .find(|y| {
            (0..100)
                .map(|x| buf[(x, *y)].symbol())
                .collect::<String>()
                .contains("LicenseRef")
        })
        .expect("licence line should be drawn");
    let x = (0..100)
        .find(|x| buf[(*x, row)].symbol() == "L")
        .expect("licence text should start somewhere");
    let cell = &buf[(x, row)];
    let dim = a.theme().comment;
    assert_ne!(
        cell.fg, dim,
        "a proprietary licence must not render as ordinary grey"
    );
    assert!(
        cell.modifier.contains(Modifier::BOLD),
        "and should be emphasised"
    );
}

#[test]
fn the_chosen_theme_persists_onto_the_package_page() {
    // The scheme picked on the previous page is the wizard's colours from
    // then on, not a preview that ends when the page does.
    let mut a = on_themes();
    for _ in 0..7 {
        a.on_key(press(KeyCode::Down));
    }
    let want = a.loaded.as_ref().unwrap().palette["base00"];
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Packages);
    assert_eq!(
        frame_bg(&a, 100, 30),
        Color::Rgb(want.r, want.g, want.b),
        "the package page should still be wearing {}",
        a.schemes[a.theme_row].name
    );
}

#[test]
fn typing_q_does_not_quit_the_wizard() {
    // The hazard this whole focus mode exists for: `q` quits everywhere
    // else, and a package field where typing `qt5` exits would be absurd.
    let mut a = on_packages();
    a.on_key(press(KeyCode::Char('i')));
    assert_eq!(a.focus, Focus::Input);
    typing(&mut a, "qt5");
    assert!(!a.done, "q must be a letter while typing");
    assert_eq!(a.extra, "qt5");
    // Ctrl-C still works, because there has to be a way out from anywhere.
    a.on_key(KeyEvent::new_with_kind(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    ));
    assert!(a.done, "ctrl-c must still quit from the text field");
}

#[test]
fn focus_moves_into_the_field_and_back() {
    let mut a = on_packages();
    assert_eq!(a.focus, Focus::List);
    a.on_key(press(KeyCode::Tab));
    assert_eq!(a.focus, Focus::Input);
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.focus, Focus::List, "esc leaves the field");
    assert_eq!(a.screen, Screen::Packages, "and does not leave the page");
    a.on_key(press(KeyCode::Char('i')));
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.focus, Focus::List, "enter leaves the field");
    assert!(!a.done, "enter in the field must not finish the wizard");
}

#[test]
fn list_keys_do_not_leak_into_the_field() {
    // `space` toggles in the list and is a space in the field; `j` moves in
    // the list and is a letter in the field.
    let mut a = on_packages();
    let chosen = a.chosen_packages().len();
    let row = a.package_row;
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "a b");
    a.on_key(press(KeyCode::Char('j')));
    assert_eq!(a.extra, "a bj");
    assert_eq!(a.package_row, row, "the list must not have moved");
    assert_eq!(
        a.chosen_packages().len() - a.extra_packages().len(),
        chosen,
        "the checklist must not have been toggled"
    );
}

#[test]
fn typed_names_are_installed_and_junk_is_dropped() {
    let mut a = on_packages();
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "emacs, tmux  RUBBISH!! neovim");
    assert_eq!(
        a.extra_packages(),
        vec!["emacs", "neovim", "tmux"],
        "should keep the plausible names and drop the rest"
    );
    let chosen = a.chosen_packages();
    for name in ["emacs", "neovim", "tmux"] {
        assert!(chosen.contains(&name), "{name} should be installed");
    }
    assert!(
        !chosen.contains(&"RUBBISH!!"),
        "junk must not reach the package list"
    );
}

#[test]
fn backspace_edits_the_field() {
    let mut a = on_packages();
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "emacsx");
    a.on_key(press(KeyCode::Backspace));
    assert_eq!(a.extra, "emacs");
    assert_eq!(a.extra_packages(), vec!["emacs"]);
}

#[test]
fn a_name_the_loadout_already_has_is_flagged() {
    // Typing `fish` should not silently look like it did something.
    let mut a = on_packages();
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "fish emacs");
    assert_eq!(a.redundant_extras(), vec!["fish"]);
    let text = flatten(&render_app(&a, 100, 30));
    assert!(text.contains("already installed: fish"), "{text}");
    assert!(
        text.contains("adding emacs fish"),
        "and should still echo what it parsed"
    );
}

#[test]
fn the_field_says_when_it_parsed_nothing() {
    let mut a = on_packages();
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "!!!");
    let text = flatten(&render_app(&a, 100, 30));
    assert!(
        text.contains("Nothing usable yet"),
        "silently ignoring what someone typed is the worst version of this:\n{text}"
    );
}

#[test]
fn esc_steps_back_to_the_themes() {
    let mut a = on_packages();
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Themes);
    assert!(!a.done);
}
