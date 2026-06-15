#!/usr/bin/env bash
# Install the headless X11/Wayland runtime that smoke-appimage.sh launches
# against (CI helper; mirrors a common desktop's runtime libraries).
set -euo pipefail

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  xvfb weston \
  libgl1-mesa-dri libglx-mesa0 libegl1 libgles2 \
  libx11-6 libxcursor1 libxrandr2 libxi6 libxext6 libxcb1 libxfixes3 \
  libxkbcommon0 libxkbcommon-x11-0 \
  libwayland-client0 libwayland-cursor0 libwayland-egl1
