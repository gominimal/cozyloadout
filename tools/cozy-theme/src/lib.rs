//! Shared by the renderer and the wizard: the colour type and the base16
//! scheme parser.
//!
//! Split out of `main.rs` when the wizard needed to load schemes too. Anything
//! only the renderer uses — the template engine, the manifest, the build —
//! stays in the binary.

use color_eyre::eyre::{bail, eyre, Context, Result};
use fs_err as fs;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser as YamlParser};
use yaml_rust2::scanner::Marker;

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Accepts `#rrggbb` or `rrggbb`, any case. base16 schemes are always
    /// 6-digit; 3-digit shorthand is not part of the format.
    pub fn parse(s: &str) -> Result<Rgb> {
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

    pub fn hex(self) -> String {
        format!("{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// WCAG relative luminance. Only used to decide dark vs light.
    pub fn luminance(self) -> f64 {
        let ch = |v: u8| {
            let s = f64::from(v) / 255.0;
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
pub fn mix(fg: Rgb, bg: Rgb, pct: f64) -> Rgb {
    let f = pct / 100.0;
    // Rust's float-to-int `as` saturates (and maps NaN to 0) rather than
    // wrapping, and the value is rounded and clamped into 0..=255 before the
    // narrowing anyway. Both lints are about *unchecked* narrowing; this one is
    // checked twice over, so allow them here rather than crate-wide.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let c = |a: u8, b: u8| {
        let blended = (f64::from(b) + f * (f64::from(a) - f64::from(b))).round();
        blended.clamp(0.0, 255.0) as u8
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

pub const SLOTS: [&str; 16] = [
    "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08",
    "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
];

/// Namespace for the per-scheme theme UUIDs. Arbitrary but fixed: changing it
/// renumbers every generated .tmTheme, which is harmless (they are rebuilt) but
/// pointless churn. Generated once, for this tool.
const UUID_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6f, 0x0c, 0x24, 0x7e, 0x4b, 0x1a, 0x4d, 0x8e, 0x9c, 0x3f, 0xa1, 0x52, 0x7d, 0x88, 0xe0, 0x14,
]);

pub struct Scheme {
    pub slug: String,
    pub name: String,
    pub author: String,
    pub is_dark: bool,
    pub palette: BTreeMap<String, Rgb>,
}

/// One open collection while walking the event stream. A mapping remembers the
/// key it is waiting on; a sequence never takes keys.
enum Frame {
    Map(Option<String>),
    Seq,
}

/// Collects `baseXX` slots and scalar metadata from a scheme, at any depth —
/// nesting is ignored, which is what lets one code path cover both the current
/// (`palette:` block) and legacy (top-level `base00:`) formats.
///
/// This reads the parser's **event stream** rather than a loaded `Yaml` tree,
/// and that is the whole point. The tree applies YAML's implicit typing, which
/// destroys colours: a legacy scheme's unquoted `base01: 073642` becomes the
/// integer 73642 and loses its leading zero, `000000` becomes 0, and `1e2021`
/// becomes a float. `Event::Scalar` hands back the original lexeme, which is
/// the only thing a hex colour can be read from.
#[derive(Default)]
pub struct SchemeSink {
    meta: BTreeMap<String, String>,
    pub palette: BTreeMap<String, Rgb>,
    stack: Vec<Frame>,
    /// First colour that failed to parse. Kept rather than returned because
    /// the receiver trait cannot fail; `Scheme::load` reports it.
    bad: Option<(String, String)>,
}

impl SchemeSink {
    /// A key/value pair, with `value` exactly as it was written.
    fn record(&mut self, key: &str, value: &str) {
        let lower = key.to_ascii_lowercase();
        if lower.len() == 6 && lower.starts_with("base") {
            // Normalise base0a -> base0A so templates have one spelling.
            let canon = format!("base{}", key[4..].to_ascii_uppercase());
            if SLOTS.contains(&canon.as_str()) {
                match Rgb::parse(value) {
                    Ok(rgb) => {
                        self.palette.insert(canon, rgb);
                    }
                    Err(e) => {
                        if self.bad.is_none() {
                            self.bad = Some((canon, e.to_string()));
                        }
                    }
                }
            }
        } else if !value.is_empty() {
            self.meta.entry(lower).or_insert_with(|| value.to_string());
        }
    }

    /// A nested collection stands where a value would; drop the pending key.
    fn take_key(&mut self) {
        if let Some(Frame::Map(slot)) = self.stack.last_mut() {
            *slot = None;
        }
    }
}

impl MarkedEventReceiver for SchemeSink {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        match ev {
            Event::MappingStart(..) => {
                self.take_key();
                self.stack.push(Frame::Map(None));
            }
            Event::SequenceStart(..) => {
                self.take_key();
                self.stack.push(Frame::Seq);
            }
            Event::MappingEnd | Event::SequenceEnd => {
                self.stack.pop();
            }
            Event::Scalar(text, ..) => match self.stack.last_mut() {
                Some(Frame::Map(slot @ None)) => *slot = Some(text),
                Some(Frame::Map(slot)) => {
                    let key = slot.take().unwrap_or_default();
                    self.record(&key, &text);
                }
                Some(Frame::Seq) | None => {}
            },
            _ => {}
        }
    }
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
    pub fn load(path: &Path) -> Result<Scheme> {
        let text = fs::read_to_string(path)?;
        let mut sink = SchemeSink::default();
        YamlParser::new_from_str(&text)
            .load(&mut sink, true)
            .wrap_err_with(|| format!("{}: not valid YAML", path.display()))?;
        if let Some((slot, why)) = sink.bad {
            bail!("{}: {slot}: {why}", path.display());
        }
        let (meta, palette) = (sink.meta, sink.palette);

        let missing: Vec<&str> = SLOTS
            .iter()
            .copied()
            .filter(|s| !palette.contains_key(*s))
            .collect();
        if missing.len() == SLOTS.len() {
            // Most likely a tinted8 scheme (named 8-colour keys) or not a
            // scheme at all — saying "missing all 16" for those reads as a
            // corrupt base16 file rather than the wrong kind of file.
            let system = meta.get("system").map_or("unknown", String::as_str);
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
            author: meta
                .get("author")
                .cloned()
                .unwrap_or_else(|| "unknown".into()),
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
    pub fn uuid(&self) -> String {
        Uuid::new_v5(&UUID_NAMESPACE, self.slug.as_bytes()).to_string()
    }

    pub fn variant(&self) -> &'static str {
        if self.is_dark {
            "dark"
        } else {
            "light"
        }
    }

    /// Every placeholder a template may reference.
    ///
    /// Names are `snake_case` because minijinja parses `base00-hex` as a
    /// subtraction. The `-hex-r/g/b` and `-dec-r/g/b` families that used to be
    /// emitted here are gone: they existed only so upstream tinted-builder
    /// templates would drop in, and moving to Jinja syntax ended that anyway.
    pub fn vars(&self) -> BTreeMap<String, String> {
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
// Discovery
// ---------------------------------------------------------------------------

/// One scheme on disk, before it is parsed. Listing is cheap and parsing is
/// not, so the wizard shows hundreds of these and only loads the selected one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SchemeEntry {
    /// Filename stem — what `just theme` accepts.
    pub name: String,
    /// Where it came from, for the list: "cozy", "base16", "base24".
    pub source: String,
    pub path: PathBuf,
}

/// Find every scheme the loadout can be built from.
///
/// The search order matches `_resolve` in the justfile: this repo's own
/// schemes first, then vendored base16, then base24, with earlier entries
/// winning a name collision. `tinted8` is deliberately unreachable — it is an
/// 8-colour system the loadout cannot use.
pub fn discover(schemes_dir: &Path) -> Vec<SchemeEntry> {
    let sources = [
        (schemes_dir.to_path_buf(), "cozy"),
        (schemes_dir.join("vendor/base16"), "base16"),
        (schemes_dir.join("vendor/base24"), "base24"),
    ];
    let mut seen = BTreeMap::new();
    for (dir, source) in sources {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<SchemeEntry> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let ext = path.extension()?.to_str()?;
                if ext != "yaml" && ext != "yml" {
                    return None;
                }
                Some(SchemeEntry {
                    name: path.file_stem()?.to_str()?.to_string(),
                    source: source.to_string(),
                    path,
                })
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in found {
            seen.entry(entry.name.clone()).or_insert(entry);
        }
    }
    seen.into_values().collect()
}

// ---------------------------------------------------------------------------
// Packages
// ---------------------------------------------------------------------------

/// A group of packages that is always installed.
#[derive(Deserialize)]
pub struct PackageGroup {
    pub title: String,
    pub about: String,
    pub packages: Vec<String>,
}

/// A package the user can decline.
#[derive(Deserialize)]
pub struct OptionalPackage {
    pub name: String,
    pub about: String,
    /// Whether the wizard preselects it.
    pub default: bool,
}

/// `templates/packages.toml`: where the loadout's package list comes from.
///
/// The generated `cozy.toml` needs a flat array, because that is minimal's
/// schema. Keeping the split here means the wizard can offer the optional
/// third without a second list to keep in step.
#[derive(Deserialize)]
pub struct Packages {
    pub base: PackageGroup,
    pub cozy: PackageGroup,
    pub optional: Vec<OptionalPackage>,
}

impl Packages {
    pub fn load(path: &Path) -> Result<Packages> {
        let text = fs::read_to_string(path)?;
        toml::from_str(&text).wrap_err_with(|| path.display().to_string())
    }

    /// Every package that will be installed, given the optional ones to keep.
    /// Sorted and de-duplicated so the generated list is stable whatever order
    /// the groups are written in.
    pub fn selected<'a>(&'a self, keep: &dyn Fn(&OptionalPackage) -> bool) -> Vec<&'a str> {
        let mut names: Vec<&str> = self
            .base
            .packages
            .iter()
            .chain(&self.cozy.packages)
            .map(String::as_str)
            .chain(
                self.optional
                    .iter()
                    .filter(|o| keep(o))
                    .map(|o| o.name.as_str()),
            )
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Everything, as the plain build installs it.
    pub fn all(&self) -> Vec<&str> {
        self.selected(&|_| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

    // The pre-2022 layout: no `palette:` block, no `#`, lowercase slot letters.
    const LEGACY: &str = r#"
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
"#;

    fn packages() -> Packages {
        Packages::load(Path::new("../../templates/packages.toml")).expect("packages.toml")
    }

    #[test]
    fn every_package_appears_exactly_once() {
        // Three hand-maintained groups; the same name in two of them would
        // install fine and quietly mean nobody knows which group owns it.
        let p = packages();
        let mut all: Vec<&str> = p
            .base
            .packages
            .iter()
            .chain(&p.cozy.packages)
            .map(String::as_str)
            .chain(p.optional.iter().map(|o| o.name.as_str()))
            .collect();
        let total = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(
            all.len(),
            total,
            "a package is listed in more than one group"
        );
    }

    #[test]
    fn manifest_package_tags_name_real_optional_packages() {
        // A manifest entry tagged with a package that does not exist, or that
        // is not optional, would silently never be skipped — the tag would look
        // like it was doing something and do nothing.
        let manifest = include_str!("../../../templates/manifest.toml");
        let p = packages();
        let optional: Vec<&str> = p.optional.iter().map(|o| o.name.as_str()).collect();
        let mut tagged = 0;
        for line in manifest.lines() {
            let Some(rest) = line.trim().strip_prefix("package") else {
                continue;
            };
            let name = rest.trim_start_matches([' ', '=']).trim().trim_matches('"');
            tagged += 1;
            assert!(
                optional.contains(&name),
                "manifest tags a config with {name:?}, which is not an optional package"
            );
        }
        assert!(
            tagged > 0,
            "no manifest entry is tagged; the skip is dead code"
        );
    }

    #[test]
    fn every_optional_themed_tool_tags_its_config() {
        // The other direction: an optional package whose config is *not*
        // tagged would install its config even when declined.
        let manifest = include_str!("../../../templates/manifest.toml");
        for o in packages().optional {
            // Only tools with a config in the tree are relevant.
            if !manifest.contains(&format!("template = \"{}/", o.name)) {
                continue;
            }
            assert!(
                manifest.contains(&format!("package  = \"{}\"", o.name)),
                "{} is optional and themed, but its manifest entry is not tagged \
                 — declining it would install a config for a missing binary",
                o.name
            );
        }
    }

    #[test]
    fn optional_packages_are_described() {
        // The wizard shows these one per row; a blank line is a bug the user
        // sees rather than a lint.
        for o in packages().optional {
            assert!(!o.about.trim().is_empty(), "{} has no description", o.name);
            assert!(
                o.about.trim_end().ends_with('.'),
                "{}: description should read as a sentence, got {:?}",
                o.name,
                o.about
            );
        }
    }

    #[test]
    fn declining_everything_still_leaves_a_working_session() {
        let p = packages();
        let bare = p.selected(&|_| false);
        for must in ["fish", "coreutils", "git", "helix", "zellij"] {
            assert!(
                bare.contains(&must),
                "{must} must survive declining every extra"
            );
        }
        assert!(
            !bare.contains(&"kittyview"),
            "an optional package leaked into the required set"
        );
        assert!(p.all().len() > bare.len(), "optional packages add nothing?");
    }

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
        assert_eq!(
            s.palette["base00"],
            Rgb {
                r: 0x14,
                g: 0x14,
                b: 0x14
            }
        );

        // A legacy scheme's bare, unquoted value, with and without a comment.
        let bare = scheme_for("bare", &CURRENT.replace(r##""#141414""##, "1d2021"));
        assert_eq!(
            bare.palette["base00"],
            Rgb {
                r: 0x1d,
                g: 0x20,
                b: 0x21
            }
        );
        let bare_c = scheme_for(
            "bare-c",
            &CURRENT.replace(r##""#141414""##, "1d2021 # hard"),
        );
        assert_eq!(
            bare_c.palette["base00"],
            Rgb {
                r: 0x1d,
                g: 0x20,
                b: 0x21
            }
        );
    }

    #[test]
    fn legacy_hex_that_looks_numeric_survives() {
        // Regression: reading colours off a *loaded* YAML tree applies implicit
        // typing, so an unquoted legacy value like `073642` arrives as the
        // integer 73642 with its leading zero gone, `000000` as 0, and `1e2021`
        // as a float. Solarized and any pure-black scheme hit this. The parser
        // reads raw scalar events precisely so it cannot.
        for (raw, want) in [
            (
                "073642",
                Rgb {
                    r: 0x07,
                    g: 0x36,
                    b: 0x42,
                },
            ),
            ("000000", Rgb { r: 0, g: 0, b: 0 }),
            (
                "1e2021",
                Rgb {
                    r: 0x1e,
                    g: 0x20,
                    b: 0x21,
                },
            ),
            (
                "123456",
                Rgb {
                    r: 0x12,
                    g: 0x34,
                    b: 0x56,
                },
            ),
        ] {
            let s = scheme_for(&format!("numeric-{raw}"), &LEGACY.replace("1d2021", raw));
            assert_eq!(s.palette["base00"], want, "base00 from unquoted {raw}");
        }
    }

    #[test]
    fn short_hex_is_still_rejected() {
        // The zero-padding shortcut would have accepted this as `012345`.
        let p = temp_dir().join("short.yaml");
        fs::write(&p, LEGACY.replace("1d2021", "12345")).unwrap();
        let Err(err) = Scheme::load(&p) else {
            panic!("expected failure")
        };
        assert!(
            err.to_string().contains("not a 6-digit hex colour"),
            "{err}"
        );
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
    fn missing_slots_are_rejected() {
        let p = temp_dir().join("partial.yaml");
        fs::write(&p, "palette:\n  base00: \"#000000\"\n").unwrap();
        let Err(err) = Scheme::load(&p) else {
            panic!("expected failure")
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
        let Err(err) = Scheme::load(&p) else {
            panic!("expected failure")
        };
        let err = err.to_string();
        assert!(err.contains("tinted8"), "{err}");
        assert!(err.contains("no base16 slots"), "{err}");
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
