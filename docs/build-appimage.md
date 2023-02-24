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

This creates `target/eso-addon-manager.AppDir`. We need to turn this in to an AppImage file. Use [linuxdeploy-plugin-appimage](https://github.com/linuxdeploy/linuxdeploy-plugin-appimage).

```shell
./linuxdeploy-plugin-appimage-x86_64.AppImage --appdir target/eso-addon-manager.AppDir
```
