# Build recipes for the cozy loadout. See README.md.
#
# `just --list` shows only the LAST line of a preceding comment block, so every
# recipe with more to say than fits on one line carries a [doc] attribute for
# the listing and keeps the detail in the comment above it.

SCHEMES := "schemes"
BUILD   := "build"
RENDER  := "cargo run --quiet --release --manifest-path tools/cozy-theme/Cargo.toml --"

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
    path=$(just _resolve {{scheme}})
    {{RENDER}} "$path" --templates templates --out {{BUILD}}

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
    usable=$(find "{{SCHEMES}}/vendor/base16" "{{SCHEMES}}/vendor/base24" \
             \( -name '*.yaml' -o -name '*.yml' \) 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$usable" -gt 0 ]]; then
        echo ""
        echo "Plus $usable vendored upstream schemes; list them with:"
        echo "  just vendored"
    else
        echo ""
        echo "\`just fetch-schemes\` adds ~500 upstream tinted-theming schemes."
    fi

# `sort -u` because a handful of names exist in both base16/ and base24/;
# `_resolve` prefers base16, so listing them twice would misrepresent the choice.
[doc('List the vendored upstream schemes')]
vendored:
    @find {{SCHEMES}}/vendor/base16 {{SCHEMES}}/vendor/base24 \
        \( -name '*.yaml' -o -name '*.yml' \) 2>/dev/null \
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
    if [[ ! -f "{{BUILD}}/cozy.toml" ]]; then
        echo "nothing built yet — run \`just theme <scheme>\`" >&2
        exit 1
    fi
    rm -f cozy.zip
    cd "{{BUILD}}" && zip -qr ../cozy.zip cozy.toml cozy
    echo "cozy.zip"

# Install the bundled loadout into ~/.config/minimal/loadouts/.
install: bundle
    mkdir -p ~/.config/minimal/loadouts
    unzip -oq cozy.zip -d ~/.config/minimal/loadouts
    @echo "installed. Apply the loadout, then run \`just bat-cache\`."

# Run this once the loadout's patches have been applied — it reads whatever is
# in ~/.config/bat/themes, so running it earlier caches an empty theme set, and
# running it this way keeps any themes of your own that live there too.
[doc("Rebuild bat's theme cache so it picks up the rendered theme")]
bat-cache:
    bat cache --build
    @echo "bat themes now available:"
    @bat --list-themes | grep -v '^ ' || true

# Deliberately not part of `install`: it edits ~/.gitconfig, which the loadout
# otherwise never touches. The include path does not change with the scheme, so
# this only needs running once ever.
[doc("Point git's global config at the delta theme (once, ever)")]
delta-include:
    git config --global include.path ~/.config/git/cozy-delta.gitconfig
    @echo "delta feature now resolving to:"
    @delta --show-config | grep -E 'syntax-theme|minus-style'

# Run the renderer's tests.
test:
    cargo test --quiet --release --manifest-path tools/cozy-theme/Cargo.toml

# Drop build artefacts. Leaves schemes/vendor/ alone.
clean:
    rm -rf {{BUILD}} cozy.zip
    cargo clean --quiet --manifest-path tools/cozy-theme/Cargo.toml
