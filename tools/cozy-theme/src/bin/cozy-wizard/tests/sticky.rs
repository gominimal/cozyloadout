//! What survives to the next run, and what deliberately does not.

#[allow(unused_imports)]
use super::util::*;
#[allow(clippy::wildcard_imports)]
use crate::*;
#[allow(unused_imports)]
use ratatui::style::{Color, Modifier};
#[allow(unused_imports)]
use std::process::Command;

#[test]
fn a_finished_run_records_every_page() {
    let mut a = completed_run(true, 4);
    a.on_key(press(KeyCode::Char(' '))); // drop the first optional package
    let dropped = a.packages[0].name.clone();
    a.on_key(press(KeyCode::Char('i')));
    typing(&mut a, "emacs");
    a.on_key(press(KeyCode::Esc));
    let theme = a.schemes[a.theme_row].name.clone();
    a.on_key(press(KeyCode::Enter)); // packages -> patches
    a.on_key(press(KeyCode::Enter)); // patches -> client
    a.on_key(press(KeyCode::Enter)); // client -> resources
    a.on_key(press(KeyCode::Enter)); // resources -> apply
    assert_eq!(a.screen, Screen::Apply);
    for _ in 0..2 {
        a.on_key(press(KeyCode::Down)); // "save settings and exit"
    }
    a.on_key(press(KeyCode::Enter));

    assert!(a.completed, "choosing anything but abort completes the run");
    let st = a.to_state();
    assert_eq!(st.greeting.as_deref(), Some(Greeting::ALL[1].key()));
    assert_eq!(st.theme.as_deref(), Some(theme.as_str()));
    assert_eq!(st.extra, "emacs");
    assert_eq!(
        st.packages.get(&dropped),
        Some(&false),
        "the dropped package is recorded off"
    );
    assert!(st.packages.len() > 1, "and the rest are recorded too");
}

#[test]
fn quitting_early_does_not_count_as_an_answer() {
    // The rule that keeps a half-answered wizard from overwriting a
    // finished one. `main` only writes when `completed` is set.
    let mut a = completed_run(false, 2);
    a.on_key(press(KeyCode::Char('q')));
    assert!(a.done, "q should end the run");
    assert!(!a.completed, "but quitting is not completing");

    let mut b = completed_run(false, 2);
    b.on_key(press(KeyCode::Enter)); // -> patches
    b.on_key(press(KeyCode::Esc)); // back to packages
    b.on_key(press(KeyCode::Char('q')));
    assert!(
        !b.completed,
        "backing out and quitting is still not completing"
    );
}

#[test]
fn a_saved_run_comes_back_selected() {
    let first = {
        let mut a = completed_run(true, 6);
        a.on_key(press(KeyCode::Char(' ')));
        a.on_key(press(KeyCode::Char('i')));
        typing(&mut a, "emacs tmux");
        a.on_key(press(KeyCode::Esc));
        a.to_state()
    };
    let theme = first.theme.clone().unwrap();
    let off: Vec<String> = first
        .packages
        .iter()
        .filter(|(_, v)| !**v)
        .map(|(k, _)| k.clone())
        .collect();

    // A second run, opened with what the first one saved.
    let mut b = app_with(PathBuf::from("../../schemes/vendor"), first);
    assert_eq!(
        b.current_greeting(),
        Greeting::ALL[1],
        "greeting should be restored"
    );
    b.on_key(press(KeyCode::Enter));
    b.on_key(press(KeyCode::Char('n')));
    b.on_key(press(KeyCode::Enter));
    assert_eq!(
        b.schemes[b.theme_row].name, theme,
        "theme should be restored"
    );
    b.on_key(press(KeyCode::Enter));
    assert_eq!(
        b.extra, "emacs tmux",
        "the typed packages should come back editable"
    );
    for name in &off {
        assert!(
            !b.chosen_packages().contains(&name.as_str()),
            "{name} should still be off"
        );
    }
}

