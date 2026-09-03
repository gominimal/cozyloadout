//! The summary screen and what its four actions do.

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
fn the_summary_names_what_was_chosen() {
    let (mut a, root) = on_patches("summary");
    a.on_key(press(KeyCode::Tab));
    a.on_key(press(KeyCode::Char(' ')));
    let text = flatten(&render_app(&a, 110, 30));
    assert!(text.contains("1 chosen"), "{text}");
    assert!(
        text.contains("dotfiles"),
        "the summary should name the path:\n{text}"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn both_pages_step_back_the_way_they_came() {
    let mut a = on_resources();
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Client);
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Patches);
}

#[test]
fn the_summary_names_both_host_settings_and_says_when_they_are_untouched() {
    let a = on_apply();
    let text = flatten(&render_app(&a, 120, 40));
    assert!(text.contains("detach"), "{text}");
    assert!(text.contains("ctrl-] then d"), "{text}");
    assert!(text.contains("vm"), "{text}");
    // Untouched, so the summary must promise not to touch anything.
    assert!(
        text.matches("default, unchanged").count() >= 2,
        "both rows should say they change nothing:\n{text}"
    );
}

#[test]
fn the_summary_says_when_a_host_setting_will_be_written() {
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    clear_input(&mut a);
    typing(&mut a, "ctrl-a");
    a.on_key(press(KeyCode::Enter)); // commit the chord
    a.on_key(press(KeyCode::Enter)); // client -> resources
    a.on_key(press(KeyCode::Enter)); // resources -> apply
    assert_eq!(a.screen, Screen::Apply);

    let text = flatten(&render_app(&a, 120, 40));
    assert!(text.contains("ctrl-a then d"), "{text}");
    assert!(
        text.contains("writes minimal's config"),
        "the summary has to say this one leaves the repo:\n{text}"
    );
}

#[test]
fn the_repo_root_is_never_an_empty_path() {
    // `Path::new("schemes/vendor")` climbs to "schemes" and then to "",
    // and an empty path is not the current directory — it is nothing.
    // Passing it to `Command::current_dir` fails, which is exactly how
    // generating from a default `--schemes` broke.
    for dir in ["schemes/vendor", "./schemes/vendor", "/abs/schemes/vendor"] {
        let a = app_with(PathBuf::from(dir), State::default());
        let root = a.repo_root();
        assert!(
            !root.as_os_str().is_empty(),
            "{dir} produced an empty repo root"
        );
        assert!(
            a.templates_dir().starts_with(&root),
            "templates should sit under the repo root"
        );
    }
}

#[test]
fn the_summary_reports_every_choice_but_the_fetch() {
    let mut a = on_apply();
    let text = flatten(&render_app(&a, 100, 30));
    for field in ["greeting", "theme", "packages", "patches"] {
        assert!(
            text.contains(field),
            "{field} missing from the summary:\n{text}"
        );
    }
    assert!(
        text.contains(&a.schemes[a.theme_row].name),
        "the scheme should be named"
    );
    // The scheme-collection question was about the disk a moment ago, not
    // a choice worth reviewing.
    for word in ["clone", "Download", "Update the upstream"] {
        assert!(
            !text.contains(word),
            "the summary should not mention {word}"
        );
    }
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Resources, "esc should still step back");
}

#[test]
fn abort_is_the_only_action_that_discards_the_answers() {
    for (steps, action, saves) in [
        (0, Action::Generate, true),
        (1, Action::SaveAs, true),
        (2, Action::SaveOnly, true),
        (3, Action::Abort, false),
    ] {
        let mut a = on_apply();
        for _ in 0..steps {
            a.on_key(press(KeyCode::Down));
        }
        assert_eq!(a.action(), action);
        assert_eq!(action.saves(), saves, "{action:?}");
    }
    // Abort really does neither.
    let mut a = on_apply();
    for _ in 0..3 {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Enter));
    assert!(a.done);
    assert!(!a.completed, "abort must not write the state file");

    // Save-only finishes without building.
    let mut b = on_apply();
    for _ in 0..2 {
        b.on_key(press(KeyCode::Down));
    }
    b.on_key(press(KeyCode::Enter));
    assert!(
        b.done && b.completed,
        "save-only should still be remembered"
    );
    assert!(
        matches!(b.applied, Applied::Idle),
        "and must not have built anything"
    );
}

