//! Shared by the renderer and the wizard: the colour type and the base16
//! scheme parser.
//!
//! Split out of `main.rs` when the wizard needed to load schemes too. Anything
//! only the renderer uses — the template engine, the manifest, the build —
//! stays in the binary.

use color_eyre::eyre::{bail, eyre, Context, Result};
use fs_err as fs;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser as YamlParser};
use yaml_rust2::scanner::Marker;

/// Whether a typed token looks like a package name.
///
/// Deliberately conservative: lowercase letters, digits, and the punctuation
/// that appears in real registry names (`ca-certificates`, `procps-ng`,
/// `libstdc++`). Anything else is a typo, a shell fragment, or a paste
/// accident, and the page shows what it accepted so a rejection is visible.
pub fn is_package_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.+".contains(c))
        && s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
}

pub mod settings;
pub use settings::Settings;

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

/// WCAG contrast ratio between two colours, `1.0`..`21.0`.
///
/// The readable form of the same luminance the variant is decided from. 4.5 is
/// the AA threshold for body text, which is the number the adjustment screen
/// holds itself to.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (hi, lo) = {
        let (x, y) = (a.luminance(), b.luminance());
        if x >= y {
            (x, y)
        } else {
            (y, x)
        }
    };
    (hi + 0.05) / (lo + 0.05)
}

