# Build AppImage

The AppImage is built with [cargo-zigbuild], which pins a glibc symbol-version
floor so the binary runs on older distributions while still linking dynamically
(winit needs `dlopen` for X11/Wayland, which a static build cannot do).

## 1. Install dependencies

[appimagetool] is fetched automatically if it is not on `PATH`.

```shell
cargo install --locked cargo-zigbuild
pip install ziglang   # or install zig from your package manager
```

## 2. Build

```shell
./scripts/build-appimage.sh
```

This produces `dist/eso-addon-manager-<version>-x86_64.AppImage` and a matching
`.zsync`. `<version>` comes from `git describe`; override it, and other settings,
with environment variables (`VERSION`, `GLIBC_VERSION`, `OUTDIR`, ...) — see the
top of the script.

## 3. Run

```shell
./dist/eso-addon-manager-*-x86_64.AppImage
```

To check it launches headlessly under X11 and Wayland (as CI does), run
`./scripts/smoke-appimage.sh`. It needs `xvfb` and `weston` on `PATH`; set
`SMOKE_BACKENDS=x11` to run only one.

[cargo-zigbuild]: https://github.com/rust-cross/cargo-zigbuild
[appimagetool]: https://appimage.github.io/appimagetool/
