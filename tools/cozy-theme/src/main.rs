//! cozy-theme — render the cozy loadout from a base16 scheme.
//!
//! Reads a tinted-theming scheme YAML, expands every file listed in
//! templates/manifest.toml, and writes a ready-to-bundle loadout tree.
//!
//! See README.md for the template grammar and the build pipeline.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

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
    fn parse(s: &str) -> Result<Rgb, String> {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("not a 6-digit hex colour: {s:?}"));
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
        Ok(Rgb { r: byte(0), g: byte(2), b: byte(4) })
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
    let c = |a: u8, b: u8| (b as f64 + f * (a as f64 - b as f64)).round().clamp(0.0, 255.0) as u8;
    Rgb { r: c(fg.r, bg.r), g: c(fg.g, bg.g), b: c(fg.b, bg.b) }
}

// ---------------------------------------------------------------------------
// Scheme
// ---------------------------------------------------------------------------

const SLOTS: [&str; 16] = [
    "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08",
    "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
];

#[allow(dead_code)]
struct Scheme {
    slug: String,
    name: String,
    author: String,
    system: String,
    is_dark: bool,
    palette: BTreeMap<String, Rgb>,
}

/// Split `key: value` and strip a trailing `# comment`, respecting quotes.
///
/// The quote handling is the whole reason this isn't a one-liner: scheme values
/// are frequently `"#141414"`, where a naive comment strip eats the colour.
fn split_kv(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("---") {
        return None;
    }
    let (k, rest) = line.split_once(':')?;
    let key = k.trim().trim_matches(['"', '\'']).to_string();
    let rest = rest.trim();

    let value = if let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
        // Quoted: take up to the closing quote, comment or not.
        let body = &rest[1..];
        match body.find(q) {
            Some(end) => body[..end].to_string(),
            None => return None,
        }
    } else {
        // Bare: a `#` only starts a comment when preceded by whitespace, so a
        // legacy scheme's unquoted `base00: 1d2021` survives either way.
        match rest.find(" #") {
            Some(i) => rest[..i].trim().to_string(),
            None => rest.to_string(),
        }
    };
    Some((key, value))
}

impl Scheme {
    /// Handles both scheme formats: the current one with a nested `palette:`
    /// block, and the legacy one with `base00:` at the top level. Nesting is
    /// ignored entirely — any `baseXX` key anywhere is a slot — which is what
    /// makes one parser cover both.
    fn load(path: &Path) -> Result<Scheme, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;

        let mut meta: BTreeMap<String, String> = BTreeMap::new();
        let mut palette: BTreeMap<String, Rgb> = BTreeMap::new();

        for line in text.lines() {
            let Some((key, value)) = split_kv(line) else { continue };
            let lower = key.to_ascii_lowercase();
            if lower.len() == 6 && lower.starts_with("base") {
                // Normalise base0a -> base0A so templates have one spelling.
                let canon = format!("base{}", key[4..].to_ascii_uppercase());
                if SLOTS.contains(&canon.as_str()) {
                    let rgb = Rgb::parse(&value)
                        .map_err(|e| format!("{}: {canon}: {e}", path.display()))?;
                    palette.insert(canon, rgb);
                }
            } else if value.is_empty() {
                continue; // a block header such as `palette:`
            } else {
                meta.entry(lower).or_insert(value);
            }
        }

        let missing: Vec<&str> =
            SLOTS.iter().copied().filter(|s| !palette.contains_key(*s)).collect();
        if missing.len() == SLOTS.len() {
            // Most likely a tinted8 scheme (named 8-colour keys) or not a
            // scheme at all — saying "missing all 16" for those reads as a
            // corrupt base16 file rather than the wrong kind of file.
            let system = meta.get("system").map(|s| s.as_str()).unwrap_or("unknown");
            return Err(format!(
                "{}: no base16 slots found — this is a {system:?} scheme, and the loadout \
                 needs base16 or base24",
                path.display()
            ));
        }
        if !missing.is_empty() {
            return Err(format!(
                "{}: scheme is missing {} of 16 slots: {}",
                path.display(),
                missing.len(),
                missing.join(", ")
            ));
        }

