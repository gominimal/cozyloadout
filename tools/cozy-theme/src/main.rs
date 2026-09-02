//! cozy-theme — render the cozy loadout from a base16 scheme.
//!
//! Reads a tinted-theming scheme YAML, expands every file listed in
//! templates/manifest.toml, and writes a ready-to-bundle loadout tree.
//!
//! See AGENTS.md for the template grammar and the build pipeline.

use clap::Parser;
use color_eyre::eyre::{bail, eyre, Context, Result};
use fs_err as fs;
use minijinja::{Environment, UndefinedBehavior};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use yaml_rust2::{Yaml, YamlLoader};

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    /// Accepts `#rrggbb` or `rrggbb`, any case. base16 schemes are always
    /// 6-digit; 3-digit shorthand is not part of the format.
    fn parse(s: &str) -> Result<Rgb> {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("not a 6-digit hex colour: {s:?}");
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
        Ok(Rgb {
            r: byte(0),
            g: byte(2),
            b: byte(4),
        })
    }

    fn hex(&self) -> String {
        format!("{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// WCAG relative luminance. Only used to decide dark vs light.
    fn luminance(&self) -> f64 {
        let ch = |v: u8| {
            let s = v as f64 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(self.r) + 0.7152 * ch(self.g) + 0.0722 * ch(self.b)
    }
}

/// `fg` blended over `bg` at `pct` percent. Rounds to nearest, so a 50% mix of
/// two slots is the true midpoint rather than biased downward.
fn mix(fg: Rgb, bg: Rgb, pct: f64) -> Rgb {
    let f = pct / 100.0;
    let c = |a: u8, b: u8| {
        (b as f64 + f * (a as f64 - b as f64))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgb {
        r: c(fg.r, bg.r),
        g: c(fg.g, bg.g),
        b: c(fg.b, bg.b),
    }
}

// ---------------------------------------------------------------------------
// Scheme
// ---------------------------------------------------------------------------

const SLOTS: [&str; 16] = [
    "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08",
    "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
];

/// Namespace for the per-scheme theme UUIDs. Arbitrary but fixed: changing it
/// renumbers every generated .tmTheme, which is harmless (they are rebuilt) but
/// pointless churn. Generated once, for this tool.
const UUID_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6f, 0x0c, 0x24, 0x7e, 0x4b, 0x1a, 0x4d, 0x8e, 0x9c, 0x3f, 0xa1, 0x52, 0x7d, 0x88, 0xe0, 0x14,
]);

struct Scheme {
    slug: String,
    name: String,
    author: String,
    is_dark: bool,
    palette: BTreeMap<String, Rgb>,
}

/// Collect `baseXX` slots and scalar metadata from a YAML document, at any
/// depth. Empty scalars are skipped so a scheme with `author: ""` still falls
/// back to "unknown" rather than reporting an empty author.
fn walk(
    node: &Yaml,
    meta: &mut BTreeMap<String, String>,
    palette: &mut BTreeMap<String, Rgb>,
) -> Result<()> {
    let Yaml::Hash(hash) = node else { return Ok(()) };
    for (key, value) in hash {
        let Some(key) = key.as_str() else { continue };
        let lower = key.to_ascii_lowercase();
        if lower.len() == 6 && lower.starts_with("base") {
            // Normalise base0a -> base0A so templates have one spelling.
            let canon = format!("base{}", key[4..].to_ascii_uppercase());
            if SLOTS.contains(&canon.as_str()) {
                // A bare `1d2021` is a YAML string; one that happens to be all
                // digits lexes as an integer, so accept both spellings.
                let raw = value
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| value.as_i64().map(|n| n.to_string()))
                    .ok_or_else(|| eyre!("{canon}: not a scalar"))?;
                palette.insert(canon.clone(), Rgb::parse(&raw).wrap_err(canon)?);
            }
        } else if matches!(value, Yaml::Hash(_)) {
            walk(value, meta, palette)?;
        } else if let Some(text) = value.as_str().filter(|s| !s.is_empty()) {
            meta.entry(lower).or_insert_with(|| text.to_string());
        }
    }
    Ok(())
}

