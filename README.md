# cozy

A themed terminal loadout for [minimal](https://github.com/gominimal/pkgs):
fish, helix, zellij, starship, bat, delta, broot and bottom, all wearing the
same base16 colour scheme.

The loadout is **built, not hand-maintained**. You give it a scheme and it
renders every config file from that one palette:

```sh
just theme gruvbox-dark-hard   # render + bundle into cozy.zip
just install                   # unzip into ~/.config/minimal/loadouts/
just bat-cache                 # rebuild bat's theme cache (needed after install)
just delta-include             # wire up delta in ~/.gitconfig (once, ever)
```

Any [tinted-theming](https://github.com/tinted-theming/schemes) scheme works, as
does any file in that format. `just fetch-schemes` clones the upstream
collection — 335 schemes — into `schemes/vendor/`.

---

## Contents

- [Using it](#using-it)
  - [Recipes](#recipes)
  - [Choosing a scheme](#choosing-a-scheme)
  - [Installing](#installing)
- [How it works](#how-it-works)
  - [Repository layout](#repository-layout)
  - [One scheme, not a pair](#one-scheme-not-a-pair)
  - [Everything is named after the scheme](#everything-is-named-after-the-scheme)
- [Editing the loadout](#editing-the-loadout)
  - [Template grammar](#template-grammar)
  - [Adding a file](#adding-a-file)
  - [Per-tool notes](#per-tool-notes)
  - [Format footguns](#format-footguns)
- [The terminal's own surface](#the-terminals-own-surface)
- [Known gaps](#known-gaps)
- [Appendix: the Minimal palettes](#appendix-the-minimal-palettes)

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
| `just fetch-schemes` | Clone the upstream scheme collection |
| `just bat-cache` | Rebuild bat's theme cache. Needed after every install |
| `just delta-include` | Point `~/.gitconfig` at the delta include. Once, ever |
| `just test` | Run the renderer's tests |
| `just clean` | Drop build artifacts (leaves `schemes/vendor/` alone) |

Building needs `just`, `zip` and a `cargo`/`rustc` pair. The renderer has no
crate dependencies, so it builds offline.

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

To see what a scheme does before committing to it, `just render <scheme>` and
read `build/`.

### Installing

`just install` unzips into `~/.config/minimal/loadouts/`. Applying the loadout
is minimal's job; the patches in `build/cozy.toml` then land in `~/.config`.

Two steps the patch system can't do, hence the extra recipes:

- **bat** compiles themes into a cache. Until `just bat-cache` runs,
  `bat --list-themes` won't show the theme and delta — which reads bat's cache
  for in-diff highlighting — falls back to Monokai.
- **delta** has no config file; it reads `[delta]` out of git config. Rather
  than overwrite `~/.gitconfig` (and take your name, email and remotes with
  it), the loadout drops an includable fragment and `just delta-include` wires
  it up. The include path doesn't change with the scheme, so that is a one-time
  step.

---

## How it works

```
schemes/<scheme>.yaml ─┐
                       ├─> cozy-theme ─> build/cozy.toml ─> cozy.zip ─> ~/.config/minimal/loadouts/
templates/**  ─────────┤                 build/cozy/**
templates/manifest.toml┘
```

### Repository layout

| Path | Role |
| --- | --- |
| `schemes/` | Scheme YAML. `minimal-dark` and `minimal-light` are checked in |
| `templates/` | The loadout, with colours as placeholders. **This is the source tree — edit here** |
| `templates/manifest.toml` | What gets rendered and where each result is patched to |
| `tools/cozy-theme/` | The renderer. Rust, no dependencies |
| `build/` | Output. Gitignored — never edit, it is deleted on every render |

`build/cozy.toml`'s `patches` list is generated from the manifest, so it cannot
describe a file that wasn't rendered. Adding a config file to the loadout means
adding a `[[file]]` entry and nothing else.

### One scheme, not a pair

A base16 scheme *is* dark or light; it does not come in variants. So the build
takes exactly one and there is no variant switch anywhere in the loadout — no
`$MINIMAL_THEME`, no `--theme-dark`/`--theme-light`, no second bottom config.
Switching palettes means rendering again.

The one place the build cares dark-vs-light is broot's preview pane, which can
only use one of six themes compiled into the binary and so can't take the
scheme. That decision comes from the **luma of `base00` against `base05`**, not
from the scheme's `variant:` field — a scheme whose background is plainly
darker than its foreground is dark whatever the metadata claims, and a backwards
preview pane is very visible.

Rendering from one palette is also what keeps the dark and light versions of a
theme honest. They used to be separate files carrying a standing warning to
hand-sync every scope assignment between them, and they had drifted.

### Everything is named after the scheme

`~/.config/helix/themes/gruvbox-dark-hard.toml`, not `minimal-dark.toml`
holding gruvbox's colours. `{slug}` in the manifest's `out` and `dest` fields
expands to the scheme slug — its filename, lowercased and hyphenated.

The consequence: rendering a *different* scheme and reinstalling leaves the
previous scheme's theme files behind in `~/.config`. They are inert — an extra
entry in `hx --health`, `bat --list-themes` and broot's skins directory — so
nothing is cleaned up automatically. Delete them by hand if they accumulate.

bottom is the exception: it has no concept of a theme name, so its scheme is
baked into `bottom.toml` and the filename is fixed.

---

## Editing the loadout

Edit `templates/`, never `build/`. Then `just render` and read the diff.

### Template grammar

Four constructs. An unrecognised placeholder is a hard error, never a
pass-through — a stray `{{typo}}` in a config file is silently ignored by the
tool that reads it, which is the worst possible failure mode.

**Colours.** Names follow tinted-builder's vocabulary, so upstream templates
mostly drop in. `XX` is `00`–`0F`, uppercase.

| Placeholder | Expands to | For |
| --- | --- | --- |
| `{{baseXX-hex}}` | `4a7aff` | Everything. Write the `#` yourself |
| `{{baseXX-rgb}}` | `74, 122, 255` | broot's `rgb(…)` form |
| `{{baseXX-rgb-r/g/b}}` | `74` | Single channels |
| `{{baseXX-hex-r/g/b}}` | `4a` | Single channels, hex |
| `{{baseXX-dec-r/g/b}}` | `0.2902` | Formats wanting 0–1 floats |

**Blends.** base16 has no dim surface colours, so a few are computed:

| Placeholder | Expands to |
| --- | --- |
| `{{mix base08 base00 15}}` | `341919` — base08 over base00 at 15% |
| `{{mix-rgb base0B base0A 50}}` | `165, 182, 76` — same, in broot's form |

**Metadata.** `{{scheme-name}}`, `{{scheme-slug}}`, `{{scheme-author}}`,
`{{scheme-system}}`, `{{scheme-variant}}` (`dark`/`light`, derived), and
`{{scheme-uuid}}` — a stable per-slug UUID for the `.tmTheme`, since Sublime
keys themes by UUID and two schemes sharing one would collide.

**Sections.** `{{#dark}}…{{/dark}}` and `{{#light}}…{{/light}}` keep their body
only for a matching scheme. They do not nest. Used once, for broot's preview
theme.

`templates/cozy.toml` additionally gets `{{patches}}`.

### Adding a file

1. Put the template under `templates/`.
2. Add a `[[file]]` block to `templates/manifest.toml` — `template`, `out`,
   `dest`, and `copy = true` if it has no colours in it (as
   `helix/languages.toml` does).
3. `just render` and look at `build/`.

### Per-tool notes

| Template | Tool | Notes |
| --- | --- | --- |
| `helix/themes/theme.toml` | helix | Scope assignments plus a `[palette]` block — the only tool with real palette indirection |
| `zellij/themes/theme.kdl` | zellij | UI-component spec (0.41+), not the legacy base-colors block: it states every surface explicitly, so a light scheme doesn't fight the format's dark assumptions |
| `fish/config.fish` | fish | Hex set explicitly, not via ANSI names; also sets the terminal's own surface and ANSI table over OSC |
| `bat/themes/theme.tmTheme` | bat, delta | Sublime `.tmTheme` — the only format bat can load. Scopes mirror the helix theme |
| `delta/delta.gitconfig` | delta | An include, not a gitconfig. Diff backgrounds are blends |
| `starship/starship.toml` | starship | Restyles modules only, no `format` |
| `broot/skins/skin.hjson` | broot | Decimal `rgb()`, plus the `good_to_bad` gauge ramp |
| `bottom/bottom.toml` | bottom | `[styles]` only; no palette indirection, so hex is inline throughout |

### Format footguns

- **git config**: `#` starts a comment, so every delta style value has to be
  quoted. An unquoted `minus-style = syntax #341919` parses as empty and delta
  quietly uses its own defaults.
- **broot** ignores unknown skin keys rather than erroring, so a mistyped key
  shows up as an unstyled widget, not a message.
- **starship** lowercases a style string before resolving it against the
  palette, so a `base0D` key can never be referenced. Its palette uses
  `base0d`; everything else in the loadout uses `base0D`.
- **fish**'s single quotes are not literal — they collapse `\\` to `\` before
  `printf` sees the format string, so `'\e]10;…\e\\\e]11;…'` loses an escape
  and prints a stray `e`. The config writes ST as `\e\x5c`, one sequence per
  `printf`.

---

## The terminal's own surface

helix, zellij and broot paint `base00` themselves; the prompt, `bat`, `delta`
and bottom's widget text don't, so those would fall through to whatever
background the emulator was configured with. `fish/config.fish` closes that with
OSC escapes: OSC 10/11/12 for foreground, background and cursor, and OSC 4 for
the 16-colour table, all reset on exit via 110/111/112/104.

The ANSI mapping is the standard base16 one:

| ANSI | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| | base00 | base08 | base0B | base0A | base0D | base0E | base0C | base05 |
| **+8** | base03 | base09 | base01 | base02 | base04 | base06 | base0F | base07 |

It costs what it always costs: slots 9–14 go to the greyscale ramp and the
leftover accents, so a program asking for **bright green gets `base01`, a
surface grey**. That is the standard base16 trade, and it is why fish itself
still sets hex rather than relying on the table.

`$__COZY_TERM_THEMED` is exported, so nested shells and zellij panes inherit it
and skip both the apply and the reset. Only the outermost fish owns the
terminal's colours — otherwise an inner shell exiting would strip the palette
from under the shell still running, and sshing into a box with this loadout
would leave your *local* terminal recoloured after logout.

Setting the surface is a request, not a guarantee: kitty, alacritty, wezterm,
foot, ghostty, contour, iTerm2 and xterm honour OSC 4/10/11/12, the Linux
console ignores them, and Apple Terminal ignores the background one. A terminal
that doesn't implement a sequence swallows it silently, so the failure mode is
"nothing happens".

---

## Known gaps

1. **broot's file preview** renders with syntect and only accepts one of six
   themes compiled into the binary — it cannot load an external theme the way
   bat can. The skin picks `OceanDark` or `GitHub` by luma, so the preview pane
   is the one surface in the loadout that isn't on-scheme. The panel chrome
   around it is.

2. **No scheme validation.** The renderer checks that all 16 slots are present
   and parseable, and nothing else. A scheme with 3:1 comments against its own
   background is rendered exactly as given.

3. **Nothing prunes old theme files** — see [Everything is named after the
   scheme](#everything-is-named-after-the-scheme).

4. **zellij and OSC is untested.** fish auto-starts zellij, so after the first
   prompt the OSC sequences would be emitted from inside a pane. zellij 0.44.3
   handles OSC, but whether it forwards a *set* to the host terminal hasn't been
   verified in a real session. The guard means only the outermost fish emits
   them, which is before zellij starts, so the intended path doesn't depend on
   passthrough. Worth confirming by eye.

---

## Appendix: the Minimal palettes

`schemes/minimal-dark.yaml` and `schemes/minimal-light.yaml` are derived from
the Minimal System design tokens used by the web app, so that a terminal running
this loadout and a browser showing the product read as the same design system.

They are two schemes among many the loadout can render — nothing below affects
the build.

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
| base07 | `#ffffff` | `gray-100-dark`             | Lightest (rare)                     | 18.4:1               |
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
| base07 | `#000000` | derived                         | (rare)                   | 19.3:1               |
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
blocks. In the light scheme the ramp runs the other way, per base16 convention:
`base00` is the lightest and the numbers get darker.

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

Three places where a literal port of the CSS would have produced a worse
terminal, and what was done instead:

1. **`base03` (comments) uses `gray-40`, not the light theme's `--outline`.**
   `gray-56` on `#f5f5f5` is 3.0:1 — too washed out for the colour you read most
   in an editor. `gray-40` gives 5.3:1 in light and a deliberately dim 3.2:1 in
   dark.

2. **Light accents are darker than the web tokens.** The CSS uses the same hex
   for `--error` and `--info` in both themes; `blue-56` on `#f5f5f5` is only
   3.5:1, which is defensible for a button fill and not for syntax you read as
   prose. Each light accent was dropped to a ~5–6.5:1 tier instead.

3. **`base06`/`base07` in the light scheme are invented.** In a light base16
   scheme the ramp has to keep going *past* `base05`, and `gray-8` is already
   the darkest grey in the system, so `#0a0a0a` and `#000000` fill the two
   slots. base16 treats both as rarely used, and nothing in the loadout
   references them except fish's OSC 4 table, which needs all sixteen.

One knowingly-kept wart: `red-56` is 4.41:1 on `#141414`, just under the 4.5
threshold. It's the design system's `--error` and swapping it would break the
tie to the product, so it stays.

### Where the derived colours go

Two families of colour in the rendered loadout aren't slots at all, and are
computed by the build rather than stored in the scheme — see
[`mix`](#template-grammar):

- **delta's diff backgrounds**, `base08`/`base0B` blended into `base00` at 15%
  (body) and 30% (emphasised). base16 has no dim red/green surface colours.

  | | dark | light |
  | --- | --- | --- |
  | minus       | `#341919` | `#ecd3d3` |
  | minus (emph)| `#541e1e` | `#e3b2b2` |
  | plus        | `#242f28` | `#d8e1db` |
  | plus (emph) | `#344a3b` | `#bacec1` |

  `base05` stays between 7.3:1 and 13.8:1 on those eight — the floor is the
  dark emphasised-plus background.

- **Gauge ramps** — broot's `good_to_bad` and bottom's
  `temp_graph_color_styles` — step `base0B → base0A → base09 → base08` with
  midpoints where the gauge needs more stops than the palette has slots.
