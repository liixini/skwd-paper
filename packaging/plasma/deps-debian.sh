#!/bin/sh
set -eu

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    cmake pkg-config \
    qt6-base-dev qt6-declarative-dev \
    libdrm-dev libegl1-mesa-dev libgl-dev
rm -rf /var/lib/apt/lists/*
