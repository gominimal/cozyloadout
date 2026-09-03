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

`tools/cozy-theme/src/lib.rs` **is** the renderer: `Rgb`, `mix`, `SLOTS`,
`Scheme` (the YAML parser), `discover`, `Packages`, the minijinja environment,
the manifest parser, output validation, and `build` behind an `Options` struct.
`main.rs` is a clap front end over it and the wizard is the other caller. It was split
out of `main.rs` when the wizard needed to load schemes; the split changed no
output, verified by hashing all 535 rendered trees before and after.

Nothing renderer-shaped stays in `main.rs` any more: it is argument parsing
and a single call. Pedantic clippy's library-API lints (`missing_errors_doc`,
`missing_panics_doc`, `must_use_candidate`) are switched off for it in
`Cargo.toml`: it is an internal crate with `publish = false`, and writing those
sections for callers who are both in this repo is documentation nobody reads.

### The wizard

`tools/cozy-theme/src/bin/cozy-wizard/`, a second binary in the same package
(`default-run = "cozy-theme"` keeps a bare `cargo run` pointing at the
renderer). `just wizard` runs it. Seven screens: greeting, schemes, themes,
packages, patches, client, resources.

Cargo takes `src/bin/<name>/main.rs` plus submodules, and the wizard uses all
of it. The layout is **one module per screen and one per model**, because the
question "where does this behaviour live" should have a boring answer:

| | |
| --- | --- |
| `main.rs` | the CLI, `App` and its shared state, the event loop |
| `ui/<screen>.rs` | one screen: what it draws *and* what its keys do |
| `ui/mod.rs` | the frame around every screen — outer block, footer, heading |
| `ui/prelude.rs` | what the screen modules draw with, so their headers stay short |
| `greeting.rs` `fetch.rs` `keys.rs` `picker.rs` `resources.rs` `theme.rs` | models: no drawing, no `Frame` |
| `hostcfg.rs` | the two writes that leave this repo |
| `tests/` | one module per screen, over shared fixtures in `tests/util.rs` |

Two rules hold it together. **A screen owns its keys as well as its pixels** —
`ui::patches` has both `draw_patches` and `on_key_patches`, so changing how a
page behaves means opening one file. And **the models never draw**: what a
directory listing contains, whether a chord is legal, what a host can allocate
are all decided without a `Frame`, and tested against real temporary
directories rather than through a rendered one. `picker.rs` was the first
module split out on that principle and the rest followed it.

`main.rs` keeps `App` because every screen module reads its fields: a child
module can see its ancestors' private items, so the state stays private to the
binary without a `pub(crate)` on all forty fields.

Its first page asks which fish greeting to install. Five options:

| Option | Mark | Glyphs |
| --- | --- | --- |
| Default | `▃🭕🭏🭕🭏 M I N I M A L` | Symbols for Legacy Computing, U+1FB00, Unicode 13 |
| Geometric shapes | `.◥◣◥◣ M I N I M A L` | Geometric Shapes, U+25A0–U+25FF, Unicode 1.1; the base is a plain `.` |
| Block elements | `████` over three lines, plus `M I N I M A L` | U+2580–U+259F, Unicode 1.1 |
| No logo | — | just the `… to detach` line |
| Nothing at all | — | a silent shell |

**Order is best-looking first, not safest first.** The sharpest mark leads even
though its glyphs are the least widely available, because this is the one
screen where a font that cannot draw something says so plainly — and the two
options directly beneath it are the fallbacks for anyone whose font cannot.

`--greeting` still defaults to `blocks`, deliberately: the CLI has no preview,
so the answer it picks without being asked should be the one that renders
everywhere. The wizard preselects the sharp mark *because* it shows you whether
it works.

The three marked ones differ *only* in which Unicode block their glyphs come
from, which is the whole point: a font either has them or draws tofu, and no
description beats putting them on screen. So the preview is drawn **unstyled,
in the terminal's own foreground** — the user is judging their font, and a
preview that is not the real thing is worthless.

**A list with one preview, not a box per option.** Five options with a
four-line mark among them do not fit as stacked boxes in a 24-row terminal, and
only the highlighted one is being judged.
`every_greeting_matches_what_the_template_will_print` pins every mark *and*
every template branch to `templates/fish/config.fish`, so the preview cannot
promise something the generated config does not do.

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

