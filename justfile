SCHEMES := "schemes"
BUILD   := "build"
RENDER  := "cargo run --quiet --release --manifest-path tools/cozy-theme/Cargo.toml --"

# Show the recipes and the schemes on hand.
default:
    @just --list --unsorted
    @echo ""
    @just schemes

# Render the loadout with a base16 scheme and bundle it into cozy.zip.
#
#     just theme                       # the default scheme
#     just theme gruvbox-dark-hard     # a name under schemes/ (or schemes/vendor/)
#     just theme ~/my-scheme.yaml      # any path
#
# The theme files are named after the scheme, so switching schemes replaces
# them rather than leaving a stale name on disk holding the wrong colours.
theme scheme="minimal-dark": (_render scheme) bundle

# Render only — leaves the tree in build/ without zipping it. Useful for
# eyeballing a diff before committing.
render scheme="minimal-dark": (_render scheme)

_render scheme:
    @{{RENDER}} "$(just _resolve {{scheme}})" --templates templates --out {{BUILD}}

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
    # system the loadout can't use — never resolve to one.
    hit=""
    for dir in "{{SCHEMES}}" "{{SCHEMES}}/vendor/base16" "{{SCHEMES}}/vendor/base24"; do
        [[ -d "$dir" ]] || continue
        hit=$(find "$dir" -not -path '*/vendor/*' \( -name "$arg.yaml" -o -name "$arg.yml" \) \
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
    echo "In {{SCHEMES}}/:"
    find "{{SCHEMES}}" -not -path '*/vendor/*' \( -name '*.yaml' -o -name '*.yml' \) 2>/dev/null \
        | sed 's|.*/||; s|\.ya\?ml$||' | sort | sed 's/^/  /'
    if [[ -d "{{SCHEMES}}/vendor" ]]; then
        n=$(find "{{SCHEMES}}/vendor" \( -name '*.yaml' -o -name '*.yml' \) | wc -l | tr -d ' ')
        echo ""
        echo "Plus $n vendored upstream schemes; list them with:"
        echo "  find {{SCHEMES}}/vendor -name '*.yaml' | sed 's|.*/||; s|\.yaml\$||' | sort"
    else
        echo ""
        echo "\`just fetch-schemes\` adds the ~335 upstream tinted-theming schemes."
    fi

# Clone the upstream tinted-theming scheme collection into schemes/vendor/.
# Gitignored, so it stays out of this repo's history.
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

# Rebuild bat's theme cache so it picks up the rendered theme.
# Run this once the loadout's patches have been applied — it reads whatever is
# in ~/.config/bat/themes, so running it earlier caches an empty theme set, and
# running it this way keeps any themes of your own that live there too.
bat-cache:
    bat cache --build
    @echo "bat themes now available:"
    @bat --list-themes | grep -v '^ ' || true

# Point git's global config at the delta theme. Deliberately not part of
# `install`: it edits ~/.gitconfig, which the loadout otherwise never touches.
# The include path is fixed, so this only needs running once ever.
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
