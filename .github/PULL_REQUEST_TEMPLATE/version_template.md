# [VERSION]

Change summary.

## Release checklist

- [ ] Bump version in `Cargo.toml`
- [ ] Add release notes to `data/com.arviceblot.eso-addon-manager.metainfo.xml`
- [ ] Take new screenshots if applicable

### Post-merge actions

- [ ] Update `cargo-sources.json`:

  ```shell
  python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
  ```

#### Update flatpak repository

- [ ] `cargo-sources.json`
- [ ] git commit hash in `com.arviceblot.eso-addon-manager.json`
