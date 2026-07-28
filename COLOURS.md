# Colours

The terminal side of this loadout is themed with a pair of base16 schemes —
**Minimal Dark** and **Minimal Light** — derived from the Minimal System design
tokens used by the web app. The goal is that a terminal running this loadout and
a browser showing the product read as the same design system.

The schemes live in [`cozy/base16/`](cozy/base16/) and are the single source of
truth. Everything else — helix, zellij, fish, bat, delta, starship, broot and
bottom — is a mapping of those 16 slots onto a specific tool.

---

## The palettes

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

---

## How the palettes were derived

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

---

## Deliberate departures from the web tokens

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
   slots. base16 treats both as rarely used, and no config in this loadout
   references them.

One knowingly-kept wart: `red-56` is 4.41:1 on `#141414`, just under the 4.5
threshold. It's the design system's `--error` and swapping it would break the
tie to the product, so it stays.

---

## What consumes the palettes

| File                                | Tool   | Notes                                        |
| ----------------------------------- | ------ | -------------------------------------------- |
| `cozy/base16/minimal-{dark,light}.yaml` | —  | Source of truth (tinted-theming format)      |
| `cozy/helix/themes/minimal-*.toml`  | helix  | Scope assignments identical; only the `[palette]` block differs |
| `cozy/zellij/themes/minimal.kdl`    | zellij | Both variants in one file, UI-component spec (zellij 0.41+) |
| `cozy/fish/config.fish`             | fish   | Hex set explicitly, not via ANSI names; also sets the terminal's own surface and ANSI table via OSC, and draws the greeting banner in `base0D` |
| `cozy/bat/themes/minimal-*.tmTheme` | bat, delta | Sublime `.tmTheme`; scope assignments mirror the helix themes |
| `cozy/bat/config`                   | bat    | Declares both variants, pins one             |
| `cozy/delta/minimal.gitconfig`      | delta  | Both variants as delta *features*; an include, not a gitconfig |
| `cozy/starship/starship.toml`       | starship | Both variants as starship palettes; restyles modules only, no `format` |
| `cozy/broot/skins/minimal-*.hjson`  | broot  | One skin file per variant                    |
| `cozy/broot/conf.hjson`             | broot  | Imports the pinned skin                      |
| `cozy/bottom/bottom{,-light}.toml`  | bottom | `[styles]` only; no palette indirection, so one whole file per variant |

`cozy.toml` deploys all of the above alongside the existing config patches.
Two of them need a step the patch system can't do:

- **bat** compiles themes into a cache. Run `just bat-cache` once the patches
  have landed; until then `bat --list-themes` won't show `minimal-dark`, and
  delta — which reads bat's cache for in-diff syntax highlighting — falls back
  to Monokai.
- **delta** has no config file of its own; it reads `[delta]` out of git config.
  Rather than overwrite `~/.gitconfig` (and take your name, email and remotes
  with it), the loadout drops an includable fragment and leaves the wiring to
  `just delta-include`, which runs
  `git config --global include.path ~/.config/git/minimal-delta.gitconfig`.

### Switching variants

- **fish** follows `$MINIMAL_THEME` (`dark` | `light`), declared in `cozy.toml`
  with a default of `dark`. Anything other than `light` gets the dark palette.
  The terminal's own background, foreground, cursor and ANSI table follow the
  same variable, since fish is what sets them — so this is the one switch that
  moves a surface no config file owns.
- **helix** pins the variant in `cozy/helix/config.toml`. For one session,
  `:theme minimal-light` works without touching the file.
- **zellij** pins the variant in `cozy/zellij/config.kdl`.
- **bat** pins it with `--theme` in `cozy/bat/config`; both variants are already
  declared there as `--theme-dark` / `--theme-light`. `--theme="auto"` makes bat
  query the terminal's background instead, per run.
- **delta** pins it with `features` in `cozy/delta/minimal.gitconfig`.
- **starship** pins it with `palette` in `cozy/starship/starship.toml`.
- **broot** pins it in the `imports` list in `cozy/broot/conf.hjson`; broot can
  also pick per-terminal via the `luma` form of an import.
- **bottom** has no switch at all, so the variant is which *file* is deployed —
  swap the `.config/bottom/bottom.toml` patch source in `cozy.toml`, or run
  `btm -C ~/.config/bottom/bottom-light.toml` for one session.

Only fish has an env-var hook, so switching the whole loadout is a handful of
one-line edits rather than one variable. Every one of them is a single token on
a single line, and each file says which line in its header comment.

### Why fish sets hex instead of ANSI names

The previous config mixed hex with ANSI names (`green`, `brblack`, `bryellow`),
which meant half the prompt tracked the host terminal's 16-colour table and half
didn't. Setting hex throughout makes the shell match helix and zellij regardless
of the terminal emulator's own palette.

### The terminal's own surface

Setting hex fixes fish's *output*, but not the surface behind it. helix, zellij
and broot each paint `base00` themselves; the prompt, `bat`, `delta` and
bottom's widget text don't, so those fall through to whatever background the
emulator was configured with.

`cozy/fish/config.fish` closes that with OSC escape sequences, driven by the
same `$MINIMAL_THEME` branch as everything else in that file:

| Sequence | Sets            | Reset |
| -------- | --------------- | ----- |
| OSC 10   | Foreground (`base05`) | OSC 110 |
| OSC 11   | Background (`base00`) | OSC 111 |
| OSC 12   | Cursor (`base05`)     | OSC 112 |
| OSC 4    | ANSI slots 0–15       | OSC 104 |

