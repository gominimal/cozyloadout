<h1 align="center">Cozy Loadout</h1>

<p align="center"><strong>One Palette, Every Tool</strong><br>A themed terminal loadout for Minimal — fish, helix, zellij, starship, bat, delta and more, generated from a single base16 scheme so your whole terminal reads as one design.</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> ·
  <a href="#choosing-a-scheme">Schemes</a> ·
  <a href="#whats-in-it">What's In It</a> ·
  <a href="AGENTS.md">Working on it</a> ·
  <a href="https://minimal.dev/docs">Minimal Docs</a> ·
  <a href="https://github.com/gominimal/cozyloadout/discussions">Discussions</a>
</p>

<p align="center">
  <a href="https://github.com/gominimal/cozyloadout/actions/workflows/ci.yml"><img src="https://github.com/gominimal/cozyloadout/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License"></a>
  <a href="https://minimal.dev/docs"><img src="https://img.shields.io/badge/docs-online-blue" alt="Docs"></a>
  <a href="https://discord.com/invite/qgX8sm6X7G"><img src="https://img.shields.io/badge/Discord-join-5865F2?logo=discord&logoColor=white" alt="Discord"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/built_with-Rust-dea584.svg" alt="Built with Rust"></a>
</p>

---

## What is the Cozy Loadout?

