//! The package screen.

#[allow(unused_imports)]
use super::util::*;
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
fn a_typed_name_that_is_also_ticked_counts_once() {
    // `fish` is in the list and on by default; typing it again must not make
    // the summary claim one package more than will be installed.
    let mut a = on_packages();
    let chosen = a.chosen_packages().len();
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "fish");
    let names = a.chosen_packages();
    assert_eq!(names.iter().filter(|n| **n == "fish").count(), 1, "{names:?}");
    assert_eq!(names.len(), chosen);
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

// -- searching the registry ------------------------------------------------

/// A packages page with a known registry behind it, so the assertions do not
/// depend on what `min` happens to have cached on this machine.
fn with_registry(tag: &str) -> (App, PathBuf) {
    let root = temp_dir(&format!("registry-{tag}"));
    std::fs::create_dir_all(root.join("lc")).unwrap();
    std::fs::write(
        root.join("lc/index"),
        r#"{"builds": [
            [0, {"name": "ripgrep", "attrs": {
                  "license_spdx": {"String": ["Unlicense", null]},
                  "upstream_version": {"String": ["15.2.0", null]}}}],
            [0, {"name": "emacs", "attrs": {
                  "license_spdx": {"String": ["GPL-3.0-or-later", null]},
                  "upstream_version": {"String": ["30.1", null]}}}],
            [0, {"name": "tmux", "attrs": null}]
        ]}"#,
    )
    .unwrap();
    let mut a = on_packages();
    a.registry = crate::registry::Registry::load(&root);
    assert!(a.registry.is_available());
    (a, root)
}

