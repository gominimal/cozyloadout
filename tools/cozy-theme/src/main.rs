//! cozy-theme — render the cozy loadout from a base16 scheme.
//!
//! Reads a tinted-theming scheme YAML, expands every file listed in
//! templates/manifest.toml, and writes a ready-to-bundle loadout tree.
//!
//! See AGENTS.md for the template grammar and the build pipeline.

use clap::Parser;
use color_eyre::eyre::{bail, Context, Result};
use cozy_theme::{mix, Scheme};
use fs_err as fs;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser as YamlParser};
use yaml_rust2::scanner::Marker;

// ---------------------------------------------------------------------------
// Template expansion
// ---------------------------------------------------------------------------

/// The template environment: minijinja with the two things this build needs
/// on top of plain substitution.
///
/// `UndefinedBehavior::Strict` is the important setting. A typo'd placeholder
/// must be a hard error — silently rendering an empty string into a config is
/// the worst failure this tool can have, because nothing downstream notices.
///
/// Auto-escaping stays off (the default for these extensions): every output is
/// a config file, not HTML, and escaping `&` or `"` would corrupt them.
fn environment(scheme: &Scheme) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    // minijinja drops a template's final newline by default. These are config
    // files and shell scripts, so the trailing newline is part of the contract
    // — without this every rendered file loses it.
    env.set_keep_trailing_newline(true);

    // Escaping is off for config files, where `&` and `"` are ordinary text and
    // escaping them would corrupt the output — with exactly one exception. The
    // .tmTheme is a plist, i.e. XML, and 12 upstream schemes carry an email
    // address in `author`: unescaped, `<edun@dunfelt.se>` makes the file
    // malformed, and bat then declines to load the theme while still exiting 0,
    // so the loadout silently ships with bat and delta unthemed.
    env.set_auto_escape_callback(|name| {
        if name.ends_with(".tmTheme") {
            AutoEscape::Html // XML for our purposes: escapes & < > " '
        } else {
            AutoEscape::None
        }
    });

    // {{ mix('base08','base00',15) }} — base16 has no dim surface colours, so
    // delta's diff backgrounds and broot's gauge ramp are computed from slots
    // rather than picked from them. `mix_rgb` is the same value in broot's
    // `r, g, b` form.
    for (name, as_rgb) in [("mix", false), ("mix_rgb", true)] {
        let palette = scheme.palette.clone();
        env.add_function(
            name,
            move |fg: String, bg: String, pct: f64| -> Result<String, minijinja::Error> {
                let slot = |s: &str| {
                    let canon =
                        format!("base{}", s.trim_start_matches("base").to_ascii_uppercase());
                    palette.get(&canon).copied().ok_or_else(|| {
                        minijinja::Error::new(
                            minijinja::ErrorKind::InvalidOperation,
                            format!("{name}: {s:?} is not a palette slot"),
                        )
                    })
                };
                if !(0.0..=100.0).contains(&pct) {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        format!("{name}: {pct} is outside 0–100"),
                    ));
                }
                let c = mix(slot(&fg)?, slot(&bg)?, pct);
                Ok(if as_rgb {
                    format!("{}, {}, {}", c.r, c.g, c.b)
                } else {
                    c.hex()
                })
            },
        );
    }
    env
}

/// Render one template. `dark`/`light` are exposed as booleans so the
/// scheme-dependent lines read as `{% if dark %}…{% endif %}`.
fn render(
    env: &Environment<'static>,
    name: &str,
    text: &str,
    vars: &BTreeMap<String, String>,
    scheme: &Scheme,
) -> Result<String> {
    let mut ctx: BTreeMap<&str, minijinja::Value> = vars
        .iter()
        .map(|(k, v)| (k.as_str(), minijinja::Value::from(v.clone())))
        .collect();
    ctx.insert("dark", minijinja::Value::from(scheme.is_dark));
    ctx.insert("light", minijinja::Value::from(!scheme.is_dark));
    env.render_named_str(name, text, ctx)
        .wrap_err_with(|| format!("{name}: template error"))
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// One `[[file]]` block. `deny_unknown_fields` is what turns a typo'd key into
/// an error instead of a silently ignored line.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    template: String,
    out: String,
    dest: Option<String>,
    #[serde(default)]
    copy: bool,
}

/// The manifest as a whole. `deny_unknown_fields` here rejects a key written
/// outside any `[[file]]`, which the hand-rolled parser used to catch by hand.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    file: Vec<Entry>,
}