impl Scheme {
    /// Handles both scheme formats: the current one with a nested `palette:`
    /// block, and the legacy one with `base00:` at the top level. Nesting is
    /// ignored entirely — any `baseXX` key at any depth is a slot — which is
    /// what makes one code path cover both.
    ///
    /// The lexing is yaml-rust2's. This used to be a hand-rolled line splitter
    /// whose entire reason for existing was the quoting hazards: a value of
    /// `"#141414"` that a naive comment strip eats, and a legacy scheme's bare
    /// `base00: 1d2021` where `#` only opens a comment after whitespace. A real
    /// YAML parser gets both right by construction.
    fn load(path: &Path) -> Result<Scheme> {
        let text = fs::read_to_string(path)?;
        let docs = YamlLoader::load_from_str(&text)
            .wrap_err_with(|| format!("{}: not valid YAML", path.display()))?;

        let mut meta: BTreeMap<String, String> = BTreeMap::new();
        let mut palette: BTreeMap<String, Rgb> = BTreeMap::new();
        for doc in &docs {
            walk(doc, &mut meta, &mut palette).wrap_err_with(|| path.display().to_string())?;
        }

        let missing: Vec<&str> =
            SLOTS.iter().copied().filter(|s| !palette.contains_key(*s)).collect();
        if missing.len() == SLOTS.len() {
            // Most likely a tinted8 scheme (named 8-colour keys) or not a
            // scheme at all — saying "missing all 16" for those reads as a
            // corrupt base16 file rather than the wrong kind of file.
            let system = meta.get("system").map(|s| s.as_str()).unwrap_or("unknown");
            bail!(
                "{}: no base16 slots found — this is a {system:?} scheme, and the loadout \
                 needs base16 or base24",
                path.display()
            );
        }
        if !missing.is_empty() {
            bail!(
                "{}: scheme is missing {} of 16 slots: {}",
                path.display(),
                missing.len(),
                missing.join(", ")
            );
        }

        let slug = slugify(
            path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| eyre!("scheme path has no file stem"))?,
        );
        if slug.is_empty() {
            bail!("{}: filename does not slugify to anything", path.display());
        }

        // `variant:` is advisory; the luma of the scheme's own surface decides.
        // A scheme whose background is darker than its foreground is dark, no
        // matter what the metadata claims — the only consumer is broot's
        // preview theme, and getting that backwards is very visible.
        let bg = palette["base00"];
        let fg = palette["base05"];
        let is_dark = bg.luminance() <= fg.luminance();

        let name = meta
            .get("name")
            .or_else(|| meta.get("scheme"))
            .cloned()
            .unwrap_or_else(|| slug.clone());

