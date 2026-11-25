//! The git page: its two values, its three ticks, and the hook lines they
//! become.

#[allow(unused_imports)]
use super::util::*;
#[allow(clippy::wildcard_imports)]
use crate::*;

#[test]
fn typing_a_name_ticks_it_and_q_is_only_a_letter() {
    let mut a = on_git();
    a.on_key(press(KeyCode::Char(' ')));
    typing(&mut a, "qj Quinn");
    assert!(!a.done, "q inside the field must not quit");
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.git_name, "qj Quinn");
    assert!(a.git_set_name, "typing a value is asking for it to be set");
    assert_eq!(a.screen, Screen::Git, "accepting stays on the page");
}

#[test]
fn emptying_a_value_unticks_it() {
    let mut a = on_git();
    a.git_name = "Ada".into();
    a.git_set_name = true;
    a.on_key(press(KeyCode::Char(' ')));
    clear_input(&mut a);
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.git_name, "");
    assert!(!a.git_set_name, "there is nothing left to set");
}

#[test]
fn esc_while_typing_keeps_the_value_that_was_there() {
    let mut a = on_git();
    a.git_email = "ada@example.com".into();
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Char(' ')));
    typing(&mut a, "junk");
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.git_email, "ada@example.com");
    assert_eq!(a.screen, Screen::Git, "esc closes the field, not the page");
}

#[test]
fn space_toggles_each_tick_without_touching_the_text() {
    let mut a = on_git();
    a.git_name = "Ada".into();
    a.git_set_name = true;
    for _ in 0..2 {
        a.on_key(press(KeyCode::Down));
    }
    a.on_key(press(KeyCode::Char(' ')));
    assert!(!a.git_set_name);
    assert_eq!(a.git_name, "Ada", "unticking keeps what was typed");
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Char(' ')));
    assert!(a.git_set_email);
    a.on_key(press(KeyCode::Down));
    a.on_key(press(KeyCode::Char(' ')));
    assert!(a.git_https);
}

#[test]
fn the_page_sits_between_patches_and_client() {
    let mut a = on_git();
    a.on_key(press(KeyCode::Enter));
    assert_eq!(a.screen, Screen::Client);
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Git);
    a.on_key(press(KeyCode::Esc));
    assert_eq!(a.screen, Screen::Patches);
}

#[test]
fn remembered_answers_come_back_on_the_next_run() {
    let saved = State {
        git_name: Some("Ada Lovelace".into()),
        git_email: Some("ada@example.com".into()),
        git_set_name: Some(true),
        git_set_email: Some(false),
        git_https: Some(true),
        ..State::default()
    };
    let a = app_with(PathBuf::from("target/does-not-exist-for-tests"), saved);
    assert_eq!(a.git_name, "Ada Lovelace");
    assert_eq!(a.git_email, "ada@example.com", "kept though unticked");
    assert!(a.git_set_name && !a.git_set_email && a.git_https);

    // And what is restored is what gets written back.
    let st = a.to_state();
    assert_eq!(st.git_name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(st.git_email.as_deref(), Some("ada@example.com"));
    assert_eq!(
        (st.git_set_name, st.git_set_email, st.git_https),
        (Some(true), Some(false), Some(true))
    );
}

#[test]
fn never_asked_means_all_off() {
    // An untouched wizard reproduces the loadout people already have.
    let a = app();
    assert!(!a.git_set_name && !a.git_set_email && !a.git_https);
    assert_eq!(a.git_setup(), cozy_theme::GitSetup::default());
}

#[test]
fn the_page_shows_the_commands_the_hook_will_run() {
    let mut a = on_git();
    a.git_name = "Ada Lovelace".into();
    a.git_set_name = true;
    a.git_https = true;
    let text = flatten(&render_app(&a, 100, 30));
    assert!(
        text.contains("git config --global user.name 'Ada Lovelace'"),
        "{text}"
    );
    assert!(text.contains("insteadOf 'git@github.com:'"), "{text}");
    assert!(
        !text.contains("user.email '"),
        "email is not ticked:\n{text}"
    );
}

#[test]
fn a_ticked_empty_value_and_an_odd_email_are_warned_about() {
    let mut a = on_git();
    a.git_set_name = true;
    a.git_email = "not-an-address".into();
    let text = flatten(&render_app(&a, 100, 30));
    assert!(text.contains("ticked but empty"), "{text}");
    assert!(text.contains("does not look like an email"), "{text}");
}

#[test]
fn the_generated_hook_runs_what_the_page_chose() {
    let out = temp_dir("git-hook");
    std::fs::create_dir_all(&out).unwrap();
    let mut a = on_git();
    a.git_name = "Ada O'Brien".into();
    a.git_set_name = true;
    a.git_email = "ada@example.com".into();
    a.git_set_email = true;
    a.git_https = true;
    run_generate_to(&a, Path::new("../.."), &out, false).unwrap();

    let hook = std::fs::read_to_string(out.join("cozy/hooks/on-activate.sh")).unwrap();
    for line in [
        r"git config --global user.name 'Ada O'\''Brien' || true",
        "git config --global user.email 'ada@example.com' || true",
        "git config --global url.https://github.com/.insteadOf 'git@github.com:' || true",
    ] {
        assert!(hook.contains(line), "missing {line:?} in:\n{hook}");
    }
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn the_summary_names_what_the_hook_will_set() {
    let mut a = on_apply();
    assert!(flatten(&render_app(&a, 120, 40)).contains("untouched"));
    a.git_name = "Ada".into();
    a.git_set_name = true;
    a.git_https = true;
    let text = flatten(&render_app(&a, 120, 40));
    assert!(
        text.contains("user.name, GitHub over HTTPS — set by the activation hook"),
        "{text}"
    );
}
