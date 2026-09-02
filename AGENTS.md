# AGENTS.md

Working notes for this repo. [README.md](README.md) covers using the loadout;
this covers changing it.

## Ground rules

1. **Edit `templates/`, never `build/`.** `build/` is deleted and rewritten on
   every render, and it is gitignored. A change made there evaporates on the
   next `just theme`.
2. **`cozy.toml` is generated.** Its `patches` list comes from
   `templates/manifest.toml`, so it can't describe a file that wasn't rendered,
   and its `packages` list from `templates/packages.toml`. Edit those, not the
   generated array — `templates/cozy.toml` is for vars and hooks.
   Patch sources are emitted as `$LOADOUT_ROOT/<path>` — minimal expands that to
   the loadout's own directory, so it needs **minimal 0.5.4 or newer**. Before
   0.5.4 the path had to be spelled out as
   `~/.config/minimal/loadouts/<loadout>/<path>`.
3. **Re-render and look at the diff** after touching a template:
   `just render && git diff --no-index <old> build/` or just read `build/`.
4. **Don't commit build artifacts.** `.gitignore` covers `build/`, `*.zip`,
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
| `tools/cozy-theme/` | The renderer. Rust; see the dependency table under *The renderer* |
| `build/` | Output. Gitignored, rewritten every render |

### The manifest

One `[[file]]` per output: `template`, `out`, `dest`, and `copy = true` for
files with no colours in them (only `helix/languages.toml` today). `{slug}` in
`out`/`dest` expands to the scheme slug and `{loadout}` to the loadout's name
(`--loadout`, `LOADOUT` in the justfile, `cozy` by default) — the latter is what
keeps a renamed loadout from shipping a file called `cozy-delta.gitconfig`.

Values are quoted strings or bare `true`/`false`, and may carry a trailing
`# comment`. Anything else is an error naming the line: the parser used to
`trim_matches('"')`, which turned `out = "bat/config" # note` into a filename
with the comment still attached and rendered to it without complaint.

Adding a file to the loadout is: drop the template in `templates/`, add a
`[[file]]` block, `just render`, read `build/`. Nothing else — the patches list
follows.

### The renderer

`tools/cozy-theme/src/main.rs`. It used to carry **zero crate dependencies**, so
`just theme` built offline with nothing but a rustc/cargo pair. That is no
longer true — the setup wizard will pull in ratatui, so the property was going
anyway, and the hand-rolled parsers it justified are gone:

| Crate | Replaces |
| --- | --- |
| `yaml-rust2` | the scheme parser — a hand-rolled line splitter that existed for quoting hazards. `default-features = false` drops `encoding_rs`; it only handles non-UTF-8 input and `read_to_string` already requires UTF-8 |
| `minijinja` | the template engine — section expansion, `{{…}}` substitution, and the `mix` evaluator |
| `toml` + `serde` | a hand-rolled TOML subset for `templates/manifest.toml` |
| `uuid` (v5) | a hand-rolled FNV-1a/xorshift hash that stamped a **version-4** nibble onto a deterministic value |
| `clap` (derive) | hand-rolled flag parsing |
| `color-eyre` | `Result<_, String>` throughout |
| `fs-err` | the `format!("{}: {e}", path.display())` prefix repeated at every io call |

Lints are **pedantic clippy**, set in `Cargo.toml`'s `[lints.clippy]` so a bare
`cargo clippy` picks them up; `just lint` adds `-D warnings` and `just check`
depends on it. One targeted `#[allow]` survives, on the narrowing cast in
`mix()`: the value is rounded and clamped into `0..=255`, and Rust's
float-to-int `as` saturates rather than wrapping, so the two "unchecked
narrowing" lints do not apply. Keep allows local and reasoned like that one
rather than widening the crate-level config.

`rust-version = "1.88"`, set by the dependencies rather than by this crate's own
source — currently `darling`, reached through ratatui. Development is on 1.97.1,
so the floor is never exercised locally; the `msrv` CI job compiles against it.

One crate was *rejected* after testing: see the handlebars note under Template
grammar. `serde_yaml` and its forks were rejected too — the original is
archived, `serde_yml` is deprecated, and `serde_yaml_ng` has not shipped since
May 2024. `yaml-rust2` has more recent downloads than all of them and contains
no `unsafe`, where the serde forks parse through transpiled C.

