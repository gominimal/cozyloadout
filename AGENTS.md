| `on_activate` fires on a real session | verified — `min session activate` reported it running |
| Any `on_attach` hook breaks fish's OSC 11 background | observed; mechanism unexplained |
| Patches applied before `on_activate` | **unmeasured** — session `$HOME` is isolated, no non-interactive probe |
# AGENTS.md

Working notes for this repo. [README.md](README.md) covers using the loadout;
this covers changing it.

## Ground rules

1. **Edit `templates/`, never `build/`.** `build/` is deleted and rewritten on
   every render, and it is gitignored. A change made there evaporates on the
   next `just theme`.
2. **`cozy.toml` is generated.** Its `patches` list comes from
   `templates/manifest.toml`, so it can't describe a file that wasn't rendered.
   Edit `templates/cozy.toml` for packages and vars, the manifest for patches.
3. **Re-render and look at the diff** after touching a template:
   `just render && git diff --no-index <old> build/` or just read `build/`.
4. **Don't commit build artifacts.** `.gitignore` covers `build/`, `cozy.zip`,
   `tools/cozy-theme/target/` and `schemes/vendor/`.

## Architecture

```
schemes/<scheme>.yaml ─┐
                       ├─> cozy-theme ─> build/cozy.toml ─> cozy.zip ─> ~/.config/minimal/loadouts/
templates/**  ─────────┤                 build/cozy/**
templates/manifest.toml┘
```

| Path | Role |
| --- | --- |
| `schemes/` | Scheme YAML. `minimal-dark` and `minimal-light` are checked in |
| `templates/` | The loadout, with colours as placeholders. The source tree |
| `templates/manifest.toml` | What gets rendered, and where each result is patched to |
| `tools/cozy-theme/` | The renderer. Rust, no dependencies |
| `build/` | Output. Gitignored, rewritten every render |

### The manifest

One `[[file]]` per output: `template`, `out`, `dest`, and `copy = true` for
files with no colours in them (only `helix/languages.toml` today). `{slug}` in
`out`/`dest` expands to the scheme slug.

Adding a file to the loadout is: drop the template in `templates/`, add a
`[[file]]` block, `just render`, read `build/`. Nothing else — the patches list
follows.

### The renderer

`tools/cozy-theme/src/main.rs`, ~720 lines, **zero crate dependencies** — the
scheme format is a flat map and the template grammar is four constructs, so both
parsers are hand-rolled. That keeps `just theme` buildable with nothing but a
rustc/cargo pair: no registry fetch, no vendor directory, works offline.

Keep it that way unless there's a strong reason not to. `rust-version = "1.71"`
is declared so an old toolchain fails clearly; that floor is *derived* (it's
where `[char; N]` gained its `Pattern` impl, used in `split_kv`) and has never
been tested — development was on 1.97.1.

`just test` runs 12 unit tests. They cover both scheme formats, the quoted-`#`
parsing hazard, luma-derived variant, `mix` against hand-computed values,
placeholder errors, and slug/UUID behaviour. Tests each get a **private temp
directory** — the slug comes from the filename and tests run in parallel in one
process, so a shared path both clobbers content across threads and collapses
distinct slugs. Two rounds of confusing failures came from exactly that.

## Template grammar

An unrecognised placeholder is a hard error, never a pass-through — a stray
`{{typo}}` in a config file is silently ignored by the tool that reads it, which
is the worst possible failure mode.

Colour names follow tinted-builder's vocabulary so upstream templates mostly
drop in. `XX` is `00`–`0F`, uppercase.

| Placeholder | Expands to | For |
| --- | --- | --- |
| `{{baseXX-hex}}` | `4a7aff` | Everything. Write the `#` yourself |
| `{{baseXX-rgb}}` | `74, 122, 255` | broot's `rgb(…)` form |
| `{{baseXX-rgb-r/g/b}}` | `74` | Single channels |
| `{{baseXX-hex-r/g/b}}` | `4a` | Single channels, hex |
| `{{baseXX-dec-r/g/b}}` | `0.2902` | Formats wanting 0–1 floats |
| `{{mix base08 base00 15}}` | `341919` | base08 over base00 at 15% |
| `{{mix-rgb base0B base0A 50}}` | `165, 182, 76` | Same, in broot's form |

Metadata: `{{scheme-name}}`, `{{scheme-slug}}`, `{{scheme-author}}`,
`{{scheme-system}}`, `{{scheme-variant}}`, `{{scheme-uuid}}`.
`templates/cozy.toml` additionally gets `{{patches}}`.