/// Parse `templates/manifest.toml`.
///
/// This was ~95 lines of hand-rolled TOML subset, kept only so the tool had no
/// dependencies. The two hazards it existed to handle are both things a real
/// parser gets right for free: a trailing `# comment` after a quoted value (the
/// obvious `trim_matches('"')` leaves the comment glued on, and the renderer
/// then writes a file with the comment in its name), and `copy = yes`, which is
/// not TOML and used to read as `false`.
fn parse_manifest(path: &Path) -> Result<Vec<Entry>> {
    let text = fs::read_to_string(path)?;
    let manifest: Manifest = toml::from_str(&text).wrap_err_with(|| path.display().to_string())?;
    if manifest.file.is_empty() {
        bail!("{}: no [[file]] entries", path.display());
    }
    Ok(manifest.file)
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/// Parse a rendered file with a real parser before it is written.
///
/// Both formats have shipped broken from here, and both fail *silently* in the
/// tool that reads them: a `cozy.toml` whose inline tables wrapped across lines
/// was invalid TOML that minimal's parser happened to tolerate, and a .tmTheme
/// has been malformed twice — an unescaped `<email@host>` from a scheme's
/// `author`, and a literal `--` inside an XML comment. bat declines a bad theme
/// and still exits 0, so the loadout installs with bat and delta unthemed.
///
/// This runs on every render rather than only in CI, which is the point: `just
/// theme <anything>` is where a bad scheme reaches a user.
fn validate(path: &Path, body: &str) -> Result<()> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => {
            toml::from_str::<toml::Value>(body).wrap_err_with(|| {
                format!("{}: generated file is not valid TOML", path.display())
            })?;
        }
        Some("tmTheme") => {
            // `allow_dtd` because the plist carries Apple's DOCTYPE. roxmltree
            // is strict about `--` in comments, which is exactly the check that
            // matters here — quick-xml accepts those without complaint.
            let options = roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            };
            roxmltree::Document::parse_with_options(body, options)
                .wrap_err_with(|| format!("{}: generated file is not valid XML", path.display()))?;
        }
        Some("yml" | "yaml") => {
            YamlParser::new_from_str(body)
                .load(&mut IgnoreEvents, true)
                .wrap_err_with(|| {
                    format!("{}: generated file is not valid YAML", path.display())
                })?;
        }
        _ => {}
    }
    Ok(())
}

/// A receiver for `validate`, which only cares whether parsing succeeds.
struct IgnoreEvents;
impl MarkedEventReceiver for IgnoreEvents {
    fn on_event(&mut self, _ev: Event, _mark: Marker) {}
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    validate(path, contents)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(fs::write(path, contents)?)
}

/// Render the cozy loadout from a base16 or base24 scheme.
#[derive(Parser)]
#[command(name = "cozy-theme", version, about, long_about = None)]
struct Args {
    /// Scheme YAML to render (a path, e.g. schemes/minimal-dark.yaml)
    scheme: PathBuf,

    /// Directory holding the templates and manifest.toml
    #[arg(long, default_value = "templates")]
    templates: PathBuf,

    /// Directory to write the rendered loadout into
    #[arg(long, default_value = "build")]
    out: PathBuf,

    /// Loadout name — names the .toml and the directory beside it
    #[arg(long, default_value = "cozy")]
    loadout: String,
}