`just test` runs 15 unit tests. They cover both scheme formats, the quoted-`#`
parsing hazard, luma-derived variant, `mix` against hand-computed values,
placeholder errors, slug/UUID behaviour, and the manifest parser — including the
value-with-a-trailing-comment case, which used to be accepted and silently
rendered to a path with the comment still glued on. Tests each get a **private
temp directory** — the slug comes from the filename and tests run in parallel in
one process, so a shared path both clobbers content across threads and collapses
distinct slugs. Two rounds of confusing failures came from exactly that; a fixed
path under the system temp dir has the same problem across concurrent runs, so
everything goes through `temp_dir()`.

### The library

`tools/cozy-theme/src/lib.rs` holds what the renderer and the wizard both need:
`Rgb`, `mix`, `SLOTS`, `Scheme` (the YAML parser) and `discover`. It was split
out of `main.rs` when the wizard needed to load schemes; the split changed no
output, verified by hashing all 535 rendered trees before and after.

Anything only the renderer uses — the template engine, the manifest, the build
— stays in `main.rs`. Pedantic clippy's library-API lints (`missing_errors_doc`,
`missing_panics_doc`, `must_use_candidate`) are switched off for it in
`Cargo.toml`: it is an internal crate with `publish = false`, and writing those
sections for callers who are both in this repo is documentation nobody reads.

### The wizard

`tools/cozy-theme/src/bin/cozy-wizard.rs`, a second binary in the same package
(`default-run = "cozy-theme"` keeps a bare `cargo run` pointing at the
renderer). `just wizard` runs it. Three screens so far: greeting, schemes,
themes.

Its first page asks which fish greeting to install: the mark drawn with Symbols
for Legacy Computing (`▃🭕🭏🭕🭏 M I N I M A L`, the U+1FB00 block, Unicode 13)
or the block-element mark that ships today (U+2580–U+259F). The newer glyphs are
missing from many fonts, so both are drawn **unstyled, in the terminal's own
foreground** — the user is judging their font, and a preview that is not the
real thing is worthless. `blocks_art_matches_the_shipped_fish_greeting` pins the
preview to `templates/fish/config.fish` so the two cannot drift.

**The greeting choice is not wired to the render yet.** The wizard prints it and
exits; nothing reads it.

Its second page offers to fetch the upstream scheme collection, and does. What
it runs is decided by what is on disk (`FetchKind`):

| On disk | Action |
| --- | --- |
| nothing | `git clone --depth 1` |
| a git checkout | `git pull --ff-only` |
| a directory that is not a checkout | **refuses** |

That last row is a deliberate divergence from `just fetch-schemes`, which `rm
-rf`s the directory and re-clones. Deleting a directory the user did not name,
from a full-screen UI that has just covered the scrollback, is not something to
do on a keypress — the wizard explains itself and points at the recipe instead.

The git command runs on a **thread**, with the event loop polling on a 100 ms
tick and a spinner. Inline it would freeze the terminal for the length of a
clone with no way out of a stalled network but killing the process. With no
fetch running the loop goes back to blocking on a key, so an idle wizard costs
nothing.

The repo URL and flags live in one place here and one place in the justfile;
`fetch_matches_the_justfile_recipe` asserts they agree, since a wizard that
cloned a different repo than the recipe would be worse than no wizard.

Its third page browses every scheme on disk and **re-paints the whole UI in the
selected one** as the cursor moves. Two things make that affordable: `discover`
lists names from filenames without parsing anything (the collection is ~480
schemes after de-duplication, and parsing them all at startup to show twenty
would be absurd), and only the selected scheme is loaded. A scheme that fails
to parse leaves the previous colours up rather than blanking the screen — a
broken file upstream must not wedge the wizard.

`Theme` maps the palette onto the roles the preview paints with, which is a
mapping rather than a choice: base16 already assigns the meanings. The preview
shows the palette itself, a starship-style prompt, a syntax-highlighted
snippet, and a diff whose backgrounds come from `mix()` — the same computation
`templates/delta/delta.gitconfig` uses, so the one part of the loadout's colour
that is *computed* rather than picked is visible before you commit to a scheme.

The highlighting is a hand-written span list over a fixed snippet, not a
grammar. Real highlighting means syntect; for a preview of a snippet we control,
tagging the spans by hand costs nothing and colours the same slots the helix and
bat themes assign.

`scrolling_repaints_the_ui_in_the_selected_scheme` asserts the rendered
background equals the *selected scheme's* base00 — not merely that it changed,
so repainting in some other scheme's colours still fails.

