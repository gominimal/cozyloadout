//! cozy-theme — render the cozy loadout from a base16 scheme.
//!
//! The rendering itself lives in the library, which the wizard calls too. This
//! binary is the command line over it and nothing else.
//!
//! See AGENTS.md for the template grammar and the build pipeline.

use clap::Parser;
use color_eyre::eyre::{bail, eyre, Result};
use cozy_theme::Options;
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
    #[arg(long, value_parser = ["blocks", "legacy"], default_value = "blocks")]
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