Sections: `{{#dark}}…{{/dark}}` and `{{#light}}…{{/light}}` keep their body only
for a matching scheme. They do not nest. Used once, for broot's preview theme.

`mix` exists because base16 has no dim surface colours: delta's diff backgrounds
and broot's `good_to_bad` gauge ramp have to be computed from slots. It rounds
to nearest, so a 50% mix is the true midpoint. The hand-written broot ramp
values it replaced truncated instead, which is why three of them shifted by one
channel value in the first render.

## Design decisions

**One scheme, not a pair.** A base16 scheme is dark or light; it doesn't come in
variants. So there is no `$MINIMAL_THEME`, no `--theme-dark`/`--theme-light`, no
second bottom config. This collapsed four dark/light file pairs into four
templates and retired a standing warning that scope assignments had to be
hand-synced between them — which had already been violated: zellij's unselected
ribbon used `base04` in the dark file and `base03` in the light one.

**Variant comes from luma, not metadata.** `base00` vs `base05` relative
luminance decides, ignoring the scheme's `variant:` field. A scheme whose
background is plainly darker than its foreground is dark whatever the metadata
claims, and the only consumer — broot's preview theme — is very visible when
backwards.

**Names track the scheme.** `~/.config/helix/themes/<slug>.toml`. A fixed name
holding another scheme's colours is a trap. The cost is that old theme files
accumulate; deliberately not solved, since tracking ownership to delete them
seemed worse than the mess.

**No scheme validation.** Considered and dropped: a contrast report with a
`--strict` flag. The instruction was to render what the user asked for.

## Session setup and the shell handover

Two separate mechanisms, for two reasons. Don't collapse them.

### Lifecycle hooks — the setup steps

Declared in `templates/cozy.toml`, scripts in `templates/hooks/`:

```toml
[[lifecycle_hooks]]
on_activate = { type = "external", value = "./hooks/on-activate.sh" }
```

