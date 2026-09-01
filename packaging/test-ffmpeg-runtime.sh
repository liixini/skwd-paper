#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <ffmpeg-prefix>" >&2
    exit 2
fi

readonly ffmpeg_prefix="$1"

if [[ ! -d "$ffmpeg_prefix/lib" || ! -d "$ffmpeg_prefix/include" ]]; then
    echo "not an FFmpeg prefix: $ffmpeg_prefix" >&2
    exit 2
fi
if ! command -v cc >/dev/null 2>&1; then
    echo "C compiler driver is unavailable: cc" >&2
    exit 127
fi

for library in libavcodec.so.63 libavformat.so.63 libavutil.so.61 libswresample.so.7 libswscale.so.10; do
    if [[ ! -f "$ffmpeg_prefix/lib/$library" ]]; then
        echo "missing $library" >&2
        exit 1
    fi
done

work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT

cat > "$work_dir/probe.c" <<'PROBE'
#include <stdio.h>
#include <libavutil/avutil.h>
#include <libavutil/hwcontext.h>
#include <libavcodec/avcodec.h>

static int codec_has(const AVCodec *codec, enum AVHWDeviceType want) {
    if (!codec) return 0;
    for (int index = 0;; index++) {
        const AVCodecHWConfig *config = avcodec_get_hw_config(codec, index);
        if (!config) return 0;
        if (config->device_type == want) return 1;
    }
}

int main(void) {
    printf("ffmpeg %s\n", av_version_info());

    int vaapi = 0, vulkan = 0;
    enum AVHWDeviceType type = AV_HWDEVICE_TYPE_NONE;
    while ((type = av_hwdevice_iterate_types(type)) != AV_HWDEVICE_TYPE_NONE) {
        printf("hwdevice %s\n", av_hwdevice_get_type_name(type));
        if (type == AV_HWDEVICE_TYPE_VAAPI) vaapi = 1;
        if (type == AV_HWDEVICE_TYPE_VULKAN) vulkan = 1;
    }

    const AVCodec *dav1d = avcodec_find_decoder_by_name("libdav1d");
    const AVCodec *h264 = avcodec_find_decoder(AV_CODEC_ID_H264);
    const AVCodec *hevc = avcodec_find_decoder(AV_CODEC_ID_HEVC);

    // Paper selects the native av1 decoder by name; the default lookup returns
    // libdav1d, which is software-only and carries no hwaccel configs.
    const AVCodec *av1 = avcodec_find_decoder_by_name("av1");
    const AVCodec *av1_default = avcodec_find_decoder(AV_CODEC_ID_AV1);
    printf("av1 default lookup: %s\n", av1_default ? av1_default->name : "none");

    int h264_vaapi = codec_has(h264, AV_HWDEVICE_TYPE_VAAPI);
    int h264_vulkan = codec_has(h264, AV_HWDEVICE_TYPE_VULKAN);
    int hevc_vaapi = codec_has(hevc, AV_HWDEVICE_TYPE_VAAPI);
    int hevc_vulkan = codec_has(hevc, AV_HWDEVICE_TYPE_VULKAN);
    int av1_vaapi = codec_has(av1, AV_HWDEVICE_TYPE_VAAPI);
    int av1_vulkan = codec_has(av1, AV_HWDEVICE_TYPE_VULKAN);

    printf("libdav1d %d\n", dav1d ? 1 : 0);
    printf("h264 vaapi=%d vulkan=%d\n", h264_vaapi, h264_vulkan);
    printf("hevc vaapi=%d vulkan=%d\n", hevc_vaapi, hevc_vulkan);
    printf("av1 vaapi=%d vulkan=%d\n", av1_vaapi, av1_vulkan);

    if (!vaapi) { printf("FAIL no vaapi hwdevice\n"); return 1; }
    if (!vulkan) { printf("FAIL no vulkan hwdevice\n"); return 1; }
    if (!dav1d) { printf("FAIL no libdav1d decoder\n"); return 1; }
    if (!h264_vaapi || !hevc_vaapi || !av1_vaapi) { printf("FAIL missing vaapi hwaccel\n"); return 1; }
    if (!h264_vulkan || !hevc_vulkan || !av1_vulkan) { printf("FAIL missing vulkan hwaccel\n"); return 1; }
    printf("PASS\n");
    return 0;
}
PROBE

cc "$work_dir/probe.c" -o "$work_dir/probe" \
    "-I$ffmpeg_prefix/include" \
    "-L$ffmpeg_prefix/lib" \
    -lavcodec -lavformat -lavutil -lswresample -lswscale \
    "-Wl,-rpath,$ffmpeg_prefix/lib"

"$work_dir/probe"
