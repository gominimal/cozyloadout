# Build recipes for the cozy loadout. Usage: README.md. Internals: AGENTS.md.
#
# `just --list` shows only the LAST line of a preceding comment block, so every
# recipe with more to say than fits on one line carries a [doc] attribute for
# the listing and keeps the detail in the comment above it.

SCHEMES := "schemes"
BUILD   := "build"
RENDER  := "cargo run --quiet --release --manifest-path tools/cozy-theme/Cargo.toml --"

# The loadout's name. It is the stem of the file minimal identifies the loadout
# by, the directory its hook scripts are anchored in, and the `{loadout}` token
# in templates/manifest.toml — so changing it here renames all three together.
LOADOUT := "cozy"

# Show the recipes and the schemes on hand.
default:
    @just --list --unsorted
    @echo ""
    @just schemes

#     just theme                       # the default scheme
#     just theme gruvbox-dark-hard     # a name under schemes/ (or schemes/vendor/)
#     just theme ~/my-scheme.yaml      # any path
#
# The theme files are named after the scheme, so switching schemes replaces
# them rather than leaving a stale name on disk holding the wrong colours.
[doc('Render the loadout with a base16 scheme and bundle it into cozy.zip')]
theme scheme="minimal-dark": (_render scheme) bundle

[doc('Render only, no zip — for eyeballing build/ before bundling')]
render scheme="minimal-dark": (_render scheme)

_render scheme:
    #!/usr/bin/env bash
    set -euo pipefail
    # Resolve into a variable first. A command substitution used directly as an
    # argument has its exit status discarded even under `set -e`, so inlining
    # this would run the renderer on "" after a failed lookup.
    #
    # Quoted: `{{scheme}}` is pasted into this script as literal text, so an
    # unquoted one hands the shell whatever the caller typed —
    # `just theme '$(...)'` would run it.
    path=$(just _resolve "{{scheme}}")
    {{RENDER}} "$path" --templates templates --out {{BUILD}} --loadout {{LOADOUT}}

# Resolve a scheme argument to a path: an existing path is used as-is, a bare
# name is searched for anywhere under schemes/ (the upstream collection nests
# its files in base16/ and base24/ subdirectories). Schemes checked into this
# repo win over vendored ones of the same name.
_resolve scheme:
    #!/usr/bin/env bash
    set -euo pipefail
    arg="{{scheme}}"
    if [[ -f "$arg" ]]; then echo "$arg"; exit 0; fi
    # Search order: this repo's schemes, then vendored base16, then base24.
    # The upstream collection also ships tinted8 schemes, which are an 8-colour
    # system the loadout can't use — listing the directories explicitly is what
    # keeps those unreachable.
    #
    # -maxdepth 1 rather than a `-not -path '*/vendor/*'` filter: the vendored
    # directories are themselves under vendor/, so such a filter excludes the
    # very paths being searched.
    hit=""
    for dir in "{{SCHEMES}}" "{{SCHEMES}}/vendor/base16" "{{SCHEMES}}/vendor/base24"; do
        [[ -d "$dir" ]] || continue
        hit=$(find "$dir" -maxdepth 1 \( -name "$arg.yaml" -o -name "$arg.yml" \) \
              2>/dev/null | sort | head -1)
        [[ -n "$hit" ]] && break
    done
    if [[ -z "$hit" ]]; then
        echo "no scheme '$arg' under {{SCHEMES}}/ — try \`just schemes\`" >&2
        exit 1
    fi
    echo "$hit"

