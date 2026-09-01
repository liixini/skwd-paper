#!/bin/sh
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    ca-certificates curl xz-utils file \
    build-essential pkg-config clang libclang-dev nasm glslc libshaderc-dev \
    libvulkan-dev \
    libasound2-dev libpulse-dev libxkbcommon-dev \
    libwayland-dev libegl1-mesa-dev \
    libdav1d-dev libyuv-dev \
    libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libswresample-dev \
    python3 ffmpeg procps
rm -rf /var/lib/apt/lists/*
