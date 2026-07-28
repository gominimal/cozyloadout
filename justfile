# Bundle the cozy loadout into cozy.zip
bundle:
    rm -f cozy.zip
    zip -r cozy.zip cozy.toml cozy

# Install the bundled loadout into ~/.config/minimal/loadouts/
install: bundle
    mkdir -p ~/.config/minimal/loadouts
    unzip -o cozy.zip -d ~/.config/minimal/loadouts
