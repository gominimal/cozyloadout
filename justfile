# Bundle the cozy loadout into cozy.zip
bundle:
    rm -f cozy.zip
    zip -r cozy.zip cozy.toml cozy

# Install the bundled loadout into ~/.config/minimal/loadouts/
install: bundle
    mkdir -p ~/.config/minimal/loadouts
    unzip -o cozy.zip -d ~/.config/minimal/loadouts

# Rebuild bat's theme cache so it picks up the Minimal themes.
# Run this once the loadout's patches have been applied — it reads whatever is
# in ~/.config/bat/themes, so running it earlier caches an empty theme set, and
# running it this way keeps any themes of your own that live there too.
bat-cache:
    bat cache --build
    @echo "bat themes now available:"
    @bat --list-themes | grep '^minimal-'

# Point git's global config at the delta theme. Deliberately not part of
# `install`: it edits ~/.gitconfig, which the loadout otherwise never touches.
delta-include:
    git config --global include.path ~/.config/git/minimal-delta.gitconfig
    @echo "delta feature now resolving to:"
    @delta --show-config | grep -E 'syntax-theme|minus-style'