#[test]
fn a_theme_that_no_longer_exists_falls_back_to_the_default() {
    let saved = State {
        theme: Some("a-scheme-nobody-has".into()),
        ..State::default()
    };
    let mut a = app_with(PathBuf::from("../../schemes/vendor"), saved);
    a.on_key(press(KeyCode::Enter));
    a.on_key(press(KeyCode::Char('n')));
    a.on_key(press(KeyCode::Enter));
    assert_eq!(
        a.theme_row, 0,
        "a deleted scheme should leave the cursor at the default"
    );
    assert!(
        a.loaded.is_some(),
        "and something must still be loaded to paint with"
    );
}

#[test]
fn a_deleted_path_does_not_come_back() {
    let dir = temp_dir("restore-paths");
    std::fs::create_dir_all(dir.join("kept")).unwrap();
    std::fs::write(dir.join("kept.txt"), "x").unwrap();
    let saved = State {
        files: vec![dir.join("kept.txt"), dir.join("gone.txt")],
        dirs: vec![dir.join("kept"), dir.join("gone")],
        ..State::default()
    };
    let mut a = app_with(PathBuf::from("../../schemes/vendor"), saved);
    crate::ui::patches::enter_patches(&mut a);
    let chosen: Vec<&PathBuf> = a.chosen_paths();
    assert_eq!(chosen.len(), 2, "only the two that still exist: {chosen:?}");
    assert!(chosen.iter().all(|p| p.exists()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_scheme_fetch_answer_is_never_recorded() {
    // Whether to clone or pull is about the disk right now, not a
    // preference — answering it once must not answer it forever.
    let a = completed_run(false, 1);
    let toml = toml::to_string_pretty(&a.to_state()).unwrap();
    for word in ["fetch", "clone", "update", "schemes_dir"] {
        assert!(
            !toml.contains(word),
            "state file should not mention {word}:\n{toml}"
        );
    }
}

#[test]
fn the_chords_and_the_vm_pick_survive_to_the_next_run() {
    let mut a = on_client();
    a.on_key(press(KeyCode::Char(' ')));
    clear_input(&mut a);
    typing(&mut a, "ctrl-a");
    a.on_key(press(KeyCode::Enter)); // commit the chord
    a.on_key(press(KeyCode::Enter)); // client -> resources
    a.on_key(press(KeyCode::Right)); // one more core

    let saved = a.to_state();
    assert_eq!(saved.leader.as_deref(), Some("ctrl-a"));
    assert_eq!(saved.vcpus, Some(a.resources.allocation().vcpus));

    let back = app_with(a.schemes_dir.clone(), saved);
    assert_eq!(back.bindings.hint(), "ctrl-a then d");
    assert_eq!(
        back.resources.allocation().vcpus,
        a.resources.allocation().vcpus
    );
}

#[test]
fn a_remembered_chord_set_that_no_longer_validates_falls_back_whole() {
    // Hand-edited, or a rule minimal tightened later. Restoring half of a
    // conflicting set would leave the user unable to detach at all, so the
    // set falls back together.
    let dir = temp_dir("bad-chords");
    let saved = State {
        leader: Some("ctrl-a".into()),
        detach: Some("ctrl-a".into()), // shadows the leader
        ..State::default()
    };
    let a = app_with(dir.clone(), saved);
    assert!(
        a.bindings.is_default(),
        "an invalid set should not be half-restored: {:?}",
        a.bindings
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_remembered_vm_size_this_host_cannot_offer_is_dropped() {
    let dir = temp_dir("big-vm");
    let saved = State {
        vcpus: Some(250),
        ram_mib: Some(1_048_576),
        ..State::default()
    };
    let a = app_with(dir.clone(), saved);
    assert!(a.resources.is_default(), "both were out of range");
    let _ = std::fs::remove_dir_all(&dir);
}
