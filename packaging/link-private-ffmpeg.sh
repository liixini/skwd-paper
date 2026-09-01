#!/usr/bin/env bash
set -euo pipefail

ffmpeg_prefix="${SKWD_FFMPEG_PREFIX:?SKWD_FFMPEG_PREFIX is required}"
case "$ffmpeg_prefix" in
    /*) ;;
    *)
        echo "SKWD_FFMPEG_PREFIX must be absolute: $ffmpeg_prefix" >&2
        exit 2
        ;;
esac
if [[ "$ffmpeg_prefix" == "/" || -L "$ffmpeg_prefix" || ! -d "$ffmpeg_prefix/lib" ]]; then
    echo "refusing unsafe SKWD_FFMPEG_PREFIX: $ffmpeg_prefix" >&2
    exit 2
fi
for library in libavcodec.so.63 libavformat.so.63 libavutil.so.61 libswresample.so.7 libswscale.so.10; do
    if [[ ! -f "$ffmpeg_prefix/lib/$library" ]]; then
        echo "private FFmpeg runtime is missing $library" >&2
        exit 1
    fi
done
if ! command -v cc >/dev/null 2>&1; then
    echo "C compiler driver is unavailable: cc" >&2
    exit 127
fi
exec cc "-L${ffmpeg_prefix}/lib" "$@"
