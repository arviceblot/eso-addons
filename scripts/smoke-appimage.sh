#!/usr/bin/env bash
# Launch the AppImage headlessly under X11 and Wayland and confirm the GUI comes
# up. With ESO_ADDONS_SMOKE_TEST set the app prints MARKER once a frame renders
# (proving the winit/GL init that crashed in #465) and exits 0; an init failure
# exits non-zero without the marker.
#
# Runs in a throwaway HOME with a seeded offline config so the result depends on
# the artifact, not on local state or network.
set -euo pipefail

APPIMAGE="${1:-$(ls dist/*-x86_64.AppImage 2>/dev/null | head -1 || true)}"
if [ -z "$APPIMAGE" ] || [ ! -x "$APPIMAGE" ]; then
  echo "usage: $0 [path-to-appimage]" >&2
  exit 1
fi
APPIMAGE="$(cd "$(dirname "$APPIMAGE")" && pwd)/$(basename "$APPIMAGE")"

MARKER="eso-addons: smoke test ok"

export APPIMAGE_EXTRACT_AND_RUN=1
export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
export ESO_ADDONS_SMOKE_TEST=1

seed_home() {
  local home="$1"
  mkdir -p "$home/.config/eso-addons" "$home/addons"
  cat > "$home/.config/eso-addons/config.json" <<JSON
{
  "addon_dir": "$home/addons",
  "file_list": "unused",
  "file_details": "unused",
  "list_files": "unused",
  "category_list": "unused",
  "update_on_launch": false,
  "onboard": false
}
JSON
}

check() {
  local label="$1" log="$2" rc="$3"
  if [ "$rc" -eq 0 ] && grep -qF "$MARKER" "$log"; then
    echo "[$label] ok"
    return 0
  fi
  echo "[$label] FAILED (exit $rc)"
  cat "$log"
  return 1
}

smoke_x11() {
  local home log rc=0
  home="$(mktemp -d)"
  seed_home "$home"
  log="$(mktemp)"
  HOME="$home" XDG_CONFIG_HOME="$home/.config" \
    timeout 60 env -u WAYLAND_DISPLAY xvfb-run -a -s "-screen 0 1280x800x24" \
    "$APPIMAGE" >"$log" 2>&1 || rc=$?
  check x11 "$log" "$rc"
}

smoke_wayland() {
  local home log rc=0 runtime wpid i
  home="$(mktemp -d)"
  seed_home "$home"
  runtime="$(mktemp -d)"
  chmod 700 "$runtime"
  XDG_RUNTIME_DIR="$runtime" weston --backend=headless-backend.so \
    --socket=wayland-smoke --idle-time=0 >/tmp/weston-smoke.log 2>&1 &
  wpid=$!
  for ((i = 0; i < 50; i++)); do
    [ -S "$runtime/wayland-smoke" ] && break
    sleep 0.2
  done
  if [ ! -S "$runtime/wayland-smoke" ]; then
    echo "[wayland] FAILED: weston did not start"
    cat /tmp/weston-smoke.log
    kill "$wpid" 2>/dev/null || true
    return 1
  fi
  log="$(mktemp)"
  HOME="$home" XDG_CONFIG_HOME="$home/.config" XDG_RUNTIME_DIR="$runtime" \
    WAYLAND_DISPLAY=wayland-smoke timeout 60 env -u DISPLAY \
    "$APPIMAGE" >"$log" 2>&1 || rc=$?
  kill "$wpid" 2>/dev/null || true
  check wayland "$log" "$rc"
}

rc=0
for backend in ${SMOKE_BACKENDS:-x11 wayland}; do
  "smoke_$backend" || rc=1
done
exit $rc