        let slug = slugify(
            path.file_stem().and_then(|s| s.to_str()).ok_or("scheme path has no file stem")?,
        );
        if slug.is_empty() {
            return Err(format!("{}: filename does not slugify to anything", path.display()));
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
            system: meta.get("system").cloned().unwrap_or_else(|| "base16".into()),
            is_dark,
            palette,
        })
    }

    /// Stable per-slug UUID for the .tmTheme. Sublime keys themes by UUID, so
    /// two schemes sharing one would collide; deriving it from the slug keeps
    /// it both unique and reproducible across builds.
    fn uuid(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.slug.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        let mut bytes = [0u8; 16];
        let mut x = h | 1;
        for chunk in bytes.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            chunk.copy_from_slice(&x.to_be_bytes());
        }
        bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
        bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
        let hx: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &hx[0..8], &hx[8..12], &hx[12..16], &hx[16..20], &hx[20..32])
    }

    fn variant(&self) -> &'static str {
        if self.is_dark {
            "dark"
        } else {
            "light"
        }
    }

    /// Every placeholder a template may reference. Names follow
    /// tinted-builder's vocabulary so upstream templates mostly drop in.
    fn vars(&self) -> BTreeMap<String, String> {
        let mut v = BTreeMap::new();
        for slot in SLOTS {
            let c = self.palette[slot];
            let hex = c.hex();
            v.insert(format!("{slot}-hex"), hex.clone());
            v.insert(format!("{slot}-hex-r"), hex[0..2].into());
            v.insert(format!("{slot}-hex-g"), hex[2..4].into());
            v.insert(format!("{slot}-hex-b"), hex[4..6].into());
            v.insert(format!("{slot}-rgb-r"), c.r.to_string());
            v.insert(format!("{slot}-rgb-g"), c.g.to_string());
            v.insert(format!("{slot}-rgb-b"), c.b.to_string());
            // Convenience triple: broot wants `rgb(126, 200, 151)` and writing
            // that as three placeholders is unreadable.
            v.insert(format!("{slot}-rgb"), format!("{}, {}, {}", c.r, c.g, c.b));
            v.insert(format!("{slot}-dec-r"), format!("{:.4}", c.r as f64 / 255.0));
            v.insert(format!("{slot}-dec-g"), format!("{:.4}", c.g as f64 / 255.0));
            v.insert(format!("{slot}-dec-b"), format!("{:.4}", c.b as f64 / 255.0));
        }
        v.insert("scheme-slug".into(), self.slug.clone());
        v.insert("scheme-name".into(), self.name.clone());
        v.insert("scheme-author".into(), self.author.clone());
        v.insert("scheme-system".into(), self.system.clone());
        v.insert("scheme-variant".into(), self.variant().into());
        v.insert("scheme-uuid".into(), self.uuid());
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

/// Drop `{{#dark}}…{{/dark}}` / `{{#light}}…{{/light}}` sections that don't
/// apply, unwrapping the one that does. Sections do not nest.
fn expand_sections(mut text: String, is_dark: bool) -> Result<String, String> {
    for (name, keep) in [("dark", is_dark), ("light", !is_dark)] {
        let open = format!("{{{{#{name}}}}}");
        let close = format!("{{{{/{name}}}}}");
        loop {
            let Some(start) = text.find(&open) else { break };
            let after = start + open.len();
            let Some(rel) = text[after..].find(&close) else {
                return Err(format!("unclosed {{{{#{name}}}}} section"));
            };
            let body = text[after..after + rel].to_string();
            let end = after + rel + close.len();
            text.replace_range(start..end, if keep { &body } else { "" });
        }
        if let Some(i) = text.find(&close) {
            return Err(format!("stray {{{{/{name}}}}} at byte {i}"));
        }
    }
    Ok(text)
}

/// Resolve one `{{ … }}` body: either a variable name or a `mix` call.
fn eval(expr: &str, vars: &BTreeMap<String, String>, scheme: &Scheme) -> Result<String, String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    match parts.as_slice() {
        [name] => vars.get(*name).cloned().ok_or_else(|| format!("unknown placeholder {name:?}")),

        // {{mix <fg-slot> <bg-slot> <pct>}} — base16 has no dim surface
        // colours, so delta's diff backgrounds and broot's gauge ramp are
        // computed from slots rather than picked from them.
        [op @ ("mix" | "mix-rgb"), fg, bg, pct] => {
            let slot = |s: &str| {
                let canon = format!("base{}", s.trim_start_matches("base").to_ascii_uppercase());
                scheme
                    .palette
                    .get(&canon)
                    .copied()
                    .ok_or_else(|| format!("{op}: {s:?} is not a palette slot"))
            };
            let pct: f64 = pct.parse().map_err(|_| format!("{op}: {pct:?} is not a number"))?;
            if !(0.0..=100.0).contains(&pct) {
                return Err(format!("{op}: {pct} is outside 0–100"));
            }
            let c = mix(slot(fg)?, slot(bg)?, pct);
            Ok(if *op == "mix" {
                c.hex()
            } else {
                format!("{}, {}, {}", c.r, c.g, c.b)
            })
        }

        _ => Err(format!("cannot parse {{{{{expr}}}}}")),
    }
}