        Ok(Scheme {
            slug,
            name,
            author: meta.get("author").cloned().unwrap_or_else(|| "unknown".into()),
            is_dark,
            palette,
        })
    }

    /// Stable per-slug UUID for the .tmTheme. Sublime keys themes by UUID, so
    /// two schemes sharing one would collide; deriving it from the slug keeps
    /// it both unique and reproducible across builds.
    ///
    /// v5 (name-based, SHA-1) is what that description *is*. This used to be a
    /// hand-rolled FNV-1a/xorshift hash with the version-4 nibble stamped on
    /// top, which claimed "random" for a value that was nothing of the sort.
    fn uuid(&self) -> String {
        Uuid::new_v5(&UUID_NAMESPACE, self.slug.as_bytes()).to_string()
    }

    fn variant(&self) -> &'static str {
        if self.is_dark {
            "dark"
        } else {
            "light"
        }
    }

    /// Every placeholder a template may reference.
    ///
    /// Names are snake_case because minijinja parses `base00-hex` as a
    /// subtraction. The `-hex-r/g/b` and `-dec-r/g/b` families that used to be
    /// emitted here are gone: they existed only so upstream tinted-builder
    /// templates would drop in, and moving to Jinja syntax ended that anyway.
    fn vars(&self) -> BTreeMap<String, String> {
        let mut v = BTreeMap::new();
        for slot in SLOTS {
            let c = self.palette[slot];
            let hex = c.hex();
            v.insert(format!("{slot}_hex"), hex);
            v.insert(format!("{slot}_rgb_r"), c.r.to_string());
            v.insert(format!("{slot}_rgb_g"), c.g.to_string());
            v.insert(format!("{slot}_rgb_b"), c.b.to_string());
            // Convenience triple: broot wants `rgb(126, 200, 151)` and writing
            // that as three placeholders is unreadable.
            v.insert(format!("{slot}_rgb"), format!("{}, {}, {}", c.r, c.g, c.b));
        }
        v.insert("scheme_slug".into(), self.slug.clone());
        v.insert("scheme_name".into(), self.name.clone());
        v.insert("scheme_author".into(), self.author.clone());
        v.insert("scheme_variant".into(), self.variant().into());
        v.insert("scheme_uuid".into(), self.uuid());
        v
    }
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

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
                    let canon = format!("base{}", s.trim_start_matches("base").to_ascii_uppercase());
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
                Ok(if as_rgb { format!("{}, {}, {}", c.r, c.g, c.b) } else { c.hex() })
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
    let mut ctx: BTreeMap<&str, minijinja::Value> =
        vars.iter().map(|(k, v)| (k.as_str(), minijinja::Value::from(v.clone()))).collect();
    ctx.insert("dark", minijinja::Value::from(scheme.is_dark));
    ctx.insert("light", minijinja::Value::from(!scheme.is_dark));
    env.render_str(text, ctx).wrap_err_with(|| format!("{name}: template error"))
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
    let manifest: Manifest =
        toml::from_str(&text).wrap_err_with(|| path.display().to_string())?;
    if manifest.file.is_empty() {
        bail!("{}: no [[file]] entries", path.display());
    }
    Ok(manifest.file)
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