/// Per-channel luma of a gamma-encoded colour, used as the grey a saturation
/// adjustment pivots around.
///
/// Deliberately *not* [`Rgb::luminance`]: that linearises first, which is right
/// for judging contrast and wrong for pulling a colour toward its own grey —
/// linear-light desaturation darkens midtones visibly. Image editors pivot on
/// the gamma-encoded luma, and so does this.
fn luma8(c: Rgb) -> f64 {
    0.2126 * f64::from(c.r) + 0.7152 * f64::from(c.g) + 0.0722 * f64::from(c.b)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn clamp8(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// Scale each channel away from (or toward) a pivot channel.
fn spread(c: Rgb, pivot: Rgb, k: f64) -> Rgb {
    let ch = |v: u8, p: u8| clamp8(f64::from(p) + (f64::from(v) - f64::from(p)) * k);
    Rgb {
        r: ch(c.r, pivot.r),
        g: ch(c.g, pivot.g),
        b: ch(c.b, pivot.b),
    }
}

// ---------------------------------------------------------------------------
// Adjustments
// ---------------------------------------------------------------------------

/// Six adjustments applied to a scheme's sixteen slots.
///
/// Every field is a percentage in `-100..=100`, and **all-zero is the identity**
/// — an unadjusted scheme renders byte-for-byte what it always did. That is the
/// property `an_untouched_adjustment_changes_nothing` holds, and it is what
/// lets these be plumbed through the renderer unconditionally.
///
/// These are deliberately not generic image filters. A scheme is sixteen slots
/// with assigned meaning — base00–03 surface, base04–07 foreground, base08–0F
/// accents — so each control acts on the slots it is *about* and leaves the
/// rest alone. That is also why there is no hue rotation: base08 is red because
/// error messages are red, and turning it green would be wrong rather than
/// merely ugly.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Adjust {
    /// Pushes the surface and foreground ramps apart around their midpoint.
    pub contrast: i8,
    /// Pulls the eight accents toward or away from their own grey. Leaves the
    /// greyscale ramp alone, where saturation has nothing to act on.
    pub saturation: i8,
    /// Moves base03 — comments — toward the foreground or back into the
    /// background. The slot schemes most often make unreadable.
    pub comments: i8,
    /// Spreads base01 and base02 apart, or collapses them together. Two slots
    /// many schemes leave nearly identical, which is what makes a selection
    /// invisible against a surface.
    pub separation: i8,
    /// Deepens the background away from the rest of the scheme, or lifts it
    /// toward the surface. base01–03 follow at a decaying rate so the ramp
    /// keeps its shape instead of collapsing onto the new floor.
    pub background: i8,
    /// A warm or cool cast over every slot.
    pub warmth: i8,
}

/// How far `background` may push base00, as a fraction of the distance to its
/// target. A full slider that reached the target outright would erase the
/// background into either pure black or the surface above it.
const BACKGROUND_REACH: f64 = 0.6;

/// The same, for `separation`: base01 and base02 slide along the ramp rather
/// than all the way onto their neighbours.
const SEPARATION_REACH: f64 = 0.5;

/// Channel shift at full `warmth`, out of 255. Enough to read as a cast,
/// not enough to recolour anything.
const WARMTH_REACH: f64 = 24.0;

impl Adjust {
    /// Whether this would change anything.
    pub fn is_identity(self) -> bool {
        self == Self::default()
    }

    /// A short stable token naming this adjustment, for the slug.
    ///
    /// Generated theme files are named after the scheme — `bat/themes/<slug>.tmTheme`,
    /// `zellij/themes/<slug>.kdl` — and the .tmTheme UUID is derived from the
    /// slug too, which Sublime keys themes by. Two different adjustments of one
    /// scheme therefore have to be two different slugs, or the second silently
    /// overwrites the first and inherits its UUID.
    ///
    /// FNV-1a over the six values: it only has to be deterministic and short,
    /// and it is never parsed back.
    pub fn token(self) -> String {
        let mut h: u32 = 0x811c_9dc5;
        for b in [
            self.contrast,
            self.saturation,
            self.comments,
            self.separation,
            self.background,
            self.warmth,
        ] {
            h ^= u32::from(b.to_le_bytes()[0]);
            h = h.wrapping_mul(0x0100_0193);
        }
        format!("{:04x}", h & 0xffff)
    }
}

impl Scheme {
    /// This scheme with the adjustments applied.
    ///
    /// The order is deliberate: the global control runs first and the targeted
    /// ones override it, so "lift the comments" is not silently undone by a
    /// contrast change, and warmth is a cast over the finished result.
    ///
    /// **`is_dark` is carried over, never recomputed.** It is derived by
    /// comparing background and foreground luminance, and it selects
    /// `scheme_variant`, which decides `duf --theme` and every `{% if dark %}`
    /// branch in the templates. An adjustment that nudged a scheme across that
    /// line would silently rewrite unrelated config, so the variant is the
    /// unadjusted scheme's answer and stays that way.
    #[must_use]
    pub fn adjusted(&self, knobs: Adjust) -> Scheme {
        let mut out = self.palette.clone();
        let get = |pal: &BTreeMap<String, Rgb>, k: &str| {
            pal.get(k).copied().unwrap_or(Rgb { r: 0, g: 0, b: 0 })
        };
        let pct = |v: i8| f64::from(v) / 100.0;

        // Contrast: everything moves away from the midpoint of the greyscale
        // ramp's two ends, which keeps the scheme's colour cast rather than
        // pivoting on a neutral grey it never contained.
        if knobs.contrast != 0 {
            let pivot = mix(get(&out, "base00"), get(&out, "base07"), 50.0);
            let k = 1.0 + pct(knobs.contrast);
            for slot in SLOTS {
                let c = get(&out, slot);
                out.insert(slot.to_string(), spread(c, pivot, k));
            }
        }

        // Background: base00 toward the extreme end (deeper) or the surface
        // above it (lifted), with base01–03 following at a halving rate.
        if knobs.background != 0 {
            let extreme = if self.is_dark {
                Rgb { r: 0, g: 0, b: 0 }
            } else {
                Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                }
            };
            let target = if knobs.background < 0 {
                extreme
            } else {
                get(&out, "base01")
            };
            let reach = pct(knobs.background).abs() * BACKGROUND_REACH * 100.0;
            for (i, slot) in ["base00", "base01", "base02", "base03"].iter().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                let follow = reach / f64::from(1_u32 << u32::try_from(i).unwrap_or(0));
                let c = get(&out, slot);
                out.insert((*slot).to_string(), mix(target, c, follow));
            }
        }

        // Separation: base01 slides toward the background and base02 toward the
        // foreground, or the two converge on each other.
        if knobs.separation != 0 {
            let reach = pct(knobs.separation).abs() * SEPARATION_REACH * 100.0;
            let (t1, t2) = if knobs.separation > 0 {
                (get(&out, "base00"), get(&out, "base04"))
            } else {
                (get(&out, "base02"), get(&out, "base01"))
            };
            let (c1, c2) = (get(&out, "base01"), get(&out, "base02"));
            out.insert("base01".into(), mix(t1, c1, reach));
            out.insert("base02".into(), mix(t2, c2, reach));
        }

        // Comments: base03 toward the foreground, or back into the background.
        if knobs.comments != 0 {
            let target = if knobs.comments > 0 {
                get(&out, "base05")
            } else {
                get(&out, "base00")
            };
            let c = get(&out, "base03");
            out.insert(
                "base03".into(),
                mix(target, c, pct(knobs.comments).abs() * 100.0),
            );
        }

        // Saturation: the accents only, pivoting on each colour's own grey.
        if knobs.saturation != 0 {
            let k = 1.0 + pct(knobs.saturation);
            for slot in &SLOTS[8..] {
                let c = get(&out, slot);
                let g = clamp8(luma8(c));
                let grey = Rgb { r: g, g, b: g };
                out.insert((*slot).to_string(), spread(c, grey, k));
            }
        }

        // Warmth: a cast over everything, red and blue in opposition.
        if knobs.warmth != 0 {
            let shift = pct(knobs.warmth) * WARMTH_REACH;
            for slot in SLOTS {
                let c = get(&out, slot);
                out.insert(
                    slot.to_string(),
                    Rgb {
                        r: clamp8(f64::from(c.r) + shift),
                        g: c.g,
                        b: clamp8(f64::from(c.b) - shift),
                    },
                );
            }
        }

        let (slug, name) = if knobs.is_identity() {
            (self.slug.clone(), self.name.clone())
        } else {
            (
                format!("{}-{}", self.slug, knobs.token()),
                format!("{} (adjusted)", self.name),
            )
        };
        Scheme {
            slug,
            name,
            author: self.author.clone(),
            is_dark: self.is_dark,
            palette: out,
        }
    }

    /// This scheme as a base16 YAML file, in the current `palette:` format.
    ///
    /// Round-trips: `Scheme::load` of this output has the same palette. That is
    /// what makes "save as" produce a *scheme* rather than an export — it lands
    /// in `schemes/`, `discover` finds it, and `just theme <name>` takes it like
    /// any other.
    ///
    /// `derived_from` names the scheme this was adapted from, when it was. The
    /// original author stays in `author:` — the palette is derived from their
    /// work — and the provenance goes in a comment above it, where it cannot be
    /// mistaken for a claim about who made this.
    #[must_use]
    pub fn to_yaml(&self, derived_from: Option<&str>) -> String {
        let mut out = String::new();
        out.push_str("# Saved by the cozy wizard.\n");
        if let Some(from) = derived_from {
            let _ = writeln!(out, "# {from}");
        }
        out.push_str("#\n# Edit it by hand or re-open it in `just wizard`.\n\n");
        let _ = writeln!(out, "system: \"base16\"");
        let _ = writeln!(out, "name: {}", yaml_string(&self.name));
        let _ = writeln!(out, "author: {}", yaml_string(&self.author));
        // Advisory only — `load` decides the variant from the palette's own
        // luma — but the format carries it and a reader expects to see it.
        let _ = writeln!(out, "variant: \"{}\"", self.variant());
        out.push_str("palette:\n");
        for slot in SLOTS {
            if let Some(c) = self.palette.get(slot) {
                let _ = writeln!(out, "  {slot}: \"#{}\"", c.hex());
            }
        }
        out
    }

    /// Write this scheme into `dir` as `<slug>.yaml`, under a name of the
    /// user's choosing.
    ///
    /// Refuses to overwrite. A scheme file is the only copy of a palette
    /// somebody tuned by hand, and "save as" is not a place to discover that
    /// the name was taken — the caller reports the clash and asks again.
    ///
    /// # Errors
    ///
    /// If the name does not slugify to anything, if a file of that name already
    /// exists, or if the write fails.
    pub fn save_as(
        &self,
        dir: &Path,
        display_name: &str,
        derived_from: Option<&str>,
    ) -> Result<PathBuf> {
        let slug = slugify(display_name);
        if slug.is_empty() {
            bail!("{display_name:?} has no letters or digits to make a filename from");
        }
        let path = dir.join(format!("{slug}.yaml"));
        if path.exists() {
            bail!("{slug}.yaml already exists — pick another name");
        }
        let named = Scheme {
            slug: slug.clone(),
            name: display_name.trim().to_string(),
            author: self.author.clone(),
            is_dark: self.is_dark,
            palette: self.palette.clone(),
        };
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        fs::write(&path, named.to_yaml(derived_from))
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    /// Render one template from `templates` with this scheme.
    ///
    /// The same environment and the same variables `build` uses, so what comes
    /// back is byte-for-byte what a full render would write. That is the point:
    /// the wizard lights its preview with the `.tmTheme` this produces, so the
    /// preview is coloured exactly as `bat` will colour the same file.
    ///
    /// # Errors
    ///
    /// If the template cannot be read or does not render.
    pub fn render_template(&self, templates: &Path, rel: &str) -> Result<String> {
        let path = templates.join(rel);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let env = environment(self);
        render(&env, &path.display().to_string(), &text, &self.vars(), self)
    }

    /// The scheme's own body-text contrast: base05 on base00.
    pub fn body_contrast(&self) -> f64 {
        let get = |k: &str| {
            self.palette
                .get(k)
                .copied()
                .unwrap_or(Rgb { r: 0, g: 0, b: 0 })
        };
        contrast_ratio(get("base05"), get("base00"))
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

/// A double-quoted YAML scalar. Scheme names come from a text field, so the
/// quote and backslash cases are reachable rather than theoretical.
fn yaml_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub fn slugify(s: &str) -> String {
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

/// The user's config directory for this tool, `~/.config/cozy` by default.
///
/// `$XDG_CONFIG_HOME` first when it is absolute, matching the spec's rule that
/// a relative value is invalid and ignored — and matching how minimal resolves
/// its own. `cozy`, not `minimal`: this is the loadout's own tool, and putting
/// files under `minimal/` would be taking a namespace that is not ours.
#[must_use]
pub fn config_dir(home: &Path) -> PathBuf {
    config_home(home).join("cozy")
}

/// `$XDG_CONFIG_HOME` when absolute, otherwise `<home>/.config`.
#[must_use]
pub fn config_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
}

/// Where schemes saved from the wizard go.
///
/// Not the repository's `schemes/`, which is checked in: a scheme you tuned is
/// yours, it should survive re-cloning the repo, and it should be there for
/// every checkout rather than the one you happened to save it from.
#[must_use]
pub fn user_schemes_dir(home: &Path) -> PathBuf {
    config_dir(home).join("schemes")
}

/// Where the wizard remembers its answers.
#[must_use]
pub fn user_settings_path(home: &Path) -> PathBuf {
    config_dir(home).join("settings.toml")
}

/// Find every scheme the loadout can be built from.
///
/// The search order matches `_resolve` in the justfile: this repo's own
/// schemes first, then vendored base16, then base24, with earlier entries
/// winning a name collision. `tinted8` is deliberately unreachable — it is an
/// 8-colour system the loadout cannot use.
pub fn discover(schemes_dir: &Path, user: Option<&Path>) -> Vec<SchemeEntry> {
    // Most specific first: a scheme you saved wins over one checked into the
    // repository, which wins over a vendored one. The same rule `schemes/`
    // already had over `vendor/`, extended one step outward.
    let sources = [
        (user.map(Path::to_path_buf), "mine"),
        (Some(schemes_dir.to_path_buf()), "cozy"),
        (Some(schemes_dir.join("vendor/base16")), "base16"),
        (Some(schemes_dir.join("vendor/base24")), "base24"),
    ];
    let mut seen = BTreeMap::new();
    for (dir, source) in sources {
        let Some(dir) = dir else { continue };
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
    /// SPDX identifier, copied from the package's `license_spdx` in the Minimal
    /// Public Registry. Shown before the user agrees to install anything, which
    /// is the point of carrying it: `claude-code` is proprietary and the rest
    /// are not, and that is worth knowing at the moment of choosing.
    pub license: String,
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
    /// Defaulted: a file with everything required is a legitimate
    /// configuration, and it should parse rather than fail on a missing key.
    #[serde(default)]
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

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Everything a render needs.
///
/// Plain data rather than the CLI's `Args`, because the wizard builds one of
/// these directly. Both front ends call the same `build`, so there is one
/// definition of what rendering means and no command line in the middle of it.
#[derive(Clone)]
pub struct Options {
    pub scheme: PathBuf,
    pub templates: PathBuf,
    pub out: PathBuf,
    pub loadout: String,
    /// `"blocks"` or `"legacy"`.
    pub greeting: String,
    /// Optional packages to install. Empty means *all of them*, which is what
    /// a plain `just theme` has always produced; `Some(vec![])` cannot be
    /// spelled here, so callers that mean "none" pass a single empty string.
    pub with: Vec<String>,
    pub patch_files: Vec<PathBuf>,
    pub patch_dirs: Vec<PathBuf>,
    /// The host home, which patch destinations are computed relative to.
    /// Defaults to `$HOME`; set explicitly so tests do not depend on the
    /// machine they run on.
    pub home: PathBuf,
    /// Destinations typed by hand, keyed by the source path they belong to.
    /// Anything absent takes the computed default.
    pub patch_dests: BTreeMap<PathBuf, String>,
    /// Adjustments applied to the scheme before anything is rendered.
    /// `Adjust::default()` is the identity, so this is free to be unconditional.
    pub adjust: Adjust,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            scheme: PathBuf::new(),
            templates: PathBuf::from("templates"),
            out: PathBuf::from("build"),
            loadout: "cozy".to_string(),
            greeting: "blocks".to_string(),
            with: Vec::new(),
            patch_files: Vec::new(),
            patch_dirs: Vec::new(),
            home: std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from),
            patch_dests: BTreeMap::new(),
            adjust: Adjust::default(),
        }
    }
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
    /// The package this config belongs to, when that package is optional.
    /// Declining the package drops the config with it — a config installed for
    /// a binary that is not there is clutter, and it is why being themed is no
    /// reason for a package to be mandatory.
    package: Option<String>,
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

/// Which packages to install.
///
/// No `--with` at all means everything, which is what a plain `just theme` has
/// always produced and what the loadout has always shipped. An explicit
/// `--with` narrows it — including `--with ""`, the wizard's way of saying
/// "none of the optional ones".
fn resolve_packages<'a>(with: &'a [String], packages: &'a Packages) -> Vec<&'a str> {
    if with.is_empty() {
        return packages.all();
    }
    let asked: Vec<&str> = with.iter().map(String::as_str).collect();
    let mut names = packages.selected(&|o| asked.contains(&o.name.as_str()));
    // Names that are not in packages.toml at all — whatever the wizard's
    // free-text field collected — pass straight through to minimal.
    for extra in &asked {
        if !extra.is_empty() && !names.contains(extra) {
            names.push(extra);
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// One patch the user asked for, already mapped to a session destination.
pub struct UserPatch {
    /// Where it lands, relative to the session's home. Ends in `/` for a
    /// directory, which is how minimal spells "and everything under it".
    pub dest: String,
    /// The host path or glob it comes from.
    pub source: String,
}

/// Where a host path lands inside the session.
///
/// `dest` is interpreted relative to the session user's home, so a path under
/// the host's home keeps its shape: `~/.config/helix` becomes `.config/helix`
/// and lands where helix will actually look. Taking only the last component —
/// which this used to do — dropped everything at the home root, so a picked
/// `~/.config/starship.toml` arrived as `~/starship.toml` and starship never
/// saw it.
///
/// A path outside the home has no home-relative form, so the leading `/` is
/// dropped instead: `/etc/hosts` becomes `etc/hosts`. Keeping the whole path
/// is also what stops two files with the same basename colliding on one dest.
pub fn patch_dest(path: &Path, home: &Path, is_dir: bool) -> String {
    let rel = path.strip_prefix(home).map_or_else(
        |_| {
            // Not under the home directory: strip the root instead.
            path.components()
                .filter(|c| {
                    !matches!(
                        c,
                        std::path::Component::RootDir | std::path::Component::Prefix(_)
                    )
                })
                .collect::<PathBuf>()
        },
        Path::to_path_buf,
    );
    let mut dest = rel.to_string_lossy().into_owned();
    if is_dir && !dest.is_empty() && !dest.ends_with('/') {
        dest.push('/');
    }
    dest
}

/// Map the user's picks to patch entries.
pub fn user_patches(
    files: &[PathBuf],
    dirs: &[PathBuf],
    home: &Path,
    overrides: &BTreeMap<PathBuf, String>,
) -> Vec<UserPatch> {
    let dest_for = |p: &PathBuf, is_dir: bool| {
        overrides
            .get(p)
            .map(|d| clean_dest(d, is_dir))
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| patch_dest(p, home, is_dir))
    };
    let mut out: Vec<UserPatch> = files
        .iter()
        .map(|p| UserPatch {
            dest: dest_for(p, false),
            source: p.display().to_string(),
        })
        .collect();
    out.extend(dirs.iter().map(|p| UserPatch {
        // A glob source with a directory dest: minimal appends each match's
        // path under the walk root to the dest, so the tree keeps its shape.
        dest: dest_for(p, true),
        source: format!("{}/**/*", p.display()),
    }));
    out
}

/// Force a hand-typed destination into the shape a loadout `dest` must have.
///
/// **A `dest` is always relative to the session user's home.** There is no way
/// to spell anything else, so `/etc/hosts` and `~/.config` and `.config` are
/// all the same request, and the leading `/` or `~/` is noise rather than a
/// different answer. Parent components are dropped for the same reason: `..`
/// cannot climb above the home it is relative to, so honouring it would be
/// inventing a meaning minimal does not have.
///
/// Directories keep a trailing `/`, which is what tells the composer the dest
/// is a directory to unpack a glob into rather than a file to write.
#[must_use]
pub fn clean_dest(dest: &str, is_dir: bool) -> String {
    let trimmed = dest.trim();
    let trimmed = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix('~'))
        .unwrap_or(trimmed);
    let mut parts: Vec<&str> = Vec::new();
    for part in trimmed.split('/') {
        match part {
            "" | "." | ".." => {}
            other => parts.push(other),
        }
    }
    let mut out = parts.join("/");
    if is_dir && !out.is_empty() {
        out.push('/');
    }
    out
}

/// Why a typed destination cannot be used, if it cannot.
///
/// Separate from [`clean_dest`], which silently repairs: the wizard wants to
/// *tell* someone that their `..` went nowhere rather than quietly dropping it.
///
/// # Errors
///
/// Returns a sentence naming the problem.
pub fn check_dest(dest: &str) -> Result<(), String> {
    let trimmed = dest.trim();
    if trimmed.is_empty() {
        return Err("a destination cannot be empty".into());
    }
    if clean_dest(trimmed, false).is_empty() {
        return Err(format!("{trimmed:?} leaves nothing to write to"));
    }
    if trimmed.split('/').any(|p| p == "..") {
        return Err("`..` cannot climb above the session's home".into());
    }
    if trimmed.starts_with('/') {
        return Err(
            "destinations are relative to the session's home, so the leading `/` is dropped".into(),
        );
    }
    // A leading `~/` is *accepted* and dropped: a dest is relative to the
    // session's home by definition, so `~/.config` is exactly what it looks
    // like and refusing it would be pedantry. A leading `/` is refused instead,
    // because it reads as an absolute path and cannot be one.
    Ok(())
}

/// Whether one of the user's patches already covers this destination.
///
/// An exact match, or anything beneath a directory patch — picking
/// `~/.config/helix` shadows the loadout's own `.config/helix/config.toml` and
/// its themes too. The user's file wins: they asked for theirs specifically,
/// and two sources for one destination is a composition minimal would refuse.
pub fn shadowed_by<'a>(dest: &str, user: &'a [UserPatch]) -> Option<&'a UserPatch> {
    user.iter().find(|u| {
        if u.dest.ends_with('/') {
            dest.starts_with(u.dest.as_str())
        } else {
            dest == u.dest
        }
    })
}

/// A patch the loadout itself would write, and the optional package it belongs
/// to. Exposed so the wizard can warn about the ones a user's own pick will
/// displace, before anything is generated.
pub struct LoadoutPatch {
    pub dest: String,
    pub package: Option<String>,
}

/// Every destination this loadout would write for a given scheme slug.
pub fn loadout_patches(templates: &Path, loadout: &str, slug: &str) -> Result<Vec<LoadoutPatch>> {
    let entries = parse_manifest(&templates.join("manifest.toml"))?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            let dest = e
                .dest?
                .replace("{slug}", slug)
                .replace("{loadout}", loadout);
            Some(LoadoutPatch {
                dest,
                package: e.package,
            })
        })
        .collect())
}