fn render(text: &str, vars: &BTreeMap<String, String>, scheme: &Scheme) -> Result<String, String> {
    let text = expand_sections(text.to_string(), scheme.is_dark)?;
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or("unclosed {{")?;
        out.push_str(&eval(after[..end].trim(), vars, scheme)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

struct Entry {
    template: String,
    out: String,
    dest: Option<String>,
    copy: bool,
}

/// Enough TOML for `[[file]]` tables of string and boolean keys. Deliberately
/// not a general parser — the manifest is ours, and a real one would be the
/// tool's only dependency.
fn parse_manifest(path: &Path) -> Result<Vec<Entry>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut cur: Option<(String, String, Option<String>, bool)> = None;

    let flush = |cur: Option<(String, String, Option<String>, bool)>,
                 entries: &mut Vec<Entry>|
     -> Result<(), String> {
        if let Some((template, out, dest, copy)) = cur {
            if template.is_empty() || out.is_empty() {
                return Err("[[file]] needs both `template` and `out`".into());
            }
            entries.push(Entry { template, out, dest, copy });
        }
        Ok(())
    };

    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[file]]" {
            flush(cur.take(), &mut entries)?;
            cur = Some((String::new(), String::new(), None, false));
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("{}:{}: cannot parse {line:?}", path.display(), n + 1));
        };
        let e = cur
            .as_mut()
            .ok_or_else(|| format!("{}:{}: key outside [[file]]", path.display(), n + 1))?;
        let v = v.trim();
        let sval = || v.trim_matches('"').to_string();
        match k.trim() {
            "template" => e.0 = sval(),
            "out" => e.1 = sval(),
            "dest" => e.2 = Some(sval()),
            "copy" => e.3 = v.starts_with("true"),
            other => return Err(format!("{}:{}: unknown key {other:?}", path.display(), n + 1)),
        }
    }
    flush(cur, &mut entries)?;
    if entries.is_empty() {
        return Err(format!("{}: no [[file]] entries", path.display()));
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

struct Args {
    scheme: PathBuf,
    templates: PathBuf,
    manifest: Option<PathBuf>,
    out: PathBuf,
    loadout: String,
}

fn parse_args() -> Result<Args, String> {
    let mut scheme = None;
    let mut templates = PathBuf::from("templates");
    let mut manifest = None;
    let mut out = PathBuf::from("build");
    let mut loadout = "cozy".to_string();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--templates" => templates = val("--templates")?.into(),
            "--manifest" => manifest = Some(val("--manifest")?.into()),
            "--out" => out = val("--out")?.into(),
            "--loadout" => loadout = val("--loadout")?,
            "-h" | "--help" => {
                println!(
                    "usage: cozy-theme <scheme.yaml> [--templates DIR] [--manifest FILE] \
                     [--out DIR] [--loadout NAME]"
                );
                std::process::exit(0);
            }
            other if other.starts_with('-') => return Err(format!("unknown flag {other}")),
            other => {
                if scheme.replace(PathBuf::from(other)).is_some() {
                    return Err("expected exactly one scheme file".into());
                }
            }
        }
    }
    Ok(Args {
        scheme: scheme.ok_or("no scheme given (try `cozy-theme --help`)")?,
        templates,
        manifest,
        out,
        loadout,
    })
}

fn build(args: &Args) -> Result<(), String> {
    let scheme = Scheme::load(&args.scheme)?;
    let manifest_path =
        args.manifest.clone().unwrap_or_else(|| args.templates.join("manifest.toml"));
    let entries = parse_manifest(&manifest_path)?;
    let vars = scheme.vars();

    let root = args.out.join(&args.loadout);
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    }

    let sub = |s: &str| s.replace("{slug}", &scheme.slug);
    let mut patches = String::new();

    for e in &entries {
        let src = args.templates.join(&e.template);
        let out_rel = sub(&e.out);
        let dst = root.join(&out_rel);

        let body = fs::read_to_string(&src).map_err(|err| format!("{}: {err}", src.display()))?;
        let body = if e.copy {
            body
        } else {
            render(&body, &vars, &scheme).map_err(|err| format!("{}: {err}", src.display()))?
        };
        write_file(&dst, &body)?;

        if let Some(dest) = &e.dest {
            // One line per entry. TOML forbids newlines inside an inline
            // table, so the wrapped `{ dest = …,\n source = … }` form this
            // file used to be written in was not actually valid TOML — it
            // survived only because minimal's parser tolerates it.
            let _ = write!(
                patches,
                "    {{ dest = \"{}\", source = \"~/.config/minimal/loadouts/{}/{}\" }},\n",
                sub(dest),
                args.loadout,
                out_rel
            );
        }
    }

    // The loadout manifest itself: same template grammar, plus `patches`,
    // which is built from the list above so it cannot describe a file that
    // wasn't rendered.
    let toml_src = args.templates.join(format!("{}.toml", args.loadout));
    let body = fs::read_to_string(&toml_src).map_err(|e| format!("{}: {e}", toml_src.display()))?;
    let mut vars = vars;
    vars.insert("patches".into(), patches.trim_end().trim_end_matches(',').to_string());
    let body =
        render(&body, &vars, &scheme).map_err(|e| format!("{}: {e}", toml_src.display()))?;
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

