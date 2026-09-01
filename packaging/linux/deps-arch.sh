#!/bin/sh
set -eu
pacman -Syu --noconfirm --needed \
    curl xz file gcc pkgconf clang nasm shaderc \
    vulkan-headers vulkan-icd-loader \
    alsa-lib libpulse libxkbcommon wayland mesa ffmpeg dav1d libyuv \
    python procps-ng
pacman -Scc --noconfirm