Its fourth page is the optional-package checklist. Space toggles a row, `a`/`n`
select all or none, and the panel under the list shows the highlighted
package's description **and SPDX licence**. The licence is there because one of
these is not like the others: `claude-code` is
`LicenseRef-Anthropic-Proprietary` and everything else is MIT or Apache-2.0, so
a non-permissive licence is drawn in the scheme's orange and bold rather than
the same grey as the rest — `the_proprietary_licence_is_called_out` asserts
that by reading the rendered cell's style, not its text.

Below the list is a free-text field for packages that are not on it — the
registry has far more than the fifteen offered, and a loadout is personal, so
adding `emacs` should not mean editing a TOML file. It introduces the page's
one piece of real complexity: **focus**. While the field has focus, printable
keys are text, which means `q` is a letter rather than quit and `space` is a
space rather than a toggle. `Ctrl-C` still exits from anywhere, because there
has to be a way out. `typing_q_does_not_quit_the_wizard` and
`list_keys_do_not_leak_into_the_field` are the tests that matter here.

Typed input is parsed on read, not on each keystroke, so half-finished names
are allowed to exist. `is_package_name` is deliberately conservative —
lowercase, digits, `- _ . +` — and the field **echoes what it parsed**, plus any
name the loadout already installs. Silently dropping half of what someone typed
would be the worst version of this widget.

The licence strings are copied from each package's `license_spdx` in the
Minimal Public Registry. **Do not guess one**: it is a claim about someone
else's software shown at the moment a user agrees to install it.

**The scheme persists across pages and dies with the wizard.** The package page
paints in the theme chosen on the previous one
(`the_chosen_theme_persists_onto_the_package_page`), and `main` emits an
explicit `ResetColor` *before* leaving the alternate screen. Leaving the
alternate screen restores the primary screen's contents, but the SGR state the
last frame set belongs to the terminal, not the screen — without the reset,
quitting from a themed page hands back a shell still wearing someone else's
colours.

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
| `optional` | everything else, each with a description; all preselected | yes |

Every optional package is `default = true`, asserted by
`every_optional_package_is_on_by_default`: the full set is what the loadout has
always installed, so an untouched wizard has to reproduce the loadout people
already have. The flag exists so that can be revisited per package without
touching code.

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
`gen_completion` returns early when a binary is missing. **difftastic sits in
`cozy` for precisely that reason** — `templates/git/git.gitconfig` wires it up
as `git dft`, and a gitconfig alias has no way to check whether its binary
exists, so declining it would leave a broken alias behind. Anything that cannot
be guarded cannot be optional.

The `[N files]` the renderer prints counts what it actually wrote, not
`entries.len()` — those stopped agreeing once entries could be skipped.

### The apply page

The last screen: a summary of every choice, a tick for installing, then four
actions.

| Action | Builds | Installs | Remembers |
| --- | --- | --- | --- |
| Generate | yes | when ticked | yes |
| Save these settings to a file | no | no | yes, and to the named file |
| Save settings and exit | no | no | yes |
| Abort | no | no | **no** |

Once an action has run, **any key exits**. Naming two specific keys made people
hunt for them, and there is nothing else to do on that screen: re-running from
it would be a second build nobody asked for. The outcome — the rendered path,
or the error — is printed *after* the alternate screen is torn down, so a
failure is still readable instead of vanishing with the frame it was drawn on.

Abort is the only one that throws the answers away — that is what makes it
different from the others, and `abort_is_the_only_action_that_discards_the_answers`
holds it.

Generating **calls `cozy_theme::build` directly**. The renderer lives in the
library; the `cozy-theme` binary is a thin CLI over it, and the wizard is the
other caller. Both run literally the same code, with `Options` in place of a
command line.

It spawned the binary first, on the reasoning that using the same executable as
`just theme` kept them from drifting. That reasoning was wrong — a shared
library gives that more strongly, with no argument serialisation in between —
and the subprocess cost two hacks that only existed because of it: locating the
binary on disk (with a `deps/` fallback so tests could find it) and making every
path absolute, because the child had its own working directory. Both are gone.

Installing is `cozy_theme::install`, in-process too, and `just install` is a
one-line recipe calling `cozy-theme --install`. So the wizard and the recipe do
the same thing by running the same code rather than by agreeing on a sequence
of shell steps.

It is a **directory copy, not zip-then-unzip**. The zip is a distributable
artifact that `just bundle` produces; installing never needed to go through it,
and shelling out to `unzip` is where the stray recipe output in the wizard came
from — `just`'s echoed command line and unzip's chatter went straight through
the alternate screen. Nothing now uses `unzip` at all.

