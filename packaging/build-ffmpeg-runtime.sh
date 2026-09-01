#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <install-prefix>" >&2
    exit 2
fi

readonly ffmpeg_version="9.0.1"
readonly ffmpeg_sha256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"
readonly ffmpeg_url="https://ffmpeg.org/releases/ffmpeg-${ffmpeg_version}.tar.xz"
readonly install_prefix="$1"

for tool in curl find grep make mktemp nasm nproc patchelf pkg-config realpath sha256sum tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "required build tool is unavailable: $tool" >&2
        exit 127
    fi
done

for module in libva libva-drm libdrm vulkan dav1d zlib; do
    if ! pkg-config --exists "$module"; then
        echo "required build dependency is unavailable: $module" >&2
        exit 127
    fi
done

vulkan_include=$(pkg-config --variable=includedir vulkan 2>/dev/null || printf /usr/include)
if [[ ! -f "${vulkan_include:-/usr/include}/vulkan/vulkan.h" ]]; then
    echo "required build dependency is unavailable: vulkan/vulkan.h" >&2
    echo "install the Vulkan headers package, not only the loader" >&2
    exit 127
fi

case "$install_prefix" in
    /*) ;;
    *)
        echo "install prefix must be absolute: $install_prefix" >&2
        exit 2
        ;;
esac
case "/$install_prefix/" in
    *$'\n'*|*$'\r'*|*$'\t'*|*' '*)
        echo "install prefix must not contain whitespace: $install_prefix" >&2
        exit 2
        ;;
    */../*|*/./*)
        echo "install prefix must be normalized: $install_prefix" >&2
        exit 2
        ;;
esac
if [[ "$install_prefix" == "/" || -L "$install_prefix" ]]; then
    echo "refusing unsafe install prefix: $install_prefix" >&2
    exit 2
fi
if [[ "$(realpath -m -- "$install_prefix")" != "${install_prefix%/}" ]]; then
    echo "install prefix must not traverse symlinks: $install_prefix" >&2
    exit 2
fi
if [[ -e "$install_prefix" && ! -d "$install_prefix" ]]; then
    echo "install prefix is not a directory: $install_prefix" >&2
    exit 2
fi
if [[ -d "$install_prefix" ]] && find "$install_prefix" -mindepth 1 -print -quit | grep -q .; then
    echo "install prefix must be empty: $install_prefix" >&2
    exit 2
fi

readonly build_jobs="${SKWD_FFMPEG_JOBS:-$(nproc)}"
if [[ ! "$build_jobs" =~ ^[1-9][0-9]*$ ]] || (( build_jobs > 64 )); then
    echo "SKWD_FFMPEG_JOBS must be an integer between 1 and 64: $build_jobs" >&2
    exit 2
fi

build_dir="$(mktemp -d)"
trap 'rm -rf -- "$build_dir"' EXIT

archive="$build_dir/ffmpeg-${ffmpeg_version}.tar.xz"

vendored_archive="${SKWD_FFMPEG_TARBALL:-}"
if [ -z "$vendored_archive" ]; then
    vendored_archive="$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)/vendor-ffmpeg/ffmpeg-${ffmpeg_version}.tar.xz"
fi
if [ -f "$vendored_archive" ]; then
    cp -- "$vendored_archive" "$archive"
else
    curl --fail --location --proto '=https' --tlsv1.2 "$ffmpeg_url" --output "$archive"
fi
printf '%s  %s\n' "$ffmpeg_sha256" "$archive" | sha256sum --check --status

tar -xf "$archive" -C "$build_dir"
source_dir="$build_dir/ffmpeg-${ffmpeg_version}"

readonly runtime_prefix="/usr/lib/skwd-paper"
readonly stage_dir="$build_dir/stage"

(
    cd "$source_dir"
    ./configure \
        --prefix="$runtime_prefix" \
        --disable-static \
        --enable-shared \
        --enable-pic \
        --disable-autodetect \
        --disable-programs \
        --disable-doc \
        --disable-debug \
        --disable-network \
        --disable-avdevice \
        --disable-avfilter \
        --disable-encoders \
        --disable-muxers \
        --enable-avcodec \
        --enable-avformat \
        --enable-avutil \
        --enable-swresample \
        --enable-swscale \
        --enable-zlib \
        --enable-vaapi \
        --enable-vulkan \
        --enable-libdrm \
        --enable-libdav1d
)

make -C "$source_dir" -j "$build_jobs"
make -C "$source_dir" install DESTDIR="$stage_dir"

mkdir -p "$install_prefix"
cp -a "$stage_dir$runtime_prefix/." "$install_prefix/"

for module in "$install_prefix"/lib/pkgconfig/*.pc; do
    [ -f "$module" ] || continue
    sed -i "s|^prefix=$runtime_prefix\$|prefix=$install_prefix|" "$module"
done

for library in \
    libavcodec.so.63 \
    libavformat.so.63 \
    libavutil.so.61 \
    libswresample.so.7 \
    libswscale.so.10
do
    if [[ ! -f "$install_prefix/lib/$library" ]]; then
        echo "private FFmpeg build did not produce $library" >&2
        exit 1
    fi
done

for library in "$install_prefix"/lib/*.so.*.*; do
    [[ -f "$library" ]] || continue
    patchelf --set-rpath '$ORIGIN' "$library"
done

for program in "$install_prefix"/bin/*; do
    [[ -f "$program" ]] || continue
    patchelf --set-rpath '$ORIGIN/../lib' "$program"
    if ! "$program" -hide_banner -version >/dev/null 2>&1; then
        echo "installed program cannot load its own libraries: $program" >&2
        exit 1
    fi
done

license_dir="$install_prefix/share/licenses/ffmpeg"
mkdir -p "$license_dir"
cp "$source_dir/COPYING.LGPLv2.1" "$source_dir/LICENSE.md" "$license_dir/"
cp "$archive" "$license_dir/"
printf '%s\n' "$ffmpeg_url" > "$license_dir/SOURCE"
printf '%s\n' "$ffmpeg_sha256" > "$license_dir/SHA256"