/// Returns the one-line summary rather than printing it: the wizard calls this
/// from inside the alternate screen, where a `println!` lands on the frame and
/// then vanishes with it. The caller decides where the line goes.
pub fn build(args: &Options) -> Result<String> {
    if !is_single_component(&args.loadout) {
        bail!(
            "--loadout {:?}: must be a single path component — no slashes, no `.` or `..`",
            args.loadout
        );
    }

    // Adjusted once, here, so everything downstream — the manifest, the slug
    // the theme files are named after, the .tmTheme UUID — sees one scheme and
    // cannot disagree about which.
    let scheme = Scheme::load(&args.scheme)?.adjusted(args.adjust);
    let entries = parse_manifest(&args.templates.join("manifest.toml"))?;
    let packages = Packages::load(&args.templates.join("packages.toml"))?;
    let selected = resolve_packages(&args.with, &packages);
    let mut vars = scheme.vars();
    vars.insert("loadout_name".into(), args.loadout.clone());
    vars.insert("greeting".into(), args.greeting.clone());
    // One quoted name per line, indented to sit inside the array in the
    // template. Same shape as `patches` below, and generated for the same
    // reason: the list has one home, and it is templates/packages.toml.
    vars.insert(
        "packages".into(),
        selected
            .iter()
            .map(|p| format!("  \"{p}\","))
            .collect::<Vec<_>>()
            .join("\n"),
    );

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
    // The user's own picks, mapped first: the loadout's own entries below check
    // against them, because a file someone chose explicitly should win over the
    // one this loadout would have shipped for the same destination.
    let user_picks = user_patches(
        &args.patch_files,
        &args.patch_dirs,
        &args.home,
        &args.patch_dests,
    );
    // Counted rather than derived from `entries.len()`: entries whose package
    // was declined are skipped, so the two numbers stopped agreeing.
    let mut written = 0usize;

    for e in &entries {
        // Skip configs whose package was declined.
        if let Some(pkg) = &e.package {
            if !selected.iter().any(|p| p == pkg) {
                continue;
            }
        }
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
        written += 1;

        if let Some(dest) = &e.dest {
            // Skipped rather than emitted alongside: two sources for one
            // destination is a composition minimal refuses, so writing both
            // would produce a loadout that cannot activate.
            if shadowed_by(&sub(dest), &user_picks).is_some() {
                continue;
            }
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

    for patch in &user_picks {
        let _ = writeln!(
            patches,
            "    {{ dest = \"{}\", source = \"{}\" }},",
            patch.dest, patch.source
        );
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

    Ok(format!(
        "{} ({}, {}) -> {}/  [{} files]",
        scheme.name,
        scheme.slug,
        scheme.variant(),
        args.out.display(),
        written + 1
    ))
}

// ---------------------------------------------------------------------------
// Installing
// ---------------------------------------------------------------------------

/// Where minimal looks for loadouts, under the given home directory.
pub fn loadouts_dir(home: &Path) -> PathBuf {
    home.join(".config/minimal/loadouts")
}

/// Copy a rendered loadout into minimal's loadouts directory.
///
/// A direct copy rather than zip-then-unzip. The zip is a distributable
/// artifact (`just bundle`), not a step installing needs, and going through it
/// meant shelling out to `unzip` — which is also where the stray recipe output
/// in the wizard came from.
///
/// Returns the directory it wrote.
pub fn install(built: &Path, loadout: &str, dest_root: &Path) -> Result<PathBuf> {
    let manifest = built.join(format!("{loadout}.toml"));
    let tree = built.join(loadout);
    if !manifest.is_file() || !tree.is_dir() {
        bail!("nothing built in {} — run a render first", built.display());
    }
    let dest = dest_root.join(loadout);

    // Delete this loadout's tree before writing over it. Copying alone
    // overwrites but never removes, so every scheme ever installed would leave
    // its <slug>.tmTheme, <slug>.toml, <slug>.kdl and <slug>.hjson behind
    // forever. Only this loadout's own generated tree goes — nothing else
    // under loadouts/ is touched, which is why the path is built from
    // `loadout` and not taken from a caller.
    fs::create_dir_all(dest_root)?;
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    copy_tree(&tree, &dest)?;
    fs::copy(&manifest, dest_root.join(format!("{loadout}.toml")))?;
    Ok(dest)
}

/// Recursive directory copy. Small and explicit rather than another
/// dependency: the tree is twenty files deep in one direction.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
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

    /// Write `text` to a private directory and parse it as a manifest.
    fn manifest_for(text: &str) -> Result<Vec<Entry>> {
        let p = temp_dir().join("manifest.toml");
        fs::write(&p, text).unwrap();
        parse_manifest(&p)
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

    /// A rendered-looking tree: a manifest and a directory beside it.
    fn built_tree(tag: &str) -> PathBuf {
        let root = temp_dir().join(format!("built-{tag}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("cozy/bat/themes")).unwrap();
        fs::write(root.join("cozy.toml"), "description = \"x\"\n").unwrap();
        fs::write(root.join("cozy/bat/themes/new.tmTheme"), "new").unwrap();
        fs::write(root.join("cozy/config"), "c").unwrap();
        root
    }

    #[test]
    fn installing_copies_the_manifest_and_the_tree() {
        let built = built_tree("copy");
        let dest_root = temp_dir().join("loadouts-copy");
        let dest = install(&built, "cozy", &dest_root).unwrap();
        assert_eq!(dest, dest_root.join("cozy"));
        assert!(
            dest_root.join("cozy.toml").is_file(),
            "the manifest goes beside the tree"
        );
        assert!(
            dest.join("bat/themes/new.tmTheme").is_file(),
            "nested files are copied"
        );
        assert!(dest.join("config").is_file());
        fs::remove_dir_all(&dest_root).unwrap();
    }

    #[test]
    fn installing_replaces_the_old_tree_rather_than_merging_into_it() {
        // `unzip -o` overwrote but never removed, so every scheme ever
        // installed left its theme files behind forever. A plain copy has the
        // same failure, which is why the old tree goes first.
        let built = built_tree("replace");
        let dest_root = temp_dir().join("loadouts-replace");
        let stale = dest_root.join("cozy/bat/themes/old.tmTheme");
        fs::create_dir_all(stale.parent().unwrap()).unwrap();
        fs::write(&stale, "old").unwrap();

        install(&built, "cozy", &dest_root).unwrap();
        assert!(
            !stale.exists(),
            "a previous scheme's theme file must not survive"
        );
        assert!(dest_root.join("cozy/bat/themes/new.tmTheme").is_file());
        fs::remove_dir_all(&dest_root).unwrap();
    }

    #[test]
    fn installing_touches_nothing_but_this_loadout() {
        // The destination path is built from `loadout`, never taken from a
        // caller, so a sibling loadout cannot be caught in the delete.
        let built = built_tree("sibling");
        let dest_root = temp_dir().join("loadouts-sibling");
        let other = dest_root.join("someone-elses/file");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, "keep").unwrap();

        install(&built, "cozy", &dest_root).unwrap();
        assert!(other.is_file(), "another loadout must be left alone");
        fs::remove_dir_all(&dest_root).unwrap();
    }

    #[test]
    fn installing_nothing_says_so() {
        let empty = temp_dir().join("not-built");
        fs::create_dir_all(&empty).unwrap();
        let err = install(&empty, "cozy", &temp_dir().join("loadouts-empty")).unwrap_err();
        assert!(
            err.to_string().contains("nothing built"),
            "an empty build directory should explain itself: {err}"
        );
    }

    // -- where the user's own files land ------------------------------------

    #[test]
    fn a_typed_destination_is_forced_relative_to_the_session_home() {
        // There is no way to spell anything else, so `/etc/hosts`, `~/.config`
        // and `.config` are all the same request. The prefix is noise, not a
        // different answer.
        assert_eq!(clean_dest("/etc/hosts", false), "etc/hosts");
        assert_eq!(clean_dest("~/.config/helix", false), ".config/helix");
        assert_eq!(clean_dest("~.zshrc", false), ".zshrc");
        assert_eq!(
            clean_dest(".config/fish/config.fish", false),
            ".config/fish/config.fish"
        );
        assert_eq!(clean_dest("  .gitconfig  ", false), ".gitconfig");
    }

    #[test]
    fn parent_components_are_dropped_rather_than_honoured() {
        // `..` cannot climb above the home a dest is relative to, so honouring
        // it would be inventing a meaning minimal does not have — and letting
        // it through would be an escape from the session's home.
        assert_eq!(clean_dest("../../etc/passwd", false), "etc/passwd");
        assert_eq!(clean_dest("a/../../../b", false), "a/b");
        assert_eq!(clean_dest("./x", false), "x");
        assert_eq!(clean_dest("a//b", false), "a/b");
    }

    #[test]
    fn a_directory_destination_keeps_its_trailing_slash() {
        // The trailing slash is what tells the composer this is a directory to
        // unpack a glob into rather than a file to write.
        assert_eq!(clean_dest(".config/helix", true), ".config/helix/");
        assert_eq!(clean_dest(".config/helix/", true), ".config/helix/");
        assert_eq!(clean_dest("", true), "", "nothing left is still nothing");
    }

    #[test]
    fn a_typed_destination_replaces_the_computed_one() {
        let home = Path::new("/home/x");
        let src = PathBuf::from("/home/x/.config/starship.toml");
        let mut overrides = BTreeMap::new();
        overrides.insert(src.clone(), "somewhere/else.toml".to_string());
        let out = user_patches(&[src], &[], home, &overrides);
        assert_eq!(out[0].dest, "somewhere/else.toml");

        // And an empty override falls back rather than writing to nothing.
        let src = PathBuf::from("/home/x/.gitconfig");
        let mut overrides = BTreeMap::new();
        overrides.insert(src.clone(), "   ".to_string());
        let out = user_patches(&[src], &[], home, &overrides);
        assert_eq!(out[0].dest, ".gitconfig");
    }

    #[test]
    fn an_override_for_a_directory_still_gets_its_glob_and_slash() {
        let home = Path::new("/home/x");
        let src = PathBuf::from("/home/x/dots");
        let mut overrides = BTreeMap::new();
        overrides.insert(src.clone(), "/.config/mine".to_string());
        let out = user_patches(&[], &[src], home, &overrides);
        assert_eq!(out[0].dest, ".config/mine/");
        assert!(out[0].source.ends_with("/**/*"), "{}", out[0].source);
    }

    #[test]
    fn an_override_that_is_not_given_leaves_the_computed_answer_alone() {
        let home = Path::new("/home/x");
        let overrides = BTreeMap::new();
        let out = user_patches(
            &[PathBuf::from("/home/x/.gitconfig")],
            &[PathBuf::from("/etc/skel")],
            home,
            &overrides,
        );
        assert_eq!(out[0].dest, ".gitconfig");
        assert_eq!(out[1].dest, "etc/skel/");
    }

    #[test]
    fn check_dest_explains_rather_than_silently_repairing() {
        // `clean_dest` repairs; this is what lets the wizard say why, instead
        // of quietly dropping half of what someone typed.
        assert!(check_dest(".config/fish/config.fish").is_ok());
        // `...` is three dots, a perfectly ordinary filename — not a parent
        // reference, and not something to refuse.
        assert!(check_dest("...").is_ok());
        // `~/` is accepted and stripped rather than refused: a dest is
        // home-relative by definition, so that is exactly what it means.
        assert!(check_dest("~/.config/helix").is_ok());
        for (bad, want) in [
            ("", "empty"),
            ("   ", "empty"),
            ("/etc/hosts", "leading `/`"),
            ("../escape", "climb above"),
            (".", "nothing to write to"),
            ("./", "nothing to write to"),
        ] {
            let err = check_dest(bad).unwrap_err();
            assert!(err.contains(want), "{bad:?} -> {err:?}, wanted {want:?}");
        }
    }

    #[test]
    fn a_path_under_home_keeps_its_shape() {
        // `dest` is relative to the session's home, so a dotfile has to arrive
        // where the tool that reads it will look. Taking the basename put
        // `~/.config/starship.toml` at `~/starship.toml`, where starship never
        // looks — which is the bug this replaced.
        let home = Path::new("/Users/evan");
        assert_eq!(
            patch_dest(Path::new("/Users/evan/.config/starship.toml"), home, false),
            ".config/starship.toml"
        );
        assert_eq!(
            patch_dest(Path::new("/Users/evan/.gitconfig"), home, false),
            ".gitconfig"
        );
        assert_eq!(
            patch_dest(Path::new("/Users/evan/.config/helix"), home, true),
            ".config/helix/",
            "a directory dest ends in a slash"
        );
    }

    #[test]
    fn a_path_outside_home_drops_its_leading_slash() {
        let home = Path::new("/Users/evan");
        assert_eq!(
            patch_dest(Path::new("/etc/hosts"), home, false),
            "etc/hosts"
        );
        assert_eq!(
            patch_dest(Path::new("/opt/things"), home, true),
            "opt/things/"
        );
    }

    #[test]
    fn two_files_with_the_same_name_no_longer_collide() {
        // The other half of keeping the whole path: basenames are not unique,
        // and two patches for one dest is a composition minimal refuses.
        let home = Path::new("/home/x");
        let a = patch_dest(Path::new("/home/x/.config/helix/config.toml"), home, false);
        let b = patch_dest(Path::new("/home/x/.config/bat/config.toml"), home, false);
        assert_ne!(a, b, "{a} and {b} should be distinct destinations");
    }

    #[test]
    fn a_directory_pick_shadows_everything_beneath_it() {
        let home = Path::new("/home/x");
        let picks = user_patches(
            &[PathBuf::from("/home/x/.config/starship.toml")],
            &[PathBuf::from("/home/x/.config/helix")],
            home,
            &BTreeMap::new(),
        );
        // Exactly the file picked.
        assert!(shadowed_by(".config/starship.toml", &picks).is_some());
        // Anything under the directory picked, including nested themes.
        assert!(shadowed_by(".config/helix/config.toml", &picks).is_some());
        assert!(shadowed_by(".config/helix/themes/nord.toml", &picks).is_some());
        // And nothing else.
        assert!(shadowed_by(".config/zellij/config.kdl", &picks).is_none());
        assert!(shadowed_by(".config/starship.toml.bak", &picks).is_none());
    }

    #[test]
    fn the_loadouts_own_patch_list_is_readable_for_the_warning() {
        // The wizard needs to know what it would displace *before* generating.
        let patches = loadout_patches(Path::new("../../templates"), "cozy", "nord").unwrap();
        assert!(patches.len() > 10, "expected the manifest's dests");
        assert!(
            patches
                .iter()
                .any(|p| p.dest == ".config/helix/themes/nord.toml"),
            "the slug should be substituted"
        );
        assert!(
            patches
                .iter()
                .any(|p| p.dest.contains("cozy-delta.gitconfig")),
            "and the loadout name too"
        );
        assert!(
            patches
                .iter()
                .any(|p| p.package.as_deref() == Some("atuin")),
            "optional packages should be identifiable so declined ones are not warned about"
        );
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
    fn a_file_with_no_optional_packages_parses() {
        // Everything-required is a legitimate configuration, and it used to
        // fail on a missing key — a confusing way to learn the field is
        // mandatory.
        let src = include_str!("../../../templates/packages.toml");
        let head = src
            .split("[[optional]]")
            .next()
            .expect("packages.toml head");
        let p: Packages = toml::from_str(head).expect("should parse with no optional entries");
        assert!(p.optional.is_empty());
        assert!(p.selected(&|_| true).contains(&"fish"));
    }

    #[test]
    fn every_optional_package_is_on_by_default() {
        // The full set is what the loadout has always installed, so an
        // untouched wizard has to reproduce it. A package added with
        // `default = false` would silently shrink the default loadout.
        for o in packages().optional {
            assert!(o.default, "{} is not preselected", o.name);
        }
    }

    #[test]
    fn every_optional_package_declares_a_licence() {
        // Shown to the user at the moment they choose to install. A blank or
        // invented value here is worse than none: it is a claim about someone
        // else's software. These are copied from the registry's `license_spdx`.
        for o in packages().optional {
            assert!(!o.license.trim().is_empty(), "{} has no licence", o.name);
            assert!(
                o.license.chars().all(|c| c.is_ascii_alphanumeric()
                    || "-.+ ".contains(c)
                    || c == 'O'
                    || c == 'R'),
                "{}: {:?} does not look like an SPDX identifier",
                o.name,
                o.license
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
        // difftastic is required because `git dft` in the gitconfig cannot
        // guard on its binary existing. If it ever moves back to `optional`,
        // the bare session grows a broken git alias.
        assert!(
            bare.contains(&"difftastic"),
            "difftastic must not be optional while the gitconfig aliases it"
        );
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

    // -- adjustments -------------------------------------------------------

    fn dark() -> Scheme {
        scheme_for("minimal-dark", CURRENT)
    }

    fn slot(s: &Scheme, k: &str) -> Rgb {
        s.palette[k]
    }

    #[test]
    fn a_saved_scheme_loads_back_with_the_same_palette() {
        // The property that makes "save as" produce a scheme rather than an
        // export: `discover` finds it and `just theme <name>` takes it.
        let s = dark().adjusted(Adjust {
            contrast: 25,
            comments: 40,
            warmth: -15,
            ..Adjust::default()
        });
        let dir = temp_dir().join("saved");
        let path = s
            .save_as(&dir, "My Scheme", Some("from Minimal Dark"))
            .unwrap();
        assert_eq!(path.file_name().unwrap(), "my-scheme.yaml");

        let back = Scheme::load(&path).unwrap();
        assert_eq!(back.palette, s.palette, "every slot must survive the trip");
        assert_eq!(back.name, "My Scheme");
        assert_eq!(back.slug, "my-scheme");
        assert_eq!(
            back.author, s.author,
            "the original author keeps the credit"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# from Minimal Dark"), "{text}");
    }

    #[test]
    fn saving_refuses_to_overwrite() {
        // A scheme file is the only copy of a palette somebody tuned by hand.
        let s = dark();
        let dir = temp_dir().join("no-clobber");
        s.save_as(&dir, "Taken", None).unwrap();
        let err = s.save_as(&dir, "Taken", None).unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
        // And differently-spelled names that slugify the same still clash.
        let err = s.save_as(&dir, "  taken  ", None).unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
    }

    #[test]
    fn a_name_with_nothing_to_slugify_is_refused() {
        let dir = temp_dir().join("empty-name");
        let err = dark()
            .save_as(&dir, "!!! ???", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no letters or digits"), "{err}");
    }

    #[test]
    fn a_name_with_quotes_still_round_trips() {
        // The name comes from a text field, so this is reachable rather than
        // theoretical: an unescaped quote would produce a file that will not
        // parse, and the failure would land on the next run.
        let dir = temp_dir().join("quoted");
        let path = dark()
            .save_as(&dir, "Evan's \"weird\" theme", None)
            .unwrap();
        let back = Scheme::load(&path).unwrap();
        assert_eq!(back.name, "Evan's \"weird\" theme");
    }

    #[test]
    fn a_saved_scheme_takes_its_variant_from_its_own_palette() {
        // `adjusted` pins is_dark so a mid-session tweak cannot flip
        // `scheme_variant` under the templates. Saving ends that: the file is a
        // scheme in its own right, and `load` decides the variant by luma like
        // it does for every other scheme. Stated here because the two rules
        // together are surprising if you only know one.
        let mut s = dark();
        assert!(s.is_dark);
        s.palette.insert(
            "base00".into(),
            Rgb {
                r: 250,
                g: 250,
                b: 250,
            },
        );
        s.palette.insert(
            "base05".into(),
            Rgb {
                r: 10,
                g: 10,
                b: 10,
            },
        );
        let dir = temp_dir().join("variant");
        let path = s.save_as(&dir, "Now Light", None).unwrap();
        assert!(!Scheme::load(&path).unwrap().is_dark);
    }

    #[test]
    fn the_library_never_writes_to_stdout() {
        // The wizard calls `build` and `install` from inside the alternate
        // screen. Anything printed there lands on the frame and then vanishes
        // with it — which is exactly the stray output that got reported, from a
        // `println!` right at the end of `build`. The summary is returned now,
        // and the caller decides where it goes.
        // The library proper, not the tests below — a test may print freely.
        let src = include_str!("lib.rs");
        let code_only = src.split("\n#[cfg(test)]").next().unwrap_or(src);
        for (i, line) in code_only.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for macro_name in ["println!", "print!", "eprintln!", "eprint!"] {
                assert!(
                    !code.contains(macro_name),
                    "lib.rs:{}: {macro_name} — the library must not write to a \
                     terminal it does not own",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn build_reports_what_it_wrote() {
        // The other half: it has to *return* the line, or the CLI has nothing
        // to print and the wizard has nothing to show.
        let dir = temp_dir().join("build-report");
        let report = build(&Options {
            scheme: PathBuf::from("../../schemes/minimal-dark.yaml"),
            templates: PathBuf::from("../../templates"),
            out: dir.clone(),
            ..Options::default()
        })
        .unwrap();
        assert!(report.contains("Minimal Dark"), "{report}");
        assert!(report.contains("minimal-dark"), "{report}");
        assert!(report.contains("files]"), "{report}");
    }

    #[test]
    fn an_untouched_adjustment_changes_nothing() {
        // The property everything else rests on: the six controls are plumbed
        // through the renderer unconditionally, so all-zero has to be the
        // identity or every unadjusted scheme drifts.
        let s = dark();
        let out = s.adjusted(Adjust::default());
        assert_eq!(out.palette, s.palette);
        assert_eq!(out.slug, s.slug, "and the slug must not gain a suffix");
        assert_eq!(out.name, s.name);
        assert!(Adjust::default().is_identity());
    }

    #[test]
    fn contrast_pushes_the_ramp_apart_and_negative_contrast_pulls_it_together() {
        let s = dark();
        let (bg0, fg0) = (slot(&s, "base00"), slot(&s, "base05"));
        let up = s.adjusted(Adjust {
            contrast: 50,
            ..Adjust::default()
        });
        assert!(
            contrast_ratio(slot(&up, "base05"), slot(&up, "base00")) > contrast_ratio(fg0, bg0),
            "more contrast should mean a higher ratio"
        );
        let down = s.adjusted(Adjust {
            contrast: -50,
            ..Adjust::default()
        });
        assert!(
            contrast_ratio(slot(&down, "base05"), slot(&down, "base00")) < contrast_ratio(fg0, bg0)
        );
    }

    #[test]
    fn saturation_moves_the_accents_and_leaves_the_greyscale_alone() {
        // The reason it is "accent saturation" and not "saturation": pulling
        // base00-07 toward their own grey does nothing useful and quietly
        // tints the surface when the ramp is not perfectly neutral.
        let s = dark();
        let out = s.adjusted(Adjust {
            saturation: -100,
            ..Adjust::default()
        });
        for grey in &SLOTS[..8] {
            assert_eq!(slot(&out, grey), slot(&s, grey), "{grey} should not move");
        }
        let red = slot(&out, "base08");
        assert_eq!(red.r, red.g, "fully desaturated accents are grey");
        assert_eq!(red.g, red.b);
    }

    #[test]
    fn full_saturation_leaves_a_grey_accent_grey() {
        // A scheme whose accent is already neutral has no saturation to scale;
        // the pivot is the colour itself, so it must come back unchanged.
        let mut s = dark();
        s.palette.insert(
            "base08".into(),
            Rgb {
                r: 90,
                g: 90,
                b: 90,
            },
        );
        let out = s.adjusted(Adjust {
            saturation: 100,
            ..Adjust::default()
        });
        assert_eq!(
            slot(&out, "base08"),
            Rgb {
                r: 90,
                g: 90,
                b: 90
            }
        );
    }

    #[test]
    fn comments_move_between_the_background_and_the_foreground() {
        let s = dark();
        let base = contrast_ratio(slot(&s, "base03"), slot(&s, "base00"));
        let up = s.adjusted(Adjust {
            comments: 60,
            ..Adjust::default()
        });
        assert!(
            contrast_ratio(slot(&up, "base03"), slot(&up, "base00")) > base,
            "lifting comments should make them easier to read"
        );
        let down = s.adjusted(Adjust {
            comments: -60,
            ..Adjust::default()
        });
        assert!(contrast_ratio(slot(&down, "base03"), slot(&down, "base00")) < base);
        // And only that slot moves.
        for other in SLOTS.iter().filter(|k| **k != "base03") {
            assert_eq!(slot(&up, other), slot(&s, other), "{other}");
        }
    }

    #[test]
    fn comments_at_full_lift_reach_the_foreground() {
        let s = dark();
        let out = s.adjusted(Adjust {
            comments: 100,
            ..Adjust::default()
        });
        assert_eq!(slot(&out, "base03"), slot(&s, "base05"));
    }

    #[test]
    fn separation_spreads_the_two_surface_slots_and_negative_collapses_them() {
        let s = dark();
        let gap = |x: &Scheme| contrast_ratio(slot(x, "base01"), slot(x, "base02"));
        let before = gap(&s);
        let wide = s.adjusted(Adjust {
            separation: 80,
            ..Adjust::default()
        });
        assert!(gap(&wide) > before, "selection should become visible");
        let tight = s.adjusted(Adjust {
            separation: -80,
            ..Adjust::default()
        });
        assert!(gap(&tight) < before);
    }

    #[test]
    fn separation_helps_even_when_the_two_slots_start_identical() {
        // The case the control exists for. A pivot-and-spread would be a no-op
        // here, which is why each slot moves toward a *neighbour* instead.
        let mut s = dark();
        let same = slot(&s, "base01");
        s.palette.insert("base02".into(), same);
        let out = s.adjusted(Adjust {
            separation: 100,
            ..Adjust::default()
        });
        assert_ne!(
            slot(&out, "base01"),
            slot(&out, "base02"),
            "identical surfaces must still come apart"
        );
    }

    #[test]
    fn background_deepens_or_lifts_and_the_ramp_follows_without_collapsing() {
        let s = dark();
        let deep = s.adjusted(Adjust {
            background: -100,
            ..Adjust::default()
        });
        assert!(
            slot(&deep, "base00").luminance() < slot(&s, "base00").luminance(),
            "a dark scheme should get darker"
        );
        let lift = s.adjusted(Adjust {
            background: 100,
            ..Adjust::default()
        });
        assert!(slot(&lift, "base00").luminance() > slot(&s, "base00").luminance());

        // base01 follows, but not as far — otherwise the ramp lands flat.
        // Measured as the *fraction* of the distance travelled, on the
        // gamma-encoded luma: relative luminance is nonlinear, so the same
        // proportional move reads as a larger delta higher up the ramp.
        let moved = |k: &str| {
            let before = luma8(slot(&s, k));
            (before - luma8(slot(&deep, k))) / before
        };
        assert!(
            moved("base01") < moved("base00"),
            "base01 should follow at a lower rate: {} vs {}",
            moved("base01"),
            moved("base00")
        );
        assert_ne!(
            slot(&deep, "base00"),
            slot(&deep, "base01"),
            "the ramp must not collapse onto the new floor"
        );
    }

    #[test]
    fn background_deepening_goes_the_other_way_for_a_light_scheme() {
        // "Deeper" means further from the text, which is lighter here.
        let mut s = dark();
        // A light scheme is the ramp the other way up. Not pure white, so
        // "deeper" has somewhere left to go.
        for (k, v) in [
            ("base00", 242_u8),
            ("base01", 226),
            ("base05", 20),
            ("base07", 0),
        ] {
            s.palette.insert(k.into(), Rgb { r: v, g: v, b: v });
        }
        s.is_dark = false;
        let deep = s.adjusted(Adjust {
            background: -100,
            ..Adjust::default()
        });
        assert!(slot(&deep, "base00").luminance() > slot(&s, "base00").luminance());
    }

    #[test]
    fn warmth_casts_red_one_way_and_blue_the_other() {
        let s = dark();
        let warm = s.adjusted(Adjust {
            warmth: 100,
            ..Adjust::default()
        });
        let cool = s.adjusted(Adjust {
            warmth: -100,
            ..Adjust::default()
        });
        let mid = slot(&s, "base05");
        assert!(slot(&warm, "base05").r >= mid.r && slot(&warm, "base05").b <= mid.b);
        assert!(slot(&cool, "base05").r <= mid.r && slot(&cool, "base05").b >= mid.b);
        assert_eq!(
            slot(&warm, "base05").g,
            mid.g,
            "green is the axis, not a target"
        );
    }

    #[test]
    fn the_variant_is_never_recomputed_from_an_adjusted_palette() {
        // An adjustment that crossed the luma line would flip scheme_variant,
        // which decides `duf --theme` and every `{% if dark %}` branch. Silently
        // rewriting unrelated config is the one outcome worth ruling out.
        let s = dark();
        assert!(s.is_dark);
        let flipped = s.adjusted(Adjust {
            contrast: -100,
            background: 100,
            ..Adjust::default()
        });
        assert!(flipped.is_dark, "the variant must survive any adjustment");
        assert_eq!(flipped.variant(), "dark");
    }

    #[test]
    fn every_channel_stays_in_gamut_at_the_extremes() {
        // Everything downstream reads 6-digit hex; a channel that wrapped or
        // saturated wrongly would still *parse*, so this is worth stating.
        let s = dark();
        for v in [-100_i8, -50, 50, 100] {
            let a = Adjust {
                contrast: v,
                saturation: v,
                comments: v,
                separation: v,
                background: v,
                warmth: v,
            };
            let out = s.adjusted(a);
            for k in SLOTS {
                assert_eq!(out.palette[k].hex().len(), 6, "{k} at {v}");
            }
        }
    }

    #[test]
    fn an_adjusted_scheme_gets_its_own_slug_and_uuid() {
        // Theme files are named after the slug and the .tmTheme UUID is derived
        // from it, so an adjusted scheme sharing a slug would overwrite the
        // stock one's files and inherit its UUID.
        let s = dark();
        let a = Adjust {
            contrast: 20,
            ..Adjust::default()
        };
        let out = s.adjusted(a);
        assert_ne!(out.slug, s.slug);
        assert!(out.slug.starts_with(&s.slug), "{}", out.slug);
        assert_ne!(out.uuid(), s.uuid());
        assert!(out.name.ends_with("(adjusted)"), "{}", out.name);
    }

    #[test]
    fn different_adjustments_get_different_slugs_and_the_same_one_is_stable() {
        let s = dark();
        let a = Adjust {
            contrast: 20,
            ..Adjust::default()
        };
        let b = Adjust {
            contrast: 21,
            ..Adjust::default()
        };
        assert_ne!(s.adjusted(a).slug, s.adjusted(b).slug);
        assert_eq!(s.adjusted(a).slug, s.adjusted(a).slug, "must be stable");
    }

    #[test]
    fn body_contrast_reads_the_slots_the_screen_reports() {
        let s = dark();
        assert!(
            (s.body_contrast() - contrast_ratio(slot(&s, "base05"), slot(&s, "base00"))).abs()
                < 1e-9
        );
        // And the ratio itself is the WCAG one: black on white is 21:1.
        let (b, w) = (
            Rgb { r: 0, g: 0, b: 0 },
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
        );
        assert!((contrast_ratio(b, w) - 21.0).abs() < 0.01);
        assert!(
            (contrast_ratio(w, b) - 21.0).abs() < 0.01,
            "order must not matter"
        );
        assert!((contrast_ratio(b, b) - 1.0).abs() < 1e-9);
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