Two things about the terminal that are easy to get backwards, both commented in
the source: `color_eyre::install()` has to come *before* `ratatui::init()`
(ratatui's restoring panic hook must be installed last, or a panic leaves you in
raw mode on the alternate screen), and `ratatui::restore()` runs unconditionally
*before* the `?` (or an error report prints onto the alternate screen and
disappears with it).

Layout heights are constants rather than measurements, because
`Paragraph::line_count` is behind ratatui's `unstable-rendered-line-info`
feature. `INTRO_ROWS` was guessed wrong twice, each time silently eating the end
of a sentence at ordinary terminal widths, so `intro_fits_in_its_rows` renders
through `TestBackend` at six widths and asserts the last word survives.

**Rows are the scarce dimension, columns are not.** Horizontal padding is free.
A blank row above and below the art in each option box costs four, and a gap
between the boxes one more — which an 80x24 terminal does not have spare on top
of a caption line inside each box. Two things bought the room: the captions ride
on the bottom border (`title_bottom`, no rows), and the spacer row above the
first box is gone, since each box carries its own top padding.

What is left is a ladder in `draw`, tried most- to least-generous, first fit
wins:

| | padding | gap |
| --- | --- | --- |
| 80x24 | yes | yes |
| 60x20 | yes | no |
| 50x18 | no | yes |

**The art never yields.** Without that rule an unconditional padding clipped the
block mark to one line at 50x18 — and the mark is the entire point of the
screen. Four tests hold the trade from both ends:
`art_survives_a_small_terminal` and `comforts_yield_before_the_art_does` for the
floor, `padding_is_present_when_there_is_room` and
`gap_separates_the_options_when_there_is_room` for the ceiling, so the fallback
cannot quietly become the only path.

### Packages

`templates/packages.toml` splits the package list three ways, and the renderer
flattens it into `cozy.toml`'s `packages` array — minimal's schema wants a flat
list, but a flat list has nowhere to record *why* a package is there.

| Group | What it is | Droppable |
| --- | --- | --- |
| `base` | GNU userland, man pages, archives, ssh, git — what any session needs whatever loadout is applied | no |
| `cozy` | the shell, multiplexer, editor, pager and the search/navigation tools the fish config builds its aliases around | no |
| `optional` | everything else, with a description and a `default` for the wizard to preselect | yes |

**Being themed is not what makes a package required.** A manifest entry can
carry a `package`, and the renderer skips that entry when the package was not
selected — so atuin, broot, lazygit and tealdeer are all themed *and* all
optional: decline one and its config is simply not rendered, its patch not
listed, its name not installed. The line is about what the loadout *is*: a
session without `fd` or `zoxide` is a different shell to work in, one without
`tealdeer` is the same shell with fewer manual pages.

Two tests hold the tagging from both ends, because a wrong tag fails silently:
`manifest_package_tags_name_real_optional_packages` (a tag naming a package
that is not optional would never skip anything) and
`every_optional_themed_tool_tags_its_config` (an untagged optional tool would
install its config when declined).

What makes declining safe is that nothing assumes these tools: the fish config
guards all fifteen it touches with `command -q`, and the hook's
`gen_completion` returns early when a binary is missing. One caveat, noted
inline in the file: **difftastic** is wired up as `git dft` in
`templates/git/git.gitconfig`, and a gitconfig alias cannot check whether its
binary exists, so dropping the package leaves that alias failing when run.

The `[N files]` the renderer prints counts what it actually wrote, not
`entries.len()` — those stopped agreeing once entries could be skipped.

## Template grammar

Templates are **Jinja**, rendered by minijinja. Two settings are load-bearing:

- **`UndefinedBehavior::Strict`** — an unrecognised placeholder is a hard error,
  never a blank. A stray `{{typo}}` that renders to nothing is silently ignored
  by the tool that reads the config, which is the worst possible failure mode.
- **`keep_trailing_newline`** — minijinja drops a template's final newline by
  default. These are config files and shell scripts; the trailing newline is
  part of the contract. Leaving this off strips it from *every* rendered file.

Auto-escaping is off (the default for these extensions) and must stay off —
escaping `&` or `"` would corrupt the outputs.

**Placeholder names are snake_case**, not the dashed tinted-builder spelling,
because Jinja parses `base00-hex` as a subtraction. `XX` is `00`–`0F`,
uppercase.