The old tree is deleted before the copy, for the reason the shell version had
to: overwriting never removes, so every scheme ever installed would leave its
theme files behind forever. The destination path is built from the loadout name
and never taken from a caller, so a sibling loadout cannot be caught in the
delete — `installing_touches_nothing_but_this_loadout` holds that.

The renderer grew the flags this needs, and they work from the command line too:

| Flag | Effect |
| --- | --- |
| `--greeting blocks\|legacy` | which fish mark to install |
| `--with a,b,c` | which optional packages; **omitted means all**, which is what a plain `just theme` has always produced |
| `--patch-file`, `--patch-dir` | the user's own files, appended to the patch list; a directory becomes a glob with a trailing-slash dest |

One trap survives the move to a library call: **the repo root must never be an
empty path.** `Path::new("schemes/vendor")` climbs to `"schemes"` and then to
`""`, and an empty path is not the current directory — it is nothing. It is
still used to find `templates/` and to run `just install`, so `repo_root` is
the one definition and `the_repo_root_is_never_an_empty_path` guards it.

### Remembering answers

`settings.rs` is the schema, and it lives in **the library**, not the wizard.
It is not only the wizard's: `cozy-theme --settings <file>` renders straight
from one, so a settings file is a complete, portable description of a loadout —
the thing you commit to a dotfiles repo or hand to a colleague.

### Where the user's own files go

Nothing the user creates is written into the checkout. Two directories, both
under `$XDG_CONFIG_HOME/cozy` (`~/.config/cozy` by default):

| | |
| --- | --- |
| `~/.config/cozy/settings.toml` | what the wizard remembers between runs |
| `~/.config/cozy/schemes/*.yaml` | schemes saved from the adjust screen |

`cozy`, not `minimal`: this is the loadout's own tool, and putting files under
`minimal/` would be taking a namespace that is not ours.

The reason is that a checkout is disposable and these are not. A scheme you
tuned should survive re-cloning the repo and be there from every checkout, not
just the one you happened to save it from. `discover` and the justfile's
`_resolve` both read that directory **first** — most specific wins: yours, then
the repo's checked-in schemes, then the vendored collection.

A `.cozy-wizard.toml` left in a working directory by an older run is still
*read* when there is no `~/.config/cozy/settings.toml` yet, so nobody loses
their answers to the move. It is never written again.

`--settings` points both binaries at a file of your choosing, which is also how
the tests avoid touching a real one.

`Settings::apply_to` is the one place a file becomes `Options`, so what the
wizard builds and what the saved file rebuilds cannot drift apart. On the CLI
the file is applied *first* and any flag typed alongside it wins — the file is a
starting point, not an override, so `--settings mine.toml --greeting none` is
one answer changed rather than a file to edit.

Three rules, each with a reason:

- **Only a finished run writes.** `completed` is set by `enter` on the last
  page and by nothing else, so quitting part-way leaves the previous answers
  intact rather than half-overwriting them.
- **`packages` is a map, not a list.** A list of chosen names cannot tell "the
  user turned this off" from "this did not exist yet", so a package added to
  the loadout later would arrive silently switched off. With the full map an
  unknown package falls back to its own `default`.
- **A remembered thing that has gone falls back to the default.** A scheme that
  is no longer in the collection leaves the cursor at the top; a file or
  directory that has been deleted is simply not restored. Neither is an error —
  it is just gone.

**Remembered answers apply on a page's first visit only.** Every page restores
from the file when it opens; doing that again on a second visit would silently
undo whatever the user changed this run — go back to check something, come
forward, and your work is gone. So the themes page prefers the currently
selected scheme over the remembered one (matched *by name*, so a re-cloned or
freshly fetched collection does not move the cursor somewhere arbitrary), and
the packages and patches pages simply do not re-initialise once populated. The
themes page still re-runs `discover` every time, because the user can go back,
fetch the collection, and return, and the new schemes should be there.
`revisiting_a_page_keeps_what_you_changed_this_run` and
`revisiting_the_patches_page_keeps_this_run_s_choices` hold that.

`esc` steps back one page from everywhere except the first, and every footer
that has a page behind it says so — `every_page_after_the_first_offers_a_way_back`
checks the hint is really there, because a key nobody mentions is a key nobody
presses.

