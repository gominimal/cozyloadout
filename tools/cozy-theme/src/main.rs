//! cozy-theme — render the cozy loadout from a base16 scheme.
//!
//! The rendering itself lives in the library, which the wizard calls too. This
//! binary is the command line over it and nothing else.
//!
//! See AGENTS.md for the template grammar and the build pipeline.

use clap::Parser;
use color_eyre::eyre::{bail, eyre, Result};
use cozy_theme::{Adjust, Options};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cozy-theme", version, about, long_about = None)]
struct Args {
    /// Scheme YAML to render (a path, e.g. schemes/minimal-dark.yaml).
    /// Optional only when `--install` is used on its own.
    scheme: Option<PathBuf>,

    /// Directory holding the templates and manifest.toml
    #[arg(long, default_value = "templates")]
    templates: PathBuf,

    /// Directory to write the rendered loadout into
    #[arg(long, default_value = "build")]
    out: PathBuf,

    /// Loadout name — names the .toml and the directory beside it
    #[arg(long, default_value = "cozy")]
    loadout: String,

    /// Which fish greeting to install
    #[arg(
        long,
        value_parser = ["blocks", "geometric", "legacy", "text", "none"],
        default_value = "blocks"
    )]
    greeting: String,

    /// Optional packages to include. Repeat the flag or comma-separate.
    /// Omit it entirely to install every optional package, which is what a
    /// plain `just theme` does and what the loadout has always shipped.
    #[arg(long = "with", value_delimiter = ',')]
    with: Vec<String>,

    /// A file of your own to patch into the session. Repeatable.
    #[arg(long = "patch-file")]
    patch_files: Vec<PathBuf>,

    /// A directory of your own to patch into the session. Repeatable.
    #[arg(long = "patch-dir")]
    patch_dirs: Vec<PathBuf>,

    /// Install what is in `--out` into minimal's loadouts directory. With a
    /// scheme, renders first; on its own, installs whatever is already built.
    #[arg(long)]
    install: bool,

    /// Where loadouts live. Defaults to ~/.config/minimal/loadouts.
    #[arg(long)]
    loadouts: Option<PathBuf>,

    // Scheme adjustments. Each is a percentage in -100..=100 and each defaults
    // to 0, which is the identity — the wizard's adjustment screen sets the
    // same six values, so nothing is reachable only through the interface.
    /// Push the surface and foreground ramps apart (-100..=100)
    #[arg(long, allow_negative_numbers = true, default_value_t = 0, value_parser = pct)]
    contrast: i8,

    /// Pull the eight accents toward or away from grey (-100..=100)
    #[arg(long, allow_negative_numbers = true, default_value_t = 0, value_parser = pct)]
    saturation: i8,

    /// Move base03 — comments — toward the foreground or the background (-100..=100)
    #[arg(long, allow_negative_numbers = true, default_value_t = 0, value_parser = pct)]
    comments: i8,

    /// Spread base01 and base02 apart, so a selection reads (-100..=100)
    #[arg(long, allow_negative_numbers = true, default_value_t = 0, value_parser = pct)]
    separation: i8,

    /// Deepen the background away from the scheme, or lift it (-100..=100)
    #[arg(long, allow_negative_numbers = true, default_value_t = 0, value_parser = pct)]
    background: i8,

    /// A warm or cool cast over every slot (-100..=100)
    #[arg(long, allow_negative_numbers = true, default_value_t = 0, value_parser = pct)]
    warmth: i8,
}

/// Percentages are bounded so an out-of-range value is refused at the command
/// line rather than silently clamped somewhere in the middle of a render.
fn pct(s: &str) -> Result<i8, String> {
    let v: i32 = s.parse().map_err(|_| format!("{s:?} is not a number"))?;
    i8::try_from(v)
        .ok()
        .filter(|v| (-100..=100).contains(v))
        .ok_or_else(|| format!("{v} is outside -100..=100"))
}

impl From<&Args> for Options {
    fn from(a: &Args) -> Self {
        Options {
            scheme: a.scheme.clone().unwrap_or_default(),
            templates: a.templates.clone(),
            out: a.out.clone(),
            loadout: a.loadout.clone(),
            greeting: a.greeting.clone(),
            with: a.with.clone(),
            patch_files: a.patch_files.clone(),
            patch_dirs: a.patch_dirs.clone(),
            adjust: Adjust {
                contrast: a.contrast,
                saturation: a.saturation,
                comments: a.comments,
                separation: a.separation,
                background: a.background,
                warmth: a.warmth,
            },
            ..Options::default()
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    if args.scheme.is_some() {
        cozy_theme::build(&Options::from(&args))?;
    } else if !args.install {
        bail!("give a scheme to render, or --install to install what is already built");
    }

    if args.install {
        let root = if let Some(dir) = &args.loadouts {
            dir.clone()
        } else {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| eyre!("HOME is not set; pass --loadouts"))?;
            cozy_theme::loadouts_dir(&home)
        };
        let dest = cozy_theme::install(&args.out, &args.loadout, &root)?;
        println!("installed into {}", dest.display());
    }
    Ok(())
}
