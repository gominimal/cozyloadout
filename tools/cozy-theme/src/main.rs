//! cozy-theme — render the cozy loadout from a base16 scheme.
//!
//! The rendering itself lives in the library, which the wizard calls too. This
//! binary is the command line over it and nothing else.
//!
//! See AGENTS.md for the template grammar and the build pipeline.

use clap::parser::ValueSource;
use clap::{CommandFactory, Parser};
use color_eyre::eyre::{bail, eyre, Result};
use cozy_theme::{Adjust, Options, Packages, Settings};
use std::path::{Path, PathBuf};

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

    /// Save the adjusted scheme under this name, as a scheme of its own, and
    /// render that. Lands beside the checked-in schemes, so `just theme <name>`
    /// picks it up afterwards.
    #[arg(long)]
    save_as: Option<String>,

    /// Where `--save-as` writes. Defaults to the directory the scheme came from.
    #[arg(long)]
    schemes_dir: Option<PathBuf>,

    /// Render from a wizard settings file — the scheme, greeting, packages,
    /// patches and adjustments it recorded. Flags given alongside it win, so
    /// `--settings mine.toml --greeting none` is one answer changed rather
    /// than a file to edit.
    #[arg(long)]
    settings: Option<PathBuf>,

    /// Where to resolve `--settings`' scheme name from.
    #[arg(long, default_value = "schemes")]
    schemes: PathBuf,
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

/// Fold a settings file into `options`, under whatever flags were typed.
///
/// Applied first and overridden by the command line: the file is a starting
/// point, not an override, so `--settings mine.toml --greeting none` is one
/// answer changed rather than a file to edit.
///
/// # Errors
///
/// If the file cannot be read, names no theme, or names one that is not under
/// `--schemes`.
fn layer_settings(args: &Args, options: &mut Options) -> Result<()> {
    let Some(path) = &args.settings else {
        return Ok(());
    };
    // `read`, not `load`: the user named this file, so a typo in it should
    // say where, rather than falling back to defaults and rendering
    // something quietly different from what they asked for.
    let settings = Settings::read(path).map_err(|e| eyre!(e))?;
    let offered = Packages::load(&args.templates.join("packages.toml"))?;
    let mut from_file = options.clone();
    settings.apply_to(&mut from_file, &offered);
    // Re-apply whichever flags the user actually typed. Asked of clap
    // rather than inferred by comparing against the defaults: that
    // comparison cannot see `--greeting blocks` or `--contrast 0`, so
    // explicitly asking for a default value silently lost to the file.
    let matches = Args::command().get_matches_from(std::env::args_os());
    let typed_flag = |name: &str| matches.value_source(name) == Some(ValueSource::CommandLine);
    let typed = Options::from(args);
    if typed_flag("greeting") {
        from_file.greeting = typed.greeting;
    }
    if typed_flag("with") {
        from_file.with = typed.with;
    }
    if typed_flag("patch_files") {
        from_file.patch_files = typed.patch_files;
    }
    if typed_flag("patch_dirs") {
        from_file.patch_dirs = typed.patch_dirs;
    }
    // Per knob, not all six together: `--warmth 10` alongside a file that
    // sets contrast should change the warmth and leave the contrast.
    for (name, from, to) in [
        (
            "contrast",
            typed.adjust.contrast,
            &mut from_file.adjust.contrast,
        ),
        (
            "saturation",
            typed.adjust.saturation,
            &mut from_file.adjust.saturation,
        ),
        (
            "comments",
            typed.adjust.comments,
            &mut from_file.adjust.comments,
        ),
        (
            "separation",
            typed.adjust.separation,
            &mut from_file.adjust.separation,
        ),
        (
            "background",
            typed.adjust.background,
            &mut from_file.adjust.background,
        ),
        ("warmth", typed.adjust.warmth, &mut from_file.adjust.warmth),
    ] {
        if typed_flag(name) {
            *to = from;
        }
    }
    *options = from_file;

    // The file names its scheme; a path on the command line still wins.
    if args.scheme.is_none() {
        let name = settings
            .theme
            .clone()
            .ok_or_else(|| eyre!("{} names no theme", path.display()))?;
        options.scheme = cozy_theme::discover(&args.schemes)
            .into_iter()
            .find(|e| e.name == name)
            .map(|e| e.path)
            .ok_or_else(|| {
                eyre!(
                    "{} wants the scheme {name:?}, which is not under {}",
                    path.display(),
                    args.schemes.display()
                )
            })?;
    }
    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    // Saving happens before the render, and the render then uses the saved
    // file: otherwise `--save-as` would write one scheme and build another, and
    // the build/ tree would not be reachable from the scheme now on disk.
    let mut options = Options::from(&args);

    layer_settings(&args, &mut options)?;
    if let (Some(name), Some(scheme)) = (&args.save_as, &args.scheme) {
        let loaded = cozy_theme::Scheme::load(scheme)?;
        let from = format!(
            "Adapted from {:?} by {}.",
            loaded.name.clone(),
            loaded.author.clone()
        );
        let dir = args.schemes_dir.clone().unwrap_or_else(|| {
            scheme
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        });
        let saved = loaded
            .adjusted(options.adjust)
            .save_as(&dir, name, Some(&from))?;
        println!("saved {}", saved.display());
        // The saved palette already carries the adjustments, so re-applying
        // them would double every one of them.
        options.scheme = saved;
        options.adjust = cozy_theme::Adjust::default();
    }

    if args.scheme.is_some() || args.settings.is_some() {
        println!("{}", cozy_theme::build(&options)?);
    } else if !args.install {
        bail!("give a scheme to render, --settings to render from a saved one, or --install to install what is already built");
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