A [Minimal](https://github.com/gominimal/minimal) **loadout** is the personal half of a development sandbox: the project's `minimal.toml` says what every contributor's session needs, and a loadout layers on what *you* want — your editor, your multiplexer, your shell, your colours.

This is a loadout that themes the whole terminal from one palette. Rather than shipping hand-maintained config files, the repository ships **templates and a small renderer**. You hand it a base16 scheme; it generates every config file in the set from that one palette, bundles them, and installs them where Minimal expects them. Re-theming the entire terminal is one command.

Twelve tools are themed through a config file, and several more — jq, difftastic, duf, man-page rendering — through the environment instead. They all end up on the same sixteen colours, so a diff in `delta`, a file in `bat`, and the same file open in `helix` agree.

Two schemes are included, **Minimal Dark** and **Minimal Light**, derived from the Minimal System design tokens so a terminal and the web app read as the same design system. Any [tinted-theming](https://github.com/tinted-theming/schemes) scheme also works, or any file in that format — `just fetch-schemes` pulls in the upstream base16 and base24 collection, all of which is rendered in CI.

> The loadout format itself is documented in the [Minimal docs](https://minimal.dev/docs).

## Requirements

To build and install the loadout you need:

| Tool | Used for |
| --- | --- |
| [`just`](https://github.com/casey/just) | running the recipes |
| Rust — `cargo` and `rustc`, 1.85+ | building the renderer |
| `bash` | recipe bodies (the multi-line recipes carry a bash shebang) |
| `zip`, `unzip` | `just bundle` and `just install` |
| `git` | cloning this repo; also `just fetch-schemes` |
| Minimal **0.5.4+** | *installing* the loadout. Patch sources use `$LOADOUT_ROOT` and the shell handover uses `SHELL`; neither works on 0.5.3 or earlier |

Plus the usual POSIX userland — `find`, `sed`, `sort`, `head`, `wc`, `tr`, `mkdir`, `rm` — which any Unix already has.

`just check` additionally uses `fish` and `dash` to syntax-check the two files that are shipped as code. Either is skipped with a message if it isn't there, so the recipe still runs — it just checks less. The generated TOML, XML and YAML need no external tool: the renderer parses each file with a real parser before writing it, so a render that succeeds has already been validated.

The renderer depends on a handful of crates (`yaml-rust2`, `minijinja`, `toml`, `serde`, `uuid`, `clap`, `color-eyre`, `fs-err`, `roxmltree`), so the first build fetches them from crates.io. Nothing is vendored, and builds after that work offline as usual.

One thing the list deliberately does **not** include:

- **The tools being themed.** You do not need helix, zellij, starship, bat, delta, broot, bottom, atuin, lazygit or tealdeer installed to build or install the loadout — no recipe invokes any of them. Minimal installs them when the loadout is applied, and the session setup runs inside the session.

## Quick Start

From a fresh clone:

```shell
git clone git@github.com:gominimal/cozyloadout.git
cd cozyloadout

just fetch-schemes             # optional: pull in the upstream schemes
just theme gruvbox-dark-hard   # render + bundle into cozy.zip
just install                   # unzip into ~/.config/minimal/loadouts/
```

Then apply the loadout and attach:

```shell
min session activate --loadout cozy --attach .
```

There is no post-install step — see [What happens on attach](#what-happens-on-attach).

Skip `just fetch-schemes` and `just theme` builds `minimal-dark`, which is checked in. `just schemes` lists everything you can pass it.

## Getting Started

### Choosing a scheme

A scheme argument is either a bare name or a path:

```shell
just theme                      # the default, minimal-dark
just theme minimal-light
just theme rose-pine-dawn       # anything under schemes/
just theme ~/my-scheme.yaml     # any path
```

Bare names are searched for in `schemes/`, then `schemes/vendor/base16/`, then `schemes/vendor/base24/` — so a scheme checked into this repo wins over a vendored one of the same name. The upstream collection also ships `tinted8` schemes; those are an 8-colour system the loadout can't use, and are never resolved to.

Both scheme formats are accepted: the current one with a nested `palette:` block, and the pre-2022 layout with `base00:` at the top level.

A base16 scheme *is* dark or light — it doesn't come in variants — so the build takes exactly one and there is no light/dark switch anywhere in the loadout. Changing palette means running `just theme` again.

To see what a scheme does before committing to it, `just render <scheme>` and read `build/`.

### Installing

`just install` replaces `~/.config/minimal/loadouts/cozy/` and unzips over it. Applying the loadout is Minimal's job; the patches in `build/cozy.toml` then land in `~/.config`.

Every theme file is named after the scheme — `~/.config/helix/themes/gruvbox-dark-hard.toml`, not a fixed name holding whatever colours you last built. One consequence: installing a *different* scheme leaves the previous one's theme files behind **in the session's `~/.config`**. They are inert, just extra entries in `hx --health`, `bat --list-themes` and broot's skins directory, and nothing removes them automatically. The loadout directory itself is rebuilt on every `just install`, so stale files don't pile up there.

### What happens on attach

`min attach` drops you straight into fish, which starts zellij. That handover is just the `SHELL = "fish"` var: Minimal starts the session's interactive shell from `$SHELL`. This needs **Minimal 0.5.4 or newer** — earlier versions always landed you in bash, and the loadout used to work around it with a `PROMPT_COMMAND` that exec'd fish before bash's first prompt, at the cost of Minimal's orientation banner.

The setup the patch system can't do runs as a **lifecycle hook**, declared in the loadout and shipped as a script in `cozy/hooks/`. `on_activate` fires when the session is created and does three things:

- **Points git at the two includes.** delta has no config file of its own and reads its settings out of git config, so the loadout ships an include; that include is also what makes delta git's pager in the first place, without which none of its styling is ever reached. The second include is git's own settings — `zdiff3` conflict markers, the histogram diff algorithm, moved-block detection, and difftastic wired up as `git dft`.
- **Builds bat's theme cache.** bat can't see a theme until its cache is built, and delta reads that same cache for in-diff highlighting, so without it both are off-scheme and delta falls back to Monokai.
- **Generates fish completions** for `rg`, `bat`, `atuin` and `procs`, which are the tools in the loadout that fish cannot complete on its own. Each one prints its own, so they are generated from the binaries in the session rather than checked in and left to go stale. The tools that can only emit completions at package-build time — `eza`, `fd`, `hyperfine`, `dust`, `bottom`, `difftastic`, `hexyl` — are not covered.

There is no `on_attach` hook. One was tried and the terminal background stopped being set while it existed; that was never explained, and there is now reason to think it was a different bug with the same symptom — see [AGENTS.md](AGENTS.md) if you're tempted to add one.

The first and third steps are **the only things the loadout writes outside `~/.config`**. The git one appends `include.path` entries and checks first, so an include of your own is never replaced or duplicated. The completions go to `$XDG_DATA_HOME/fish/vendor_completions.d`, which is *lower* precedence than `~/.config/fish/completions` — so a completion you wrote yourself still wins, and is never overwritten.

## What's In It

Themed from the palette, one config file each:

| | |
| --- | --- |
| **Shell and session** | fish, starship, zellij, atuin |
| **Editing and reading** | helix, bat, broot |
| **Git** | delta, lazygit, git itself |
| **System** | bottom, tealdeer |

Installed and configured through the environment rather than a theme file: jq, difftastic, duf, eza, fd, ripgrep, fzf, zoxide, procs, dust, hexyl, tokei, hyperfine, bandwhich, glow, gh, man-page rendering, and the GNU userland the session needs. `claude-code` comes along too.

## Documentation

- **[AGENTS.md](AGENTS.md)** — the working guide to this repository: the render pipeline, the template grammar, per-tool notes, the terminal-surface model, and a record of what has been verified versus assumed. Read it before changing a template.
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — development workflow and what we look for.
- **[Minimal documentation](https://minimal.dev/docs)** — sandboxes, `minimal.toml`, and the loadout schema this repository targets.

## Building and Testing

| Recipe | Does |
| --- | --- |
| `just` | List recipes and available schemes |
| `just theme [scheme]` | Render + bundle. Defaults to `minimal-dark` |
| `just render [scheme]` | Render only, no zip — for eyeballing `build/` first |
| `just install` | Bundle and unzip into `~/.config/minimal/loadouts/` |
| `just schemes` | List what `just theme` will accept |
| `just vendored` | List the vendored upstream schemes by name |
| `just fetch-schemes` | Clone the upstream scheme collection |
| `just test` | Run the renderer's tests |
| `just fmt` | Apply rustfmt |
| `just fmt-check` | rustfmt in check mode |
| `just lint` | Pedantic clippy, warnings denied |
| `just deny` | Supply-chain audit: advisories, licences, bans, sources |
| `just check` | The full local gate — everything CI runs |
| `just check-schemes` | Render every vendored scheme and check each result parses |
| `just clean` | Drop build artifacts (leaves `schemes/vendor/` alone) |

Before opening a PR:

```shell
just check
```

which runs the renderer's unit tests, `cargo fmt --check`, pedantic clippy with warnings denied, the `cargo-deny` supply-chain audit, both checked-in schemes rendered, `dash -n` over the generated hook, and `fish --no-execute` over the generated shell config.

If you touched a template or the renderer, also run `just check-schemes`. It renders the whole upstream collection (a few minutes) and expects zero failures — this is the check that catches a template which works for the two schemes here and breaks on a light scheme, or a legacy-format one.

**Edit `templates/`, never `build/`.** `build/` is deleted and rewritten on every render and is gitignored.

## Known gaps

1. **broot's file preview is off-scheme.** broot renders previews with syntect and only accepts one of six themes compiled into its binary — it cannot load an external theme the way bat can. The skin picks the nearest of those by luma, so the preview pane is the one surface in the loadout that isn't on your scheme. The panel chrome around it is.

2. **Old theme files aren't pruned** after switching schemes — see [Installing](#installing).

3. **Schemes aren't validated for legibility.** The renderer checks that all 16 slots are present and parseable, and nothing else. A scheme whose comments sit at 3:1 against its own background renders exactly as given.

4. **zellij and the terminal surface is untested.** fish sets the terminal's own background, foreground, cursor and 16-colour table over OSC escapes, which is what colours the surface behind things that don't paint their own — the prompt, `bat`, `delta`, bottom's widget text. The first apply happens in the outermost fish, before zellij starts, so the intended path doesn't depend on zellij forwarding them; the per-attach re-apply does, because by then the only fish running a prompt is inside a pane. Whether zellij forwards a *set* to the host terminal hasn't been confirmed in a real session. Terminals that don't implement a sequence swallow it silently, so the failure mode is "nothing happens" — the Linux console ignores these entirely, and Apple Terminal ignores the background one.

5. **Re-attaching takes one keypress to come back on-scheme.** A session's shell is started once and outlives the terminals that attach to it, so the palette has to be re-sent per attach — Minimal replays the screen when you attach, and a screen replay carries no terminal colours. The config re-sends on the next prompt or command, watching the same file Minimal rewrites on every attach to keep `TERM` current. So the first moment after a re-attach can show the previous terminal's colours; pressing return fixes it.

## Contributing

We'd love your help, and you don't need to be a Rust expert to pitch in: bug reports, better colours, docs fixes, and new tools are all valued contributions. Please [open an Issue](https://github.com/gominimal/cozyloadout/issues/new/choose) (or start a [Discussion](https://github.com/gominimal/cozyloadout/discussions) if it's large in scope) to outline the improvements you're seeking.

If you want to contribute code, templates, docs, etc., please head over to [CONTRIBUTING.md](./CONTRIBUTING.md) for the development workflow and what we look for in a contribution.

### Contributor License Agreement

Before we can merge your first pull request, you'll need to accept our **Individual Contributor License Agreement (ICLA)**. This is a one-time, ~30 second step: [CLA Assistant](https://cla-assistant.io/) will post a link on your PR, you click through, sign in with GitHub, and you're done. You're then covered for all future contributions to this repository.

If you're contributing on your employer's time, or with code your employer might own, your employer will also need a **Corporate CLA (CCLA)** on file listing you as an authorized contributor. See [CONTRIBUTING.md](./CONTRIBUTING.md) for details, or email **security@minimal.dev** if you need help getting one set up.

Full text: [ICLA](./legal/ICLA.md) · [CCLA](./legal/CCLA.md)

## Code of Conduct

We want everyone to feel welcome here, whatever your background or experience level. This project follows the [Contributor Covenant](./CODE_OF_CONDUCT.md); by participating (contributing code, filing issues, or joining discussions) you agree to uphold it.

## Security

If you believe you've found a security vulnerability, please email **security@minimal.dev** instead of opening a public issue. See [SECURITY.md](./SECURITY.md) for scope. We appreciate responsible disclosure and will get back to you quickly.

## License

This project is licensed under the [Apache License Version 2.0](LICENSE). See the [LICENSE](LICENSE) file for details.