The scheme-collection question is **deliberately not recorded**. Whether to
clone or pull is about the state of the disk right now, not a preference, and
answering it once should not answer it forever;
`the_scheme_fetch_answer_is_never_recorded` asserts the written file never
mentions it.

A corrupt or hand-edited file reads as defaults rather than failing. This is a
convenience, and the worst it should ever cost is the convenience.

### Adjusting a scheme

`a` on the themes page swaps the scheme list for six knobs. They are not
generic image filters: a scheme is sixteen slots with assigned meaning, so each
control acts on the slots it is *about*.

| Knob | Slots | What it does |
| --- | --- | --- |
| contrast | all | pushes everything away from the midpoint of base00 and base07 |
| accents | base08–0F | pulls the accents toward or away from their own grey |
| comments | base03 | toward base05, or back into base00 |
| surfaces | base01/02 | spreads them apart, so a selection reads against a surface |
| background | base00–03 | base00 toward its extreme or the surface above, the rest following at a halving rate |
| warmth | all | a red/blue cast |

**There is no hue rotation, deliberately.** base08 is red because errors are
red, across delta, helix diagnostics and bottom's gauges. Rotating hue makes
errors green — the one control that produces a *wrong* result rather than an
ugly one. Global brightness is absent for a duller reason: it decomposes into
contrast plus background, both of which are better questions.

Four things hold this together:

- **All-zero is the identity.** `Adjust::default()` renders byte-for-byte what
  the scheme always did, which is what lets `adjust` be plumbed through
  `Options` unconditionally. `an_untouched_adjustment_changes_nothing` states
  it, and the corpus check is what proves it — all 1068 renders unchanged.
- **`is_dark` is carried over, never recomputed.** It is derived by comparing
  background and foreground luminance and it selects `scheme_variant`, which
  decides `duf --theme` and every `{% if dark %}` branch. An adjustment that
  nudged a scheme across that line would silently rewrite unrelated config, so
  the variant is the unadjusted scheme's answer.
- **An adjusted scheme gets its own slug**, `<slug>-<token>`, where the token is
  FNV-1a over the six values. Theme files are named after the slug
  (`bat/themes/<slug>.tmTheme`) and the .tmTheme UUID is derived from it, which
  Sublime keys themes by — so sharing a slug would overwrite the stock scheme's
  files and inherit its UUID.
- **The whole thing is `Scheme::adjusted`, in the library.** The wizard's
  preview, its swatches, its displaced-config list and the generated files all
  read `App::scheme()`, so nothing can disagree about which scheme this is; and
  the CLI gets the same six values as flags, so the knobs are not a feature only
  the wizard can reach.

The panel takes the list's place rather than sitting beside it: the column is
thirty cells wide, the preview is the point of the screen, and a scheme list you
cannot move through while adjusting costs nothing. The WCAG ratio for base05 on
base00 sits under the knobs with a pass/fail at 4.5:1, which is what makes the
screen a measurement rather than a matter of taste.

#### Saving an adjusted scheme

`s` — offered once something is set — writes the adjusted palette into
`schemes/` as a scheme of its own. `Scheme::to_yaml` emits the current
`palette:` format and `save_as` writes it;
`a_saved_scheme_loads_back_with_the_same_palette` is the round-trip that makes
this a *scheme* rather than an export, so `discover` finds it and
`just theme <name>` takes it like any other.

Four decisions worth keeping:

- **The knobs reset to zero on save, and the panel closes.** The edits are now
  *in* the scheme; leaving them set would apply every one of them a second time
  on top of a palette that already has them. Closing the panel is what makes the
  new scheme visible in the list, selected, with the confirmation under it.
- **The original author keeps the `author:` field.** The palette is derived from
  their work. Provenance goes in a comment above it, where it cannot be mistaken
  for a claim about who made this.
- **Two levels of name check.** `save_as` refuses to overwrite a file — a scheme
  file is the only copy of a palette somebody tuned by hand. The wizard checks
  the *discovered* set as well, because `discover` lets `schemes/` shadow a
  vendored scheme of the same name, so saving "gruvbox-dark" would quietly hide
  the real one. A refused name keeps the prompt open with the text still in it.
- **The saved file's variant is decided by its own luma.** `adjusted` pins
  `is_dark` so a mid-session tweak cannot flip `scheme_variant` under the
  templates; saving ends that, because the file is now a scheme in its own right
  and `load` treats it like every other.
  `a_saved_scheme_takes_its_variant_from_its_own_palette` states it — the two
  rules together are surprising if you only know one.

