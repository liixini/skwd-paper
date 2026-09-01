#!/bin/sh
set -eu
zypper --non-interactive install \
    curl xz file gcc gcc-c++ pkgconf-pkg-config clang-devel nasm shaderc shaderc-devel \
    alsa-devel libpulse-devel libxkbcommon-devel \
    wayland-devel vulkan-devel \
    'pkgconfig(libavcodec)' 'pkgconfig(libavformat)' 'pkgconfig(libavutil)' \
    'pkgconfig(libswscale)' 'pkgconfig(libswresample)' \
    python3 ffmpeg procps
zypper clean -a