/// The loadout name is the stem of the file minimal identifies the loadout by,
/// and the name of the directory its hook scripts are anchored in, so it has to
/// be a single path component. minimal applies the same rule.
///
/// Checking for slashes is not enough, and the gap was destructive rather than
/// merely wrong: `--loadout ..` put `root` at the *parent* of `--out`, which
/// `remove_dir_all` below then deleted before writing the tree over the top.
/// Anything that is not one `Normal` component is rejected — that covers `..`,
/// `.`, absolute paths and Windows prefixes as well as `a/b`.
fn is_single_component(name: &str) -> bool {
    // Separators are rejected outright rather than left to `components()`,
    // which normalises them away: `sub/` collapses to one Normal component but
    // would put the manifest at `<out>/sub/.toml` instead of `<out>/sub.toml`.
    // A backslash is a legal Unix filename character and so would also survive,
    // but the name is interpolated into the hook script and into paths minimal
    // reads, and the original guard rejected it — this check is meant to be
    // strictly stronger than that one, not differently shaped.
    if name.contains('/') || name.contains('\\') {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn build(args: &Args) -> Result<()> {
    if !is_single_component(&args.loadout) {
        bail!(
            "--loadout {:?}: must be a single path component — no slashes, no `.` or `..`",
            args.loadout
        );
    }

    let scheme = Scheme::load(&args.scheme)?;
    let entries = parse_manifest(&args.templates.join("manifest.toml"))?;
    let mut vars = scheme.vars();
    vars.insert("loadout_name".into(), args.loadout.clone());

    let env = environment(&scheme);
    let root = args.out.join(&args.loadout);
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }

    let sub = |s: &str| {
        s.replace("{slug}", &scheme.slug)
            .replace("{loadout}", &args.loadout)
    };
    let mut patches = String::new();

    for e in &entries {
        let src = args.templates.join(&e.template);
        let out_rel = sub(&e.out);
        let dst = root.join(&out_rel);

        let body = fs::read_to_string(&src)?;
        let body = if e.copy {
            body
        } else {
            render(&env, &src.display().to_string(), &body, &vars, &scheme)?
        };
        write_file(&dst, &body)?;

        if let Some(dest) = &e.dest {
            // One line per entry. TOML forbids newlines inside an inline
            // table, so the wrapped `{ dest = …,\n source = … }` form this
            // file used to be written in was not actually valid TOML — it
            // survived only because minimal's parser tolerates it.
            let _ = writeln!(
                patches,
                "    {{ dest = \"{}\", source = \"$LOADOUT_ROOT/{}\" }},",
                sub(dest),
                out_rel
            );
        }
    }

    // The loadout manifest itself: same template grammar, plus `patches`,
    // which is built from the list above so it cannot describe a file that
    // wasn't rendered.
    let toml_src = args.templates.join(format!("{}.toml", args.loadout));
    let body = fs::read_to_string(&toml_src)?;
    vars.insert(
        "patches".into(),
        patches.trim_end().trim_end_matches(',').to_string(),
    );
    let body = render(&env, &toml_src.display().to_string(), &body, &vars, &scheme)?;
    write_file(&args.out.join(format!("{}.toml", args.loadout)), &body)?;

    println!(
        "{} ({}, {}) -> {}/  [{} files]",
        scheme.name,
        scheme.slug,
        scheme.variant(),
        args.out.display(),
        entries.len() + 1
    );
    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    build(&Args::parse())
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A private directory per call. The slug comes from the filename and tests
    /// run in parallel in one process, so a shared path would both clobber
    /// content across threads and collapse distinct slugs — and a *fixed* path
    /// under the system temp dir is worse still, since it can be owned by
    /// another user or another checkout's test run. Every test that touches the
    /// filesystem goes through here or [`temp_dir`].
    fn temp_dir() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("cozy-theme-test-{}", std::process::id()))
            .join(n.to_string());
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write `text` to `<private dir>/<slug>.yaml` and load it.
    fn scheme_for(slug: &str, text: &str) -> Scheme {
        let p = temp_dir().join(format!("{slug}.yaml"));
        fs::write(&p, text).unwrap();
        Scheme::load(&p).unwrap()
    }

    const CURRENT: &str = r##"
system: "base16"
name: "Minimal Dark"
author: "Minimal System (gominimal)"
variant: "dark"
palette:
  base00: "#141414" # gray-8
  base01: "#292929"
  base02: "#3d3d3d"
  base03: "#666666"
  base04: "#8f8f8f"
  base05: "#e0e0e0"
  base06: "#ebebeb"
  base07: "#ffffff"
  base08: "#e93535"
  base09: "#dd8440"
  base0A: "#cca300"
  base0B: "#7ec897"
  base0C: "#6bbec7"
  base0D: "#4a7aff"
  base0E: "#aa81da"
  base0F: "#af715a"
"##;

    fn manifest_for(text: &str) -> Result<Vec<Entry>> {
        let p = temp_dir().join("manifest.toml");
        fs::write(&p, text).unwrap();
        parse_manifest(&p)
    }

    #[test]
    fn renders_vars_sections_and_mix() {
        let s = scheme_for("minimal-dark", CURRENT);
        let v = s.vars();
        let env = environment(&s);
        let r = |t: &str| render(&env, "test", t, &v, &s).unwrap();
        assert_eq!(r("#{{base0D_hex}}"), "#4a7aff");
        assert_eq!(r("rgb({{base00_rgb}})"), "rgb(20, 20, 20)");
        assert_eq!(r("{{base00_rgb_r}}"), "20");
        assert_eq!(r("{{ mix('base08','base00',15) }}"), "341919");
        assert_eq!(r("{{ mix_rgb('base08','base00',15) }}"), "52, 25, 25");
        assert_eq!(
            r("{% if dark %}Ocean{% endif %}{% if light %}GitHub{% endif %}"),
            "Ocean"
        );
    }

    #[test]
    fn unknown_placeholder_is_an_error() {
        // Passing it through would ship an empty value into a config file that
        // the target tool then silently ignores. `UndefinedBehavior::Strict` is
        // what makes this an error rather than a blank.
        let s = scheme_for("minimal-dark", CURRENT);
        let v = s.vars();
        let env = environment(&s);
        let bad = |t: &str| render(&env, "test", t, &v, &s).is_err();
        assert!(bad("{{base0G_hex}}"), "unknown placeholder");
        assert!(bad("{{ mix('base08','base00') }}"), "too few mix args");
        assert!(bad("{{ mix('base08','base00',150) }}"), "pct out of range");
        assert!(bad("{{ mix('base0G','base00',15) }}"), "bad slot");
        assert!(bad("{{unclosed"), "unclosed delimiter");
        // The families dropped with the tinted-builder vocabulary must now
        // fail rather than silently resolve.
        assert!(bad("{{base00_hex_r}}"), "removed hex-channel family");
        assert!(bad("{{base00_dec_r}}"), "removed dec-channel family");
    }

    #[test]
    fn loadout_name_must_be_one_component() {
        // `--loadout ..` used to put the output root at the parent of --out,
        // which remove_dir_all then deleted. Verified destructive before the
        // fix: it wiped a sibling directory.
        for bad in ["", "..", ".", "a/b", "a\\b", "/abs", "../escape", "sub/"] {
            assert!(!is_single_component(bad), "should be rejected: {bad:?}");
        }
        for good in ["cozy", "my-loadout", "cozy.2"] {
            assert!(is_single_component(good), "should be accepted: {good:?}");
        }
    }

    #[test]
    fn manifest_parses_entries() {
        let m = manifest_for(
            r#"
            # a comment
            [[file]]
            template = "fish/config.fish"
            out      = "fish/config.fish"
            dest     = ".config/fish/config.fish"

            [[file]]
            template = "helix/languages.toml"
            out      = "helix/languages.toml"
            copy     = true
            "#,
        )
        .unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].out, "fish/config.fish");
        assert_eq!(m[0].dest.as_deref(), Some(".config/fish/config.fish"));
        assert!(!m[0].copy);
        assert_eq!(m[1].dest, None);
        assert!(m[1].copy);
    }

    #[test]
    fn manifest_values_keep_their_trailing_comments_out() {
        // The failure this guards: `trim_matches('"')` leaves the comment glued
        // to the value, and the renderer then writes a file called
        // `config.fish" # note` without complaining.
        let m = manifest_for(
            r#"
            [[file]]
            template = "bat/config"   # no colours in here
            out      = "bat/config"
            copy     = false          # ...but the scheme name is
            "#,
        )
        .unwrap();
        assert_eq!(m[0].template, "bat/config");
        assert!(!m[0].copy);
    }

    #[test]
    fn manifest_rejects_what_it_cannot_understand() {
        let bad = [
            // Unquoted string: would have been taken as-is before.
            "[[file]]\ntemplate = bat/config\nout = \"bat/config\"\n",
            // `copy = yes` used to read as false.
            "[[file]]\ntemplate = \"a\"\nout = \"b\"\ncopy = yes\n",
            // Junk after a closing quote.
            "[[file]]\ntemplate = \"a\" oops\nout = \"b\"\n",
            "[[file]]\ntemplate = \"unterminated\nout = \"b\"\n",
            "[[file]]\ntemplate = \"a\"\nout = \"b\"\nwat = \"x\"\n",
            "template = \"a\"\n",           // key outside [[file]]
            "[[file]]\ntemplate = \"a\"\n", // no `out`
            "# nothing but a comment\n",
        ];
        for m in bad {
            assert!(manifest_for(m).is_err(), "should have been rejected:\n{m}");
        }
    }
}
