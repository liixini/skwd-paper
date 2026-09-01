#!/bin/sh
set -e
cd "$(dirname "$0")/.."
VULKAN_INCLUDE_DIR="$HOME/.cache/skwd-build/Vulkan-Headers/include" \
  cargo build --release -p skwd-wall-vk --features shared-device
strings target/release/skwd-wall-vk | grep -q "path = shared-device (zero-copy)" \
  && echo "skwd-wall-vk: zero-copy build OK"
