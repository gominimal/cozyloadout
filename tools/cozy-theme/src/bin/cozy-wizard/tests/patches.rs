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

// -- the preview pane ------------------------------------------------------

/// A patches page whose tree has something worth previewing in it.
fn with_previewable(tag: &str) -> (App, PathBuf) {
    let (mut a, root) = on_patches(tag);
    std::fs::write(root.join("notes.txt"), "first\nsecond\nthird\n").unwrap();
    std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    std::fs::write(root.join("hollow"), "").unwrap();
    a.pickers[0].reload();
    a.pickers[1].reload();
    (a, root)
}

/// Move the cursor onto `name`, from wherever it is now.
///
/// Rewinds to the top first: pressing Down until a match clamps at the bottom
/// and spins forever when the target is above the cursor. Bounded as well, so a
/// name that is not in the listing fails the test instead of hanging it.
fn land_on(a: &mut App, name: &str) {
    let len = a.picker().entries.len();
    for _ in 0..=len {
        a.on_key(press(KeyCode::Up));
    }
    for _ in 0..=len {
        if a.picker().current().map(|e| e.name.as_str()) == Some(name) {
            return;
        }
        a.on_key(press(KeyCode::Down));
    }
    let names: Vec<&str> = a.picker().entries.iter().map(|e| e.name.as_str()).collect();
    panic!("no entry {name:?} in {names:?}");
}