#[test]
fn any_key_exits_once_the_action_has_run() {
    // Naming two specific keys made people hunt for them, and there is
    // nothing else to do on this screen afterwards.
    for key in [
        KeyCode::Char('x'),
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Down,
        KeyCode::Tab,
    ] {
        let mut a = on_apply();
        a.applied = Applied::Ok("rendered".into());
        a.on_key(press(key));
        assert!(a.done, "{key:?} should have exited");
    }
    // A failure ends the same way rather than sitting there re-running.
    for key in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Esc] {
        let mut a = on_apply();
        a.applied = Applied::Failed("git exploded".into());
        a.on_key(press(key));
        assert!(a.done, "{key:?} should have exited after a failure");
    }
}

#[test]
fn the_settled_screen_says_how_to_leave() {
    for applied in [
        Applied::Ok("rendered something".into()),
        Applied::Failed("something broke".into()),
    ] {
        let mut a = on_apply();
        a.applied = applied;
        let text = flatten(&render_app(&a, 110, 30));
        assert!(text.contains("press any key to exit"), "{text}");
    }
}

#[test]
fn installing_is_ticked_by_default_and_space_toggles_it() {
    // It is the point of running the wizard, so an untouched run should
    // produce a usable session rather than a build/ directory.
    let mut a = on_apply();
    assert!(a.install);
    let text = flatten(&render_app(&a, 100, 30));
    assert!(text.contains("[x] install into"), "{text}");
    assert!(
        text.contains("Generate and install"),
        "the action should say what it will do:\n{text}"
    );

    a.on_key(press(KeyCode::Char(' ')));
    assert!(!a.install);
    let text = flatten(&render_app(&a, 100, 30));
    assert!(text.contains("[ ] install into"), "{text}");
    assert!(
        text.contains("Generate ") && !text.contains("Generate and install"),
        "and so should the label:\n{text}"
    );
}

#[test]
fn the_tick_toggles_from_anywhere_on_the_page() {
    // It is not a row, so which action is highlighted must not matter.
    let mut a = on_apply();
    for _ in 0..3 {
        a.on_key(press(KeyCode::Down));
    }
    assert_eq!(a.action(), Action::Abort);
    a.on_key(press(KeyCode::Char(' ')));
    assert!(!a.install, "space should still reach the tick");
    assert_eq!(a.action(), Action::Abort, "and not move the cursor");
}

#[test]
fn generate_is_the_first_thing_offered() {
    let a = on_apply();
    assert_eq!(a.action(), Action::Generate);
}

#[test]
fn the_cursor_clamps_at_both_ends_of_the_action_list() {
    let mut a = on_apply();
    a.on_key(press(KeyCode::Up));
    assert_eq!(
        a.action(),
        Action::Generate,
        "the cursor starts on generate and up should stay there"
    );
    for _ in 0..10 {
        a.on_key(press(KeyCode::Down));
    }
    assert_eq!(a.action(), Action::Abort, "down should stop at the end");
}