fn write_file(path: &Path, contents: &str) -> Result<()> {
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

fn build(args: &Args) -> Result<()> {
    // The loadout name is the stem of the file minimal identifies the loadout
    // by, and the name of the directory its hook scripts are anchored in, so it
    // has to be a single path component. minimal applies the same rule.
    if args.loadout.is_empty() || args.loadout.contains(['/', '\\']) {
        bail!("--loadout {:?}: must be a non-empty name, with no slashes", args.loadout);
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

    /// Write `text` to a private directory and parse it as a manifest.
    fn manifest_for(text: &str) -> Result<Vec<Entry>> {
        let p = temp_dir().join("manifest.toml");
        fs::write(&p, text).unwrap();
        parse_manifest(&p)
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

    // The pre-2022 layout: no `palette:` block, no `#`, lowercase slot letters.
    const LEGACY: &str = r##"
scheme: "Gruvbox dark, hard"
author: "Dawid Kurek"
base00: 1d2021
base01: 3c3836
base02: 504945
base03: 665c54
base04: bdae93
base05: d5c4a1
base06: ebdbb2
base07: fbf1c7
base08: fb4934
base09: fe8019
base0a: fabd2f
base0b: b8bb26
base0c: 8ec07c
base0d: 83a598
base0e: d3869b
base0f: d65d0e
"##;

    #[test]
    fn parses_current_format() {
        let s = scheme_for("minimal-dark", CURRENT);
        assert_eq!(s.name, "Minimal Dark");
        assert_eq!(s.palette["base00"].hex(), "141414");
        assert_eq!(s.palette["base0F"].hex(), "af715a");
        assert!(s.is_dark);
    }

    #[test]
    fn parses_legacy_format_and_normalises_slot_case() {
        let s = scheme_for("gruvbox-dark-hard", LEGACY);
        assert_eq!(s.name, "Gruvbox dark, hard");
        assert_eq!(s.palette["base0A"].hex(), "fabd2f");
        assert!(s.is_dark);
    }

    #[test]
    fn quoted_hash_survives_comment_stripping() {
        // The failure this guards: stripping `#`-comments before checking for
        // quotes turns `"#141414"` into an empty value. yaml-rust2 gets this
        // right by construction, but the hazard is why the parser exists, so
        // the case stays covered end to end.
        let s = scheme_for(
            "hazards",
            r##"
palette:
  base00: "#141414" # gray-8
  base01: "#1f1f1f"
  base02: "#2a2a2a"
  base03: "#666666"
  base04: "#8a8a8a"
  base05: "#d4d4d4"
  base06: "#e8e8e8"
  base07: "#f5f5f5"
  base08: "#ff5f56"
  base09: "#ff9f43"
  base0A: "#ffd93d"
  base0B: "#6bcf7f"
  base0C: "#4ecdc4"
  base0D: "#4a7aff"
  base0E: "#b57edc"
  base0F: "#8b6f47"
"##,
        );
        assert_eq!(s.palette["base00"], Rgb { r: 0x14, g: 0x14, b: 0x14 });

        // A legacy scheme's bare, unquoted value, with and without a comment.
        let bare = scheme_for("bare", &CURRENT.replace(r##""#141414""##, "1d2021"));
        assert_eq!(bare.palette["base00"], Rgb { r: 0x1d, g: 0x20, b: 0x21 });
        let bare_c = scheme_for("bare-c", &CURRENT.replace(r##""#141414""##, "1d2021 # hard"));
        assert_eq!(bare_c.palette["base00"], Rgb { r: 0x1d, g: 0x20, b: 0x21 });
    }

    #[test]
    fn variant_follows_luma_not_metadata() {
        // Declared dark, but the surface is plainly light.
        let s = scheme_for(
            "inverted",
            &CURRENT.replace(r##"base00: "#141414""##, r##"base00: "#f5f5f5""##),
        );
        assert!(!s.is_dark, "luma should override the declared variant");
    }

    #[test]
    fn mix_matches_the_hand_computed_diff_backgrounds() {
        let s = scheme_for("minimal-dark", CURRENT);
        let bg = s.palette["base00"];
        // The values AGENTS.md tabulates, computed by hand for delta.
        assert_eq!(mix(s.palette["base08"], bg, 15.0).hex(), "341919");
        assert_eq!(mix(s.palette["base08"], bg, 30.0).hex(), "541e1e");
        assert_eq!(mix(s.palette["base0B"], bg, 15.0).hex(), "242f28");
        assert_eq!(mix(s.palette["base0B"], bg, 30.0).hex(), "344a3b");
    }

    #[test]
    fn mix_endpoints_are_exact() {
        let s = scheme_for("minimal-dark", CURRENT);
        let (a, b) = (s.palette["base08"], s.palette["base00"]);
        assert_eq!(mix(a, b, 100.0), a);
        assert_eq!(mix(a, b, 0.0), b);
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
    fn missing_slots_are_rejected() {
        let p = temp_dir().join("partial.yaml");
        fs::write(&p, "palette:\n  base00: \"#000000\"\n").unwrap();
        let err = match Scheme::load(&p) {
            Ok(_) => panic!("expected failure"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("missing 15 of 16 slots"), "{err}");
    }

    #[test]
    fn wrong_scheme_system_says_so() {
        // The upstream collection ships tinted8 schemes alongside base16 ones.
        // They have named 8-colour keys, so every slot is "missing" — the error
        // should name the system rather than imply a corrupt base16 file.
        let p = temp_dir().join("nord.yaml");
        fs::write(
            &p,
            "scheme:\n  system: \"tinted8\"\n  name: \"Nord\"\npalette:\n  black: \"#2e3440\"\n",
        )
        .unwrap();
        let err = match Scheme::load(&p) {
            Ok(_) => panic!("expected failure"),
            Err(e) => e,
        };
        let err = err.to_string();
        assert!(err.contains("tinted8"), "{err}");
        assert!(err.contains("no base16 slots"), "{err}");
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

    #[test]
    fn slug_comes_from_the_filename() {
        assert_eq!(slugify("Gruvbox Dark, Hard"), "gruvbox-dark-hard");
        assert_eq!(slugify("base16-rose-pine"), "base16-rose-pine");
    }

    #[test]
    fn uuid_is_stable_and_distinct() {
        let a = scheme_for("minimal-dark", CURRENT);
        assert_eq!(a.uuid(), a.uuid());
        assert_eq!(a.uuid().len(), 36);
        let b = scheme_for("gruvbox-dark-hard", LEGACY);
        assert_ne!(a.uuid(), b.uuid());
    }
}