#[test]
fn slash_searches_the_registry() {
    let (mut a, root) = with_registry("search");
    a.on_key(press(KeyCode::Char('/')));
    assert!(a.searching.is_some());
    typing(&mut a, "rip");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("ripgrep"), "{text}");
    assert!(text.contains("15.2.0"), "with its version:\n{text}");
    assert!(text.contains("Unlicense"), "and its licence:\n{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_search_is_fuzzy() {
    let (mut a, root) = with_registry("fuzzy");
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "rpgrp");
    assert_eq!(a.registry_hits()[0].name, "ripgrep");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn enter_adds_the_highlighted_package_to_the_list() {
    let (mut a, root) = with_registry("add");
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "emacs");
    a.on_key(press(KeyCode::Enter));
    assert!(a.searching.is_none(), "the search closes");
    assert!(a.extra_packages().contains(&"emacs"), "{:?}", a.extra);
    assert!(a.chosen_packages().contains(&"emacs"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn adding_a_second_package_keeps_the_first() {
    let (mut a, root) = with_registry("add-two");
    for name in ["emacs", "tmux"] {
        a.on_key(press(KeyCode::Char('/')));
        typing(&mut a, name);
        a.on_key(press(KeyCode::Enter));
    }
    let extras = a.extra_packages();
    assert!(
        extras.contains(&"emacs") && extras.contains(&"tmux"),
        "{extras:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn adding_the_same_package_twice_does_not_duplicate_it() {
    let (mut a, root) = with_registry("add-dup");
    for _ in 0..2 {
        a.on_key(press(KeyCode::Char('/')));
        typing(&mut a, "emacs");
        a.on_key(press(KeyCode::Enter));
    }
    assert_eq!(a.extra_packages(), vec!["emacs"], "{:?}", a.extra);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_name_the_registry_does_not_have_is_flagged_not_refused() {
    // Still the user's call — the index can be stale, and a name it has never
    // heard of may be real. But a typo is otherwise only discovered when the
    // session fails to build.
    let (mut a, root) = with_registry("unknown");
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "emacs ripgrepp");
    assert_eq!(a.unknown_extras(), vec!["ripgrepp"]);
    assert!(
        a.extra_packages().contains(&"ripgrepp"),
        "flagged, not dropped: {:?}",
        a.extra_packages()
    );
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("not in the registry"), "{text}");
    assert!(text.contains("ripgrepp"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn nothing_is_flagged_when_there_is_no_index_to_check_against() {
    // "Not in the registry" without a registry would be a claim rather than a
    // finding, and would warn about every name someone typed.
    let mut a = on_packages();
    a.registry = crate::registry::Registry::load(Path::new("/definitely/not/here"));
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "emacs obviousnonsense");
    assert!(a.unknown_extras().is_empty());
    let text = flatten(&render_app(&a, 110, 30));
    assert!(!text.contains("not in the registry"), "{text}");
}

#[test]
fn the_search_says_so_when_there_is_no_index() {
    let mut a = on_packages();
    a.registry = crate::registry::Registry::load(Path::new("/definitely/not/here"));
    a.on_key(press(KeyCode::Char('/')));
    let text = flatten(&render_app(&a, 110, 30));
    assert!(
        text.contains("resolves"),
        "it should say where an index comes from:\n{text}"
    );
}

#[test]
fn a_search_matching_nothing_says_so() {
    let (mut a, root) = with_registry("nomatch");
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "zzznope");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("Nothing in the registry matches"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn esc_leaves_the_search_without_adding_anything() {
    let (mut a, root) = with_registry("cancel");
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "emacs");
    a.on_key(press(KeyCode::Esc));
    assert!(a.searching.is_none());
    assert!(a.extra_packages().is_empty());
    assert_eq!(
        a.screen,
        Screen::Packages,
        "esc closed the search, not the page"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn q_is_a_letter_while_searching_the_registry() {
    let (mut a, root) = with_registry("q-key");
    a.on_key(press(KeyCode::Char('/')));
    a.on_key(press(KeyCode::Char('q')));
    assert!(!a.done, "q should be text here, not the quit key");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_package_already_on_the_list_is_marked_as_such() {
    let (mut a, root) = with_registry("already");
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "emacs");
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "emacs");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("already added"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "prints frames to look at rather than asserting"]
fn show_the_registry_search() {
    let (mut a, root) = with_registry("show");
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "m");
    println!("\n=== searching");
    for line in render_app(&a, 100, 22) {
        println!("|{}|", line.trim_end());
    }
    a.on_key(press(KeyCode::Esc));
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "emacs ripgrepp");
    println!("\n=== a name the registry does not have");
    for line in render_app(&a, 100, 22) {
        if line.contains('›') || line.contains("registry") || line.contains("adding") {
            println!("|{}|", line.trim_end());
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn no_test_reaches_the_network() {
    // Every App a test builds must leave the site fetch off. A real request per
    // fixture would be slow, flaky, and pointed at somebody's actual web
    // server — and the suite did exactly that for one commit.
    for (name, a) in [
        ("app", app()),
        ("on_packages", on_packages()),
        ("on_apply", on_apply()),
    ] {
        assert!(!a.fetch_registry, "{name} would reach minimal.dev");
        assert!(a.registry_fetch.is_none(), "{name} has a fetch in flight");
    }
}

#[test]
fn the_site_bundle_replaces_the_local_index_when_it_lands() {
    // The local index answers immediately; the site is current and carries
    // categories and advisories, so it wins when it arrives.
    let (mut a, root) = with_registry("swap");
    assert!(matches!(
        a.registry.source,
        Some(crate::registry::Source::LocalIndex(_))
    ));

    let (tx, rx) = std::sync::mpsc::channel();
    a.registry_fetch = Some(rx);
    tx.send(Ok(crate::registry::Registry::from_bundle(
        r#"<script type="application/json" id="pkgs-bundle">
            {"packages":[{"name":"openssl","categories":["library"],
                          "version":"3.6.3","activeAdvisoryCount":11}]}
            </script>"#,
    )
    .unwrap()))
        .unwrap();
    a.tick();

    assert_eq!(a.registry.source, Some(crate::registry::Source::Site));
    assert!(a.registry.knows("openssl"));
    assert!(a.registry_fetch.is_none(), "the fetch is done with");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn advisories_are_shown_in_the_search() {
    // The one thing worth knowing before installing something that the local
    // index cannot tell you.
    let mut a = on_packages();
    a.registry = crate::registry::Registry::from_bundle(
        r#"<script type="application/json" id="pkgs-bundle">
            {"packages":[{"name":"openssl","categories":["library"],
                          "version":"3.6.3","activeAdvisoryCount":11}]}
            </script>"#,
    )
    .unwrap();
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "openssl");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("11 advisories"), "{text}");
    assert!(text.contains("library"), "and its category:\n{text}");
    assert!(
        text.contains("minimal.dev"),
        "and where it came from:\n{text}"
    );
}

#[test]
fn a_failed_fetch_leaves_the_local_index_alone() {
    // There is a working registry either way; a network failure is not worth
    // stopping for, or even mentioning when something else answered.
    let (mut a, root) = with_registry("fetch-fails");
    let (tx, rx) = std::sync::mpsc::channel();
    a.registry_fetch = Some(rx);
    tx.send(Err("could not resolve host".to_string())).unwrap();
    a.tick();

    assert!(a.registry.knows("ripgrep"), "the local index still answers");
    assert!(a.registry_note.is_none(), "and nothing is complained about");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_fetch_with_no_local_index_says_why() {
    let mut a = on_packages();
    a.registry = crate::registry::Registry::default();
    let (tx, rx) = std::sync::mpsc::channel();
    a.registry_fetch = Some(rx);
    tx.send(Err("could not resolve host".to_string())).unwrap();
    a.tick();
    assert_eq!(a.registry_note.as_deref(), Some("could not resolve host"));

    a.on_key(press(KeyCode::Char('/')));
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("could not resolve host"), "{text}");
}

#[test]
fn an_optional_package_already_ticked_shows_as_added() {
    // Reported: `bottom` showed as added and `atuin` did not. `bottom` is in
    // the `cozy` group, so it is in `always`; `atuin` is *optional* and ticked
    // on by default — and the check never looked at the list on this very page.
    let mut a = on_packages();
    assert!(
        a.packages.iter().any(|p| p.name == "atuin"),
        "atuin is one of the optional packages"
    );
    assert!(a.chosen_packages().contains(&"atuin"), "and on by default");

    a.registry = crate::registry::Registry::from_bundle(
        r#"<script type="application/json" id="pkgs-bundle">
            {"packages":[{"name":"atuin","version":"18.9.0"},
                         {"name":"bottom","version":"0.10.2"}]}
            </script>"#,
    )
    .unwrap();

    for name in ["atuin", "bottom"] {
        a.searching = Some(name.to_string());
        a.registry_row = 0;
        let text = flatten(&render_app(&a, 110, 30));
        assert!(
            text.contains("already added") || text.contains("installed anyway"),
            "{name} is going in and the search does not say so:\n{text}"
        );
    }
}

#[test]
fn the_search_distinguishes_all_four_states() {
    let mut a = on_packages();
    a.registry = crate::registry::Registry::from_bundle(
        r#"<script type="application/json" id="pkgs-bundle">
            {"packages":[{"name":"bottom"},{"name":"atuin"},
                         {"name":"glow"},{"name":"cowsay"}]}
            </script>"#,
    )
    .unwrap();

    // Turn one of the optional packages off so the fourth state is reachable.
    while a.packages[a.package_row].name != "glow" {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Char(' ')));
    assert!(!a.chosen_packages().contains(&"glow"));

    for (name, want) in [
        ("bottom", "installed anyway"),
        ("atuin", "already added"),
        ("glow", "turned off above"),
        ("cowsay", ""),
    ] {
        assert_eq!(a.package_state(name).note(), want, "{name}");
    }

    // And a package turned off says so on screen rather than nothing, which
    // would read as "not going in" when the row above says otherwise.
    a.searching = Some("glow".to_string());
    a.registry_row = 0;
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("turned off above"), "{text}");
}

#[test]
fn a_package_typed_by_hand_shows_as_added_too() {
    let mut a = on_packages();
    a.registry = crate::registry::Registry::from_bundle(
        r#"<script type="application/json" id="pkgs-bundle">
            {"packages":[{"name":"cowsay"}]}
            </script>"#,
    )
    .unwrap();
    assert_eq!(a.package_state("cowsay"), crate::PackageState::New);

    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "cowsay");
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.package_state("cowsay").note(), "already added");
}

#[test]
fn adding_from_the_search_immediately_reads_back_as_added() {
    // The loop the report was really about: add it, search again, see it.
    let mut a = on_packages();
    a.registry = crate::registry::Registry::from_bundle(
        r#"<script type="application/json" id="pkgs-bundle">
            {"packages":[{"name":"cowsay"}]}
            </script>"#,
    )
    .unwrap();
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "cowsay");
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('/')));
    typing(&mut a, "cowsay");
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("already added"), "{text}");
}

#[test]
#[ignore = "prints a frame to look at rather than asserting"]
fn show_the_states() {
    let mut a = on_packages();
    a.registry = crate::registry::Registry::from_bundle(
        r#"<script type="application/json" id="pkgs-bundle">
            {"packages":[{"name":"bottom","version":"0.10.2","categories":["tool"]},
                         {"name":"atuin","version":"18.9.0","categories":["tool"]},
                         {"name":"glow","version":"3.0.0","categories":["tool"]},
                         {"name":"openssl","version":"3.6.3","categories":["library"],
                          "activeAdvisoryCount":11}]}
            </script>"#,
    )
    .unwrap();
    while a.packages[a.package_row].name != "glow" {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Char(' ')));
    a.searching = Some(String::new());
    for line in render_app(&a, 100, 20) {
        println!("|{}|", line.trim_end());
    }
}