The ANSI mapping is the standard base16 one:

| ANSI | 0      | 1      | 2      | 3      | 4      | 5      | 6      | 7      |
| ---- | ------ | ------ | ------ | ------ | ------ | ------ | ------ | ------ |
|      | base00 | base08 | base0B | base0A | base0D | base0E | base0C | base05 |
| **+8** | base03 | base09 | base01 | base02 | base04 | base06 | base0F | base07 |

Setting it means a program that still speaks in ANSI names lands in the palette
rather than in the emulator's defaults — which is the other half of the problem
hex-only solved for fish alone. It has a cost: slots 9–14 go to the greyscale
ramp and the leftover accents, so a program asking for **bright green gets
`base01`, a dark surface grey**. That is the standard base16 trade, and it is
the reason fish itself still sets hex rather than relying on the table.

`$__MINIMAL_TERM_THEMED` is exported, so nested shells and zellij panes inherit
it and skip both the apply and the reset. Only the outermost fish owns the
terminal's colours and restores them on exit — otherwise an inner shell exiting
would strip the palette from under the shell still running, and sshing into a
box with this loadout would leave your *local* terminal recoloured after logout.

One editing hazard worth knowing: **fish's single quotes are not literal.** They
collapse `\\` to `\` before `printf` ever sees the format string, so the
natural-looking `'\e]10;…\e\\\e]11;…'` loses an escape and prints a stray `e`
where the second `ESC` should be. The config writes ST as `\e\x5c` and emits one
sequence per `printf` for that reason.

---

## Known gaps

Nothing in the loadout's package list is still on its own colours. Four
partial exceptions are worth knowing about:

1. **broot's file preview.** broot renders previews with syntect but only
   accepts one of six themes compiled into the binary — it cannot load an
   external `.tmTheme` the way bat can. The skins pick the nearest available
   (`OceanDark` for the dark variant, `GitHub` for the light one), so the
   preview pane is the one surface in the loadout that isn't Minimal. The
   panel chrome around it is.

2. **Diff backgrounds are blends, not slots.** base16 has no dim red/green
   surface colours, so delta's `minus-style` / `plus-style` backgrounds are
   `base08` and `base0B` mixed into `base00` at 15%, and the `*-emph` variants
   at 30%:

   | | dark | light |
   | --- | --- | --- |
   | minus       | `#341919` | `#ecd3d3` |
   | minus (emph)| `#541e1e` | `#e3b2b2` |
   | plus        | `#242f28` | `#d8e1db` |
   | plus (emph) | `#344a3b` | `#bacec1` |

   `base05` stays between 7.3:1 and 13.8:1 on those eight — the floor is the
   dark emphasised-plus background. broot's `good_to_bad` ramp and
   bottom's `temp_graph_color_styles` are built the same way, stepping
   `base0B → base0A → base09 → base08` with midpoints where the gauge needs
   more stops than the palette has slots.

3. **Slot names are lowercase in starship.** starship lowercases a style string
   before resolving it against the palette, so a `base0D` key can never be
   referenced and the module silently renders unstyled. Its palettes use
   `base0d`; everything else in the loadout uses the `base0D` spelling.

4. **The OSC sequences depend on the emulator, and zellij is untested.** Setting
   the terminal surface is a request, not a guarantee: kitty, alacritty, wezterm,
   foot, ghostty, contour, iTerm2 and xterm all honour OSC 4/10/11/12, the Linux
   console ignores them, and Apple Terminal ignores the background one. A
   terminal that doesn't implement a sequence swallows it silently, so the
   failure mode is "nothing happens", not corruption.

   The open question is **zellij**, which fish auto-starts here, so after the
   first prompt the OSCs are emitted from inside a pane. zellij 0.44.3 does
   handle OSC, but whether it forwards a *set* to the host terminal or absorbs
   it hasn't been verified in a real session — the guard means only the
   outermost fish emits them, which is before zellij starts, so the intended
   path doesn't depend on passthrough. Worth confirming by eye.

---

## Regenerating

The YAML files are hand-maintained. When editing them, keep the derived files in
sync:

- helix: `[palette]` block at the bottom of each theme file
- zellij: hex values inline (both variants in `minimal.kdl`)
- fish: the `__min_b*` block in `config.fish` — all sixteen slots, since the
  OSC 4 table needs the ones fish's own colours never reference
- bat: hex inline in both `.tmTheme` files, then `just bat-cache`
- delta: hex inline in both feature blocks, plus the blended diff backgrounds
- starship: the two `[palettes.minimal_*]` blocks (lowercase slot names)
- broot: hex inline in both skins, including the `good_to_bad` ramp
- bottom: hex inline in both `[styles]` files

Scope assignments in the two helix themes are intentionally identical — if you
change a scope in one, change it in the other. The same holds for each of the
other dark/light pairs: they differ only in the palette, so the light file of
each pair is derived from the dark one by substituting the sixteen slots.

Two formats have footguns worth remembering when editing:

- In **git config**, `#` starts a comment, so every delta style value has to be
  quoted. An unquoted `minus-style = syntax #341919` parses as empty and delta
  quietly falls back to its own defaults.
- **broot** ignores unknown skin keys rather than erroring, so a mistyped key
  shows up as an unstyled widget, not as a message.
