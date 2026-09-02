<h1 align="center">Cozy Loadout</h1>

<p align="center"><strong>One Palette, Every Tool</strong><br>A themed terminal loadout for Minimal — fish, helix, zellij, starship, bat, delta and more, generated from a single base16 scheme.</p>

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

## What is it?

A [Minimal](https://github.com/gominimal/minimal) loadout that themes your whole terminal from one base16 scheme.

Instead of hand-maintained dotfiles, this repo ships **templates and a renderer**. You give it a scheme; it generates every config file in the set from that one palette, bundles them, and installs them where Minimal expects them. Re-theming the terminal is one command.

Twelve tools get a themed config file, and a dozen more are configured through the environment. They all land on the same sixteen colours, so a diff in `delta`, a file in `bat`, and that same file open in `helix` agree.

**Minimal Dark** and **Minimal Light** are included, matching the Minimal System design tokens. Any [tinted-theming](https://github.com/tinted-theming/schemes) scheme works too — `just fetch-schemes` pulls the upstream base16 and base24 collection, all of which is rendered in CI.

## Quick Start

```shell
git clone git@github.com:gominimal/cozyloadout.git
cd cozyloadout

just fetch-schemes             # optional: pull in the upstream schemes
just theme gruvbox-dark-hard   # render + bundle into cozy.zip
just install                   # unzip into ~/.config/minimal/loadouts/

min session activate --loadout cozy --attach .
```

`min attach` drops you into fish, which starts zellij. Everything else — git pointed at delta's config, bat's theme cache, fish completions — is done by the loadout's `on_activate` hook. There is no manual setup step.

Skip `just fetch-schemes` and `just theme` builds `minimal-dark`, which is checked in.

## Requirements

| Tool | Used for |
| --- | --- |
| [`just`](https://github.com/casey/just) | running the recipes |
| Rust — `cargo` and `rustc`, 1.85+ | building the renderer |
| `bash` | recipe bodies |
| `zip`, `unzip` | `just bundle` and `just install` |
| `git` | cloning this repo; also `just fetch-schemes` |
| Minimal **0.5.4+** | installing the loadout — earlier versions don't support `$LOADOUT_ROOT` patch sources or the `SHELL` handover |

Plus the usual POSIX userland. `fish` and `dash` are optional, used only by `just check` to syntax-check generated files.

You do **not** need the tools being themed — helix, zellij, bat and the rest. Minimal installs those when the loadout is applied.

## Choosing a scheme

```shell
just theme                      # the default, minimal-dark
just theme minimal-light
just theme rose-pine-dawn       # anything under schemes/
just theme ~/my-scheme.yaml     # any path
```

Bare names resolve from `schemes/`, then `schemes/vendor/base16/`, then `schemes/vendor/base24/`, so a scheme checked in here wins over a vendored one of the same name. Both scheme formats are accepted — the current `palette:` block and the pre-2022 top-level layout. The upstream `tinted8` schemes are an 8-colour system this can't use and are never resolved to.

A base16 scheme *is* dark or light, so there's no light/dark switch in the loadout — changing palette means running `just theme` again. Use `just render <scheme>` to look at `build/` before committing to one.

## Installing

`just install` replaces `~/.config/minimal/loadouts/cozy/`. Applying the loadout is Minimal's job; the patches then land in the session's `~/.config`.

Theme files are named after the scheme rather than a fixed name, so switching schemes doesn't overwrite the previous one's. The loadout directory is wiped and rebuilt on every `just install`, so nothing accumulates there. A **session that was already running** under the old scheme keeps the old theme files in its `~/.config`, though: patches are filesystem mappings established when the sandbox is built, so they only add. They're inert — extra entries in `hx --health` and `bat --list-themes` — and a session created after the switch has only the current scheme's.

The loadout writes outside `~/.config` in exactly two places: it appends `include.path` entries to your git config (checking first, so your own includes are never touched), and it writes fish completions to `$XDG_DATA_HOME/fish/vendor_completions.d`, which is lower precedence than your own.

## What's In It

Themed from the palette, one config file each:

| | |
| --- | --- |
| **Shell and session** | fish, starship, zellij, atuin |
| **Editing and reading** | helix, bat, broot |
| **Git** | delta, lazygit, git |
| **System** | bottom, tealdeer |

Also installed and configured through the environment: jq, difftastic, duf, eza, fd, ripgrep, fzf, zoxide, procs, dust, hexyl, tokei, hyperfine, bandwhich, glow, gh, man-page rendering, `claude-code`, and the GNU userland.

## Building and Testing

| Recipe | Does |
| --- | --- |
| `just` | List recipes and available schemes |
| `just theme [scheme]` | Render + bundle. Defaults to `minimal-dark` |
| `just render [scheme]` | Render only, no zip |
| `just install` | Bundle and unzip into `~/.config/minimal/loadouts/` |
| `just schemes` | List what `just theme` will accept |
| `just vendored` | List the vendored upstream schemes |
| `just fetch-schemes` | Clone the upstream scheme collection |
| `just check` | The full local gate — everything CI runs |
| `just check-schemes` | Render every vendored scheme |
| `just clean` | Drop build artifacts |

`just --list` shows the rest, including the individual `test`, `fmt`, `lint` and `deny` recipes that `just check` bundles.

**Edit `templates/`, never `build/`** — `build/` is deleted and rewritten on every render. Run `just check` before opening a PR, and `just check-schemes` too if you touched a template or the renderer.

[AGENTS.md](AGENTS.md) is the guide to the internals: the render pipeline, the template grammar, per-tool notes, and a record of what's been verified versus assumed.

## Known gaps

1. **broot's file preview is off-scheme.** broot renders previews with syntect and only accepts one of six themes built into its binary. The skin picks the nearest by luma, so the preview pane is the one surface not on your scheme.

2. **A long-lived session keeps old theme files** after you switch schemes. The loadout directory is rebuilt on each install, but Minimal's patches only add files to a session, never remove them — so the previous scheme's themes linger in an already-running session's `~/.config`. A new session is clean. See [Installing](#installing).

3. **Schemes aren't checked for legibility.** The renderer validates structure, not contrast. A scheme whose comments sit at 3:1 against its own background renders exactly as given.

4. **The terminal surface isn't verified through zellij.** fish sets the terminal's background, foreground, cursor and 16-colour table over OSC escapes. The first apply happens before zellij starts; the per-attach re-apply goes through a zellij pane, and whether zellij forwards those to the host terminal hasn't been confirmed. Terminals that ignore a sequence do so silently.

5. **Re-attaching takes one keypress to come back on-scheme.** A screen replay carries no terminal colours, so the palette is re-sent on the next prompt. The first moment after re-attaching can show the previous terminal's colours; pressing return fixes it.

## Contributing

Bug reports, better colours, docs fixes, and new tools are all welcome. [Open an issue](https://github.com/gominimal/cozyloadout/issues/new/choose) or start a [discussion](https://github.com/gominimal/cozyloadout/discussions), and see [CONTRIBUTING.md](./CONTRIBUTING.md) for the workflow.

Before we can merge your first pull request you'll need to accept our **Individual Contributor License Agreement**. [CLA Assistant](https://cla-assistant.io/) posts a link on your PR; it takes about 30 seconds and covers all your future contributions. If you're contributing on your employer's time, they'll also need a **Corporate CLA** on file. Full text: [ICLA](./legal/ICLA.md) · [CCLA](./legal/CCLA.md).

## Code of Conduct

This project follows the [Contributor Covenant](./CODE_OF_CONDUCT.md); by participating you agree to uphold it.

## Security

Please email **security@minimal.dev** rather than opening a public issue. See [SECURITY.md](./SECURITY.md) for scope.

## License

Licensed under the [Apache License Version 2.0](LICENSE).
