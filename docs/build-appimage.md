# Build AppImage

## 1. Install cargo-appimage

Requires [appimagetool](https://appimage.github.io/appimagetool/)

```shell
cargo install cargo-appimage
```

## 2. Build

```shell
cargo appimage
```

This creates `target/eso-addon-manager.AppDir`. Then it turns this directory into an AppImage file created in the project root.

