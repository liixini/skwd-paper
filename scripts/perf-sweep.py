#!/usr/bin/env python3
"""Standing performance sweep for the wallpaper stack.

Runs low, mid, and max workload tiers through the wallpaper renderer, measuring
per-process CPU/RSS/PSS/VRAM and GPU util/power deltas against a continuously
re-sampled idle floor. Low includes Vulkan with matched 1366x768 AV1, VP9,
lean H.264, and HEVC sources. Writes self-describing JSON
for regression diffing.

  perf-sweep.py                      complete coverage matrix, thorough profile
  perf-sweep.py --quick              fast profile: short windows, ONE 1s transition
  perf-sweep.py --full               123 cases: + every sand/effect on video<->video
  perf-sweep.py --out results.json
  perf-sweep.py --single-output DP-1
  perf-sweep.py --floor-video path/to/1366x768.mp4
  perf-sweep.py --scene item-a --scene item-b
  perf-sweep.py --only "nv12"
  perf-sweep.py --plan
  perf-sweep.py --check baseline.json   fail CPU/GPU/power/RSS/PSS/VRAM regressions
                                        (profile, sources, and outputs must match)
"""

import argparse
import concurrent.futures
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VK = ROOT / "target/release/skwd-wall-vk"
STILL = ROOT / "target/release/skwd-wall-still"
TINIER = ROOT / "target/release/skwd-paper-tinier"
TINIER_CACHE = Path.home() / ".cache/skwd-paper-v2/engine-sweep"
WALL = os.environ.get("SKWD_WALL_CLI") or shutil.which("skwd-wall")
ZERO_COPY_MARKER = b"path = shared-device (zero-copy)"
VIDEO_DIR = Path.home() / "wallpaper-videos"
STILL_DIR = Path.home() / "wallpaper"
FLOOR_SIZE = (1366, 768)
FLOOR_FPS = 30
MATRIX_VERSION = 18
COMPOSITOR_NAME = "niri"
FLOOR_CODECS = {
    "av1": {
        "encoder": "libsvtav1",
        "extension": "av1.mp4",
        "args": ["-preset", "10", "-crf", "35"],
    },
    "vp9": {
        "encoder": "libvpx-vp9",
        "extension": "vp9.webm",
        "args": ["-deadline", "good", "-cpu-used", "5", "-crf", "35", "-b:v", "0", "-row-mt", "1"],
    },
    "h264": {
        "encoder": "libx264",
        "extension": "h264.mp4",
        "args": [
            "-preset",
            "medium",
            "-crf",
            "23",
            "-tune",
            "fastdecode",
            "-refs",
            "2",
            "-bf",
            "0",
        ],
    },
    "hevc": {
        "encoder": "libx265",
        "extension": "hevc.mp4",
        "args": ["-preset", "fast", "-crf", "28", "-refs", "2", "-bf", "2"],
    },
}
PROFILES = {
    "thorough": {"settle": 5, "window": 10, "hz": 1, "swap_ms": 2500, "burst": True},
    "quick": {"settle": 1.5, "window": 1.5, "hz": 6, "swap_ms": 1000, "burst": False},
}
CHECK_MARGINS = {
    "cpu": 1.0,
    "gpu_delta": 1.0,
    "power_delta": 2.0,
    "rss": 8.0,
    "pss": 8.0,
    "vram": 16.0,
    "combined_pss": 8.0,
}
COMPOSITOR_CHECK_MARGINS = {
    "cpu": 1.0,
    "cpu_delta": 1.0,
    "rss_delta": 8.0,
    "pss_delta": 8.0,
    "vram_delta": 16.0,
}
MEDIA_INFO = {}


def sh(args):
    return subprocess.check_output(args).decode()


def drm_devices():
    devices = {}
    for render in Path("/sys/class/drm").glob("renderD*"):
        device = render / "device"
        try:
            resolved = device.resolve()
            vendor = (device / "vendor").read_text().strip()
        except OSError:
            continue
        devices[str(resolved)] = {"path": device, "vendor": vendor}
    return list(devices.values())


DRM_DEVICES = drm_devices()
NVIDIA_SMI = shutil.which("nvidia-smi")


def read_float(path, scale=1.0):
    try:
        return float(path.read_text().strip()) / scale
    except (OSError, ValueError):
        return None


def sysfs_gpu_sample():
    utils = []
    powers = []
    for device in DRM_DEVICES:
        util = read_float(device["path"] / "gpu_busy_percent")
        if util is not None:
            utils.append(util)
        for power_path in (device["path"] / "hwmon").glob("hwmon*/power1_average"):
            power = read_float(power_path, 1_000_000.0)
            if power is not None:
                powers.append(power)
    return max(utils, default=0.0), sum(powers)


