#!/usr/bin/env bash
# Build the Linux AppImage. VERSION (default: git describe) and GLIBC_VERSION
# (default: 2.28) may be overridden from the environment.
set -euo pipefail

BINARY_NAME="eso-addon-manager"
ARCH="${ARCH:-x86_64}"
TARGET="${ARCH}-unknown-linux-gnu"
OUTDIR="dist"
GLIBC_VERSION="${GLIBC_VERSION:-2.28}"
VERSION="${VERSION:-$(git describe --tags --always --dirty 2>/dev/null || echo dev)}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo zigbuild --release --target="${TARGET}.${GLIBC_VERSION}"
BIN="target/${TARGET}/release/${BINARY_NAME}"

APPIMAGETOOL="$(command -v appimagetool || true)"
if [ -z "$APPIMAGETOOL" ]; then
  APPIMAGETOOL="${ROOT}/.cache/appimagetool"
  if [ ! -x "$APPIMAGETOOL" ]; then
    mkdir -p "${ROOT}/.cache"
    curl -fL -o "$APPIMAGETOOL" \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage"
    chmod +x "$APPIMAGETOOL"
  fi
fi

APPDIR="$(mktemp -d)/AppDir"
mkdir -p "${APPDIR}/usr/bin"
cp "$BIN" "${APPDIR}/usr/bin/"
cp "data/${BINARY_NAME}.desktop" "${APPDIR}/"
cp "data/icon.png" "${APPDIR}/${BINARY_NAME}.png"
cat > "${APPDIR}/AppRun" <<EOF
#!/bin/sh
HERE="\$(dirname "\$(readlink -f "\$0")")"
exec "\$HERE/usr/bin/${BINARY_NAME}" "\$@"
EOF
chmod +x "${APPDIR}/AppRun"

# appimagetool writes the .zsync into its working directory, so run it from OUTDIR.
mkdir -p "$OUTDIR"
cd "$OUTDIR"
ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" \
  -u "gh-releases-zsync|arviceblot|eso-addons|latest|${BINARY_NAME}-*${ARCH}.AppImage.zsync" \
  "$APPDIR" "${BINARY_NAME}-${VERSION}-${ARCH}.AppImage"