fn main() {
    let result = parse_args().and_then(|a| build(&a));
    if let Err(e) = result {
        eprintln!("cozy-theme: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The slug comes from the filename, and tests run in parallel in one
    /// process, so every call gets a private directory. A shared path would
    /// both clobber content across threads and collapse distinct slugs.
    fn scheme_for(slug: &str, text: &str) -> Scheme {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("cozy-theme-test-{}", std::process::id()))
            .join(n.to_string());
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{slug}.yaml"));
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
        // quotes turns `"#141414"` into an empty value.
        assert_eq!(split_kv(r##"  base00: "#141414" # gray-8"##).unwrap().1, "#141414");
        assert_eq!(split_kv("base00: 1d2021").unwrap().1, "1d2021");
        assert_eq!(split_kv("base00: 1d2021 # hard").unwrap().1, "1d2021");
    }

    #[test]
    fn variant_follows_luma_not_metadata() {
        // Declared dark, but the surface is plainly light.
        let s = scheme_for("inverted", &CURRENT.replace(r##"base00: "#141414""##, r##"base00: "#f5f5f5""##));
        assert!(!s.is_dark, "luma should override the declared variant");
    }

    #[test]
    fn mix_matches_the_hand_computed_diff_backgrounds() {
        let s = scheme_for("minimal-dark", CURRENT);
        let bg = s.palette["base00"];
        // The values README.md tabulated by hand for delta.
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
        assert_eq!(render("#{{base0D-hex}}", &v, &s).unwrap(), "#4a7aff");
        assert_eq!(render("rgb({{base00-rgb}})", &v, &s).unwrap(), "rgb(20, 20, 20)");
        assert_eq!(render("{{base00-rgb-r}}", &v, &s).unwrap(), "20");
        assert_eq!(render("{{mix base08 base00 15}}", &v, &s).unwrap(), "341919");
        assert_eq!(render("{{mix-rgb base08 base00 15}}", &v, &s).unwrap(), "52, 25, 25");
        assert_eq!(render("{{#dark}}Ocean{{/dark}}{{#light}}GitHub{{/light}}", &v, &s).unwrap(), "Ocean");
    }

    #[test]
    fn unknown_placeholder_is_an_error() {
        // Passing it through would ship a literal `{{typo}}` into a config file
        // that the target tool then silently ignores.
        let s = scheme_for("minimal-dark", CURRENT);
        assert!(render("{{base0G-hex}}", &s.vars(), &s).is_err());
        assert!(render("{{mix base08 base00}}", &s.vars(), &s).is_err());
        assert!(render("{{mix base08 base00 150}}", &s.vars(), &s).is_err());
        assert!(render("{{unclosed", &s.vars(), &s).is_err());
    }

    #[test]
    fn missing_slots_are_rejected() {
        let dir = std::env::temp_dir().join("cozy-theme-test-partial");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("partial.yaml");
        fs::write(&p, "palette:\n  base00: \"#000000\"\n").unwrap();
        let err = match Scheme::load(&p) { Ok(_) => panic!("expected failure"), Err(e) => e };
        assert!(err.contains("missing 15 of 16 slots"), "{err}");
    }

    #[test]
    fn wrong_scheme_system_says_so() {
        // The upstream collection ships tinted8 schemes alongside base16 ones.
        // They have named 8-colour keys, so every slot is "missing" — the error
        // should name the system rather than imply a corrupt base16 file.
        let dir = std::env::temp_dir().join("cozy-theme-test-tinted8");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("nord.yaml");
        fs::write(
            &p,
            "scheme:\n  system: \"tinted8\"\n  name: \"Nord\"\npalette:\n  black: \"#2e3440\"\n",
        )
        .unwrap();
        let err = match Scheme::load(&p) {
            Ok(_) => panic!("expected failure"),
            Err(e) => e,
        };
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
