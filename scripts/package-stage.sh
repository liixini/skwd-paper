#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
destination=${1:?usage: scripts/package-stage.sh DESTINATION}
case "$destination" in
    /)
        echo "refusing to stage directly into /" >&2
        exit 2
        ;;
    /*) ;;
    *) destination="$root/$destination" ;;
esac

if [ -L "$destination" ] || { [ -e "$destination" ] && [ ! -d "$destination" ]; }; then
    echo "package destination must be a directory: $destination" >&2
    exit 2
fi
if [ -d "$destination" ] && [ -n "$(find "$destination" -mindepth 1 -print -quit)" ]; then
    echo "package destination is not empty: $destination" >&2
    exit 2
fi

binary_directory=${SKWD_PAPER_BIN_DIR:-$root/target/release}
case "$binary_directory" in
    /*) ;;
    *) binary_directory="$root/$binary_directory" ;;
esac

for binary in skwd-paper skwd-paper-tinier skwd-wall-still skwd-wall-vk; do
    path="$binary_directory/$binary"
    if [ ! -f "$path" ] || [ ! -x "$path" ]; then
        echo "missing executable Paper release binary: $path" >&2
        exit 1
    fi
done

for path in \
    "$root/LICENSE" \
    "$root/LICENSES/dav1d-BSD-2-Clause.txt" \
    "$root/LICENSES/ffmpeg-sys-the-third-WTFPL.txt" \
    "$root/LICENSES/OpenH264-BSD-2-Clause.txt" \
    "$root/LICENSES/libyuv-BSD-3-Clause.txt"
do
    if [ ! -f "$path" ]; then
        echo "missing Paper license material: $path" >&2
        exit 1
    fi
done

umask 022
license_directory="$destination/usr/share/licenses/skwd-paper"
third_party_directory="$license_directory/third-party"
mkdir -p "$destination/usr/bin" "$destination/usr/lib/skwd-paper" "$third_party_directory"

install -m755 "$binary_directory/skwd-paper" "$destination/usr/bin/skwd-paper-v2"
for binary in skwd-wall-still skwd-wall-vk; do
    install -m755 "$binary_directory/$binary" "$destination/usr/bin/$binary"
done
install -m755 "$binary_directory/skwd-paper-tinier" \
    "$destination/usr/lib/skwd-paper/skwd-paper-tinier"

install -m644 "$root/LICENSE" "$license_directory/LICENSE"
install -m644 "$root/LICENSES/OpenH264-BSD-2-Clause.txt" \
    "$third_party_directory/OpenH264-BSD-2-Clause.txt"
install -m644 "$root/LICENSES/dav1d-BSD-2-Clause.txt" \
    "$third_party_directory/dav1d-BSD-2-Clause.txt"
install -m644 "$root/LICENSES/libyuv-BSD-3-Clause.txt" \
    "$third_party_directory/libyuv-BSD-3-Clause.txt"
install -m644 "$root/LICENSES/ffmpeg-sys-the-third-WTFPL.txt" \
    "$third_party_directory/ffmpeg-sys-the-third-LICENSE"