def nvidia_gpu_sample():
    try:
        result = subprocess.run(
            [
                NVIDIA_SMI,
                "--query-gpu=utilization.gpu,power.draw",
                "--format=csv,noheader,nounits",
            ],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    rows = []
    for line in result.stdout.splitlines():
        try:
            util, power = (float(value.strip()) for value in line.split(",", 1))
        except ValueError:
            continue
        rows.append((util, power))
    if not rows:
        return None
    return max(row[0] for row in rows), sum(row[1] for row in rows)


def gpu_sampler_metadata():
    power = any(
        (device["path"] / "hwmon").glob("hwmon*/power1_average")
        for device in DRM_DEVICES
    )
    return {
        "backend": "nvidia-smi" if NVIDIA_SMI else "drm-sysfs",
        "vendors": sorted({device["vendor"] for device in DRM_DEVICES}),
        "gpu_utilization": bool(
            NVIDIA_SMI
            or any(
                (device["path"] / "gpu_busy_percent").is_file()
                for device in DRM_DEVICES
            )
        ),
        "board_power": bool(NVIDIA_SMI or power),
        "process_vram": bool(NVIDIA_SMI or Path("/proc/self/fdinfo").is_dir()),
    }


def parse_size(text):
    try:
        width, height = (int(part) for part in text.lower().split("x", 1))
    except (TypeError, ValueError):
        raise argparse.ArgumentTypeError("size must be WIDTHxHEIGHT") from None
    if width < 64 or height < 64:
        raise argparse.ArgumentTypeError("size dimensions must be at least 64")
    return width, height


def frame_rate(info):
    try:
        return float(Fraction(info["frame_rate"]))
    except (KeyError, ValueError, ZeroDivisionError):
        return 0.0


def media_info(path):
    path = str(Path(path).resolve())
    if path in MEDIA_INFO:
        return MEDIA_INFO[path]
    result = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_name,r_frame_rate",
            "-of",
            "json",
            path,
        ],
        capture_output=True,
        text=True,
        timeout=20,
    )
    if result.returncode != 0:
        raise RuntimeError(f"ffprobe failed for {path}: {result.stderr.strip()}")
    try:
        stream = json.loads(result.stdout)["streams"][0]
        stat = os.stat(path)
        info = {
            "path": path,
            "width": int(stream["width"]),
            "height": int(stream["height"]),
            "codec": str(stream.get("codec_name") or ""),
            "frame_rate": str(stream.get("r_frame_rate") or ""),
            "size": stat.st_size,
            "mtime_ns": stat.st_mtime_ns,
        }
    except (KeyError, IndexError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise RuntimeError(f"ffprobe returned no usable video stream for {path}") from error
    MEDIA_INFO[path] = info
    return info


def tinier_fixtures():
    entries = {
        "floor": (TINIER_CACHE / "matrix-floor-1366x768-tokyo-qp20.ivf", "30"),
        "mid": (TINIER_CACHE / "matrix-mid-2560x1440-5mGuCdlCcNM-qp20.ivf", "60"),
        "max": (TINIER_CACHE / "matrix-max-3840x2160-tokyo-qp20.ivf", "30"),
    }
    return {
        key: (str(path), fps) for key, (path, fps) in entries.items() if path.exists()
    }


def choose_sources(paths):
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
        infos = list(executor.map(media_info, paths))
    probed = sorted(
        zip(infos, (str(path) for path in paths)),
        key=lambda pair: (
            pair[0]["width"] * pair[0]["height"],
            pair[0]["width"],
            pair[0]["height"],
            pair[0]["size"],
            pair[1],
        ),
    )
    if len(probed) < 2:
        sys.exit("need >=2 probeable videos")
    mid = probed[(len(probed) - 1) // 2][1]
    return mid, probed[-1][1], probed[-2][1]


def choose_stills(paths):
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
        infos = list(executor.map(media_info, paths))
    probed = sorted(
        zip(infos, (str(path) for path in paths)),
        key=lambda pair: (
            pair[0]["width"] * pair[0]["height"],
            pair[0]["size"],
            pair[1],
        ),
    )
    if len(probed) < 2:
        sys.exit("need >=2 probeable stills")
    return probed[-1][1], probed[-2][1]


def ensure_floor_videos(source, size, supplied=None):
    width, height = size
    if supplied:
        info = media_info(supplied)
        if (info["width"], info["height"]) != size:
            sys.exit(
                f"--floor-video must be {width}x{height}, got "
                f"{info['width']}x{info['height']}"
            )
        source = str(Path(supplied).resolve())
    source_info = media_info(source)
    cache_base = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    cache = cache_base / "skwd-wall" / "perf-fixtures"
    cache.mkdir(parents=True, exist_ok=True)
    if not shutil.which("ffmpeg"):
        sys.exit("ffmpeg is required to generate the floor video; pass --floor-video")
    scale = (
        f"scale={width}:{height}:force_original_aspect_ratio=decrease,"
        f"pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,fps={FLOOR_FPS}"
    )
    fixtures = {}
    for codec, spec in FLOOR_CODECS.items():
        fingerprint = hashlib.sha256(
            json.dumps(
                {
                    "source": source_info,
                    "width": width,
                    "height": height,
                    "fps": FLOOR_FPS,
                    "codec": codec,
                    "encoder": spec,
                },
                sort_keys=True,
            ).encode()
        ).hexdigest()[:16]
        destination = (
            cache / f"floor-{width}x{height}-{FLOOR_FPS}fps-{fingerprint}.{spec['extension']}"
        )
        if destination.is_file():
            info = media_info(destination)
            if (info["width"], info["height"]) == size and info["codec"] == codec:
                fixtures[codec] = str(destination)
                continue
        temporary = destination.with_name(f"{destination.stem}.tmp{destination.suffix}")
        result = subprocess.run(
            [
                "ffmpeg",
                "-v",
                "error",
                "-nostdin",
                "-y",
                "-stream_loop",
                "-1",
                "-i",
                source,
                "-t",
                "20",
                "-map",
                "0:v:0",
                "-vf",
                scale,
                "-an",
                "-c:v",
                spec["encoder"],
                *spec["args"],
                "-pix_fmt",
                "yuv420p",
                str(temporary),
            ],
            capture_output=True,
            text=True,
            timeout=600,
        )
        if result.returncode != 0:
            temporary.unlink(missing_ok=True)
            sys.exit(
                f"floor {codec} generation failed; install the {spec['encoder']} "
                f"ffmpeg encoder: {result.stderr.strip()[-400:]}"
            )
        temporary.replace(destination)
        MEDIA_INFO.pop(str(destination.resolve()), None)
        info = media_info(destination)
        if (info["width"], info["height"]) != size or info["codec"] != codec:
            sys.exit(
                f"generated floor {codec} is {info['codec']} "
                f"{info['width']}x{info['height']}, expected {codec} {width}x{height}"
            )
        if frame_rate(info) > FLOOR_FPS + 0.01:
            sys.exit(f"generated floor {codec} is {frame_rate(info):.2f} fps")
        fixtures[codec] = str(destination)
    return fixtures


def config_monitor():
    return os.environ.get("SKWD_PAPER_OUTPUT", "").strip()


def active_outputs():
    if not WALL:
        return []
    result = subprocess.run(
        [WALL, "displays", "--json"],
        capture_output=True,
        text=True,
        timeout=10,
    )
    if result.returncode != 0:
        return []
    try:
        outputs = json.loads(result.stdout)
    except json.JSONDecodeError:
        return []
    if not isinstance(outputs, list):
        return []
    return sorted(
        (output for output in outputs if isinstance(output, dict) and output.get("name")),
        key=lambda output: output["name"],
    )


def resolve_single_output(requested, outputs):
    names = {output["name"] for output in outputs}
    candidates = [requested, config_monitor()]
    candidates.extend(output["name"] for output in outputs)
    for candidate in candidates:
        if candidate and (not names or candidate in names):
            return candidate
    sys.exit("could not discover a single output; pass --single-output OUTPUT")


def ranked_outputs(outputs, fallback):
    usable = [
        output
        for output in outputs
        if isinstance(output.get("width"), int) and isinstance(output.get("height"), int)
    ]
    if not usable:
        return [{"name": fallback, "width": None, "height": None}]
    return sorted(
        usable,
        key=lambda output: (
            output["width"] * output["height"],
            output["width"],
            output["height"],
            output["name"],
        ),
    )


def require_shared_device_binary():
    if not VK.is_file():
        sys.exit(f"missing {VK}; run scripts/build-vk-zerocopy.sh")
    if ZERO_COPY_MARKER not in VK.read_bytes():
        sys.exit(
            "performance matrix requires skwd-wall-vk with the shared-device "
            "feature; run scripts/build-vk-zerocopy.sh after the workspace build"
        )
    if not STILL.is_file():
        sys.exit(f"missing {STILL}; run cargo build --release -p skwd-wall-still")


def gpu_sample():
    if NVIDIA_SMI:
        sample = nvidia_gpu_sample()
        if sample is not None:
            return sample
    return sysfs_gpu_sample()


def gpu_procs():
    if NVIDIA_SMI:
        try:
            result = subprocess.run(
                [NVIDIA_SMI, "--query-compute-apps=pid,process_name", "--format=csv,noheader"],
                capture_output=True,
                text=True,
                timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired):
            result = None
        if result is not None and result.returncode == 0:
            return [
                line.split(", ", 1)[1].split()[0].rsplit("/", 1)[-1]
                for line in result.stdout.splitlines()
                if ", " in line
            ]
    tokens = ("steam_app", "gamescope", "proton", "wine", "lutris", "heroic", "mangohud")
    processes = []
    for comm_path in Path("/proc").glob("[0-9]*/comm"):
        try:
            name = comm_path.read_text().strip()
            cmdline = (
                (comm_path.parent / "cmdline")
                .read_bytes()
                .replace(b"\0", b" ")
                .decode(errors="replace")
            )
        except OSError:
            continue
        haystack = f"{name} {cmdline}".casefold()
        if any(token in haystack for token in tokens):
            processes.append(name)
    return processes


def proc_cpu(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            parts = f.read().split()
        return (int(parts[13]) + int(parts[14])) / os.sysconf("SC_CLK_TCK")
    except OSError:
        return None


def proc_vram(pid, proc_root=Path("/proc")):
    if NVIDIA_SMI:
        try:
            result = subprocess.run(
                [
                    NVIDIA_SMI,
                    "--query-compute-apps=pid,used_memory",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                text=True,
                timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired):
            result = None
        if result is not None and result.returncode == 0:
            for line in result.stdout.splitlines():
                if not line.strip():
                    continue
                process, memory = line.split(", ")
                if int(process) == pid:
                    return int(memory)
    readings = []
    for fdinfo in (proc_root / str(pid) / "fdinfo").glob("*"):
        try:
            text = fdinfo.read_text(errors="replace")
        except OSError:
            continue
        total_kib = drm_vram_kib(text)
        if total_kib:
            readings.append(total_kib)
    return max(readings, default=0) // 1024


def drm_vram_kib(text):
    total_kib = 0
    for line in text.splitlines():
        if not line.startswith("drm-memory-"):
            continue
        parts = line.split()
        if len(parts) < 2:
            continue
        try:
            amount = int(parts[1])
        except ValueError:
            continue
        unit = parts[2].casefold() if len(parts) > 2 else "bytes"
        if unit == "kib":
            total_kib += amount
        elif unit == "mib":
            total_kib += amount * 1024
        else:
            total_kib += amount // 1024
    return total_kib


def proc_pss(pid):
    try:
        total = 0
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                if line.startswith("Pss:"):
                    total += int(line.split()[1])
        return total // 1024
    except OSError:
        return 0


def proc_rss(pid):
    try:
        with open(f"/proc/{pid}/status") as status:
            for line in status:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1]) // 1024
    except OSError:
        pass
    return 0


def process_pids(name, proc_root=Path("/proc")):
    pids = []
    try:
        entries = proc_root.iterdir()
    except OSError:
        return pids
    for entry in entries:
        if not entry.name.isdigit():
            continue
        try:
            comm = (entry / "comm").read_text().strip()
        except OSError:
            continue
        if comm == name:
            pids.append(int(entry.name))
    return sorted(pids)


def process_comm(pid, proc_root=Path("/proc")):
    try:
        return (proc_root / str(pid) / "comm").read_text().strip()
    except OSError:
        return ""


def user_service_main_pid(unit):
    try:
        result = subprocess.run(
            [
                "systemctl",
                "--user",
                "show",
                unit,
                "--property=MainPID",
                "--value",
            ],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    try:
        pid = int(result.stdout.strip())
    except ValueError:
        return None
    return pid if pid > 0 and process_comm(pid) else None


def require_compositor(name):
    service_pid = user_service_main_pid(f"{name}.service")
    if service_pid is not None:
        return service_pid
    pids = process_pids(name)
    if len(pids) != 1:
        sys.exit(f"expected exactly one {name} process, found {pids}")
    return pids[0]


def parse_pmon_vram(output, pids):
    wanted = set(pids)
    total = 0
    for line in output.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        fields = line.split()
        if len(fields) < 4:
            continue
        try:
            pid = int(fields[1])
            memory = int(fields[3])
        except ValueError:
            continue
        if pid in wanted:
            total += memory
    return total


def pmon_vram(pids):
    if not NVIDIA_SMI:
        return sum(proc_vram(pid) for pid in pids)
    try:
        output = subprocess.check_output(
            [NVIDIA_SMI, "pmon", "-c", "1", "-s", "m"],
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return 0
    return parse_pmon_vram(output, pids)


def process_snapshot(pid):
    return {
        "rss": proc_rss(pid),
        "pss": proc_pss(pid),
        "vram": pmon_vram({pid}),
    }


def compositor_metrics(
    name, pid, before, after, cpu_start, cpu_end, elapsed, cpu_idle=0.0
):
    cpu = round(max(0.0, cpu_end - cpu_start) / elapsed * 100.0, 1)
    return {
        "name": name,
        "pid": pid,
        "process_name": process_comm(pid),
        "cpu": cpu,
        "cpu_idle": cpu_idle,
        "cpu_delta": round(cpu - cpu_idle, 1),
        "rss_before": before["rss"],
        "rss": after["rss"],
        "rss_delta": after["rss"] - before["rss"],
        "pss_before": before["pss"],
        "pss": after["pss"],
        "pss_delta": after["pss"] - before["pss"],
        "vram_before": before["vram"],
        "vram": after["vram"],
        "vram_delta": after["vram"] - before["vram"],
    }


def idle_floor(prof, compositor_pid=None):
    cpu_start = proc_cpu(compositor_pid) if compositor_pid is not None else None
    started = time.monotonic()
    utils, powers = [], []
    n = max(2, int(2 * prof["hz"]))
    for _ in range(n):
        u, p = gpu_sample()
        utils.append(u)
        powers.append(p)
        time.sleep(1.0 / prof["hz"])
    floor = sum(utils) / len(utils), sum(powers) / len(powers)
    if compositor_pid is None:
        return floor
    cpu_end = proc_cpu(compositor_pid)
    elapsed = time.monotonic() - started
    cpu = (
        max(0.0, cpu_end - cpu_start) / elapsed * 100.0
        if cpu_start is not None and cpu_end is not None
        else 0.0
    )
    return *floor, round(cpu, 1)


class Placement:
    def __init__(self, output, src, scene=False, fps=None):
        self.output = output
        self.src = src
        self.scene = scene
        self.fps = fps

    def metadata(self):
        return {
            "output": self.output,
            "source": scene_info(self.src) if self.scene else media_info(self.src),
            "kind": media_kind(self.src, self.scene),
        }


def media_kind(path, scene=False):
    if scene:
        return "we-scene"
    return "static" if Path(path).suffix.casefold() in {".png", ".jpg", ".jpeg", ".webp"} else "video"


class Scenario:
    def __init__(
        self,
        name,
        tier,
        src,
        output,
        env=None,
        swap_to=None,
        shader=None,
        scene=False,
        placements=None,
        start_from=None,
        start_scene=None,
        independent=False,
        reuse=None,
        engine="vulkan",
        fps=None,
    ):
        self.name = name
        self.tier = tier
        self.src = src
        self.output = output
        self.env = env or {}
        self.swap_to = swap_to
        self.shader = shader
        self.scene = scene
        self.placements = placements or []
        self.start_from = start_from
        self.start_scene = start_scene
        self.independent = independent
        self.reuse = reuse
        self.engine = engine
        self.fps = fps

    def topology(self):
        if self.placements:
            return "mixed"
        return "all" if self.output == "*" else "single"

    def metadata(self):
        swap_scene = self.scene and self.swap_to
        return {
            "name": self.name,
            "tier": self.tier,
            "output": self.output,
            "source": scene_info(self.src) if self.scene else media_info(self.src),
            "swap_source": (
                scene_info(self.swap_to)
                if swap_scene
                else media_info(self.swap_to) if self.swap_to else None
            ),
            "start_source": media_info(self.start_from) if self.start_from else None,
            "start_scene": scene_info(self.start_scene) if self.start_scene else None,
            "workload": media_kind(self.src, self.scene),
            "topology": self.topology(),
            "placements": [placement.metadata() for placement in self.placements],
            "renderer": (
                "tinier"
                if self.engine == "tinier"
                else "mixed"
                if self.placements
                else "still"
                if media_kind(self.src, self.scene) == "static"
                and not self.swap_to
                and not self.start_from
                else "vulkan"
            ),
            "reuse": self.reuse,
            "path": self.env.get("SKWD_VK_PATH", "dmabuf-present"),
            "decode": self.env.get("SKWD_VK_DECODE", "auto"),
            "present": self.env.get("SKWD_VK_PRESENT", "dmabuf"),
            "nv12_modifier": (
                "linear" if self.env.get("SKWD_VK_NV12_LINEAR") == "1" else "negotiated"
            ),
            "nv12_backend": self.env.get("SKWD_VK_NV12_BACKEND"),
            "shader": self.shader,
        }


def send_swap(proc, to, shader, swap_ms):
    cmd = json.dumps({"to": to, "mute": True, "shader": shader,
                      "duration_ms": swap_ms}) + "\n"
    proc.stdin.write(cmd.encode())
    proc.stdin.flush()


def tinier_command(output, src, fps):
    command = [str(TINIER)]
    if output not in ("*", "mixed"):
        command.extend(["--output", output])
    command.extend([str(src), str(fps)])
    return command


def scenario_commands(sc, prof):
    if sc.engine == "tinier":
        if sc.placements:
            return [
                tinier_command(placement.output, placement.src, placement.fps)
                for placement in sc.placements
            ]
        return [tinier_command(sc.output, sc.src, sc.fps)]
    suffix = ["--mute"]
    if sc.start_from:
        suffix.extend(
            [
                "--transition-from",
                sc.start_from,
                "--duration-ms",
                str(prof["swap_ms"]),
            ]
        )
        suffix.append("--persist")
        if sc.shader:
            suffix.extend(["--shader", sc.shader])
    if not sc.placements:
        if (
            media_kind(sc.src, sc.scene) == "static"
            and not sc.swap_to
            and not sc.start_from
        ):
            return [[str(STILL), sc.output, sc.src, "--persist"]]
        command = [str(VK), sc.output, sc.src, *suffix]
        if sc.scene:
            command.extend(["--scene", sc.src])
        return [command]
    videos = [
        placement
        for placement in sc.placements
        if media_kind(placement.src, placement.scene) == "video"
    ]
    statics = [
        placement
        for placement in sc.placements
        if media_kind(placement.src, placement.scene) == "static"
    ]
    scenes = [placement for placement in sc.placements if placement.scene]
    commands = []
    if sc.independent:
        commands.extend(
            [str(VK), placement.output, placement.src, *suffix] for placement in videos
        )
    elif len(videos) == 1:
        commands.append([str(VK), videos[0].output, videos[0].src, *suffix])
    elif videos:
        manifest = json.dumps(
            [
                {
                    "output": placement.output,
                    "video": placement.src,
                    "mute": True,
                    "volume": 100,
                }
                for placement in videos
            ],
            separators=(",", ":"),
        )
        commands.append([str(VK), "--multi-json", manifest, *suffix])
    for placement in statics:
        commands.append([str(STILL), placement.output, placement.src, "--persist"])
    for placement in scenes:
        commands.append(
            [
                str(VK),
                placement.output,
                placement.src,
                *suffix,
                "--scene",
                placement.src,
            ]
        )
    return commands


def proc_total(procs, metric):
    return sum(metric(proc.pid) or 0 for proc in procs)


def run_scenario(sc, floor, prof, compositor_pid):
    env = dict(os.environ)
    env.update(sc.env)
    compositor_before = process_snapshot(compositor_pid)
    procs = [
        subprocess.Popen(
            command,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        for command in scenario_commands(sc, prof)
    ]
    metadata = sc.metadata()
    try:
        mixed_scene = any(placement.scene for placement in sc.placements)
        settle = max(prof["settle"], 5) if mixed_scene else prof["settle"]
        time.sleep(0.2 if sc.start_from else settle)
        if any(proc.poll() is not None for proc in procs):
            return {**metadata, "error": "renderer died at spawn"}
        c1 = proc_total(procs, proc_cpu)
        compositor_cpu_start = proc_cpu(compositor_pid)
        if compositor_cpu_start is None:
            return {**metadata, "error": f"{COMPOSITOR_NAME} died before measurement"}
        t1 = time.monotonic()
        utils, powers = [], []
        flip = False
        steps = max(1, int(prof["window"] * prof["hz"]))
        swap_every = max(1, int(3 * prof["hz"]))
        for i in range(steps):
            if sc.swap_to and (i == 0 if not prof["burst"] else i % swap_every == 0):
                flip = not flip
                to = sc.swap_to if flip else sc.src
                try:
                    for proc in procs:
                        send_swap(proc, to, sc.shader, prof["swap_ms"])
                except BrokenPipeError:
                    return {**metadata, "error": "renderer died mid-swap"}
            u, p = gpu_sample()
            utils.append(u)
            powers.append(p)
            time.sleep(1.0 / prof["hz"])
        c2 = proc_total(procs, proc_cpu)
        compositor_cpu_end = proc_cpu(compositor_pid)
        t2 = time.monotonic()
        if any(proc.poll() is not None for proc in procs):
            return {**metadata, "error": "renderer died mid-window"}
        if compositor_cpu_end is None:
            return {**metadata, "error": f"{COMPOSITOR_NAME} died mid-window"}
        cpu = (
            (c2 - c1) / (t2 - t1) * 100.0
            if c1 is not None and c2 is not None
            else 0.0
        )
        compositor_after = process_snapshot(compositor_pid)
        compositor = compositor_metrics(
            COMPOSITOR_NAME,
            compositor_pid,
            compositor_before,
            compositor_after,
            compositor_cpu_start,
            compositor_cpu_end,
            t2 - t1,
            floor[2],
        )
        renderer_pss = proc_total(procs, proc_pss)
        return {
            **metadata,
            "cpu": round(cpu, 1),
            "gpu_delta": round(sum(utils) / len(utils) - floor[0], 1),
            "gpu_max": max(utils),
            "power_delta": round(sum(powers) / len(powers) - floor[1], 1),
            "power_max": round(max(powers), 1),
            "processes": len(procs),
            "vram": proc_total(procs, proc_vram),
            "rss": proc_total(procs, proc_rss),
            "pss": renderer_pss,
            "combined_pss": renderer_pss + compositor["pss_delta"],
            "compositor": compositor,
        }
    finally:
        for proc in procs:
            if proc.poll() is None:
                proc.terminate()
        for proc in procs:
            if proc.poll() is not None:
                continue
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        time.sleep(0.5 if not prof["burst"] else 2)


def build_matrix(
    full,
    floor_videos,
    mid_video,
    max_video,
    max_video2,
    still,
    still2,
    single_output,
    max_output,
    scene=None,
    outputs=None,
    scenes=None,
    tinier=None,
):
    scenes = list(scenes or ([scene] if scene else []))
    scene = scenes[0] if scenes else None
    ranked = ranked_outputs(outputs or [], single_output)
    low_output = ranked[0]["name"]
    mid_output = ranked[len(ranked) // 2]["name"]
    high_output = ranked[-1]["name"]
    m = []
    for codec, floor_video in floor_videos.items():
        label = "h264 lean" if codec == "h264" else codec
        m.append(
            Scenario(
                f"low vk {label} floor",
                "low",
                floor_video,
                single_output,
            )
        )
    m.extend(
        [
        Scenario(
            "low vk av1 floor nv12",
            "low",
            floor_videos["av1"],
            single_output,
            {"SKWD_VK_PATH": "nv12", "SKWD_VK_NV12_BACKEND": "presenter"},
        ),
        Scenario(
            "low vk av1 floor nv12 linear",
            "low",
            floor_videos["av1"],
            single_output,
            {
                "SKWD_VK_PATH": "nv12",
                "SKWD_VK_NV12_BACKEND": "presenter",
                "SKWD_VK_NV12_LINEAR": "1",
            },
        ),
        Scenario(
            "low vk h264 lean floor nv12 presenter A/B transfer-only",
            "low",
            floor_videos["h264"],
            single_output,
            {"SKWD_VK_PATH": "nv12", "SKWD_VK_NV12_BACKEND": "presenter"},
        ),
        Scenario(
            "low vk h264 lean floor nv12 presenter A/B full-renderer",
            "low",
            floor_videos["h264"],
            single_output,
            {"SKWD_VK_PATH": "nv12", "SKWD_VK_NV12_BACKEND": "renderer"},
        ),
        Scenario("mid video hw", "mid", mid_video, single_output),
        Scenario(
            "mid video hw nv12",
            "mid",
            mid_video,
            single_output,
            {"SKWD_VK_PATH": "nv12", "SKWD_VK_NV12_BACKEND": "presenter"},
        ),
        Scenario(
            "mid video sw-decode",
            "mid",
            mid_video,
            single_output,
            {"SKWD_VK_DECODE": "sw"},
        ),
        Scenario(
            "mid video shm-present",
            "mid",
            mid_video,
            single_output,
            {"SKWD_VK_PRESENT": "shm"},
        ),
        Scenario(
            "max same video adaptive all outputs",
            "max",
            max_video,
            max_output,
            reuse="adaptive-export",
        ),
        Scenario(
            "max same video adaptive xr24 steady",
            "max",
            max_video,
            max_output,
            {"SKWD_VK_HYBRID_NV12": "0"},
            reuse="adaptive-xr24",
        ),
        Scenario(
            "max same video shared native exports",
            "max",
            max_video,
            max_output,
            {"SKWD_VK_REUSE_EXPORT": "native"},
            reuse="native-exports",
        ),
        Scenario(
            "max same video shared max export",
            "max",
            max_video,
            max_output,
            {"SKWD_VK_REUSE_EXPORT": "max"},
            reuse="max-export",
        ),
        Scenario(
            "max video sw-decode",
            "max",
            max_video,
            max_output,
            {"SKWD_VK_DECODE": "sw"},
        ),
        Scenario("max video single", "max", max_video, single_output),
        ]
    )
    if tinier and "max" in tinier:
        max_ivf, max_fps = tinier["max"]
        m.extend(
            [
                Scenario(
                    "max tinier video single",
                    "max",
                    max_ivf,
                    single_output,
                    engine="tinier",
                    fps=max_fps,
                ),
                Scenario(
                    "max tinier same video all outputs",
                    "max",
                    max_ivf,
                    max_output,
                    engine="tinier",
                    fps=max_fps,
                ),
            ]
        )
    pairs = [
        ("vid<->vid", max_video, max_video2),
        ("still<->still", still, still2),
        ("vid<->still", max_video, still),
    ]
    classes = [("crossfade", None), ("sand", "sand-tornado"), ("effect", "inkwell-drop")]
    for pname, a, b in pairs:
        for cname, shader in classes:
            m.append(
                Scenario(
                    f"max {pname} {cname}",
                    "max",
                    a,
                    max_output,
                    swap_to=b,
                    shader=shader,
                )
            )
    if scene:
        m.extend(
            [
                Scenario(
                    "low WE scene performance",
                    "low",
                    scene,
                    single_output,
                    {
                        "SKWD_PAPER_WE_FPS": "30",
                        "SKWD_VK_SCENE_MAX": "2048",
                        "SKWD_VK_SCENE_FX": "4",
                        "SKWD_VK_FX_PASSES": "8",
                    },
                    scene=True,
                ),
                Scenario(
                    "mid WE scene native",
                    "mid",
                    scene,
                    single_output,
                    {"SKWD_PAPER_WE_FPS": "60"},
                    scene=True,
                ),
                Scenario(
                    "max WE scene native",
                    "max",
                    scene,
                    max_output,
                    {"SKWD_PAPER_WE_FPS": "60"},
                    scene=True,
                ),
            ]
        )
    resolution_sources = [
        ("low", low_output, floor_videos["h264"]),
        ("mid", mid_output, mid_video),
        ("max", high_output, max_video),
    ]
    for tier, output, video in resolution_sources:
        m.extend(
            [
                Scenario(
                    f"{tier} resolution video single {output}",
                    tier,
                    video,
                    output,
                ),
                Scenario(
                    f"{tier} resolution static single {output}",
                    tier,
                    still,
                    output,
                ),
            ]
        )
        if scene:
            m.append(
                Scenario(
                    f"{tier} resolution WE single {output}",
                    tier,
                    scene,
                    output,
                    {
                        "SKWD_PAPER_WE_FPS": "30" if tier == "low" else "60",
                        **(
                            {
                                "SKWD_VK_SCENE_MAX": "2048",
                                "SKWD_VK_SCENE_FX": "4",
                                "SKWD_VK_FX_PASSES": "8",
                            }
                            if tier == "low"
                            else {}
                        ),
                    },
                    scene=True,
                )
            )
    m.append(Scenario("max static all outputs", "max", still, max_output))
    placement_outputs = [output["name"] for output in ranked]
    if len(placement_outputs) >= 2:
        media_sources = [
            floor_videos["h264"],
            mid_video,
            max_video,
        ]
        tinier_mixed = (
            [tinier["floor"], tinier["mid"], tinier["max"]]
            if tinier and {"floor", "mid", "max"} <= set(tinier)
            else []
        )
        m.extend(
            [
                Scenario(
                    "max same video independent outputs",
                    "max",
                    max_video,
                    "mixed",
                    placements=[
                        Placement(output, max_video) for output in placement_outputs
                    ],
                    independent=True,
                    reuse="none",
                ),
                Scenario(
                    "max mixed distinct videos",
                    "max",
                    max_video,
                    "mixed",
                    placements=[
                        Placement(output, media_sources[i % len(media_sources)])
                        for i, output in enumerate(placement_outputs)
                    ],
                    reuse="device",
                ),
                *(
                    [
                        Scenario(
                            "max tinier mixed distinct videos",
                            "max",
                            tinier["max"][0],
                            "mixed",
                            placements=[
                                Placement(
                                    output,
                                    tinier_mixed[i % len(tinier_mixed)][0],
                                    fps=tinier_mixed[i % len(tinier_mixed)][1],
                                )
                                for i, output in enumerate(placement_outputs)
                            ],
                            engine="tinier",
                        )
                    ]
                    if tinier_mixed
                    else []
                ),
                Scenario(
                    "max mixed distinct statics",
                    "max",
                    still,
                    "mixed",
                    placements=[
                        Placement(output, still if i % 2 == 0 else still2)
                        for i, output in enumerate(placement_outputs)
                    ],
                ),
                Scenario(
                    "max mixed video static",
                    "max",
                    max_video,
                    "mixed",
                    placements=[
                        Placement(placement_outputs[0], max_video),
                        Placement(placement_outputs[1], still),
                    ],
                ),
            ]
        )
        if scene:
            m.extend(
                [
                    Scenario(
                        "max mixed video WE",
                        "max",
                        max_video,
                        "mixed",
                        placements=[
                            Placement(placement_outputs[0], max_video),
                            Placement(placement_outputs[1], scene, scene=True),
                        ],
                    ),
                    Scenario(
                        "max mixed static WE",
                        "max",
                        still,
                        "mixed",
                        placements=[
                            Placement(placement_outputs[0], still),
                            Placement(placement_outputs[1], scene, scene=True),
                        ],
                    ),
                ]
            )
            if len(scenes) >= 2:
                m.append(
                    Scenario(
                        "max mixed per-output WE scenes",
                        "max",
                        scene,
                        "mixed",
                        placements=[
                            Placement(
                                output,
                                scenes[i % len(scenes)],
                                scene=True,
                            )
                            for i, output in enumerate(placement_outputs)
                        ],
                        scene=True,
                    )
                )
        if len(placement_outputs) >= 3 and scene:
            m.append(
                Scenario(
                    "max mixed video static WE",
                    "max",
                    max_video,
                    "mixed",
                    placements=[
                        Placement(placement_outputs[0], max_video),
                        Placement(placement_outputs[1], still),
                        Placement(placement_outputs[2], scene, scene=True),
                    ],
                )
            )
    for pname, a, b in pairs:
        for cname, shader in classes:
            m.append(
                Scenario(
                    f"max single {pname} {cname}",
                    "max",
                    a,
                    single_output,
                    swap_to=b,
                    shader=shader,
                )
            )
    if len(scenes) >= 2:
        for output_label, output in (("single", single_output), ("all", max_output)):
            for cname, shader in classes:
                m.append(
                    Scenario(
                        f"max {output_label} WE<->WE {cname}",
                        "max",
                        scenes[0],
                        output,
                        {"SKWD_PAPER_WE_FPS": "60"},
                        swap_to=scenes[1],
                        shader=shader,
                        scene=True,
                    )
                )
    if scene:
        for source_label, source in (("video", max_video), ("static", still)):
            for output_label, output in (("single", single_output), ("all", max_output)):
                for cname, shader in classes:
                    m.append(
                        Scenario(
                            f"max {output_label} {source_label}->WE {cname}",
                            "max",
                            scene,
                            output,
                            {"SKWD_PAPER_WE_FPS": "60"},
                            shader=shader,
                            scene=True,
                            start_from=source,
                        )
                    )
        preview = scene_preview(scene)
        if preview:
            for target_label, target in (("video", max_video), ("static", still)):
                for output_label, output in (("single", single_output), ("all", max_output)):
                    for cname, shader in classes:
                        m.append(
                            Scenario(
                                f"max {output_label} WE->{target_label} {cname}",
                                "max",
                                target,
                                output,
                                shader=shader,
                                start_from=preview,
                                start_scene=scene,
                            )
                        )
    if full:
        sand_src = (ROOT / "crates/paper-shaders/src/sand.rs").read_text()
        styles = [s.strip().strip('",') for s in
                  sand_src.split("SAND_STYLES")[1].split("];")[0].splitlines()
                  if s.strip().startswith('"')]
        effects_src = (ROOT / "crates/paper-shaders/src/effects/catalog.rs").read_text()
        effects = [line.split('"')[1] for line in
                  effects_src.split("EFFECTS")[1].split("];")[0].splitlines()
                  if line.strip().startswith('("')]
        for s in styles:
            m.append(
                Scenario(
                    f"max full sand {s}",
                    "max",
                    max_video,
                    max_output,
                    swap_to=max_video2,
                    shader=s,
                )
            )
        for e in effects:
            m.append(
                Scenario(
                    f"max full effect {e}",
                    "max",
                    max_video,
                    max_output,
                    swap_to=max_video2,
                    shader=e,
                )
            )
    return m


def validate_matrix(
    matrix,
    floor_videos,
    floor_size,
    single_output,
    max_output,
    outputs=None,
    scenes=None,
):
    names = [scenario.name for scenario in matrix]
    if len(names) != len(set(names)):
        sys.exit("performance matrix contains duplicate scenario names")
    tiers = {scenario.tier for scenario in matrix}
    if tiers != {"low", "mid", "max"}:
        sys.exit(f"performance matrix must contain low/mid/max tiers, got {sorted(tiers)}")
    if set(floor_videos) != set(FLOOR_CODECS):
        sys.exit("floor fixture set must contain AV1, VP9, H.264, and HEVC")
    for codec, floor_video in floor_videos.items():
        floor_info = media_info(floor_video)
        if (floor_info["width"], floor_info["height"]) != floor_size:
            sys.exit(f"low tier {codec} source does not match the floor resolution")
        if floor_info["codec"] != codec:
            sys.exit(f"low tier {codec} fixture reports codec {floor_info['codec']}")
        if frame_rate(floor_info) > FLOOR_FPS + 0.01:
            sys.exit(f"low tier {codec} source exceeds the {FLOOR_FPS} fps floor cap")
    required = {
        ("low", "dmabuf-present", single_output),
        ("mid", "dmabuf-present", single_output),
        ("max", "dmabuf-present", max_output),
    }
    actual = {
        (
            scenario.tier,
            scenario.env.get("SKWD_VK_PATH", "dmabuf-present"),
            scenario.output,
        )
        for scenario in matrix
    }
    missing = required - actual
    if missing:
        sys.exit(f"performance matrix is missing mandatory coverage: {sorted(missing)}")
    for codec, floor_video in floor_videos.items():
        if not any(
            scenario.tier == "low"
            and scenario.src == floor_video
            and scenario.env.get("SKWD_VK_PATH", "dmabuf-present") == "dmabuf-present"
            for scenario in matrix
        ):
            sys.exit(f"performance matrix is missing low {codec} on dmabuf-present")
    if not any(scenario.tier == "max" and scenario.swap_to for scenario in matrix):
        sys.exit("performance matrix is missing max-tier transitions")
    names = set(names)
    ranked = ranked_outputs(outputs or [], single_output)
    resolution_names = set()
    for tier, output in (
        ("low", ranked[0]["name"]),
        ("mid", ranked[len(ranked) // 2]["name"]),
        ("max", ranked[-1]["name"]),
    ):
        resolution_names.update(
            {
                f"{tier} resolution video single {output}",
                f"{tier} resolution static single {output}",
            }
        )
        if scenes:
            resolution_names.add(f"{tier} resolution WE single {output}")
    required_names = resolution_names | {"max static all outputs"}
    required_names.add("max same video adaptive all outputs")
    required_names.add("max same video adaptive xr24 steady")
    required_names.add("max same video shared native exports")
    required_names.add("max same video shared max export")
    classes = {"crossfade", "sand", "effect"}
    for pair in ("vid<->vid", "still<->still", "vid<->still"):
        required_names.update(f"max single {pair} {kind}" for kind in classes)
    placement_outputs = [output["name"] for output in ranked]
    if len(placement_outputs) >= 2:
        required_names.update(
            {
                "max same video independent outputs",
                "max mixed distinct videos",
                "max mixed distinct statics",
                "max mixed video static",
            }
        )
        if scenes:
            required_names.update({"max mixed video WE", "max mixed static WE"})
        if scenes and len(scenes) >= 2:
            required_names.add("max mixed per-output WE scenes")
    if len(placement_outputs) >= 3 and scenes:
        required_names.add("max mixed video static WE")
    if scenes and len(scenes) >= 2:
        for topology in ("single", "all"):
            required_names.update(f"max {topology} WE<->WE {kind}" for kind in classes)
    if scenes:
        for source in ("video", "static"):
            for topology in ("single", "all"):
                required_names.update(
                    f"max {topology} {source}->WE {kind}" for kind in classes
                )
        if scene_preview(scenes[0]):
            for target in ("video", "static"):
                for topology in ("single", "all"):
                    required_names.update(
                        f"max {topology} WE->{target} {kind}" for kind in classes
                    )
    missing_names = required_names - names
    if missing_names:
        sys.exit(f"performance matrix is missing complete coverage: {sorted(missing_names)}")


def matrix_id(matrix):
    plan = [scenario.metadata() for scenario in matrix]
    return hashlib.sha256(json.dumps(plan, sort_keys=True).encode()).hexdigest()[:16]


def scene_info(path):
    root = Path(path)
    packages = [
        candidate
        for name in ("scene.pkg", "gifscene.pkg")
        if (candidate := root / name).is_file()
    ]
    project = {}
    try:
        project = json.loads((root / "project.json").read_text())
    except (OSError, json.JSONDecodeError):
        pass
    return {
        "path": str(root),
        "type": "we-scene",
        "title": project.get("title", root.name),
        "package_bytes": sum(package.stat().st_size for package in packages),
        "mtime_ns": max((package.stat().st_mtime_ns for package in packages), default=0),
    }


def scene_preview(path):
    root = Path(path)
    try:
        project = json.loads((root / "project.json").read_text())
    except (OSError, json.JSONDecodeError):
        project = {}
    preview = project.get("preview")
    if isinstance(preview, str) and (root / preview).is_file():
        return str(root / preview)
    candidates = sorted(root.glob("preview.*"))
    return str(candidates[0]) if candidates else None


def default_scenes():
    reference = (
        Path.home()
        / ".local/share/Steam/steamapps/workshop/content/431960/2444472355"
    )
    workshop = Path.home() / ".local/share/Steam/steamapps/workshop/content/431960"
    candidates = list(workshop.glob("*/scene.pkg"))
    if not candidates:
        return [str(reference)] if (reference / "scene.pkg").is_file() else []
    ordered = sorted(candidates, key=lambda path: path.stat().st_size, reverse=True)
    scenes = []
    if (reference / "scene.pkg").is_file():
        scenes.append(str(reference))
    scenes.extend(str(path.parent) for path in ordered if path.parent != reference)
    return scenes[:3]


def default_scene():
    scenes = default_scenes()
    return scenes[0] if scenes else None


def main():
    global VK
    ap = argparse.ArgumentParser()
    ap.add_argument("--full", action="store_true")
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--out", default=str(ROOT / "perf-sweep-latest.json"))
    ap.add_argument("--check")
    ap.add_argument("--window", type=float)
    ap.add_argument("--single-output")
    ap.add_argument("--max-output", default="*")
    ap.add_argument("--floor-video")
    ap.add_argument("--floor-size", type=parse_size, default=FLOOR_SIZE)
    ap.add_argument("--scene", action="append")
    ap.add_argument("--renderer", default=str(VK))
    ap.add_argument("--only", action="append")
    ap.add_argument("--plan", action="store_true")
    args = ap.parse_args()
    VK = Path(args.renderer)
    profile_name = "quick" if args.quick else "thorough"
    prof = dict(PROFILES[profile_name])
    if args.window:
        prof["window"] = args.window

    require_shared_device_binary()
    videos = sorted(VIDEO_DIR.glob("*.mp4"))
    stills = sorted(list(STILL_DIR.glob("*.webp")) + list(STILL_DIR.glob("*.png")))
    if len(videos) < 2 or len(stills) < 2:
        sys.exit("need >=2 videos and >=2 stills")
    mid_video, max_video, max_video2 = choose_sources(videos)
    floor_videos = ensure_floor_videos(max_video, args.floor_size, args.floor_video)
    still, still2 = choose_stills(stills)
    outputs = active_outputs()
    single_output = resolve_single_output(args.single_output, outputs)
    scenes = args.scene or default_scenes()
    matrix = build_matrix(
        args.full,
        floor_videos,
        mid_video,
        max_video,
        max_video2,
        still,
        still2,
        single_output,
        args.max_output,
        scenes[0] if scenes else None,
        outputs,
        scenes,
        tinier_fixtures(),
    )
    validate_matrix(
        matrix,
        floor_videos,
        args.floor_size,
        single_output,
        args.max_output,
        outputs,
        scenes,
    )
    if args.only:
        terms = [term.casefold() for term in args.only]
        matrix = [
            scenario
            for scenario in matrix
            if all(term in scenario.name.casefold() for term in terms)
        ]
        if not matrix:
            sys.exit(f"no performance scenarios match {args.only}")
    plan_id = matrix_id(matrix)
    sources = {
        "floor_codecs": {codec: media_info(path) for codec, path in floor_videos.items()},
        "mid": media_info(mid_video),
        "max": media_info(max_video),
        "max_second": media_info(max_video2),
    }
    if args.plan:
        print(
            json.dumps(
                {
                    "schema": 4,
                    "matrix_version": MATRIX_VERSION,
                    "matrix_id": plan_id,
                    "scenario_count": len(matrix),
                    "profile": profile_name,
                    "floor_resolution": {
                        "width": args.floor_size[0],
                        "height": args.floor_size[1],
                    },
                    "single_output": single_output,
                    "max_output": args.max_output,
                    "active_outputs": outputs,
                    "gpu_sampler": gpu_sampler_metadata(),
                    "compositor": {"name": COMPOSITOR_NAME},
                    "sources": sources,
                    "scenarios": [scenario.metadata() for scenario in matrix],
                },
                indent=2,
            )
        )
        return

    compositor_pid = require_compositor(COMPOSITOR_NAME)
    subprocess.run(["pkill", "-x", "skwd-wall-vk"], check=False)
    subprocess.run(["pkill", "-x", "skwd-wall-still"], check=False)
    time.sleep(1)
    if subprocess.run(["pgrep", "-x", "skwd-walld"], capture_output=True).returncode == 0:
        print("WARNING: skwd-walld running; rotation/applies may perturb results")
    others = sorted({p for p in gpu_procs() if not p.startswith("skwd")})
    if others:
        print(f"WARNING: other GPU consumers active: {others} "
              "(deltas vs re-sampled floor compensate, absolutes will drift)")

    rev = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"],
                         capture_output=True, text=True).stdout.strip()
    cap = os.environ.get("SKWD_PAPER_SAND_FPS", "unset")
    sampler = gpu_sampler_metadata()

    results = []
    floor = idle_floor(prof, compositor_pid)
    floors = [floor]
    print(f"idle floor: gpu {floor[0]:.1f}% {floor[1]:.1f}W niri {floor[2]:.1f}% | rev {rev} | "
          f"profile {profile_name} | matrix {plan_id} | sand fps cap {cap} | "
          f"{len(matrix)} scenarios")
    print(
        f"gpu sampler: {sampler['backend']} vendors={sampler['vendors']} "
        f"util={sampler['gpu_utilization']} power={sampler['board_power']} "
        f"process-vram={sampler['process_vram']}"
    )
    print(
        f"tiers: low={single_output} {args.floor_size[0]}x{args.floor_size[1]} source | "
        f"mid={single_output} {sources['mid']['width']}x{sources['mid']['height']} source | "
        f"max={args.max_output} {sources['max']['width']}x{sources['max']['height']} source"
    )
    for i, sc in enumerate(matrix):
        if i > 0 and i % 5 == 0:
            floor = idle_floor(prof, compositor_pid)
            floors.append(floor)
            print(
                f"  [idle re-sample: gpu {floor[0]:.1f}% {floor[1]:.1f}W "
                f"niri {floor[2]:.1f}%]"
            )
        r = run_scenario(sc, floor, prof, compositor_pid)
        results.append(r)
        if "error" in r:
            print(f"{r['name']:<34} ERROR: {r['error']}")
        else:
            compositor = r["compositor"]
            print(f"{r['name']:<34} cpu={r['cpu']:>5}% gpuD={r['gpu_delta']:>5}% "
                  f"max={r['gpu_max']:>3.0f}% WD={r['power_delta']:>6} "
                  f"Wmax={r['power_max']:>6} rss={r['rss']:>4} "
                  f"pss={r['pss']:>4} vram={r['vram']:>4} "
                  f"niriC={compositor['cpu']:>5}% "
                  f"niriD={compositor['cpu_delta']:>+5}% "
                  f"niriR={compositor['rss_delta']:>+4} "
                  f"niriP={compositor['pss_delta']:>+4} "
                  f"niriV={compositor['vram_delta']:>+4} "
                  f"combinedP={r['combined_pss']:>4}")

    drift = max(f[0] for f in floors) - min(f[0] for f in floors)
    tainted = (bool(others) and drift > 3.0) or drift > 10.0
    if drift > 3.0:
        print(f"WARNING: idle floor drifted {drift:.1f}% during sweep")
    if tainted:
        print("TAINTED: foreign GPU load during sweep - unusable as a baseline")

    payload = {
        "schema": 4,
        "matrix_version": MATRIX_VERSION,
        "matrix_id": plan_id,
        "scenario_count": len(matrix),
        "rev": rev,
        "time": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "profile": profile_name,
        "tainted": tainted,
        "sand_fps_cap": cap,
        "floor_resolution": {
            "width": args.floor_size[0],
            "height": args.floor_size[1],
        },
        "single_output": single_output,
        "max_output": args.max_output,
        "active_outputs": outputs,
        "gpu_sampler": sampler,
        "compositor": {
            "name": COMPOSITOR_NAME,
            "pid": compositor_pid,
            "process_name": process_comm(compositor_pid),
        },
        "sources": sources,
        "floors": floors,
        "results": results,
    }
    Path(args.out).write_text(json.dumps(payload, indent=1))
    print(f"written: {args.out}")

    errors = [r for r in results if "error" in r]
    if errors:
        sys.exit(f"{len(errors)} scenario(s) errored")

    if args.check:
        base = json.loads(Path(args.check).read_text())
        if base.get("tainted"):
            sys.exit(f"baseline {args.check} is tainted (recorded under foreign GPU "
                     "load) - re-record it on a quiet machine")
        base_profile = base.get("profile", "thorough")
        if base_profile != profile_name:
            sys.exit(f"profile mismatch: baseline is '{base_profile}', this run is "
                     f"'{profile_name}' - deltas are not comparable")
        if base.get("matrix_version") != MATRIX_VERSION:
            sys.exit(
                f"matrix version mismatch: baseline is {base.get('matrix_version')}, "
                f"this run is {MATRIX_VERSION}"
            )
        if base.get("matrix_id") != plan_id:
            sys.exit(
                f"matrix mismatch: baseline is {base.get('matrix_id')}, "
                f"this run is {plan_id}; record a baseline with the same sources and outputs"
            )
        by_name = {r["name"]: r for r in base["results"] if "error" not in r}
        bad = []
        for r in results:
            b = by_name.get(r["name"])
            if not b:
                bad.append(f"{r['name']}: missing from baseline")
                continue
            for key, margin in CHECK_MARGINS.items():
                if r[key] > max(b[key], 0.0) * 1.3 + margin:
                    bad.append(f"{r['name']}: {key} {b[key]} -> {r[key]}")
            current_compositor = r.get("compositor")
            base_compositor = b.get("compositor")
            if not current_compositor or not base_compositor:
                bad.append(f"{r['name']}: missing compositor metrics")
                continue
            for key, margin in COMPOSITOR_CHECK_MARGINS.items():
                current = current_compositor[key]
                baseline = base_compositor[key]
                if current > max(baseline, 0.0) * 1.3 + margin:
                    bad.append(
                        f"{r['name']}: compositor.{key} {baseline} -> {current}"
                    )
        if bad:
            print("REGRESSIONS vs baseline:")
            for line in bad:
                print(f"  {line}")
            sys.exit(1)
        print(f"holding the floor vs {args.check}")


if __name__ == "__main__":
    main()
