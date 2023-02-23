# Build flatpak

Copy [flatpak-cargo-generator.py](https://github.com/flatpak/flatpak-builder-tools/raw/master/cargo/flatpak-cargo-generator.py) to the repo.

Update the `cargo-sources.json` file with:

```shell
python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
```

Test the build/install with:

```shell
flatpak-builder --install ./test-flatpak com.arviceblot.eso-addon-manager.json --user -y
```

Test running with:

```shell
flatpak run com.arviceblot.eso-addon-manager
```
