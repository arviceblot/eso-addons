# Build flatpak

## Install flatpak-builder and rust dependencies

```shell
flatpak install flathub org.flatpak.Builder
flatpak install org.freedesktop.Sdk.Extension.rust-stable
```

Copy [flatpak-cargo-generator.py](https://github.com/flatpak/flatpak-builder-tools/raw/master/cargo/flatpak-cargo-generator.py) to the repo.

Update the `cargo-sources.json` file with:

```shell
python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
```

Then build with:

```shell
flatpak run org.flatpak.Builder --install --force-clean ./test-flatpak com.arviceblot.eso-addon-manager.json --user -y
```

Test running with:

```shell
flatpak run com.arviceblot.eso-addon-manager
```