`--save-as` does the same from the command line, and renders the saved file
rather than the original, so `build/` is always reachable from a scheme that is
actually on disk.

#### An adjustment belongs to one scheme

**Moving to a different scheme clears the knobs.** +40 comments rescues one
palette and ruins the next, so a new scheme starts from what its author
published. Three cases are deliberately *not* a scheme change: a clamped move
(holding ↑ at the top of the list), closing the panel, and leaving the page and
coming back — the last being the same rule that keeps every other page's
this-run choices. `move_theme` guards on the row actually changing.

A remembered scheme that is no longer on disk drops its adjustments too. They
were tuned against a palette this checkout does not have, and re-applying them
to whatever sorts first would be worse than starting clean.

**They are still sticky across runs**, because the scheme is: the settings file
records the theme by name and the six knobs beside it, and on the next run the
scheme is found, so its adjustments stand. The two rules only look like they
conflict — an adjustment survives exactly as long as the scheme it was made
against does.

Because the knobs are off screen while you browse, the bottom of the list says
whether anything is set (`contrast +100`, truncated to the column) or offers
`a to adjust`. Without it the reset would be a silent loss: you would reopen the
panel and find your work gone with nothing having said so.

### The client and VM pages

Two pages that configure **minimal, not the loadout**. Everything else the
wizard collects lands in `build/` and is undone by deleting it; these two write
outside the repo, so both pages say so and the summary marks them.

Nothing in a loadout can set either. `LoadoutFile`
(`crates/sessions/src/core/loadout.rs`) carries packages, vars, patches and
lifecycle hooks and nothing else — session keys are *client* config in
`<config>/minimal/config.toml`, and VM resources are minvmd's. That is why
these are separate pages rather than more rows in the generated `cozy.toml`.

**The client page** edits `[session-keys]`. `keys.rs` is a port of minimal's
`crates/sessions/src/keys.rs`: the parser range, the termios-special set, the
wrapping-ambiguous set, and the shadowing rule. Porting rather than
approximating is the point — a wizard that accepted a chord minimal rejects
would write a file that fails to load, and one that rejected a chord minimal
accepts would lie about what is possible. A plain-glyph leader is *allowed*
(minimal allows it) and warned about rather than refused.

Two details worth keeping:

- **`ctrl-<x>` while editing is captured as the text `ctrl-x`**, not acted on.
  The page has to be able to configure the very chord you press to get out of a
  session.
- **The file is edited with `toml_edit`, not rewritten.** It is the user's
  file: it may hold a `[loadouts]` section the wizard knows nothing about and
  comments explaining why. `toml` would round-trip it through a value tree and
  drop both. A file that will not parse is left alone and reported, because the
  alternative is overwriting contents we could not read.

`the_written_shape_is_one_minimal_can_actually_read` transcribes
`SessionKeysConfig` with `deny_unknown_fields` and deserialises the output, the
same trick the atuin and lazygit configs are checked with.

**The VM page** models minvmd's own policy: the vcpu ceiling
(`max_vm_vcpus`, cores minus a 2-core host reserve, floored at the baseline),
the RAM floor, the arch-conditional default, and the x86_64 MMIO hole — sizes
inside which are *not offered at all*, since a guest sized there panics at boot
and there is nothing the user could do about it. Only the RAM ceiling
(three quarters of host memory) is cozy's own, because minvmd merely warns
about over-allocation where a picker has to draw a line. The constants are
copied with their source named and `mirrors_minvmds_published_policy` fails when
the copy drifts.

Applied by running `minvmd config set`, never by writing minvmd's `config.toml`
directly: that command validates against host capacity, serialises the
read-modify-write under the lifecycle lock, and derives its own state
directory. Reproducing any of that here would be a guess that breaks silently.
minvmd missing from `PATH` is not an error — the page says the choice will be
remembered but not applied.

Both are applied only when the install tick is set: a bare render promises
nothing outside the repo changes, and both of these are outside it.
Neither failure is fatal to an install that has already written the loadout;
they append a line to the report instead. And neither is applied at all when it
still equals the default, so an untouched wizard writes no config file it did
not need to.

### The apply page

A summary, a tick, and four actions.

