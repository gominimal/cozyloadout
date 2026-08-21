# cozy

A themed terminal loadout for [minimal](https://github.com/gominimal/pkgs):
fish, helix, zellij, starship, bat, delta, broot, bottom, atuin, lazygit and
tealdeer, all wearing the same base16 colour scheme — plus jq, difftastic, duf
and man-page rendering, which are themed through the environment rather than a
config file.

Rather than shipping hand-maintained config files, this repo ships *templates*
and a small renderer. You hand it a base16 scheme; it generates every config
file in the set from that one palette, bundles them, and installs them where
minimal expects them. Re-theming the whole terminal is one command.

Two schemes are included — **Minimal Dark** and **Minimal Light**, derived from
the Minimal System design tokens so a terminal and the web app read as the same
design system. Any [tinted-theming](https://github.com/tinted-theming/schemes)
scheme also works, or any file in that format; `just fetch-schemes` pulls in the
upstream collection, around 480 base16 and base24 schemes.

---

## Quick start

From a fresh clone:

```sh
git clone git@github.com:gominimal/cozyloadout.git
cd cozyloadout

just fetch-schemes             # optional: pull in ~480 upstream schemes
just theme gruvbox-dark-hard   # render + bundle into cozy.zip
just install                   # unzip into ~/.config/minimal/loadouts/
```

Then apply the loadout with minimal and attach. There is no post-install step —
see [What happens on attach](#what-happens-on-attach).

Skip `just fetch-schemes` and `just theme` builds `minimal-dark`, which is
checked in. `just schemes` lists everything you can pass it.

## Requirements

To build and install the loadout you need:

| Tool | Used for |
| --- | --- |
| [`just`](https://github.com/casey/just) | running the recipes |
| Rust — `cargo` and `rustc`, 1.71+ | building the renderer |
| `bash` | recipe bodies (the multi-line recipes carry a bash shebang) |
| `zip`, `unzip` | `just bundle` and `just install` |
| `git` | cloning this repo; also `just fetch-schemes` |

Plus the usual POSIX userland — `find`, `sed`, `sort`, `head`, `wc`, `tr`,
`mkdir`, `rm` — which any Unix already has.

`just check` additionally uses `fish` and `dash` to syntax-check the two files
that are shipped as code, and a `python3` with `tomllib` (3.11+) to parse the
generated loadout TOML. Each is skipped with a message if it isn't there, so the
recipe still runs — it just checks less.

That is the whole list. Two things it deliberately does **not** include:

- **Crate dependencies.** The renderer has none, so `cargo` never reaches the
  network and the build works offline.
- **The tools being themed.** You do not need helix, zellij, starship, bat,
  delta, broot, bottom, atuin, lazygit or tealdeer installed to build or
  install the loadout — no recipe invokes any of them. minimal installs them
  when the loadout is applied, and the session setup runs inside the session.

---

## Using it

### Recipes

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
| `just check` | Everything CI runs: tests, both schemes rendered, syntax and TOML checks |
| `just check-schemes` | Render every vendored scheme and check each result parses |
| `just clean` | Drop build artifacts (leaves `schemes/vendor/` alone) |

### Choosing a scheme

A scheme argument is either a bare name or a path:

```sh
just theme                      # the default, minimal-dark
just theme minimal-light
just theme rose-pine-dawn       # anything under schemes/
just theme ~/my-scheme.yaml     # any path
```

Bare names are searched for in `schemes/`, then `schemes/vendor/base16/`, then
`schemes/vendor/base24/` — so a scheme checked into this repo wins over a
vendored one of the same name. The upstream collection also ships `tinted8`
schemes; those are an 8-colour system the loadout can't use, and are never
resolved to.

Both scheme formats are accepted: the current one with a nested `palette:`
block, and the pre-2022 layout with `base00:` at the top level.

A base16 scheme *is* dark or light — it doesn't come in variants — so the build
takes exactly one and there is no light/dark switch anywhere in the loadout.
Changing palette means running `just theme` again.

To see what a scheme does before committing to it, `just render <scheme>` and
read `build/`.

### Installing

`just install` replaces `~/.config/minimal/loadouts/cozy/` and unzips over it.
Applying the loadout is minimal's job; the patches in `build/cozy.toml` then land
in `~/.config`.

Every theme file is named after the scheme —
`~/.config/helix/themes/gruvbox-dark-hard.toml`, not a fixed name holding
whatever colours you last built. One consequence: installing a *different*
scheme leaves the previous one's theme files behind **in the session's
`~/.config`**. They are inert, just extra entries in `hx --health`,
`bat --list-themes` and broot's skins directory, and nothing removes them
automatically. The loadout directory itself is rebuilt on every `just install`,
so stale files don't pile up there.

### What happens on attach

`min attach` drops you straight into fish, which starts zellij. The session's
shell is `bash --noprofile --rcfile <daemon rc> -i` and reads none of your
startup files, so the loadout arranges the handover through `PROMPT_COMMAND`,
which bash evaluates before its first prompt. The one thing that costs is
minimal's orientation banner, which the same variable would otherwise print;
your terminal's `TERM` is kept current by the daemon regardless.

The setup the patch system can't do runs as a **lifecycle hook**, declared in the
loadout and shipped as a script in `cozy/hooks/`:

- **`on_activate`** — when the session is created — points git at delta's
  config, and builds bat's theme cache. delta has no config file of its own and
  reads its settings out of git config, so the loadout ships an include; that
  include is also what makes delta git's pager in the first place, without which
  none of its styling is ever reached. bat can't see a theme until its cache is
  built, and delta reads that same cache for in-diff highlighting, so without it
  both are off-scheme and delta falls back to Monokai.

There is no `on_attach` hook. One was tried and the terminal background stopped
being set while it existed; that was never explained, and there is now reason to
think it was a different bug with the same symptom — see AGENTS.md if you're
tempted to add one.

Pointing git at the include is **the only thing the loadout writes outside
`~/.config`**. It appends a single `include.path` entry and checks first, so an
include of your own is never replaced or duplicated.

---

## Known gaps

1. **broot's file preview is off-scheme.** broot renders previews with syntect
   and only accepts one of six themes compiled into its binary — it cannot load
   an external theme the way bat can. The skin picks the nearest of those by
   luma, so the preview pane is the one surface in the loadout that isn't on
   your scheme. The panel chrome around it is.

2. **Old theme files aren't pruned** after switching schemes — see
   [Installing](#installing).

3. **Schemes aren't validated.** The renderer checks that all 16 slots are
   present and parseable, and nothing else. A scheme whose comments sit at 3:1
   against its own background renders exactly as given.

4. **zellij and the terminal surface is untested.** fish sets the terminal's own
   background, foreground, cursor and 16-colour table over OSC escapes, which is
   what colours the surface behind things that don't paint their own — the
   prompt, `bat`, `delta`, bottom's widget text. The first apply happens in the
   outermost fish, before zellij starts, so the intended path doesn't depend on
   zellij forwarding them; the per-attach re-apply (below) does, because by then
   the only fish running a prompt is inside a pane. Whether zellij forwards a
   *set* to the host terminal hasn't been confirmed in a real session.
   Terminals that don't implement a sequence swallow it silently, so the failure
   mode is "nothing happens" — the Linux console ignores these entirely, and
   Apple Terminal ignores the background one.

5. **Re-attaching takes one keypress to come back on-scheme.** A session's shell
   is started once and outlives the terminals that attach to it, so the palette
   has to be re-sent per attach — minimal replays the screen when you attach,
   and a screen replay carries no terminal colours. The config re-sends on the
   next prompt or command, watching the same file minimal rewrites on every
   attach to keep `TERM` current. So the first moment after a re-attach can show
   the previous terminal's colours; pressing return fixes it.

6. **Detaching leaves your local terminal wearing the scheme.** Nothing in the
   session can notice a detach — fish is still running in there — and what
   minimal sends the departing terminal resets text attributes and modes but not
   colours. If that bothers you, reset them where you launch from:

   ```fish
   function mina
       min session attach $argv
       printf '\e]110\e\x5c\e]111\e\x5c\e]112\e\x5c\e]104\e\x5c'
   end
   ```

7. **The loadout can collide with a project.** Contributions from a loadout and
   from a project's `minimal.toml` are merged, and the same variable with two
   different values — or the same patch destination from two different sources —
   fails the activation rather than picking a winner. This loadout contributes
   `SHELL`, `EDITOR`, `PAGER`, `PROMPT_COMMAND` and thirteen patches under
   `~/.config`, so a project that sets `EDITOR` to anything but `hx` will not
   activate alongside it. The way out is your user policy's `ignore` list.

---

Working on this repo — architecture, template grammar, and the reasoning behind
the design — is documented in [AGENTS.md](AGENTS.md).