#[test]
fn generate_actually_renders_what_was_chosen() {
    // The whole point of the page. Runs the real renderer against a
    // throwaway output directory and reads what came out.
    let out = temp_dir("apply-generate");
    std::fs::create_dir_all(&out).unwrap();

    let mut a = on_apply();
    // A repo whose `build/` is the temp dir: the renderer is told where to
    // write, so this does not disturb the checkout.
    a.repo = PathBuf::from("../..");
    a.greeting = Some(Greeting::Legacy);
    // Turn everything optional off, so the effect is visible in the output.
    a.wanted.iter_mut().for_each(|w| *w = false);

    let scheme = a.schemes[a.theme_row].name.clone();
    let report = run_generate_to(&a, Path::new("../.."), &out, false)
        .unwrap_or_else(|e| panic!("generate failed: {e}"));
    assert!(
        report.contains(&scheme),
        "the renderer should name the scheme: {report}"
    );

    let manifest = std::fs::read_to_string(out.join("cozy.toml")).unwrap();
    assert!(
        manifest.contains("\"fish\""),
        "required packages must survive"
    );
    assert!(
        !manifest.contains("\"tealdeer\""),
        "declined packages must be gone"
    );
    assert!(
        !out.join("cozy/atuin").exists(),
        "a declined package's config must not be rendered"
    );
    let fish = std::fs::read_to_string(out.join("cozy/fish/config.fish")).unwrap();
    assert!(
        fish.contains("🭕"),
        "the legacy greeting should have been chosen"
    );
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
#[ignore = "prints frames for eyeballing; run with --ignored --nocapture"]
fn dump_frames() {
    for (w, h) in [(50u16, 18u16), (60, 20), (80, 24)] {
        println!("\n=== greeting {w}x{h} ===");
        for row in render(w, h) {
            println!("|{}|", row.trim_end());
        }
    }
    let a = on_schemes();
    for (w, h) in [(60u16, 20u16), (80, 24)] {
        println!("\n=== schemes {w}x{h} ===");
        for row in render_app(&a, w, h) {
            println!("|{}|", row.trim_end());
        }
    }
    let done = app_after_fetch("dump");
    println!("\n=== schemes after a successful fetch 80x24 ===");
    for row in render_app(&done, 80, 24) {
        println!("|{}|", row.trim_end());
    }
    let _ = std::fs::remove_dir_all(&done.schemes_dir);
    let mut a = on_themes();
    for (w, h) in [(100u16, 30u16), (80, 24), (60, 20)] {
        println!("\n=== themes {w}x{h} ({} schemes) ===", a.schemes.len());
        for row in render_app(&a, w, h) {
            println!("|{}|", row.trim_end());
        }
    }
    for _ in 0..5 {
        a.on_key(press(KeyCode::Down));
    }
    println!(
        "\n=== themes after 5 down: {} ===",
        a.schemes[a.theme_row].name
    );

    let mut p = on_packages();
    for _ in 0..7 {
        p.on_key(press(KeyCode::Down));
    }
    p.on_key(press(KeyCode::Char(' ')));
    p.on_key(press(KeyCode::Char('i')));
    for c in "emacs tmux fzf".chars() {
        p.on_key(press(KeyCode::Char(c)));
    }
    for (w, h) in [(80u16, 24u16), (60, 20)] {
        println!("\n=== packages {w}x{h} ===");
        for row in render_app(&p, w, h) {
            println!("|{}|", row.trim_end());
        }
    }

    let mut r = on_packages();
    crate::ui::patches::enter_patches(&mut r);
    crate::ui::apply::enter_apply(&mut r);
    println!("\n=== apply 90x28 ===");
    for row in render_app(&r, 90, 28) {
        println!("|{}|", row.trim_end());
    }

    let mut q = on_packages();
    crate::ui::patches::enter_patches(&mut q);
    q.pickers[0].move_cursor(2, 10);
    q.pickers[0].toggle();
    for (w, h) in [(100u16, 30u16), (70, 22)] {
        println!("\n=== patches {w}x{h} ===");
        for row in render_app(&q, w, h) {
            println!("|{}|", row.trim_end());
        }
    }
}

// -- saving the settings to a file of their own ----------------------------

#[test]
fn save_as_opens_a_prompt_rather_than_writing_blind() {
    let mut a = on_apply();
    a.on_key(press(KeyCode::Down)); // "Save these settings to a file"
    assert_eq!(a.action(), Action::SaveAs);
    a.on_key(press(KeyCode::Enter));
    assert!(a.saving_settings.is_some());
    assert!(!a.done, "the prompt should not end the run");

    let text = flatten(&render_app(&a, 110, 34));
    assert!(text.contains("Save these settings to"), "{text}");
    assert!(
        text.contains("my-loadout.toml"),
        "a suggestion, in the user's own directory rather than the checkout:\n{text}"
    );
    assert!(text.contains(".config/cozy"), "{text}");
    assert!(
        text.contains("cozy-theme --settings"),
        "and what the file is for:\n{text}"
    );
}

#[test]
fn the_saved_file_rebuilds_the_same_loadout() {
    // The claim the prompt makes, checked rather than asserted at the reader.
    let out = temp_dir("settings-roundtrip");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join("mine.toml");

    let mut a = on_apply();
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Enter));
    clear_input(&mut a);
    typing(&mut a, path.to_str().unwrap());
    a.on_key(press(KeyCode::Enter));
    assert!(a.saving_settings.is_none(), "a good path should close it");
    assert!(path.exists(), "expected {}", path.display());

    let back = cozy_theme::Settings::load(&path);
    assert_eq!(back, a.to_state(), "the file must hold every answer");
    assert!(a.completed, "writing it counts as finishing the run");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn saving_settings_refuses_to_overwrite_and_keeps_the_prompt_open() {
    let out = temp_dir("settings-clobber");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join("taken.toml");
    std::fs::write(&path, "theme = \"something\"\n").unwrap();

    let mut a = on_apply();
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Enter));
    clear_input(&mut a);
    typing(&mut a, path.to_str().unwrap());
    a.on_key(press(KeyCode::Enter));

    assert!(a.saving_settings.is_some(), "the prompt stays open");
    let text = flatten(&render_app(&a, 110, 34));
    assert!(text.contains("already exists"), "{text}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "theme = \"something\"\n",
        "and the file is untouched"
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn esc_cancels_the_settings_prompt() {
    let mut a = on_apply();
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Esc));
    assert!(a.saving_settings.is_none());
    assert!(!a.done, "cancelling returns to the actions");
    assert_eq!(a.action(), Action::SaveAs);
}