#[test]
fn a_file_shows_its_first_lines() {
    let (mut a, root) = with_previewable("preview-file");
    land_on(&mut a, "notes.txt");
    let text = flatten(&render_app(&a, 120, 24));
    assert!(text.contains("Preview"), "{text}");
    for line in ["first", "second", "third"] {
        assert!(text.contains(line), "missing {line}:\n{text}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_binary_file_reports_its_size_instead_of_its_bytes() {
    // Dumping control characters into the pane would corrupt the frame around
    // it, not just look wrong.
    let (mut a, root) = with_previewable("preview-binary");
    land_on(&mut a, "blob.bin");
    let text = flatten(&render_app(&a, 120, 24));
    assert!(text.contains("binary"), "{text}");
    assert!(text.contains("4 B"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_empty_file_says_so_rather_than_looking_broken() {
    let (mut a, root) = with_previewable("preview-empty");
    land_on(&mut a, "hollow");
    assert!(flatten(&render_app(&a, 120, 24)).contains("empty"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_directory_shows_what_patching_it_in_would_copy() {
    // The number that matters is the recursive one: the patch source becomes
    // `<dir>/**/*`, so the whole tree comes with it.
    let (mut a, root) = with_previewable("preview-dir");
    a.on_key(press(KeyCode::Tab));
    land_on(&mut a, "dotfiles");
    let text = flatten(&render_app(&a, 120, 24));
    assert!(text.contains("file"), "{text}");
    assert!(text.contains("folder"), "{text}");
    assert!(
        text.contains("config.toml"),
        "a sample of what is in it:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_preview_follows_the_cursor() {
    let (mut a, root) = with_previewable("preview-follow");
    land_on(&mut a, "notes.txt");
    assert!(flatten(&render_app(&a, 120, 24)).contains("first"));
    land_on(&mut a, "blob.bin");
    let text = flatten(&render_app(&a, 120, 24));
    assert!(text.contains("binary"), "{text}");
    assert!(
        !text.contains("first"),
        "the old preview must not linger:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_preview_is_read_once_per_path_not_once_per_frame() {
    // Drawing happens on every keystroke and on a ten-a-second tick. Reading
    // the file each time would put the disk in that loop.
    let (mut a, root) = with_previewable("preview-cache");
    land_on(&mut a, "notes.txt");
    let _ = render_app(&a, 120, 24);

    // Change the file behind the wizard's back; the cache should hold.
    std::fs::write(root.join("notes.txt"), "REPLACED\n").unwrap();
    let text = flatten(&render_app(&a, 120, 24));
    assert!(
        text.contains("first"),
        "the cached read should stand:\n{text}"
    );
    assert!(!text.contains("REPLACED"), "{text}");

    // Moving away and back re-reads, because the key changed in between.
    land_on(&mut a, "blob.bin");
    let _ = render_app(&a, 120, 24);
    land_on(&mut a, "notes.txt");
    assert!(flatten(&render_app(&a, 120, 24)).contains("REPLACED"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_pane_is_dropped_before_the_pickers_are() {
    // A ladder: both pickers and a preview, then both pickers, then only the
    // focused one. The listing is what you cannot do without.
    let (mut a, root) = with_previewable("preview-narrow");
    land_on(&mut a, "notes.txt");
    assert!(flatten(&render_app(&a, 120, 24)).contains("Preview"));

    let mid = flatten(&render_app(&a, 90, 24));
    assert!(!mid.contains("Preview"), "no room for three panes:\n{mid}");
    assert!(
        mid.contains("Files") && mid.contains("Directories"),
        "{mid}"
    );

    let narrow = flatten(&render_app(&a, 60, 24));
    assert!(!narrow.contains("Directories"), "{narrow}");
    assert!(narrow.contains("Files"), "{narrow}");
    let _ = std::fs::remove_dir_all(&root);
}

/// The distinct foreground colours of `needle` itself, wherever it is drawn.
///
/// Scoped to the needle's own cells rather than the whole row: this page has
/// three columns, and a row-wide scan collects both pickers and the borders
/// between them along with the thing under test.
fn text_colours(app: &App, w: u16, h: u16, needle: &str) -> Vec<ratatui::style::Color> {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();

    for y in 0..h {
        let row: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
        let Some(byte_at) = row.find(needle) else {
            continue;
        };
        let start = row[..byte_at].chars().count();
        let mut seen: Vec<ratatui::style::Color> = Vec::new();
        for k in 0..needle.chars().count() {
            let x = u16::try_from(start + k).unwrap();
            let cell = &buf[(x, y)];
            if cell.symbol().trim().is_empty() {
                continue;
            }
            if !seen.contains(&cell.fg) {
                seen.push(cell.fg);
            }
        }
        return seen;
    }
    panic!("no row containing {needle:?}");
}

#[test]
fn a_previewed_file_is_actually_syntax_coloured() {
    // The pane draws spans, so this asks the rendered buffer rather than the
    // text: a highlighter that returned the right strings in one colour would
    // pass a text assertion and still be broken.
    let (mut a, root) = on_patches("colours");
    std::fs::write(
        root.join("server.toml"),
        "# a comment\nport = 8080\nname = \"cozy\"\n",
    )
    .unwrap();
    a.pickers[0].reload();
    land_on(&mut a, "server.toml");

    let comment = text_colours(&a, 120, 24, "# a comment");
    let value = text_colours(&a, 120, 24, "port = 8080");
    assert_eq!(comment.len(), 1, "a comment is one colour: {comment:?}");
    assert!(
        value.len() > 1,
        "a key and its number should differ: {value:?}"
    );
    assert!(
        !value.contains(&comment[0]),
        "and neither should match the comment colour: {value:?} vs {comment:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_colours_come_from_the_chosen_scheme() {
    // The point of lighting it from the loadout's own .tmTheme: the preview is
    // what `bat` will show, not an approximation with fixed colours.
    let (mut a, root) = on_patches("scheme-colours");
    std::fs::write(root.join("a.toml"), "x = \"hello\"\n").unwrap();
    a.pickers[0].reload();
    land_on(&mut a, "a.toml");
    let before = text_colours(&a, 120, 24, "x = ");

    // A different scheme has to repaint the code, not just the chrome.
    a.adjust = cozy_theme::Adjust {
        warmth: 100,
        saturation: -100,
        ..cozy_theme::Adjust::default()
    };
    let after = text_colours(&a, 120, 24, "x = ");
    assert_ne!(
        before, after,
        "adjusting the scheme should relight the file"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_unhighlightable_file_still_reads() {
    // No grammar for `.zzzz`; the pane's own foreground has to show through
    // rather than the terminal's default.
    let (mut a, root) = on_patches("no-grammar");
    std::fs::write(root.join("mystery.zzzz"), "just some words\n").unwrap();
    a.pickers[0].reload();
    land_on(&mut a, "mystery.zzzz");
    let text = flatten(&render_app(&a, 120, 24));
    assert!(text.contains("just some words"), "{text}");
    let colours = text_colours(&a, 120, 24, "just some words");
    assert!(
        !colours.contains(&ratatui::style::Color::Reset),
        "an unlit line should still take the pane's colour: {colours:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "prints frames to look at rather than asserting"]
fn show_the_preview_pane() {
    let (mut a, root) = on_patches("show-preview");
    std::fs::write(
        root.join("server.toml"),
        "# how the thing is configured\n[server]\nport = 8080\nname = \"cozy\"\ndebug = true\n",
    )
    .unwrap();
    a.pickers[0].reload();
    std::fs::write(
        root.join("notes.txt"),
        "# a config\n[server]\nport = 8080\nname = \"cozy\"\ndebug = true\n",
    )
    .unwrap();
    a.pickers[0].reload();
    land_on(&mut a, "server.toml");
    println!("\n=== a file");
    for line in render_app(&a, 120, 22) {
        println!("|{}|", line.trim_end());
    }
    a.on_key(press(KeyCode::Tab));
    println!("\n=== a directory");
    for line in render_app(&a, 120, 22) {
        println!("|{}|", line.trim_end());
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "walks a real home directory; run it deliberately"]
fn preview_a_real_directory() {
    // The bounds are the point: a home directory has node_modules and disk
    // images in it, and this pane reads whatever the cursor lands on.
    let home = std::env::var("HOME").unwrap();
    let start = std::time::Instant::now();
    let p = crate::preview::Preview::read(std::path::Path::new(&home), true, 8);
    println!("{:?} in {:?}", p, start.elapsed());
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "a preview must not stall the interface"
    );
}