# List every scheme available to `just theme`.
schemes:
    #!/usr/bin/env bash
    set -euo pipefail
    # `\?` is not a BRE quantifier in BSD sed, so strip the two extensions
    # separately rather than with `\.ya\?ml$`.
    strip() { sed 's|.*/||; s|\.yaml$||; s|\.yml$||'; }
    echo "In {{SCHEMES}}/:"
    find "{{SCHEMES}}" -not -path '*/vendor/*' \( -name '*.yaml' -o -name '*.yml' \) 2>/dev/null \
        | strip | sort | sed 's/^/  /'
    # Count only base16/ and base24/: the upstream repo also carries tinted8
    # schemes and its own workflow YAML, neither of which this can render.
    #
    # The directories are collected before being searched. Handing find a path
    # that does not exist makes it exit non-zero, which under `set -o pipefail`
    # fails the whole recipe — and on a fresh clone, before `just fetch-schemes`
    # has run, neither path exists. That took out bare `just` too, since the
    # default recipe calls this one.
    usable=0
    dirs=()
    for d in "{{SCHEMES}}/vendor/base16" "{{SCHEMES}}/vendor/base24"; do
        [[ -d "$d" ]] && dirs+=("$d")
    done
    # Deduplicated, matching `just vendored`: a handful of names exist in both
    # base16/ and base24/, and a name is what you actually pass to `just theme`,
    # so counting files would overstate the choice on offer.
    if (( ${#dirs[@]} > 0 )); then
        usable=$(find "${dirs[@]}" \( -name '*.yaml' -o -name '*.yml' \) \
                 | sed 's|.*/||; s|\.yaml$||; s|\.yml$||' | sort -u | wc -l | tr -d ' ')
    fi
    echo ""
    if (( usable > 0 )); then
        echo "Plus $usable vendored upstream schemes; list them with:"
        echo "  just vendored"
    else
        echo "\`just fetch-schemes\` adds ~480 upstream tinted-theming schemes."
    fi

# `sort -u` because a handful of names exist in both base16/ and base24/;
# `_resolve` prefers base16, so listing them twice would misrepresent the choice.
[doc('List the vendored upstream schemes')]
vendored:
    #!/usr/bin/env bash
    set -euo pipefail
    dirs=()
    for d in "{{SCHEMES}}/vendor/base16" "{{SCHEMES}}/vendor/base24"; do
        [[ -d "$d" ]] && dirs+=("$d")
    done
    if (( ${#dirs[@]} == 0 )); then
        echo "no vendored schemes — run \`just fetch-schemes\` first" >&2
        exit 1
    fi
    find "${dirs[@]}" \( -name '*.yaml' -o -name '*.yml' \) \
        | sed 's|.*/||; s|\.yaml$||; s|\.yml$||' | sort -u

# Gitignored, so it stays out of this repo's history.
[doc('Clone the upstream tinted-theming scheme collection into schemes/vendor/')]
fetch-schemes:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -d "{{SCHEMES}}/vendor/.git" ]]; then
        git -C "{{SCHEMES}}/vendor" pull --ff-only
    else
        rm -rf "{{SCHEMES}}/vendor"
        git clone --depth 1 https://github.com/tinted-theming/schemes.git "{{SCHEMES}}/vendor"
    fi
    just schemes

# Bundle whatever is currently in build/ into cozy.zip.
bundle:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ ! -f "{{BUILD}}/{{LOADOUT}}.toml" ]]; then
        echo "nothing built yet — run \`just theme <scheme>\`" >&2
        exit 1
    fi
    rm -f {{LOADOUT}}.zip
    cd "{{BUILD}}" && zip -qr ../{{LOADOUT}}.zip {{LOADOUT}}.toml {{LOADOUT}}
    echo "{{LOADOUT}}.zip"

# There is nothing to run afterwards: bat's theme cache and delta's gitconfig
# include are handled by the loadout's lifecycle hooks.
[doc('Install the bundled loadout into ~/.config/minimal/loadouts/')]
install: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    dir=~/.config/minimal/loadouts
    mkdir -p "$dir"
    # Delete the loadout tree before unzipping over it. `unzip -o` overwrites
    # but never removes, so without this every scheme you have ever installed
    # leaves its <slug>.tmTheme, <slug>.toml, <slug>.kdl and <slug>.hjson behind
    # in the loadout directory forever. Only this loadout's own generated tree
    # is removed — nothing else under loadouts/ is touched.
    rm -rf "$dir/{{LOADOUT}}"
    unzip -oq {{LOADOUT}}.zip -d "$dir"
    echo "installed into $dir. Apply the loadout and attach."

# Run the renderer's tests.
test:
    cargo test --quiet --release --manifest-path tools/cozy-theme/Cargo.toml

# Pedantic clippy, warnings denied. The lint level lives in Cargo.toml's
# `[lints.clippy]` so a bare `cargo clippy` sees it too; this recipe only adds
# the gate. `--all-targets` so the test module is linted as well — it drifts
# first, since it is the code nobody re-reads.
lint:
    cargo clippy --quiet --release --all-targets --manifest-path tools/cozy-theme/Cargo.toml -- -D warnings

# Everything CI runs, and everything worth running before a commit: the unit
# tests, pedantic clippy, both checked-in schemes rendered, the shell and fish
# files syntax checked, and the generated loadout TOML parsed by something that
# is not our own parser. Each of these has caught a real bug — see AGENTS.md.
[doc('Run the full local check suite (tests, lints, renders, syntax, TOML validity)')]
check: test lint
    #!/usr/bin/env bash
    set -euo pipefail
    for scheme in minimal-dark minimal-light; do
        echo "--- $scheme"
        just render "$scheme"
        just _validate
    done
    echo "--- ok"

# Syntax and validity checks over whatever is currently in build/. Split out so
# `check` and `check-schemes` can both use it.
_validate:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    # POSIX sh, not bash: minimal runs hook scripts under sh unless a shebang
    # says otherwise, and these deliberately carry none. dash is the strictest
    # sh commonly to hand; fall back to whatever /bin/sh is.
    for hook in {{BUILD}}/{{LOADOUT}}/hooks/*.sh; do
        if command -v dash >/dev/null 2>&1; then dash -n "$hook"; else sh -n "$hook"; fi
    done
    if command -v fish >/dev/null 2>&1; then
        fish --no-execute {{BUILD}}/{{LOADOUT}}/fish/config.fish
    else
        echo "  (skipped: no fish to syntax-check the config with)" >&2
    fi

# Renders every vendored scheme and parses the result. Slow (a few minutes for
# ~530 schemes) and needs `just fetch-schemes` first, so it is not part of
# `just check`; CI runs it on its own.
[doc('Render every vendored upstream scheme and check each result parses')]
check-schemes:
    #!/usr/bin/env bash
    set -euo pipefail
    dirs=()
    for d in "{{SCHEMES}}/vendor/base16" "{{SCHEMES}}/vendor/base24"; do
        [[ -d "$d" ]] && dirs+=("$d")
    done
    if (( ${#dirs[@]} == 0 )); then
        echo "no vendored schemes — run \`just fetch-schemes\` first" >&2
        exit 1
    fi
    n=0
    while IFS= read -r scheme; do
        # The renderer parses its own TOML, XML and YAML output before writing
        # any of it, so a render that succeeds has already been validated.
        just render "$scheme" >/dev/null
        n=$((n + 1))
    done < <(find "${dirs[@]}" \( -name '*.yaml' -o -name '*.yml' \) | sort)
    echo "$n schemes rendered, every generated file parses"

# Drop build artefacts. Leaves schemes/vendor/ alone.
clean:
    rm -rf {{BUILD}} {{LOADOUT}}.zip
    cargo clean --quiet --manifest-path tools/cozy-theme/Cargo.toml