#[test]
fn q_is_a_letter_while_typing_a_settings_path() {
    let mut a = on_apply();
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('q')));
    assert!(!a.done, "q should be text here, not the quit key");
    assert!(a.saving_settings.as_deref().unwrap_or("").ends_with('q'));
}

#[test]
fn a_named_settings_file_is_what_the_run_starts_from() {
    // The other half of "save as": handing it back to a later run.
    let dir = temp_dir("settings-start");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("start.toml");
    let saved = cozy_theme::Settings {
        greeting: Some("legacy".into()),
        extra: "emacs".into(),
        contrast: Some(35),
        ..cozy_theme::Settings::default()
    };
    saved.save(&path).unwrap();

    let mut back = app_with(
        PathBuf::from("../../schemes/vendor"),
        cozy_theme::Settings::load(&path),
    );
    // Applied at construction, because neither depends on anything discovered
    // later.
    assert_eq!(back.adjust.contrast, 35);
    assert_eq!(back.current_greeting(), Greeting::Legacy);
    // The package field waits for its own page, which is where the list it
    // belongs to gets loaded.
    let templates = back.templates_dir();
    crate::ui::packages::enter_packages(&mut back, &templates);
    assert_eq!(back.extra, "emacs");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "prints frames to look at rather than asserting"]
fn show_the_apply_screen() {
    let mut a = on_apply();
    println!("\n=== install ticked");
    for line in render_app(&a, 96, 30) {
        println!("|{}|", line.trim_end());
    }
    a.on_key(press(KeyCode::Char(' ')));
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Enter));
    println!("\n=== unticked, save-settings prompt");
    for line in render_app(&a, 96, 30) {
        println!("|{}|", line.trim_end());
    }
}

#[test]
fn the_render_summary_lands_on_the_frame_not_on_stdout() {
    // `build` used to `println!` its summary, which the wizard calls from
    // inside the alternate screen — so the line painted over the frame and
    // then vanished with it. It is returned now, and this is where it goes.
    let out = temp_dir("report-frame");
    std::fs::create_dir_all(&out).unwrap();
    let mut a = on_apply();
    a.applied = Applied::Ok(run_generate_to(&a, Path::new("../.."), &out, false).unwrap());

    let text = flatten(&render_app(&a, 110, 34));
    assert!(
        text.contains("files]"),
        "the summary should be visible on the last frame:\n{text}"
    );
    assert!(text.contains("0x96f"), "naming what was rendered:\n{text}");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn an_old_settings_file_is_read_once_and_then_migrated() {
    // The move must not lose anyone's answers, and must not leave the file
    // where it was either — reading and writing the old path would mean it
    // never moved.
    let dir = temp_dir("migrate");
    std::fs::create_dir_all(&dir).unwrap();
    let legacy = dir.join(cozy_theme::settings::FILE);
    let modern = dir.join("config/cozy/settings.toml");

    // Only the old one exists: read it, write the new one.
    std::fs::write(&legacy, "theme = \"from-the-old-file\"\n").unwrap();
    let (read, write) = settings_paths_in(&modern, &legacy);
    assert_eq!(read, legacy, "the old answers still come back");
    assert_eq!(write, modern, "but the run writes the new location");

    // Once the new one exists, the old one is out of the picture.
    std::fs::create_dir_all(modern.parent().unwrap()).unwrap();
    std::fs::write(&modern, "theme = \"moved\"\n").unwrap();
    let (read, write) = settings_paths_in(&modern, &legacy);
    assert_eq!(read, modern);
    assert_eq!(write, modern);
    std::fs::remove_dir_all(&dir).unwrap();
}
