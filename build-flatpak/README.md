# Build flatpak

Copy [flatpak-cargo-generator.py](https://github.com/flatpak/flatpak-builder-tools/raw/master/cargo/flatpak-cargo-generator.py) to the repo.

Update the `cargo-sources.json` file with:

```shell
python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
```

Test the build/install with:

```shell
flatpak-builder --install --force-clean ./test-flatpak com.arviceblot.eso-addon-manager.json --user -y
```

On Steam Deck, flatpak-builder is not immediately available, but can be installed via flatpak:

```shell
flatpak install flathub org.flatpak.Builder
```

Then build with:

```shell
flatpak run org.flatpak.Builder --install --force-clean ./test-flatpak com.arviceblot.eso-addon-manager.json --user -y
```

You may also need to install the rust extension if a build error indicates it is missing:

```shell
flatpak install org.freedesktop.Sdk.Extension.rust-stable
```

Test running with:

```shell
flatpak run com.arviceblot.eso-addon-manager
```