**Do not add an `on_attach` hook.** There was one, briefly, as a safety net for
the bat cache. Declaring *any* `on_attach` hook stops fish's OSC 11 from taking
effect, so the terminal background never gets set; `min session activate
--no-hooks` restores it. The hook script was not the cause — captured on a pty
it writes zero bytes, and it runs before fish anyway, so fish's sequence should
win regardless. `on_attach` is the only event that runs on the attached terminal
("the other three have no terminal and their output is captured into the daemon
log"), which is what singles it out. Whatever the mechanism is, it lives in the
attach path, and minimal was being actively developed when this surfaced —
re-test before reintroducing one.

If something genuinely needs to happen per-attach, put it in the fish config:
that is what sets the terminal colours in the first place, and it demonstrably
coexists with them.

Schema, per <https://minimal.dev/docs/reference/loadouts>: `[[lifecycle_hooks]]`
is an array of tables; events are `on_activate`, `on_destroy`, `on_attach`,
`on_detach`; each value is `{ type = "inline"|"external", value = …, timeout = …
}` with timeout defaulting to 60s and **capped at 300s — over the cap the file
is rejected at parse time**. External `value` resolves against the directory
beside the loadout file, which for `cozy.toml` is `cozy/` — exactly where the
renderer puts things, so `./hooks/x.sh` just works. Setup hooks run in
declaration order, teardown in reverse. Scripts run under POSIX `sh` unless a
shebang overrides.

Four things to keep in mind when editing these:

1. **`on_activate` must never exit non-zero.** "A failing `on_activate` fails
   the activation — the session does not become attachable." Both scripts end in
   a bare `exit 0` and guard every step. Verified: with no tools on `PATH` at
   all, and with a `bat` that exits 3, both still exit 0.
2. **No `grep`.** It is *not* in the loadout's package list — `ripgrep` is, and
   coreutils doesn't ship grep. The first draft used `grep -q` for the
   already-configured check; with grep absent the negation inverts and appends a
   duplicate `include.path` on every activation. Both scripts now use `case`,
   which is a shell builtin. Verified: three activations with no grep on `PATH`
   leave exactly one entry.
3. **POSIX `sh`, not bash.** Check with `dash -n build/cozy/hooks/*.sh`.
4. **Patch ordering against `on_activate` is undocumented and unmeasured.** It
   is not guaranteed the `.tmTheme` exists when the activate hook runs, so the
   hook only builds the cache if the file is there. It could not be measured:
   session `` is isolated from the host, and `min session attach` has no
   non-interactive command flag to probe with. Symptom if the ordering is
   unlucky: `bat --list-themes` in a fresh session omits the scheme, and delta
   falls back to Monokai. Fix it in the fish config, not with an on_attach hook.

The daemon was unreachable on this machine, so the hooks have been run directly
against throwaway `$HOME`s but **never observed firing from a real session**.

### PROMPT_COMMAND — the shell handover

```toml
PROMPT_COMMAND = "unset PROMPT_COMMAND; command -v fish >/dev/null && exec fish"
```

**Lifecycle hooks cannot replace this**, which is counterintuitive enough to be
worth writing down. The attach shell is `bash --noprofile -l`, sourcing no
startup files; there is no documented way to choose what you land in; and
`on_attach` runs *before* that bash and is timeout-capped, so a hook cannot be
your interactive session — it would be killed at ≤300s and drop you into bash
anyway. The reference is explicit: "Interactive setup happens through
environment variables instead." `PROMPT_COMMAND` is that environment variable.

Filed upstream as `minimal#957` — it works, but a shell-specific side door isn't
an interface.

Two things about it are load-bearing and were verified, not assumed:

- **`unset` first.** It fires once rather than per-prompt, and unsetting drops
  it from the *exported* environment too, so the fish it execs into — and any
  bash nested under that — never sees it. Measured: `PROMPT_COMMAND` count in
  fish's env is 0. No relaunch loop.
- **The `command -v` guard.** `exec` of a missing binary behaves differently
  depending on where it runs. Typed at an interactive prompt it prints
  `not found` and leaves the shell alive; from inside `PROMPT_COMMAND` it takes
  the shell down (bash 5.3, **exit 127**). The original suggestion omitted the
  guard on the belief that a failed exec is survivable. It isn't here —
  unguarded, a missing fish means every attach exits immediately.

No `$ZELLIJ` guard, deliberately. That's needed when exec'ing zellij directly;
here fish's own config runs `zellij setup --generate-auto-start`, whose script
already carries the check. Adding it would be actively wrong — inside a pane
running bash, exec'ing fish is what you want.

### Why the delta step uses `--add`

Worth preserving, because the obvious form is destructive. The original
`just delta-include` recipe ran `git config --global include.path <ours>`, which
*replaces* rather than appends. Demonstrated against a throwaway `$HOME`:

```
before: ~/my-own-stuff.gitconfig
after:  ~/.config/git/cozy-delta.gitconfig
```

Survivable when run by hand once; a trap when it runs automatically. The hook
checks `--get-all` first and uses `--add`, matching on basename so an entry
already added as an absolute path is recognised rather than duplicated.

The bat step probes `bat --list-themes` (~8ms measured) and only rebuilds when
the scheme is absent, so the steady state is one cheap check.

This setup has lived in three places: `just` recipes run by hand, then a
guarded block in the fish config, now lifecycle hooks. Each move was a real
improvement; the *logic* has barely changed, so read the hook scripts rather
than reinventing it.

## Per-tool notes

| Template | Tool | Notes |
| --- | --- | --- |
| `helix/themes/theme.toml` | helix | Scopes plus a `[palette]` block — the only tool with real palette indirection |
| `zellij/themes/theme.kdl` | zellij | UI-component spec (0.41+), not the legacy base-colors block: it states every surface explicitly, so a light scheme doesn't fight the format's dark assumptions |
| `fish/config.fish` | fish | Hex set explicitly, not via ANSI names; also sets the terminal surface and ANSI table over OSC |
| `bat/themes/theme.tmTheme` | bat, delta | Sublime `.tmTheme` — the only format bat can load. Scopes mirror the helix theme |
| `delta/delta.gitconfig` | delta | An include, not a gitconfig. Diff backgrounds are blends |
| `starship/starship.toml` | starship | Restyles modules only, no `format` |
| `broot/skins/skin.hjson` | broot | Decimal `rgb()`, plus the `good_to_bad` ramp |
| `bottom/bottom.toml` | bottom | `[styles]` only; no palette indirection, so hex is inline throughout |

### Format footguns

- **git config**: `#` starts a comment, so every delta style value has to be
  quoted. An unquoted `minus-style = syntax #341919` parses as empty and delta
  quietly uses its own defaults.
- **broot** ignores unknown skin keys rather than erroring, so a mistyped key
  shows up as an unstyled widget, not a message.
- **starship** lowercases a style string before resolving it against the
  palette, so a `base0D` key can never be referenced. Its palette uses
  `base0d`; everything else uses `base0D`.
- **fish** does not expand `\n` in double-quoted strings, and its single quotes
  are not literal — they collapse `\\` to `\` before `printf` sees the format
  string, so `'\e]10;…\e\\\e]11;…'` loses an escape and prints a stray `e`. The
  config writes ST as `\e\x5c`, one sequence per `printf`, and the blank line in
  the greeting is a real newline inside the quotes.
- **TOML** forbids newlines inside an inline table. The original `cozy.toml`
  wrapped its `patches` entries across two lines, which was never valid TOML —
  it survived only because minimal's parser tolerates it. The renderer emits one
  line per entry; don't "tidy" that back.
- **just** `--list` shows only the **last** line of a preceding comment block.
  Recipes with multi-line comments carry a `[doc('…')]` attribute, or the
  listing displays a fragment of prose.

## The terminal surface

helix, zellij and broot paint `base00` themselves; the prompt, `bat`, `delta`
and bottom's widget text don't. `fish/config.fish` closes that with OSC 10/11/12
(foreground, background, cursor) and OSC 4 (the 16-colour table), reset on exit
via 110/111/112/104. Standard base16 ANSI mapping:

| ANSI | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| | base00 | base08 | base0B | base0A | base0D | base0E | base0C | base05 |
| **+8** | base03 | base09 | base01 | base02 | base04 | base06 | base0F | base07 |

It costs what it always costs: slots 9–14 go to the greyscale ramp and leftover
accents, so a program asking for bright green gets `base01`, a surface grey.
That's the standard base16 trade, and why fish itself sets hex rather than
relying on the table.

## Verifying changes

```sh
just test                                    # 12 unit tests
just render minimal-dark && just render minimal-light
fish --no-execute build/cozy/fish/config.fish   # fish syntax check
```

Beyond that, the checks that have actually caught bugs here:

- **Render the whole upstream collection.** Loop `schemes/vendor/base16` and
  `base24` through the binary; 529 files, expect zero failures. `tinted8` is an
  8-colour system and is correctly rejected.
- **Parse the generated `cozy.toml` with a real TOML parser.** This is what
  caught the inline-table bug — every scheme was producing invalid TOML.
  See *Environment* below for finding a Python with `tomllib`.
- **Walk the quick start in a throwaway clone** with `build/`, `cozy.zip`,
  `schemes/vendor/` and `target/` removed. This caught `just schemes` exiting 1
  on a fresh clone — `find` errors on the not-yet-existing vendor directories
  and `set -o pipefail` propagated it, which took out bare `just` as well since
  the default recipe calls it.
- **Build offline** with `CARGO_NET_OFFLINE=true` and an empty `CARGO_HOME`, to
  keep the no-dependencies claim honest.
- **Sandbox `$HOME`** when testing the first-run setup — it writes
  `~/.gitconfig`. `env HOME=/tmp/somewhere fish …`.

Colour changes are best checked by diffing the *set* of colours in a rendered
file against a known-good one, rather than diffing the files, since comments and
structure move around.

## Environment

Notes for this machine specifically; verify rather than assume if things look
odd.

- **The tool shell is fish**, so bash-isms (`set -- $x`, arrays, `[[`) fail
  silently or oddly in one-liners. Write a script to a scratchpad file and run
  `bash script.sh`.
- **`grep` is ugrep**, `sed` is **BSD sed**, `find` is **bfs**. Consequences hit
  in practice: `\?` is not a BRE quantifier in BSD sed (strip `.yaml` and `.yml`
  separately, not with `\.ya\?ml$`), and bfs is breadth-first so `find | head -1`
  needs an explicit `sort` for deterministic results.
- **`python3` is 3.9 with no `tomllib`.** A newer one lives in the nix store;
  find it with
  `ls /nix/store/*/bin/python3.1[1-9]` and test `import tomllib`. The exact
  store path changes.
- **`just` is at `/run/current-system/sw/bin/just`.** There are Linux `just`
  builds in the nix store that will fail with "exec format error" on this
  Darwin host — don't reach for those.
- The build runs on the **host**, not inside a minimal session.

## Verified vs assumed

Worth knowing before relying on any of it:

| Claim | Status |
| --- | --- |
| Renders 529/529 upstream base16+base24 schemes | verified |
| Generated `cozy.toml` is valid TOML for every scheme | verified |
| Renderer builds with no network | verified |
| `PROMPT_COMMAND` hands over to fish; unset prevents loops | verified against a pty |
| Unguarded `exec` in `PROMPT_COMMAND` exits 127 | verified |
| Hooks are idempotent, tolerate missing/broken tools, preserve other git includes | verified against throwaway `$HOME`s |
| Hook scripts are POSIX-sh clean | verified with `dash -n` |
| `just` recipes | all run except `install` against the real `~/.config` |
| Lifecycle hook schema and semantics | taken from the loadouts reference, not observed |
| `on_activate` fires on a real session | verified — `min session activate` reported running it |
| Any `on_attach` hook breaks fish's OSC 11 background | observed; `--no-hooks` restores it, mechanism unexplained |
| Patches applied before `on_activate` | **unmeasured** — session `$HOME` is isolated, and attach has no non-interactive probe |
| Rust floor of 1.71 | **derived, never tested** — only 1.97.1 available |
| `ctrl-w` detaches, per the fish greeting | **unverified** — not in `min --help`, `min session attach --help`, or the binary's strings |
| zellij forwards OSC sets to the host terminal | **unverified** — see README's Known gaps |

## Appendix: the Minimal palettes

`schemes/minimal-dark.yaml` and `schemes/minimal-light.yaml` are derived from
the Minimal System design tokens used by the web app. Provenance only — nothing
here affects the build, and neither scheme is special to the renderer.

### Minimal Dark

| Slot   | Hex       | Source token                | Role                                | Contrast on `base00` |
| ------ | --------- | --------------------------- | ----------------------------------- | -------------------- |
| base00 | `#141414` | `gray-8` / `--background`   | Default background                  | —                    |
| base01 | `#292929` | `gray-16` / `--card`        | Lighter surface: gutter, statusline | —                    |
| base02 | `#3d3d3d` | `gray-24` / `--border`      | Selection                           | —                    |
| base03 | `#666666` | `gray-40` / `--outline`     | Comments, invisibles                | 3.2:1                |
| base04 | `#8f8f8f` | `gray-56` / `--muted-fg`    | Secondary text                      | 5.7:1                |
| base05 | `#e0e0e0` | `gray-88` / `--foreground`  | Default foreground                  | 14.0:1               |
| base06 | `#ebebeb` | `gray-92`                   | Light foreground (rare)             | 15.5:1               |
| base07 | `#ffffff` | `gray-100-dark`             | Lightest — the greeting's mark      | 18.4:1               |
| base08 | `#e93535` | `red-56` / `--error`        | Variables, tags, diff removed       | 4.4:1                |
| base09 | `#dd8440` | derived — orange            | Constants, numbers, link URLs       | 6.5:1                |
| base0A | `#cca300` | `yellow-40`                 | Types, classes, search highlight    | 7.7:1                |
| base0B | `#7ec897` | `green-64` / `--success`    | Strings, diff added                 | 9.3:1                |
| base0C | `#6bbec7` | derived — cyan              | Operators, escapes, regex           | 8.6:1                |
| base0D | `#4a7aff` | `blue-56` / `--info`        | Functions, headings                 | 4.8:1                |
| base0E | `#aa81da` | derived — purple            | Keywords, storage                   | 6.0:1                |
| base0F | `#af715a` | derived — brown             | Deprecated, embedded tags           | 4.7:1                |

### Minimal Light

| Slot   | Hex       | Source token                    | Role                     | Contrast on `base00` |
| ------ | --------- | ------------------------------- | ------------------------ | -------------------- |
| base00 | `#f5f5f5` | `gray-96-light` / `--background`| Default background       | —                    |
| base01 | `#ebebeb` | `gray-92-light`                 | Lighter surface          | —                    |
| base02 | `#cccccc` | `gray-80-light`                 | Selection                | —                    |
| base03 | `#666666` | `gray-40` / `--muted-fg`        | Comments, invisibles     | 5.3:1                |
| base04 | `#3d3d3d` | `gray-24`                       | Secondary text           | 10.0:1               |
| base05 | `#141414` | `gray-8` / `--foreground`       | Default foreground       | 16.9:1               |
| base06 | `#0a0a0a` | derived                         | (rare)                   | 18.2:1               |
| base07 | `#000000` | derived                         | The greeting's mark      | 19.3:1               |
| base08 | `#b81414` | `red-40`                        | Variables, tags          | 6.1:1                |
| base09 | `#8b4a18` | derived — orange                | Constants, numbers       | 6.2:1                |
| base0A | `#7a6200` | `yellow-24`                     | Types, classes           | 5.4:1                |
| base0B | `#317247` | derived — green                 | Strings, diff added      | 5.3:1                |
| base0C | `#2d6f76` | derived — cyan                  | Operators, escapes       | 5.3:1                |
| base0D | `#2850bd` | derived — blue                  | Functions, headings      | 6.5:1                |
| base0E | `#7743b1` | derived — purple                | Keywords, storage        | 6.0:1                |
| base0F | `#754938` | derived — brown                 | Deprecated               | 7.0:1                |

Contrast figures are WCAG 2.1 ratios computed against each scheme's own
`base00`, not estimates.

### How they were derived

**The greyscale ramp is a direct lift.** `base00`–`base05` are the design
system's own `--background` → `--card` → `--border` → `--outline` →
`--muted-foreground` → `--foreground` chain, read off the `:root` and `.dark`
blocks. In the light scheme the ramp runs the other way, per base16 convention.

**Accents reuse Minimal System tokens wherever they exist** — `--error`,
`--info`, `--success` and the yellow ramp cover four of the eight accent slots.

**The other four are derived.** base16 needs red, orange, yellow, green, cyan,
blue, purple and brown; the Minimal System palette has no orange, cyan, purple
or brown. Those were generated to sit inside the existing palette's conventions
rather than pulled from another scheme:

- Shade numbers in the design system are HSL lightness (`gray-40` = `#666666` =
  L 40%, `red-56` = `#e93535` = L 56%), so derived shades follow the same rule.
- Saturation matches the nearest existing family — the palette runs red at
  S 80%, green at S 40%, yellow and blue at S 100%. Derived hues sit at
  S 35–70%, which keeps them from shouting next to `green-64`.
- Hues are placed at 26° (orange), 186° (cyan), 268° (purple) and 16° (brown),
  spacing them evenly against the existing 0° / 48° / 140° / 224°.

If the Minimal System palette later gains real orange/cyan/purple ramps, those
four slots should be replaced with the official values.

### Deliberate departures from the web tokens

1. **`base03` (comments) uses `gray-40`, not the light theme's `--outline`.**
   `gray-56` on `#f5f5f5` is 3.0:1 — too washed out for the colour you read most
   in an editor. `gray-40` gives 5.3:1 in light and a deliberately dim 3.2:1 in
   dark.

2. **Light accents are darker than the web tokens.** The CSS uses the same hex
   for `--error` and `--info` in both themes; `blue-56` on `#f5f5f5` is only
   3.5:1, defensible for a button fill and not for syntax you read as prose.
   Each light accent was dropped to a ~5–6.5:1 tier instead.

3. **`base06`/`base07` in the light scheme are invented.** In a light base16
   scheme the ramp has to keep going *past* `base05`, and `gray-8` is already
   the darkest grey in the system, so `#0a0a0a` and `#000000` fill the two
   slots. base16 treats both as rarely used; the loadout references them in
   fish's OSC 4 table, which needs all sixteen, and in the greeting, which draws
   the Minimal mark in `base07`. That last one is why the light values matter:
   `base07` is `#ffffff` in the dark scheme and `#000000` here, so the mark is
   the scheme's maximum contrast against `base00` either way rather than
   literally white.

One knowingly-kept wart: `red-56` is 4.41:1 on `#141414`, just under the 4.5
threshold. It's the design system's `--error` and swapping it would break the
tie to the product, so it stays.

### Colours the build computes

Neither of these are slots; see `mix` in *Template grammar*.

- **delta's diff backgrounds**, `base08`/`base0B` blended into `base00` at 15%
  (body) and 30% (emphasised):

  | | dark | light |
  | --- | --- | --- |
  | minus       | `#341919` | `#ecd3d3` |
  | minus (emph)| `#541e1e` | `#e3b2b2` |
  | plus        | `#242f28` | `#d8e1db` |
  | plus (emph) | `#344a3b` | `#bacec1` |

  `base05` stays between 7.3:1 and 13.8:1 on those eight — the floor is the dark
  emphasised-plus background.

- **Gauge ramps** — broot's `good_to_bad` and bottom's
  `temp_graph_color_styles` — step `base0B → base0A → base09 → base08` with
  midpoints where the gauge needs more stops than the palette has slots.