**Installing is a checkbox, not an action.** It used to be two entries —
"Generate" and "Generate and install" — which made the same choice twice: once
in the list and once in whether you scrolled past it. As a tick it is one fact,
visible whichever action is highlighted, and the Generate row renames itself to
match. It is **on by default**, because installing is what running the wizard is
for and an untouched run should produce a usable session rather than a `build/`
directory.

`space` toggles it from anywhere on the page rather than making it a focusable
row. The list is a list of *actions*, and a row where `enter` toggled instead of
acting would be the one place on the screen where `enter` means something else.

The host settings ride with the tick, not with the action: a bare render
promises that nothing outside this repo changes, and minimal's config and
minvmd's state are both outside it.

**"Save these settings to a file"** writes every answer to a path you choose,
which `cozy-theme --settings <file>` then rebuilds from. It refuses to
overwrite, and a refused path keeps the prompt open with the text still in it —
the same shape as the other two text prompts in the wizard. Writing it counts as
finishing the run, so the automatic file records it too.

### The patches page

Two pickers side by side, files on the left and directories on the right, for
patching your own dotfiles into the session. They are separate rather than one
browser with a mode because a loadout patches the two differently — a file maps
to a single `dest`, a directory to a glob — and because seeing both sets of
choices at once is the point.

Key choices worth keeping:

- **Arrows walk the tree; enter finishes.** Descending is on the arrow that
  points into the tree, which leaves `enter` meaning what it means on every
  other page. A file browser that stole `enter` would make this the one screen
  where finishing is a different key.
- **Hidden entries are shown.** Patching in dotfiles is the entire use case, so
  a picker that hid `.config` would be useless.
- **Directories appear in the file picker but cannot be chosen**, and get no
  empty checkbox — you walk through them, you do not select them. `Pick::accepts`
  is the single source of that rule; the drawing code asks it rather than
  keeping a second copy.
- **A failed listing keeps its error.** An unreadable directory and an empty one
  look identical on screen otherwise.

Both pickers start at `$HOME`. Selections survive walking away and coming back,
and the summary line under them names every path with `~` for the home prefix.

#### Where a picked path lands

A loadout's `dest` is interpreted relative to the *session's* home, so mapping a
host path to one is the whole job:

| Picked | `dest` |
| --- | --- |
| `~/.config/starship.toml` | `.config/starship.toml` |
| `~/.gitconfig` | `.gitconfig` |
| `/etc/hosts` | `etc/hosts` |
| `~/.config/helix` (a directory) | `.config/helix/`, sourced from `…/helix/**/*` |

A path under the host's home keeps its shape, because that shape is what makes
it a dotfile in the first place. A path outside the home has no home-relative
form, so it drops its leading `/` and arrives at the same place under the
session home — which at least keeps `/etc/hosts` distinguishable from
`~/.config/hosts`, where flattening to the basename would not.

`patch_dest` is the single implementation, and both the wizard's warnings and
the renderer's output go through it. Directories get a trailing `/` on the dest
and a `**/*` glob on the source; files get neither.

#### When your file collides with the loadout's

Yours wins — you asked for it specifically — and the loadout's own entry is
dropped rather than both being written and one silently losing. Picking
`~/.config/helix` displaces every config the loadout puts under `.config/helix`.

But it is said out loud, in two places: the patches page (`using yours instead
of …`) and the summary page (`replaces …`). A config the loadout exists to
install quietly going missing is the kind of thing you discover three sessions
later wondering why the theme is half-applied. `displaced_configs` computes it
by running the manifest through the same `patch_dest`, and skips configs
belonging to packages that were declined — those are not written anyway, so
calling them displaced would be noise.

The warning renders *above* the chosen-path line, and the summary strip grows a
row to fit it. It used to sit below a line that wraps, so a few long paths
clipped the warning off the bottom exactly when it had something to say;
`a_long_list_of_picks_cannot_push_the_warning_off_the_screen` holds that.

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
| The greeting's detach chord | verified against minimal's source: `minimald` seeds `MINIMAL_DETACH_HINT` per attach channel as `"{leader} then {detach_key}"` (`crates/minimald/src/session.rs`), defaulting to `ctrl-]` and `d` (`crates/sessions/src/keys.rs`). The template reads the var and falls back to the same `ctrl-] then d` minimal's own banner does. **Mint-scoped**, per `docs/reference/loadouts.md`: a second client attaching with a remapped chord gets a working one, but the greeting still shows the minting channel's |
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