| Placeholder | Expands to | For |
| --- | --- | --- |
| `{{baseXX_hex}}` | `4a7aff` | Everything. Write the `#` yourself |
| `{{baseXX_rgb}}` | `74, 122, 255` | broot's `rgb(…)` form |
| `{{baseXX_rgb_r/g/b}}` | `74` | Single channels |
| `{{ mix('base08','base00',15) }}` | `341919` | base08 over base00 at 15% |
| `{{ mix_rgb('base0B','base0A',50) }}` | `165, 182, 76` | Same, in broot's form |

`mix` exists because base16 has no dim surface colours: delta's diff backgrounds
and broot's gauge ramp are computed from slots rather than picked from them. The
slot arguments are **quoted strings** — bare words are variable references, and
in an earlier handlebars trial they resolved to empty without erroring.

Metadata: `{{scheme_name}}`, `{{scheme_slug}}`, `{{scheme_author}}`,
`{{scheme_variant}}`, `{{scheme_uuid}}`, and `{{loadout_name}}` — the loadout's
own name, for the files that have to spell it (the delta include's path, the
hook's `include.path` check). `templates/cozy.toml` additionally gets
`{{patches}}`.

Sections use Jinja's conditional against the `dark` / `light` booleans:
`{% if dark %}…{% endif %}`. Five uses, all inline on one line — keep them that
way, since a block spanning newlines brings Jinja's whitespace-control rules
into play and those change the rendered bytes.

### What this grammar deliberately is not

The dashed `{{baseXX-hex}}` names and `{{#dark}}…{{/dark}}` sections were a
mustache-ish dialect chosen so upstream tinted-builder templates would mostly
drop in. Moving to Jinja ends that, which is why the `-hex-r/g/b` and
`-dec-r/g/b` placeholder families were dropped at the same time — they had no
consumer in this repo and existed only for that compatibility.

Before settling on minijinja, handlebars was tried because its syntax looked
like an exact match for the old dialect. It is not, and the failure is worth
recording: `{{#dark}}` is not a mustache section to handlebars-rust but a
missing helper, and `{{mix base08 base00 15}}` renders `MIX(,,15)` — bare
parameters resolve as variables, come back empty, and **strict mode does not
catch it**. Do not revisit it.

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

**Do not add an `on_attach` hook without re-testing from scratch.** There was
one, briefly, as a safety net for the bat cache, and while it existed the
terminal background stopped being set; `min session activate --no-hooks`
restored it. That was never explained — the hook script writes zero bytes when
captured on a pty — and the observation is now doubtful, because the loadout had
a *second* bug at the time that produces the same symptom on its own: the OSC
palette was emitted once per shell, so any attach after the first landed on an
unthemed surface whether a hook existed or not (see *The terminal surface*).
Both were live at once, and only one of them was understood.

What is known about the attach path, from the source rather than from the
symptom: `Host::attach` publishes the connection env, flushes a `vt100` screen
dump of the session to the newly attached client, installs the binding, and only
then runs the `on_attach` hooks, each of which gets the session's pty slave
opened fresh for write. Nothing there resets a palette. The screen dump does not
carry one either, which is the actual reason a re-attach comes up unthemed.

If something genuinely needs to happen per-attach, prefer the fish config: it
already re-applies the palette per attach off the same signal minimal uses for
`TERM`, and it demonstrably coexists with everything else.

Schema, per <https://minimal.dev/docs/reference/loadouts>: `[[lifecycle_hooks]]`
is an array of tables; events are `on_activate`, `on_destroy`, `on_attach`,
`on_detach`; each value is `{ type = "inline"|"external", value = …, timeout = …
}` with timeout defaulting to 60s and **capped at 300s — over the cap the file
is rejected at parse time**. External `value` resolves against the directory
beside the loadout file, named after the loadout — `cozy.toml`'s scripts live in
`cozy/`, exactly where the renderer puts things, so `./hooks/x.sh` just works.
Setup hooks run in declaration order, teardown in reverse. Scripts run under
POSIX `sh` unless a shebang overrides, and are fed to the interpreter on stdin
rather than as a file argument.

Four things to keep in mind when editing these:

1. **`on_activate` must never exit non-zero.** "A failing `on_activate` fails
   the activation — the session does not become attachable." The script ends in
   a bare `exit 0` and guards every step. Verified: with no tools on `PATH` at
   all, and with a `bat` that exits 3, it still exits 0.
2. **No `grep`.** It is *not* in the loadout's package list — `ripgrep` is, and
   coreutils doesn't ship grep. The first draft used `grep -q` for the
   already-configured check; with grep absent the negation inverts and appends a
   duplicate `include.path` on every activation. The script uses `case`, which
   is a shell builtin. Verified: three activations with no grep on `PATH` leave
   exactly one entry.
3. **POSIX `sh`, not bash.** `just check` runs `dash -n` over the rendered
   hooks.
4. **Patches land before `on_activate`** — settled from the source, not
   measured end-to-end. A loadout's patches become filesystem mappings on the
   sandbox (`EnvPatches` → `Vec<common::FsMapping>`), established when the
   container is built, and a hook run is not even planned unless there is a
   session leader pid to inject into. The `.tmTheme` is therefore on disk when
   the hook runs; the hook still checks, because the check is one stat and a
   wrong assumption here would otherwise cost the activation. Symptom to watch
   for if that is ever wrong: `bat --list-themes` in a fresh session omits the
   scheme, and delta falls back to Monokai. Fix it in the fish config, not with
   an `on_attach` hook.

`min session activate` has reported running the hook, but its **effects have
never been observed from inside a real session** — the daemon was unreachable
for most of this work, and a session's `$HOME` is isolated from the host. What
has been tested is the script itself, run directly against throwaway `$HOME`s.

### SHELL — the shell handover

```toml
[vars]
SHELL = "fish"
```

minimal launches the session's interactive shell from `$SHELL`, so declaring it
is the whole handover. **Requires minimal 0.5.4 or newer.**

Before 0.5.4 this did not work: the session shell was `bash --noprofile --rcfile
<daemon rc> -i` — spawned once when the session is created, not per attach — and
it sourced none of your startup files; the one rc it read was the daemon's own,
which installs the `DEBUG` trap that keeps `TERM` current and nothing else.
There was no way to choose what you landed in, so the loadout went through
`PROMPT_COMMAND`:

```toml
PROMPT_COMMAND = "unset PROMPT_COMMAND; command -v fish >/dev/null && exec fish"
```

That is gone. It is recorded here because two of its details cost real time to
find, and anyone reading an older `cozy.toml` will hit them: the `unset` had to
come first so the variable fired once and did not survive into fish's exported
environment (no relaunch loop), and the `command -v` guard was load-bearing —
`exec` of a missing binary from inside `PROMPT_COMMAND` takes the shell down
with it (bash 5.3, **exit 127**), so an unguarded version meant every attach
exited immediately. Setting the variable also replaced the launcher's baseline
value, which cost minimal's orientation banner. `$SHELL` has none of these
problems, and the banner comes back.

**Lifecycle hooks still cannot do this job**, which is counterintuitive enough
to keep written down. A hook is a separate process the daemon spawns against the
session's pty, not the shell you are talking to, and it is timeout-capped, so it
would be killed at ≤300s and drop you into bash anyway.

No `$ZELLIJ` guard anywhere, deliberately. That's needed when exec'ing zellij
directly; here fish's own config runs `zellij setup --generate-auto-start`,
whose script already carries the check.

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

The bat step checks the `.tmTheme` is on disk and then rebuilds the cache
unconditionally. An earlier draft probed `bat --list-themes` (~8ms measured) to
skip a rebuild when the scheme was already cached, which was pointless here: the
hook runs once per session, against a session that has just been created, so the
cache is always cold and the probe never skipped anything.

The include this points git at also carries `core.pager`/`interactive.diffFilter`
— without them delta is styled but never invoked, and diffs page through
`$PAGER` (which this loadout sets to `bat`) instead.

This setup has lived in three places: `just` recipes run by hand, then a
guarded block in the fish config, now lifecycle hooks. Each move was a real
improvement; the *logic* has barely changed, so read the hook script rather
than reinventing it.

## Per-tool notes

| Template | Tool | Notes |
| --- | --- | --- |
| `helix/themes/theme.toml` | helix | Scopes plus a `[palette]` block — the only tool with real palette indirection |
| `zellij/themes/theme.kdl` | zellij | UI-component spec (0.41+), not the legacy base-colors block: it states every surface explicitly, so a light scheme doesn't fight the format's dark assumptions |
| `fish/config.fish` | fish | Hex set explicitly, not via ANSI names; also sets the terminal surface and ANSI table over OSC |
| `bat/themes/theme.tmTheme` | bat, delta | Sublime `.tmTheme` — the only format bat can load. Scopes mirror the helix theme |
| `delta/delta.gitconfig` | delta | An include, not a gitconfig. Sets `core.pager` too, or none of the styling runs. Diff backgrounds are blends |
| `helix/languages.toml` | helix | The one file with no colours in it — `copy = true`. rust-analyzer settings; the toolchain is the project's to supply |
| `starship/starship.toml` | starship | Restyles modules only, no `format` |
| `broot/skins/skin.hjson` | broot | Decimal `rgb()`, plus the `good_to_bad` ramp |
| `bottom/bottom.toml` | bottom | `[styles]` only; no palette indirection, so hex is inline throughout |
| `atuin/themes/theme.toml` | atuin | The second tool with theme-name indirection after helix. Keys are the `Meaning` enum in PascalCase — the camelCase in atuin's source is a `strum` Display impl, not the serde one |
| `atuin/config.toml` | atuin | Names the slug the theme file renders to; the two entries move together. Also carries preferences, as helix's config does |
| `lazygit/config.yml` | lazygit, delta, difftastic | Colours inline, like bottom's. `git.diffRenderers` wires up delta (`stdinFilter`) and difftastic (`extDiff` — difft takes paths, not stdin) |
| `tealdeer/config.toml` | tealdeer | `[style.*]` blocks; colours are an externally-tagged enum, so truecolour is `{ rgb = { r = …, g = …, b = … } }`. `auto_update` matters: a cold cache means `tldr` errors on arrival |

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
- **The template grammar has no escape, and that includes comments.**
  `expand_sections` and `render` walk the whole file before either knows what
  the file is, so a dark/light marker named in prose counts towards the
  open/close balance and a doubled-brace placeholder in prose is an unknown
  name. Both are hard render errors. This bites twice here: describing the
  section markers in a comment, and quoting lazygit's own `diffContext`
  template variable, which is spelled with the same doubled braces. Say what
  the construct does instead of writing it out.
- **lazygit** is strict about *keys* and silent about *values*. An unknown key
  fails startup — good — but `theme.GetTextStyle` falls through when a colour
  is neither an attribute name, a name in its `ColorMap`, nor a valid hex
  value, so a typo'd triplet is an unstyled widget with no message. broot's
  failure mode, on a tool that otherwise validates.
- **tealdeer** is the reverse: `RawConfig` has no `deny_unknown_fields`, so a
  mistyped section is ignored and the page renders unstyled.
- **atuin** resolves a theme by *filename* (`themes/<name>.toml`, where
  `<name>` is what `config.toml` asks for). The `name` inside the theme file is
  its own declaration and does not select it — keep them equal anyway, and
  remember they are two manifest entries that have to move together.

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

### The palette is a per-attach fact

The non-obvious part, and the source of a bug that lived here for a while: a
session shell is spawned **once**, when the session is created, and outlives
every terminal that attaches to it. The palette therefore cannot be applied once
per shell. It was, guarded by an exported `__COZY_TERM_THEMED`, and the result
was that only whichever attach happened to be live for those OSC bytes got a
themed surface — every later one, and any attach from a different terminal, came
up unthemed with the guard preventing recovery.

This is the same problem minimal solves for `TERM`, so the fix uses the same
signal. minimal rewrites `~/.local/state/minimal/attach-env.fish` on **every**
attach and ships a `vendor_conf.d` hook that sources it from `fish_prompt` and
`fish_preexec`; the config watches that file's mtime from the same two events
and re-emits the palette when it changes. Both events, for minimal's own stated
reason: after a re-attach the prompt on screen was drawn before the detach and
has already fired its prompt event, so without the preexec half the first
command you type still runs under the old terminal's palette.

Two consequences worth knowing:

- **Recovery is one keypress, not instant.** Nothing fires on attach itself —
  the daemon replays a `vt100` screen dump, which restores cells and attributes
  but carries no OSC palette state. The next prompt or command re-applies.
- **The watcher is unguarded by `__COZY_TERM_THEMED`,** unlike the initial
  apply. The outermost fish hands over to zellij and then sits blocked for the
  rest of the session, so it can never be the one to notice; the fish inside
  each pane has to. Re-emitting is idempotent, so panes racing costs nothing.

**Detach still leaves the host terminal recoloured**, and that one cannot be
fixed from in here. `fish_exit` doesn't fire on a detach — fish is still running
in the session — and no in-session event does. What the daemon sends the
departing terminal is `Host::unwind_codes`: input-mode diffs, `\e[?1049l`,
`\e[?25h`, `\e[m`, `\e[?1004l`. That is exactly the right list to add OSC
`104`/`110`/`111`/`112` to, and it is a one-line change upstream. Until then the
README documents a host-side wrapper.

## Verifying changes

```sh
just check           # tests, both schemes rendered, syntax checks, TOML parse
just check-schemes   # every vendored upstream scheme (needs `just fetch-schemes`)
```

Both run in CI (`.github/workflows/ci.yml`). Every check below has caught a real
bug here at least once, which is why they are recipes now rather than a list of
things to remember:

- **The unit tests.** 15 of them, covering both parsers.
- **Both checked-in schemes rendered.** A template that only works for a dark
  scheme is the easy mistake.
- **`cargo fmt --check` and pedantic clippy**, both gated in `just check`.
  Formatting drifted silently before it was checked — a commit was spent putting
  it back by hand.
- **`cargo-deny`** over the renderer's tree: advisories, licences, bans and
  sources, policy in `deny.toml`. It earned its place immediately by catching
  that the crate declared no `license` field at all, which is indistinguishable
  from an incompatible one. It matters most for Dependabot PRs, which is the
  path by which a copyleft or advisory-carrying crate would otherwise arrive
  unnoticed.
- **The MSRV job** compiles against the `rust-version` in `Cargo.toml` rather
  than trusting it. The floor was derived by reading the dependencies' own
  manifests, and Dependabot moves it silently — uuid, indexmap and hashbrown
  already moved it twice: 1.71 -> 1.85 with the parsers, 1.85 -> 1.88 with
  ratatui.
- **A weekly `check-schemes` cron.** `just fetch-schemes` clones tinted-theming
  at HEAD, so the rendered corpus is an input nobody here controls. The schedule
  surfaces an upstream scheme that breaks rendering as upstream drift rather
  than as a mystery failure on an unrelated PR.
- **`dash -n` over the rendered hooks.** They run under POSIX `sh` and carry no
  shebang, so a bashism is a runtime failure in the one script that must never
  fail.
- **`fish --no-execute` over the rendered config.**
- **The renderer parses its own generated TOML, XML and YAML** before writing
  any of it, so a successful render is already a validated one and no external
  parser is involved. Three bugs came from here, all of them silent in the tool
  that reads the file: `cozy.toml`'s wrapped inline tables (invalid TOML that
  minimal's parser tolerated), and the .tmTheme's unescaped `<email@host>` and
  literal `--` inside an XML comment (bat declines a bad theme and still exits
  0). Because it runs in the renderer rather than in CI, `just theme <anything>`
  is covered too — which is where a bad scheme actually reaches a user.
- **Render the whole upstream collection** (`just check-schemes`).
  `schemes/vendor/base16` and `base24` in full, expect zero failures, and
  the result parsed each time. `tinted8` is an 8-colour system and is correctly
  rejected.
- **Walk the quick start from a clean tree** — CI does this with `just clean &&
  just && just theme && just bundle`. It caught `just schemes` exiting 1 on a
  fresh clone: `find` errors on the not-yet-existing vendor directories and
  `set -o pipefail` propagated it, which took out bare `just` as well since the
  default recipe calls it.
- **Build with `--locked`**, so a Cargo.lock that has drifted out of step with
  Cargo.toml fails in CI rather than silently resolving to untested versions.

Two things CI cannot do, so do them by hand when you touch that code:

- **Sandbox `$HOME`** when testing the first-run setup — it writes
  `~/.gitconfig`. `env HOME=/tmp/somewhere sh build/cozy/hooks/on-activate.sh`.
- **Attach twice, from two different terminals**, when you touch the OSC block.
  The failure mode it guards against only appears on the second attach.

Colour changes are best checked by diffing the *set* of colours in a rendered
file against a known-good one, rather than diffing the files, since comments and
structure move around.

## Environment

Notes for **one particular dev machine** — a Darwin host with a nix userland —
not for this repo in general, and not for CI, which is plain Ubuntu with GNU
tools. If none of the below matches what you are looking at, you are on a
different box and it does not apply to you. Verify rather than assume.

- **The tool shell is fish**, so bash-isms (`set -- $x`, arrays, `[[`) fail
  silently or oddly in one-liners. Write a script to a scratchpad file and run
  `bash script.sh`.
- **`grep` is ugrep**, `sed` is **BSD sed**, `find` is **bfs**. Consequences hit
  in practice: `\?` is not a BRE quantifier in BSD sed (strip `.yaml` and `.yml`
  separately, not with `\.ya\?ml$`), and bfs is breadth-first so `find | head -1`
  needs an explicit `sort` for deterministic results.
- **`just` is at `/run/current-system/sw/bin/just`.** There are Linux `just`
  builds in the nix store that will fail with "exec format error" on this
  Darwin host — don't reach for those.
- The build runs on the **host**, not inside a minimal session.

## Verified vs assumed

Worth knowing before relying on any of it:

| Claim | Status |
| --- | --- |
| Renders the whole upstream base16+base24 collection | verified — `just check-schemes`, zero failures. The collection grows upstream, so re-run rather than trusting a past count |
| The crate migration changed no output | **verified** — full rendered tree hashed per scheme before and after; all 535 byte-for-byte identical |
| Legacy unquoted hex that looks numeric (`073642`, `000000`, `1e2021`) parses | verified — regression test; the scheme parser reads raw scalar events precisely because a loaded YAML tree applies implicit typing and destroys these |
| Every generated `.tmTheme` is well-formed XML | verified — all 533 vendored schemes parsed with a real XML parser; `just check` now gates it |
| `--loadout` cannot escape `--out` | verified — `--loadout ..` previously deleted a sibling of `--out`; now rejected, with a test |
| Generated `cozy.toml` is valid TOML for every scheme | verified |
| Renderer builds `--locked` in CI | verified — the old no-network check went when the crate dependencies landed |
| `$SHELL` starts fish on attach | verified — needs minimal 0.5.4 |
| `PROMPT_COMMAND` handover (removed) worked; unset prevented loops | was verified against a pty, before removal |
| Unguarded `exec` in `PROMPT_COMMAND` exits 127 | verified |
| Hooks are idempotent, tolerate missing/broken tools, preserve other git includes | verified against throwaway `$HOME`s |
| Hook scripts are POSIX-sh clean | verified with `dash -n` |
| `just` recipes | all run except `install` against the real `~/.config` |
| `on_activate` fires on a real session | verified — `min session activate` reported running it |
| Lifecycle hook schema and semantics | from the loadouts reference; the specifics below are from minimal's source |
| Loadout identity is the filename, `name` is obsolete | read in `sessions::core::loadout` — a declared `name` loads with a warning and is discarded |
| Session shell is `bash --noprofile --rcfile <daemon rc> -i`, spawned once per session | read in `minimald::session_host` |
| Patches applied before `on_activate` | **settled from the source, not measured**: patches are sandbox fs mappings made at container build, and a hook needs a live session leader |
| Re-attach carries no OSC palette | read in `minimald::session_host` — the attach flush is a `vt100` screen dump |
| Detach leaves the palette on the host terminal | read in `Host::unwind_codes` — it resets SGR, alt screen, cursor, focus reporting, and no OSC colours |
| Any `on_attach` hook breaks fish's OSC 11 background | **doubtful** — observed once, but the once-per-shell palette bug produces the same symptom and was live at the same time. Re-test |
| Rust floor of 1.88 | derived by reading the dependencies' own `rust-version` fields, then **gated in CI** by the `msrv` job, which compiles against it. Never tested locally — only 1.97.1 is available here |
| `ctrl-w` detaches, per the fish greeting | verified — documented in the CLI reference and in minimal's own orientation banner |
| zellij forwards OSC sets to the host terminal | **unverified** — see README's Known gaps |
| The per-attach re-apply fires in a real session | **unverified** — the logic is tested, the daemon was unreachable here |
| atuin, lazygit and tealdeer config *schemas* | verified against the upstream source at the versions the MPR pins (atuin 18.19.0, lazygit 0.64.1, tealdeer 1.8.1) — the generated files were deserialised with those crates' derives transcribed and `deny_unknown_fields` on, so every key is one upstream reads |
| Those three configs *in a running tool* | **unverified** — none of the three was installed where this was written. Schema-correct is not the same as looking right |
| `col -bx` + `MANROFFOPT=-c` is what bat needs for man pages | derived — groff emits SGR rather than overstrike from 1.23 on and the MPR pins 1.24.1, so `-c` is required rather than cargo-culted. Not run |
| atuin must init *after* fzf to win ctrl-r | **derived, not measured** — both bind the key and last writer wins. If ctrl-r ever gives a flat history search, this ordering is the first thing to check |

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
