# Colours

The terminal side of this loadout is themed with a pair of base16 schemes —
**Minimal Dark** and **Minimal Light** — derived from the Minimal System design
tokens used by the web app. The goal is that a terminal running this loadout and
a browser showing the product read as the same design system.

The schemes live in [`cozy/base16/`](cozy/base16/) and are the single source of
truth. Everything else (helix themes, zellij themes, fish colours) is a mapping
of those 16 slots onto a specific tool.

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
| `cozy/fish/config.fish`             | fish   | Hex set explicitly, not via ANSI names       |

`cozy.toml` deploys the two helix themes and the zellij theme alongside the
existing config patches.

### Switching variants

- **fish** follows `$MINIMAL_THEME` (`dark` | `light`), declared in `cozy.toml`
  with a default of `dark`. Anything other than `light` gets the dark palette.
- **helix** pins the variant in `cozy/helix/config.toml`. For one session,
  `:theme minimal-light` works without touching the file.
- **zellij** pins the variant in `cozy/zellij/config.kdl`.

Helix and zellij have no env-var hook for this, so switching the whole loadout
is a two-line edit rather than one variable.

### Why fish sets hex instead of ANSI names

The previous config mixed hex with ANSI names (`green`, `brblack`, `bryellow`),
which meant half the prompt tracked the host terminal's 16-colour table and half
didn't. Setting hex throughout makes the shell match helix and zellij regardless
of the terminal emulator's own palette.

If you'd rather drive everything from the terminal's ANSI table instead, the
standard base16 mapping is:

| ANSI | 0      | 1      | 2      | 3      | 4      | 5      | 6      | 7      |
| ---- | ------ | ------ | ------ | ------ | ------ | ------ | ------ | ------ |
|      | base00 | base08 | base0B | base0A | base0D | base0E | base0C | base05 |
| **+8** | base03 | base09 | base01 | base02 | base04 | base06 | base0F | base07 |

---

## Not themed yet

`bat`, `delta`, `starship`, `broot` and `bottom` still use their own defaults.
`bat` and `delta` need a Sublime `.tmTheme` rather than a base16 file, which is
a bigger generated artifact than the configs above; `starship` and `broot` would
need their own config files, which this loadout doesn't currently ship.

---

## Regenerating

The YAML files are hand-maintained. When editing them, keep the derived files in
sync:

- helix: `[palette]` block at the bottom of each theme file
- zellij: hex values inline (both variants in `minimal.kdl`)
- fish: the `__min_b*` block in `config.fish`

Scope assignments in the two helix themes are intentionally identical — if you
change a scope in one, change it in the other.
