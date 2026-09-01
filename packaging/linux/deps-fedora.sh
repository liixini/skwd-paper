#!/bin/sh
set -eu
dnf install -y \
    curl xz file gcc gcc-c++ pkgconf-pkg-config clang clang-devel nasm glslc \
    libshaderc-devel vulkan-headers \
    alsa-lib-devel pulseaudio-libs-devel libxkbcommon-devel \
    wayland-devel vulkan-loader-devel \
    libdav1d-devel libyuv-devel \
    libavcodec-free-devel libavformat-free-devel libavutil-free-devel \
    libswscale-free-devel libswresample-free-devel \
    python3 ffmpeg-free procps-ng
dnf clean all
