#!/usr/bin/env python3
"""Matched live performance sweep against static, video, and scene competitors."""

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import signal
import stat
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PERF_PATH = ROOT / "scripts" / "perf-sweep.py"
VK = ROOT / "target" / "release" / "skwd-wall-vk"
STILL = ROOT / "target" / "release" / "skwd-wall-still"
MATRIX_VERSION = 15
PHONTO = "phonto"
WPAPERD = "wpaperd"
HYPRPAPER = "hyprpaper"
KACAU = "kacau-wall"
KACAU_LIBRARY_PATH = None
WALLR = "wallr"
KACAU_REVISION = "f84e70c48238efd9e9cd3b23dfe8f565b873f1e9"
WALLR_VERSION = "0.3.4"
WALLR_REVISION = "5c28d775a79ca7d4bf7e4768b53a4e59e1a29a7c"
VALIDATE_MOTION = False
VALIDATION_WORKSPACES = {}
MIN_VIDEO_MEAN_SIGNAL = 0.08
MIN_VIDEO_SIGNAL_DEVIATION = 0.03
VIDEO_VALIDATION_SAMPLES = 4
VIDEO_VALIDATION_INTERVAL = 0.75
YIN = "yin"
YINCTL = "yinctl"
YIN_HOME = None
YIN_LIBRARY_PATH = None
PROFILES = {
    "quick": {"settle": 2.0, "window": 2.0, "hz": 5},
    "thorough": {"settle": 5.0, "window": 10.0, "hz": 2},
}
WALLPAPER_PROCESS_NAMES = (
    "skwd-paper",
    "skwd-walld",
    "skwd-wall-vk",
    "skwd-wall-still",
    "awww-daemon",
    "wpaperd",
    "hyprpaper",
    "wallr",
    "phonto",
    "mpvpaper",
    "linux-wallpaper",
    "yin",
    "kacau-wall",
)
WALLPAPER_LAYER_NAMESPACES = (
    "skwd-paper",
    "skwd-wall-vk",
    "skwd-wall-still",
    "awww",
    "wpaperd-",
    "hyprpaper",
    "wallr",
    "phonto",
    "linux-wallpaper",
    "yin-wallpaper",
    "kacau:wall:",
)
COMPOSITOR_STABLE_SAMPLES = 5
COMPOSITOR_STABLE_INTERVAL = 0.5
COMPOSITOR_STABLE_TIMEOUT = 20.0
COMPOSITOR_STABILITY_TOLERANCE = {"rss": 1, "pss": 1, "vram": 0}
COMPOSITOR_RECOVERY_TOLERANCE = {"rss": 8, "pss": 8, "vram": 16}
GPU_FLOOR_DRIFT_LIMIT = 1.0
POWER_FLOOR_DRIFT_LIMIT = 2.0
COMPOSITOR_CPU_FLOOR_DRIFT_LIMIT = 2.0
SCENARIO_ATTEMPTS = 3


def load_perf_module():
    spec = importlib.util.spec_from_file_location("skwd_perf_sweep", PERF_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


P = load_perf_module()


class Scenario:
    def __init__(
        self,
        name,
        engine,
        workload,
        topology,
        source,
        fps=None,
        tuned=False,
        extra=None,
    ):
        self.name = name
        self.engine = engine
        self.workload = workload
        self.topology = topology
        self.source = str(source)
        self.fps = fps
        self.tuned = tuned
        self.extra = extra or {}

    def metadata(self, single_output):
        source = (
            {"path": self.source, "scene_id": Path(self.source).name}
            if self.workload == "we"
            else P.media_info(self.source)
        )
        metadata = {
            "name": self.name,
            "engine": self.engine,
            "workload": self.workload,
            "topology": self.topology,
            "output": single_output if self.topology == "single" else "*",
            "source": source,
            "fps_cap": self.fps,
            "tuned": self.tuned,
            **self.extra,
        }
        if self.engine == "kacau-wall":
            metadata["revision"] = KACAU_REVISION
            metadata["video_path"] = (
                f"cached Vulkan Video ({self.fps} fps "
                f"{'matched' if self.tuned else 'default'})"
                if self.workload == "video"
                else "cached image"
            )
        if self.engine == "wallr":
            metadata["version"] = WALLR_VERSION
            metadata["revision"] = WALLR_REVISION
            if self.workload == "video":
                metadata["video_path"] = "FFmpeg hardware decode auto"
        return metadata


def build_matrix(floor_video, mid_video, max_video, still, scene, yin_floor_video=None):
    yin_floor_video = yin_floor_video or floor_video
    matrix = []
    for topology in ("single", "all"):
        matrix.extend(
            [
                Scenario(
                    f"static {topology} skwd",
                    "skwd-wall-still",
                    "static",
                    topology,
                    still,
                ),
                Scenario(
                    f"static {topology} awww",
                    "awww",
                    "static",
                    topology,
                    still,
                ),
                Scenario(
                    f"static {topology} hyprpaper",
                    "hyprpaper",
                    "static",
                    topology,
                    still,
                ),
                Scenario(
                    f"static {topology} wallr",
                    "wallr",
                    "static",
                    topology,
                    still,
                ),
                Scenario(
                    f"static {topology} kacau",
                    "kacau-wall",
                    "static",
                    topology,
                    still,
                ),
                Scenario(
                    f"static {topology} yin",
                    "yin",
                    "static",
                    topology,
                    still,
                    tuned=True,
                ),
            ]
        )
        if topology == "all":
            matrix.append(
                Scenario(
                    "static all wpaperd",
                    "wpaperd",
                    "static",
                    "all",
                    still,
                )
            )
        for label, source in (
            ("floor", floor_video),
            ("mid", mid_video),
            ("max", max_video),
        ):
            matrix.extend(
                [
                    Scenario(
                        f"video {label} {topology} skwd",
                        "skwd-wall-vk",
                        "video",
                        topology,
                        source,
                    ),
                    Scenario(
                        f"video {label} {topology} mpvpaper default",
                        "mpvpaper",
                        "video",
                        topology,
                        source,
                    ),
                    Scenario(
                        f"video {label} {topology} mpvpaper hwdec",
                        "mpvpaper",
                        "video",
                        topology,
                        source,
                        tuned=True,
                    ),
                    Scenario(
                        f"video {label} {topology} phonto",
                        "phonto",
                        "video",
                        topology,
                        source,
                    ),
                    Scenario(
                        f"video {label} {topology} wallr auto",
                        "wallr",
                        "video",
                        topology,
                        source,
                        fps={"floor": 30, "mid": 60, "max": 30}[label],
                        tuned=True,
                    ),
                    Scenario(
                        f"video {label} {topology} kacau cached 30fps",
                        "kacau-wall",
                        "video",
                        topology,
                        source,
                        fps=30,
                    ),
                    Scenario(
                        f"video {label} {topology} yin cuda-copy",
                        "yin",
                        "video",
                        topology,
                        yin_floor_video if label == "floor" else source,
                        tuned=True,
                    ),
                ]
            )
            if label == "max":
                matrix.append(
                    Scenario(
                        f"video {label} {topology} kacau cached 60fps matched",
                        "kacau-wall",
                        "video",
                        topology,
                        source,
                        fps=60,
                        tuned=True,
                    )
                )
        for fps in (30, 60):
            matrix.extend(
                [
                    Scenario(
                        f"we {fps}fps {topology} skwd",
                        "skwd-wall-vk",
                        "we",
                        topology,
                        scene,
                        fps=fps,
                    ),
                    Scenario(
                        f"we {fps}fps {topology} lwe",
                        "linux-wallpaperengine",
                        "we",
                        topology,
                        scene,
                        fps=fps,
                    ),
                ]
            )
    return matrix


def build_resolution_matrix(label, video, fps=30):
    extra = {"comparison": "matched-resolution", "resolution_label": label}
    return [
        Scenario(
            f"video {label} Skwd-paper-vk",
            "skwd-wall-vk",
            "video",
            "single",
            video,
            fps=fps,
            extra=extra,
        ),
        Scenario(
            f"video {label} mpvpaper",
            "mpvpaper",
            "video",
            "single",
            video,
            fps=fps,
            tuned=True,
            extra=extra,
        ),
        Scenario(
            f"video {label} Phonto",
            "phonto",
            "video",
            "single",
            video,
            fps=fps,
            extra=extra,
        ),
        Scenario(
            f"video {label} Wallr auto",
            "wallr",
            "video",
            "single",
            video,
            fps=fps,
            tuned=True,
            extra=extra,
        ),
        Scenario(
            f"video {label} Kacau cached {fps}fps",
            "kacau-wall",
            "video",
            "single",
            video,
            fps=fps,
            extra=extra,
        ),
        Scenario(
            f"video {label} Yin cuda-copy",
            "yin",
            "video",
            "single",
            video,
            fps=fps,
            tuned=True,
            extra=extra,
        ),
    ]


def scenario_command(scenario, single_output, outputs):
    output = single_output if scenario.topology == "single" else "*"
    if scenario.engine == "skwd-wall-still":
        return [str(STILL), output, scenario.source, "--persist"]
    if scenario.engine == "skwd-wall-vk":
        command = [str(VK), output, scenario.source, "--mute"]
        if scenario.workload == "we":
            command.extend(["--scene", scenario.source])
        return command
    if scenario.engine == "mpvpaper":
        options = ["no-audio", "loop"]
        if scenario.tuned:
            options.append("hwdec=auto")
        return ["mpvpaper", "-o", " ".join(options), output, scenario.source]
    if scenario.engine == "phonto":
        selected = [single_output] if scenario.topology == "single" else outputs
        command = [PHONTO, "--layer", "background", "--scale", "fill"]
        for name in selected:
            command.extend(["--display", name, scenario.source])
        return command
    if scenario.engine == "awww":
        return ["awww-daemon", "--no-cache", "--quiet"]
    if scenario.engine == "wpaperd":
        return [WPAPERD]
    if scenario.engine == "hyprpaper":
        return [HYPRPAPER]
    if scenario.engine == "wallr":
        return [WALLR]
    if scenario.engine == "kacau-wall":
        return [KACAU]
    if scenario.engine == "yin":
        return [YIN, "--use-cuda-copy"]
    if scenario.engine == "linux-wallpaperengine":
        selected = [single_output] if scenario.topology == "single" else outputs
        command = [
            "linux-wallpaperengine",
            "--silent",
            "--no-fullscreen-pause",
            "--layer",
            "background",
            "--fps",
            str(scenario.fps),
        ]
        for name in selected:
            command.extend(["--screen-root", name])
        command.append(Path(scenario.source).name)
        return command
    raise ValueError(f"unknown engine {scenario.engine}")


def ppid(pid):
    try:
        with open(f"/proc/{pid}/status") as status:
            for line in status:
                if line.startswith("PPid:"):
                    return int(line.split()[1])
    except OSError:
        pass
    return None


def process_tree(root_pid):
    live = {int(entry.name) for entry in Path("/proc").iterdir() if entry.name.isdigit()}
    result = {root_pid} if root_pid in live else set()
    changed = True
    while changed:
        changed = False
        for pid in live - result:
            if ppid(pid) in result:
                result.add(pid)
                changed = True
    return result


def process_name(pid):
    try:
        return Path(f"/proc/{pid}/comm").read_text().strip()
    except OSError:
        return ""


def cpu_total(pids):
    values = [P.proc_cpu(pid) for pid in pids]
    return sum(value for value in values if value is not None)


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


def vram_total(pids):
    try:
        output = subprocess.check_output(
            [
                "nvidia-smi",
                "pmon",
                "-c",
                "1",
                "-s",
                "m",
            ],
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return 0
    return parse_pmon_vram(output, pids)


def stop_process(proc):
    if proc.poll() is not None:
        return
    try:
        os.killpg(proc.pid, signal.SIGTERM)
        proc.wait(timeout=5)
    except (ProcessLookupError, subprocess.TimeoutExpired):
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait()


def remove_stale_yin_socket(path=Path("/tmp/yin")):
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError:
        return
    if not stat.S_ISSOCK(mode):
        raise RuntimeError(f"refusing to replace non-socket Yin IPC path: {path}")
    path.unlink()


def kacau_socket(env=None):
    env = env or os.environ
    runtime = env.get("XDG_RUNTIME_DIR")
    if runtime:
        return Path(runtime) / "kacau-wall.sock"
    return Path(env.get("HOME", str(Path.home()))) / "kacau-wall.sock"


def remove_stale_kacau_socket(path):
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError:
        return
    if not stat.S_ISSOCK(mode):
        raise RuntimeError(f"refusing to replace non-socket Kacau IPC path: {path}")
    path.unlink()


def directory_bytes(path):
    return sum(entry.stat().st_size for entry in path.rglob("*") if entry.is_file())


def wait_for_kacau_media(proc, scenario, cache, timeout=300.0):
    """Wait for Kacau's asynchronous cache preparation to hand off to its decoder."""
    expected_suffix = ".bin" if scenario.workload == "static" else ".h264"
    deadline = time.monotonic() + timeout
    previous = None
    stable = 0
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError("Kacau daemon exited during media preparation")
        files = [
            entry
            for entry in cache.rglob(f"*{expected_suffix}")
            if entry.is_file() and entry.stat().st_size > 0
        ]
        sizes = tuple(sorted((str(entry), entry.stat().st_size) for entry in files))
        ffmpeg_live = any(
            process_name(pid).startswith("ffmpeg") for pid in process_tree(proc.pid)
        )
        if files and sizes == previous and not ffmpeg_live:
            stable += 1
            if stable >= 3:
                return
        else:
            stable = 0
        previous = sizes
        time.sleep(0.2)
    raise RuntimeError(f"Kacau did not finish preparing {scenario.workload} media")


def wait_for_stable_file(path, timeout=10.0):
    deadline = time.monotonic() + timeout
    previous = None
    stable = 0
    while time.monotonic() < deadline:
        try:
            size = path.stat().st_size
        except FileNotFoundError:
            size = 0
        if size > 0 and size == previous:
            stable += 1
            if stable >= 2:
                return
        else:
            stable = 0
        previous = size
        time.sleep(0.1)
    raise RuntimeError(f"engine did not write validation frame {path.name}")


def frame_content_signature(path):
    crop = ["-gravity", "center", "-crop", "75%x75%+0+0", "+repage"]
    measured = subprocess.run(
        [
            "magick",
            str(path),
            *crop,
            "-format",
            "%[fx:mean] %[fx:standard_deviation]",
            "info:",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=20,
    )
    if measured.returncode != 0:
        raise RuntimeError(measured.stderr.strip() or "frame signal check failed")
    try:
        mean, deviation = (float(value) for value in measured.stdout.split())
    except (TypeError, ValueError) as error:
        raise RuntimeError(f"invalid frame signal result: {measured.stdout!r}") from error
    pixels = subprocess.run(
        ["magick", str(path), *crop, "-depth", "8", "rgb:-"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=20,
    )
    if pixels.returncode != 0:
        raise RuntimeError(
            pixels.stderr.decode(errors="replace").strip() or "frame crop failed"
        )
    return hashlib.sha256(pixels.stdout).hexdigest(), mean, deviation


def validate_video_motion(outputs, workdir):
    output_checks = {}
    for output in outputs:
        hashes = []
        means = []
        deviations = []
        safe_output = output.replace("/", "_")
        for index in range(VIDEO_VALIDATION_SAMPLES):
            prepare_validation_output(output)
            frame = workdir / f"motion-{safe_output}-{index}.ppm"
            captured = subprocess.run(
                ["grim", "-o", output, "-t", "ppm", str(frame)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=20,
            )
            if captured.returncode != 0:
                raise RuntimeError(captured.stderr.strip() or "video screenshot failed")
            wait_for_stable_file(frame)
            digest, mean, deviation = frame_content_signature(frame)
            hashes.append(digest)
            means.append(mean)
            deviations.append(deviation)
            if index + 1 < VIDEO_VALIDATION_SAMPLES:
                time.sleep(VIDEO_VALIDATION_INTERVAL)
        unique_frames = len(set(hashes))
        if (
            max(means) < MIN_VIDEO_MEAN_SIGNAL
            or max(deviations) < MIN_VIDEO_SIGNAL_DEVIATION
        ):
            raise RuntimeError(
                f"video validation captured blank or near-black content on {output} "
                f"(mean={max(means):.4f}, deviation={max(deviations):.4f})"
            )
        if unique_frames < 2:
            raise RuntimeError(
                f"video validation captured no motion on {output} "
                f"({unique_frames}/{len(hashes)} unique central crops, "
                f"mean={max(means):.4f}, deviation={max(deviations):.4f})"
            )
        output_checks[output] = {
            "hashes": [value[:16] for value in hashes],
            "unique_frames": unique_frames,
            "mean_signal": [round(value, 6) for value in means],
            "signal_deviation": [round(value, 6) for value in deviations],
        }
    return output_checks


def warm_validation_capture(outputs):
    if not VALIDATE_MOTION:
        return
    with tempfile.TemporaryDirectory(prefix="skwd-validation-warmup-") as temporary:
        workdir = Path(temporary)
        for output in outputs:
            prepare_validation_output(output)
            frame = workdir / f"warm-{output.replace('/', '_')}.ppm"
            captured = subprocess.run(
                ["grim", "-o", output, "-t", "ppm", str(frame)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=20,
            )
            if captured.returncode != 0:
                raise RuntimeError(
                    captured.stderr.strip() or "validation capture warm-up failed"
                )
            wait_for_stable_file(frame)


def activate_validation_baseline(outputs):
    for output in outputs:
        if output in VALIDATION_WORKSPACES:
            prepare_validation_output(output)


def activate_validation_workspace(output):
    workspace = VALIDATION_WORKSPACES.get(output)
    if workspace is None:
        return
    focused = subprocess.run(
        ["niri", "msg", "action", "focus-workspace", workspace],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=10,
    )
    if focused.returncode != 0:
        raise RuntimeError(
            focused.stderr.strip() or f"could not focus validation workspace {workspace}"
        )


def prepare_validation_output(output, attempts=3):
    error = None
    for _ in range(attempts):
        activate_validation_workspace(output)
        try:
            require_empty_active_workspace(output)
            return
        except RuntimeError as caught:
            error = caught
    raise error


def require_empty_active_workspace(output):
    queried = subprocess.run(
        ["niri", "msg", "-j", "workspaces"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=10,
    )
    if queried.returncode != 0:
        raise RuntimeError(queried.stderr.strip() or "could not inspect Niri workspaces")
    try:
        workspaces = json.loads(queried.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("Niri returned invalid workspace data") from error
    active = next(
        (
            workspace
            for workspace in workspaces
            if workspace.get("output") == output and workspace.get("is_active")
        ),
        None,
    )
    if active is None:
        raise RuntimeError(f"{output} has no active Niri workspace")
    if active.get("active_window_id") is not None:
        raise RuntimeError(
            f"video validation requires an empty active workspace on {output}"
        )


def motion_validation_outputs(extra, expected_outputs):
    return sorted(extra.get("active_wallpaper_outputs", expected_outputs))


def scenario_media_outputs(scenario, single_output, outputs):
    return [single_output] if scenario.topology == "single" else sorted(outputs)


def requires_motion_validation(scenario):
    return scenario.workload in {"video", "we"}


def fatal_engine_log(error_log):
    try:
        text = Path(error_log.name).read_text(errors="replace")
    except OSError:
        return None
    markers = (
        "failed to decode media source",
        "failed to create h264 decoder",
        "panicked at",
        "video optimization failed",
    )
    lowered = text.casefold()
    return next((marker for marker in markers if marker in lowered), None)


def kacau_apply_command(scenario, single_output):
    command = [KACAU, "set", scenario.source]
    if scenario.topology == "single":
        command.append(single_output)
    command.extend(["--fit-mode", "cover", "--transition", "none"])
    if scenario.workload == "video":
        command.extend(["--frame-rate", str(scenario.fps or 30)])
    return command


def write_wpaperd_config(scenario, workdir):
    config = workdir / "wpaperd.toml"
    config.write_text(
        "[any]\n"
        f"path = {json.dumps(scenario.source)}\n"
        'mode = "center"\n'
        "initial-transition = false\n"
    )
    return config


def write_hyprpaper_config(scenario, single_output, outputs, workdir):
    selected = [single_output] if scenario.topology == "single" else outputs
    values = [scenario.source, *selected]
    if any("\n" in value or "\r" in value for value in values):
        raise RuntimeError("hyprpaper config values must not contain newlines")
    blocks = []
    for output in selected:
        blocks.append(
            "wallpaper {\n"
            f"    monitor = {output}\n"
            f"    path = {scenario.source}\n"
            "    fit_mode = cover\n"
            "}\n"
        )
    config = workdir / "hyprpaper.conf"
    config.write_text("splash = false\nipc = false\n\n" + "\n".join(blocks))
    return config


def write_wallr_config(workdir):
    config = workdir / "wallr.yaml"
    socket = workdir / "wallr.sock"
    cache = workdir / "cache" / "wallr"
    config.write_text(
        "wallpaper:\n"
        "  mode: fill\n"
        "  loop_video: true\n"
        "  mute: true\n"
        "animation:\n"
        "  duration: 1ms\n"
        "theme:\n"
        "  provider: none\n"
        "matugen:\n"
        "  enabled: false\n"
        "video:\n"
        "  hw_decode: auto\n"
        "  preferred_gpu: auto\n"
        "  preload_frames: 2\n"
        "daemon:\n"
        "  auto_start: false\n"
        f"  socket: {json.dumps(str(socket))}\n"
        "  max_fps: 60\n"
        "cache:\n"
        f"  dir: {json.dumps(str(cache))}\n"
        "  max_size: 512MB\n"
    )
    return config


def wallr_apply_command(scenario, single_output, config):
    command = [
        WALLR,
        "--config",
        str(config),
        "set",
        scenario.source,
        "--no-theme",
        "--mode",
        "fill",
        "--effect",
        "simple",
        "--duration",
        "1ms",
    ]
    if scenario.topology == "single":
        command.extend(["--monitor", single_output])
    return command


def prepend_library_path(env, path):
    inherited = env.get("LD_LIBRARY_PATH")
    env["LD_LIBRARY_PATH"] = f"{path}:{inherited}" if inherited else str(path)


def launch_scenario(scenario, single_output, outputs, workdir):
    env = dict(os.environ)
    env["XDG_CACHE_HOME"] = str(workdir / "cache")
    if scenario.engine == "yin":
        home = Path(YIN_HOME) if YIN_HOME else workdir / "home"
        home.mkdir(parents=True, exist_ok=True)
        env["HOME"] = str(home)
        if YIN_LIBRARY_PATH:
            prepend_library_path(env, YIN_LIBRARY_PATH)
        remove_stale_yin_socket()
    elif scenario.engine == "kacau-wall":
        home = workdir / "home"
        home.mkdir(parents=True, exist_ok=True)
        env["HOME"] = str(home)
        env["XDG_CONFIG_HOME"] = str(workdir / "config")
        if KACAU_LIBRARY_PATH:
            prepend_library_path(env, KACAU_LIBRARY_PATH)
        remove_stale_kacau_socket(kacau_socket(env))
    elif scenario.engine == "wallr":
        home = workdir / "home"
        home.mkdir(parents=True, exist_ok=True)
        env["HOME"] = str(home)
        env["XDG_CONFIG_HOME"] = str(workdir / "config")
    elif scenario.engine == "wpaperd":
        env["XDG_STATE_HOME"] = str(workdir / "state")
    if scenario.workload == "we" and scenario.engine == "skwd-wall-vk":
        env["SKWD_PAPER_WE_FPS"] = str(scenario.fps)
    error_log = open(workdir / "stderr.log", "w+b")
    command = scenario_command(scenario, single_output, outputs)
    if scenario.engine == "wpaperd":
        command.extend(["--config", str(write_wpaperd_config(scenario, workdir))])
    elif scenario.engine == "hyprpaper":
        command.extend(
            [
                "--config",
                str(write_hyprpaper_config(scenario, single_output, outputs, workdir)),
            ]
        )
    elif scenario.engine == "wallr":
        wallr_config = write_wallr_config(workdir)
        command = [
            WALLR,
            "--config",
            str(wallr_config),
            "daemon",
            "--max-fps",
            str(scenario.fps or 60),
        ]
    proc = subprocess.Popen(
        command,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=error_log,
        start_new_session=True,
    )
    extra = {}
    if scenario.engine == "awww":
        time.sleep(0.8)
        selected = single_output if scenario.topology == "single" else ",".join(outputs)
        applied = subprocess.run(
            [
                "awww",
                "img",
                scenario.source,
                "--outputs",
                selected,
                "--resize",
                "crop",
                "--transition-type",
                "none",
            ],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            timeout=20,
        )
        if applied.returncode != 0:
            stop_process(proc)
            error_log.close()
            raise RuntimeError(applied.stderr.strip() or "awww img failed")
    elif scenario.engine == "yin":
        socket = Path("/tmp/yin")
        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline and not socket.exists():
            if proc.poll() is not None:
                break
            time.sleep(0.05)
        if proc.poll() is not None or not socket.exists():
            stop_process(proc)
            detail = error_tail(error_log)
            error_log.close()
            raise RuntimeError(
                "Yin daemon did not create /tmp/yin"
                + (f": {detail}" if detail else "")
            )
        selected = [single_output] if scenario.topology == "single" else outputs
        prepare_start = time.monotonic()
        for name in selected:
            applied = subprocess.run(
                [YINCTL, "--img", scenario.source, "--output", name],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=300,
            )
            response = (applied.stdout + "\n" + applied.stderr).strip()
            if applied.returncode != 0 or "Img sent to daemon!" not in response:
                stop_process(proc)
                error_log.close()
                raise RuntimeError(response or "yinctl failed")
        cache = Path(env["HOME"]) / ".cache" / "yin"
        extra = {
            "prepare_seconds": round(time.monotonic() - prepare_start, 3),
            "cache_bytes": sum(
                entry.stat().st_size for entry in cache.glob("*") if entry.is_file()
            ),
            "active_wallpaper_outputs": sorted(selected),
        }
    elif scenario.engine == "kacau-wall":
        socket = kacau_socket(env)
        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline and not socket.exists():
            if proc.poll() is not None:
                break
            time.sleep(0.05)
        if proc.poll() is not None or not socket.exists():
            stop_process(proc)
            detail = error_tail(error_log)
            error_log.close()
            raise RuntimeError(
                "Kacau daemon did not create its IPC socket"
                + (f": {detail}" if detail else "")
            )

        command = kacau_apply_command(scenario, single_output)
        prepare_start = time.monotonic()
        applied = subprocess.run(
            command,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=20,
        )
        if applied.returncode != 0:
            stop_process(proc)
            error_log.close()
            raise RuntimeError(applied.stderr.strip() or "kacau-wall set failed")
        cache = Path(env["XDG_CACHE_HOME"]) / "kacau"
        try:
            wait_for_kacau_media(proc, scenario, cache)
        except RuntimeError:
            stop_process(proc)
            error_log.close()
            raise
        time.sleep(1.0)
        selected = [single_output] if scenario.topology == "single" else outputs
        extra = {
            "prepare_seconds": round(time.monotonic() - prepare_start, 3),
            "cache_bytes": directory_bytes(cache),
            "active_wallpaper_outputs": sorted(selected),
        }
        fatal_log = fatal_engine_log(error_log)
        if fatal_log:
            stop_process(proc)
            error_log.close()
            raise RuntimeError(f"engine log contains: {fatal_log}")
    elif scenario.engine == "wallr":
        socket = workdir / "wallr.sock"
        deadline = time.monotonic() + 20.0
        while time.monotonic() < deadline and not socket.exists():
            if proc.poll() is not None:
                break
            time.sleep(0.05)
        if proc.poll() is not None or not socket.exists():
            stop_process(proc)
            detail = error_tail(error_log)
            error_log.close()
            raise RuntimeError(
                "Wallr daemon did not create its IPC socket"
                + (f": {detail}" if detail else "")
            )
        selected = [single_output] if scenario.topology == "single" else outputs
        applied = subprocess.run(
            wallr_apply_command(scenario, single_output, wallr_config),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=60,
        )
        if applied.returncode != 0:
            stop_process(proc)
            error_log.close()
            raise RuntimeError(applied.stderr.strip() or "wallr set failed")
        info = subprocess.run(
            [WALLR, "--config", str(wallr_config), "ipc", "info"],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=20,
        )
        extra = {
            "active_wallpaper_outputs": sorted(selected),
            "decoder_info": info.stdout.strip() if info.returncode == 0 else None,
        }
    return proc, error_log, extra


def error_tail(error_log):
    error_log.flush()
    error_log.seek(0)
    return error_log.read().decode(errors="replace")[-2000:].strip()


def niri_layers():
    result = subprocess.run(
        ["niri", "msg", "-j", "layers"],
        capture_output=True,
        text=True,
        timeout=5,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "niri layer query failed")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"invalid niri layer response: {error}") from error


def rendered_outputs(engine):
    namespace = {
        "skwd-wall-vk": "skwd-wall-vk",
        "mpvpaper": "mpvpaper",
        "phonto": "phonto",
        "hyprpaper": "hyprpaper",
        "wallr": "wallr",
        "yin": "yin-wallpaper",
    }.get(engine)
    namespace_prefix = {
        "kacau-wall": "kacau:wall:",
        "wpaperd": "wpaperd-",
    }.get(engine)
    if namespace is None and namespace_prefix is None:
        return None
    try:
        layers = niri_layers()
    except (OSError, RuntimeError, subprocess.TimeoutExpired):
        return None
    return sorted(
        {
            layer.get("output")
            for layer in layers
            if (
                layer.get("namespace") == namespace
                or (
                    namespace_prefix is not None
                    and str(layer.get("namespace", "")).startswith(namespace_prefix)
                )
            )
            and layer.get("output")
        }
    )


def wallpaper_processes():
    processes = {}
    for name in WALLPAPER_PROCESS_NAMES:
        pids = P.process_pids(name)
        if pids:
            processes[name] = pids
    return processes


def wallpaper_layers():
    return [
        layer
        for layer in niri_layers()
        if any(
            str(layer.get("namespace", "")).startswith(namespace)
            for namespace in WALLPAPER_LAYER_NAMESPACES
        )
    ]


def signal_wallpaper_processes(processes, sig):
    for pid in sorted({pid for pids in processes.values() for pid in pids}):
        if pid == os.getpid():
            continue
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            pass


def kill_running_wallpapers(timeout=10.0):
    deadline = time.monotonic() + timeout
    processes = wallpaper_processes()
    if processes:
        signal_wallpaper_processes(processes, signal.SIGTERM)
    while processes and time.monotonic() < deadline:
        time.sleep(0.1)
        processes = wallpaper_processes()
    if processes:
        signal_wallpaper_processes(processes, signal.SIGKILL)
        time.sleep(0.1)
        processes = wallpaper_processes()
    if processes:
        detail = ", ".join(
            f"{name}={','.join(map(str, pids))}" for name, pids in processes.items()
        )
        raise RuntimeError(f"wallpaper processes survived cleanup: {detail}")


def require_clean_wallpaper_state(timeout=10.0):
    kill_running_wallpapers(timeout)
    deadline = time.monotonic() + timeout
    layers = wallpaper_layers()
    while layers and time.monotonic() < deadline:
        time.sleep(0.1)
        layers = wallpaper_layers()
    if layers:
        namespaces = sorted({str(layer.get("namespace", "")) for layer in layers})
        raise RuntimeError(f"wallpaper layers survived cleanup: {', '.join(namespaces)}")
    processes = wallpaper_processes()
    if processes:
        detail = ", ".join(
            f"{name}={','.join(map(str, pids))}" for name, pids in processes.items()
        )
        raise RuntimeError(f"wallpaper processes returned during cleanup: {detail}")


def summarize_snapshots(samples):
    snapshot = {
        metric: statistics.median(sample[metric] for sample in samples)
        for metric in ("rss", "pss", "vram")
    }
    spread = {
        metric: max(sample[metric] for sample in samples)
        - min(sample[metric] for sample in samples)
        for metric in snapshot
    }
    return {"snapshot": snapshot, "spread": spread, "samples": samples}


def compositor_snapshot_is_stable(summary):
    return all(
        summary["spread"][metric] <= COMPOSITOR_STABILITY_TOLERANCE[metric]
        for metric in COMPOSITOR_STABILITY_TOLERANCE
    )


def compositor_baseline_recovered(before, after):
    return all(
        abs(after[metric] - before[metric])
        <= COMPOSITOR_RECOVERY_TOLERANCE[metric]
        for metric in COMPOSITOR_RECOVERY_TOLERANCE
    )


def stable_compositor_snapshot(compositor_pid, reference=None):
    deadline = time.monotonic() + COMPOSITOR_STABLE_TIMEOUT
    samples = []
    latest = None
    while time.monotonic() < deadline:
        samples.append(P.process_snapshot(compositor_pid))
        samples = samples[-COMPOSITOR_STABLE_SAMPLES:]
        if len(samples) == COMPOSITOR_STABLE_SAMPLES:
            latest = summarize_snapshots(samples)
            stable = compositor_snapshot_is_stable(latest)
            recovered = reference is None or compositor_baseline_recovered(
                reference, latest["snapshot"]
            )
            if stable and recovered:
                return latest
        time.sleep(COMPOSITOR_STABLE_INTERVAL)
    detail = latest or summarize_snapshots(samples)
    raise RuntimeError(f"niri memory did not stabilize: {detail}")


def clean_compositor_baseline(compositor_pid, profile, attempts=3):
    for _ in range(attempts):
        require_clean_wallpaper_state()
        compositor = stable_compositor_snapshot(compositor_pid)
        floor = P.idle_floor(profile, compositor_pid)
        if not wallpaper_processes() and not wallpaper_layers():
            return floor, compositor
    raise RuntimeError("wallpaper processes or layers returned during baseline sampling")


def run_scenario(
    scenario,
    floor,
    profile,
    single_output,
    outputs,
    compositor_pid,
    compositor_before,
):
    metadata = {
        **scenario.metadata(single_output),
        "idle_gpu": round(floor[0], 1),
        "idle_power": round(floor[1], 1),
    }
    with tempfile.TemporaryDirectory(prefix="skwd-competitor-") as temporary:
        workdir = Path(temporary)
        try:
            proc, error_log, extra = launch_scenario(
                scenario, single_output, outputs, workdir
            )
            metadata.update(extra)
        except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
            return {**metadata, "error": str(error)}
        try:
            settle = profile["settle"]
            if scenario.workload == "we":
                settle = max(settle, 6.0)
            time.sleep(settle)
            if proc.poll() is not None:
                return {**metadata, "error": error_tail(error_log) or "engine died at spawn"}
            fatal_log = fatal_engine_log(error_log)
            if fatal_log:
                return {**metadata, "error": f"engine log contains: {fatal_log}"}
            visible_outputs = rendered_outputs(scenario.engine)
            expected_outputs = (
                sorted(outputs)
                if scenario.engine in ("yin", "kacau-wall", "wallr")
                else [single_output]
                if scenario.topology == "single"
                else sorted(outputs)
            )
            if visible_outputs is not None and visible_outputs != sorted(expected_outputs):
                return {
                    **metadata,
                    "error": (
                        f"rendered outputs {visible_outputs}, expected "
                        f"{sorted(expected_outputs)}"
                    ),
                }
            if requires_motion_validation(scenario) and VALIDATE_MOTION:
                validation_outputs = motion_validation_outputs(extra, expected_outputs)
                metadata["motion_validation_outputs"] = validation_outputs
                try:
                    for output in validation_outputs:
                        prepare_validation_output(output)
                    metadata["motion_frame_hashes"] = validate_video_motion(
                        validation_outputs, workdir
                    )
                except (RuntimeError, subprocess.TimeoutExpired) as error:
                    fatal_log = fatal_engine_log(error_log)
                    detail = f"{error}"
                    if fatal_log:
                        detail += f"; engine log contains: {fatal_log}"
                    return {**metadata, "error": detail}
            start_pids = process_tree(proc.pid)
            cpu_start = cpu_total(start_pids)
            compositor_cpu_start = P.proc_cpu(compositor_pid)
            if compositor_cpu_start is None:
                return {**metadata, "error": f"{P.COMPOSITOR_NAME} died before measurement"}
            start = time.monotonic()
            utils = []
            powers = []
            samples = max(1, int(profile["window"] * profile["hz"]))
            for _ in range(samples):
                util, power = P.gpu_sample()
                utils.append(util)
                powers.append(power)
                time.sleep(1.0 / profile["hz"])
            elapsed = time.monotonic() - start
            pids = process_tree(proc.pid)
            compositor_cpu_end = P.proc_cpu(compositor_pid)
            if proc.poll() is not None:
                return {**metadata, "error": error_tail(error_log) or "engine died mid-window"}
            if compositor_cpu_end is None:
                return {**metadata, "error": f"{P.COMPOSITOR_NAME} died mid-window"}
            fatal_log = fatal_engine_log(error_log)
            if fatal_log:
                return {**metadata, "error": f"engine log contains: {fatal_log}"}
            cpu = max(0.0, cpu_total(pids) - cpu_start) / elapsed * 100.0
            gpu_mean = sum(utils) / len(utils)
            power_mean = sum(powers) / len(powers)
            try:
                compositor_active = stable_compositor_snapshot(compositor_pid)
            except RuntimeError as error:
                return {**metadata, "error": str(error)}
            compositor = P.compositor_metrics(
                P.COMPOSITOR_NAME,
                compositor_pid,
                compositor_before["snapshot"],
                compositor_active["snapshot"],
                compositor_cpu_start,
                compositor_cpu_end,
                elapsed,
                floor[2],
            )
            compositor["baseline_samples"] = compositor_before
            compositor["active_samples"] = compositor_active
            renderer_pss = sum(P.proc_pss(pid) for pid in pids)
            return {
                **metadata,
                "cpu": round(cpu, 1),
                "gpu_mean": round(gpu_mean, 1),
                "gpu_delta": round(gpu_mean - floor[0], 1),
                "gpu_max": max(utils),
                "power_mean": round(power_mean, 1),
                "power_delta": round(power_mean - floor[1], 1),
                "power_max": round(max(powers), 1),
                "processes": len(pids),
                "process_names": sorted(filter(None, (process_name(pid) for pid in pids))),
                "rendered_outputs": visible_outputs,
                "vram": vram_total(pids),
                "rss": sum(P.proc_rss(pid) for pid in pids),
                "pss": renderer_pss,
                "combined_pss": renderer_pss + compositor["pss_delta"],
                "compositor": compositor,
            }
        finally:
            stop_process(proc)
            error_log.close()
            time.sleep(0.7)


def executable_available(command):
    return Path(command).is_file() or shutil.which(command)


def ensure_available(matrix):
    engines = {scenario.engine for scenario in matrix}
    required = {"nvidia-smi"}
    if "awww" in engines:
        required.update(("awww", "awww-daemon"))
    if "mpvpaper" in engines:
        required.add("mpvpaper")
    if "wpaperd" in engines:
        required.add(WPAPERD)
    if "hyprpaper" in engines:
        required.add(HYPRPAPER)
    if "wallr" in engines:
        required.add(WALLR)
    if "linux-wallpaperengine" in engines:
        required.add("linux-wallpaperengine")
    if "phonto" in engines:
        required.add(PHONTO)
    if "kacau-wall" in engines:
        required.add(KACAU)
    if VALIDATE_MOTION and any(requires_motion_validation(scenario) for scenario in matrix):
        required.update(("grim", "magick"))
    if "yin" in engines:
        required.update((YIN, YINCTL))
    missing = [command for command in sorted(required) if not executable_available(command)]
    if missing:
        sys.exit(f"missing competitor tools: {', '.join(missing)}")
    if {"skwd-wall-vk", "skwd-wall-still"} & engines:
        P.require_shared_device_binary()


def ensure_quiet():
    require_clean_wallpaper_state()


def midpoint(before, after):
    return (before + after) / 2


def finalize_baseline_metrics(result, before, after, floor_before, floor_after):
    floor_drift = {
        "gpu": round(abs(floor_after[0] - floor_before[0]), 1),
        "power": round(abs(floor_after[1] - floor_before[1]), 1),
        "compositor_cpu": round(abs(floor_after[2] - floor_before[2]), 1),
    }
    qualification = {
        "compositor_memory": compositor_baseline_recovered(
            before["snapshot"], after["snapshot"]
        ),
        "gpu": floor_drift["gpu"] <= GPU_FLOOR_DRIFT_LIMIT,
        "power": floor_drift["power"] <= POWER_FLOOR_DRIFT_LIMIT,
        "compositor_cpu": (
            floor_drift["compositor_cpu"] <= COMPOSITOR_CPU_FLOOR_DRIFT_LIMIT
        ),
    }
    result["baseline"] = {
        "before": before,
        "after_cleanup": after,
        "floor_before": {
            "gpu": round(floor_before[0], 1),
            "power": round(floor_before[1], 1),
            "compositor_cpu": round(floor_before[2], 1),
        },
        "floor_after": {
            "gpu": round(floor_after[0], 1),
            "power": round(floor_after[1], 1),
            "compositor_cpu": round(floor_after[2], 1),
        },
        "floor_drift": floor_drift,
        "qualified": qualification,
    }
    if "error" in result or "compositor" not in result:
        return result
    compositor = result["compositor"]
    for metric in ("rss", "pss", "vram"):
        baseline = midpoint(
            before["snapshot"][metric], after["snapshot"][metric]
        )
        compositor[f"{metric}_before"] = before["snapshot"][metric]
        compositor[f"{metric}_after_cleanup"] = after["snapshot"][metric]
        compositor[f"{metric}_baseline"] = baseline
        compositor[f"{metric}_delta"] = round(compositor[metric] - baseline, 1)
    compositor["post_cleanup_samples"] = after
    compositor["cpu_idle_before"] = round(floor_before[2], 1)
    compositor["cpu_idle_after"] = round(floor_after[2], 1)
    compositor["cpu_idle"] = round(midpoint(floor_before[2], floor_after[2]), 1)
    compositor["cpu_delta"] = round(compositor["cpu"] - compositor["cpu_idle"], 1)
    result["idle_gpu"] = round(midpoint(floor_before[0], floor_after[0]), 1)
    result["idle_power"] = round(midpoint(floor_before[1], floor_after[1]), 1)
    result["gpu_delta"] = round(result["gpu_mean"] - result["idle_gpu"], 1)
    result["power_delta"] = round(
        result["power_mean"] - result["idle_power"], 1
    )
    result["combined_pss"] = round(result["pss"] + compositor["pss_delta"], 1)
    return result


def matrix_id(matrix, single_output):
    payload = [scenario.metadata(single_output) for scenario in matrix]
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()[:16]


def parse_validation_workspaces(values):
    workspaces = {}
    for value in values or []:
        output, separator, workspace = value.partition("=")
        if not separator or not output or not workspace:
            raise ValueError(
                f"invalid validation workspace {value!r}; expected OUTPUT=NAME"
            )
        workspaces[output] = workspace
    return workspaces


def main():
    global PHONTO, WPAPERD, HYPRPAPER, KACAU, KACAU_LIBRARY_PATH, WALLR
    global VALIDATE_MOTION
    global VALIDATION_WORKSPACES, YIN, YINCTL, YIN_HOME, YIN_LIBRARY_PATH
    parser = argparse.ArgumentParser()
    parser.add_argument("--quick", action="store_true")
    parser.add_argument("--window", type=float)
    parser.add_argument("--out", default=str(ROOT / "competitor-sweep-latest.json"))
    parser.add_argument("--single-output")
    parser.add_argument("--floor-video")
    parser.add_argument("--scene")
    parser.add_argument(
        "--resolution-label",
        choices=("1080p", "4K"),
        help="run only the matched-resolution video matrix",
    )
    parser.add_argument(
        "--resolution-video",
        help="30 fps video fixture at the selected resolution",
    )
    parser.add_argument(
        "--phonto",
        default=PHONTO,
        help="Phonto executable (default: resolve phonto from PATH)",
    )
    parser.add_argument("--wpaperd", default=WPAPERD, help="wpaperd executable")
    parser.add_argument(
        "--hyprpaper", default=HYPRPAPER, help="hyprpaper executable"
    )
    parser.add_argument("--kacau", default=KACAU, help="Kacau Wall executable")
    parser.add_argument(
        "--kacau-library-path",
        help="Prepend this directory to LD_LIBRARY_PATH for Kacau Wall only",
    )
    parser.add_argument("--wallr", default=WALLR, help="Wallr executable")
    parser.add_argument(
        "--validate-motion",
        action="store_true",
        help="Capture each video output over a short window and require visible motion",
    )
    parser.add_argument(
        "--validation-workspace",
        action="append",
        metavar="OUTPUT=NAME",
        help="Focus this named empty workspace before every validation capture",
    )
    parser.add_argument("--yin", default=YIN, help="Yin daemon executable")
    parser.add_argument("--yinctl", default=YINCTL, help="Yin client executable")
    parser.add_argument(
        "--yin-home",
        help="Persistent HOME for pre-warmed Yin output-sized video caches",
    )
    parser.add_argument(
        "--yin-library-path",
        help="Prepend this directory to LD_LIBRARY_PATH for Yin only",
    )
    parser.add_argument("--only", action="append")
    parser.add_argument(
        "--engine",
        action="append",
        help="Only run this exact engine name (repeatable)",
    )
    parser.add_argument(
        "--workload",
        action="append",
        choices=("static", "video", "we"),
        help="Only run this workload (repeatable)",
    )
    parser.add_argument("--plan", action="store_true")
    args = parser.parse_args()
    PHONTO = args.phonto
    WPAPERD = args.wpaperd
    HYPRPAPER = args.hyprpaper
    KACAU = args.kacau
    KACAU_LIBRARY_PATH = args.kacau_library_path
    if KACAU_LIBRARY_PATH and not Path(KACAU_LIBRARY_PATH).is_dir():
        parser.error(f"Kacau library path is not a directory: {KACAU_LIBRARY_PATH}")
    WALLR = args.wallr
    VALIDATE_MOTION = args.validate_motion
    try:
        VALIDATION_WORKSPACES = parse_validation_workspaces(
            args.validation_workspace
        )
    except ValueError as error:
        parser.error(str(error))
    YIN = args.yin
    YINCTL = args.yinctl
    YIN_HOME = args.yin_home
    YIN_LIBRARY_PATH = args.yin_library_path
    if YIN_LIBRARY_PATH and not Path(YIN_LIBRARY_PATH).is_dir():
        parser.error(f"Yin library path is not a directory: {YIN_LIBRARY_PATH}")
    outputs_info = P.active_outputs()
    if not outputs_info:
        sys.exit("could not discover active outputs")
    ranked = P.ranked_outputs(outputs_info, "")
    output_names = [output["name"] for output in ranked]
    single_output = args.single_output or ranked[0]["name"]
    if single_output not in output_names:
        sys.exit(f"unknown single output {single_output}; available: {', '.join(output_names)}")
    if args.resolution_label:
        if not args.resolution_video:
            sys.exit("--resolution-label requires --resolution-video")
        expected_size = {
            "1080p": (1920, 1080),
            "4K": (3840, 2160),
        }[args.resolution_label]
        selected_output = next(
            output for output in outputs_info if output["name"] == single_output
        )
        output_size = (selected_output["width"], selected_output["height"])
        if output_size != expected_size:
            sys.exit(
                f"{args.resolution_label} matrix requires a {expected_size[0]}x"
                f"{expected_size[1]} output; {single_output} is "
                f"{output_size[0]}x{output_size[1]}"
            )
        source = str(Path(args.resolution_video).resolve())
        info = P.media_info(source)
        if (info["width"], info["height"]) != expected_size:
            sys.exit(
                f"--resolution-video must be {expected_size[0]}x{expected_size[1]}, "
                f"got {info['width']}x{info['height']}"
            )
        if abs(P.frame_rate(info) - 30.0) > 0.01:
            sys.exit(
                f"--resolution-video must be 30 fps, got "
                f"{P.frame_rate(info):.2f} fps"
            )
        matrix = build_resolution_matrix(args.resolution_label, source)
    else:
        videos = sorted(P.VIDEO_DIR.glob("*.mp4"))
        stills = sorted(
            list(P.STILL_DIR.glob("*.webp")) + list(P.STILL_DIR.glob("*.png"))
        )
        if len(videos) < 2 or len(stills) < 2:
            sys.exit("need >=2 videos and >=2 stills")
        mid_video, max_video, _ = P.choose_sources(videos)
        floor_videos = P.ensure_floor_videos(max_video, P.FLOOR_SIZE, args.floor_video)
        still, _ = P.choose_stills(stills)
        scene = args.scene or str(
            Path.home()
            / ".local/share/Steam/steamapps/workshop/content/431960/2165290843"
        )
        if not (Path(scene) / "scene.pkg").is_file():
            sys.exit(f"Wallpaper Engine scene is unavailable: {scene}")
        matrix = build_matrix(
            floor_videos["vp9"],
            mid_video,
            max_video,
            still,
            scene,
            yin_floor_video=floor_videos["h264"],
        )
    if args.only:
        terms = [term.casefold() for term in args.only]
        matrix = [
            scenario
            for scenario in matrix
            if all(term in scenario.name.casefold() for term in terms)
        ]
        if not matrix:
            sys.exit(f"no competitor scenarios match {args.only}")
    if args.engine:
        engines = set(args.engine)
        matrix = [scenario for scenario in matrix if scenario.engine in engines]
        if not matrix:
            sys.exit(f"no competitor scenarios match engines {args.engine}")
    if args.workload:
        workloads = set(args.workload)
        matrix = [scenario for scenario in matrix if scenario.workload in workloads]
        if not matrix:
            sys.exit(f"no competitor scenarios match workloads {args.workload}")
    plan_hash = matrix_id(matrix, single_output)
    profile_name = "quick" if args.quick else "thorough"
    profile = dict(PROFILES[profile_name])
    if args.window:
        profile["window"] = args.window
    payload = {
        "schema": 3,
        "matrix_version": MATRIX_VERSION,
        "matrix_id": plan_hash,
        "scenario_count": len(matrix),
        "profile": profile_name,
        "single_output": single_output,
        "resolution_label": args.resolution_label,
        "baseline": {
            "preexisting_wallpaper_engines": "terminated before sampling",
            "scenario_cleanup": "process and layer cleanup verified before every baseline",
            "compositor_samples": COMPOSITOR_STABLE_SAMPLES,
            "compositor_sample_interval_seconds": COMPOSITOR_STABLE_INTERVAL,
            "compositor_stability_tolerance_mib": COMPOSITOR_STABILITY_TOLERANCE,
            "compositor_recovery_tolerance_mib": COMPOSITOR_RECOVERY_TOLERANCE,
            "gpu_floor_drift_limit_points": GPU_FLOOR_DRIFT_LIMIT,
            "power_floor_drift_limit_watts": POWER_FLOOR_DRIFT_LIMIT,
            "compositor_cpu_floor_drift_limit_points": (
                COMPOSITOR_CPU_FLOOR_DRIFT_LIMIT
            ),
        },
        "competitor_compatibility": {
            "kacau_library_path": KACAU_LIBRARY_PATH,
            "yin_library_path": YIN_LIBRARY_PATH,
        },
        "visual_validation": {
            "enabled": VALIDATE_MOTION,
            "samples": VIDEO_VALIDATION_SAMPLES if VALIDATE_MOTION else 0,
            "interval_seconds": VIDEO_VALIDATION_INTERVAL if VALIDATE_MOTION else None,
            "crop": "center 75%",
            "minimum_mean_signal": MIN_VIDEO_MEAN_SIGNAL,
            "minimum_signal_deviation": MIN_VIDEO_SIGNAL_DEVIATION,
            "requires_empty_active_workspace": VALIDATE_MOTION,
            "validation_workspaces": VALIDATION_WORKSPACES,
        },
        "active_outputs": outputs_info,
        "compositor": {"name": P.COMPOSITOR_NAME},
        "scenarios": [scenario.metadata(single_output) for scenario in matrix],
    }
    if args.plan:
        print(json.dumps(payload, indent=2))
        return
    ensure_available(matrix)
    ensure_quiet()
    compositor_pid = P.require_compositor(P.COMPOSITOR_NAME)
    payload["compositor"]["pid"] = compositor_pid
    payload["compositor"]["process_name"] = P.process_comm(compositor_pid)
    validation_outputs = sorted(
        {
            output
            for scenario in matrix
            if requires_motion_validation(scenario)
            for output in scenario_media_outputs(scenario, single_output, output_names)
        }
    )
    if VALIDATE_MOTION and validation_outputs:
        warm_validation_capture(validation_outputs)
        payload["visual_validation"]["capture_warmed_before_sampling"] = True
    floors = []
    results = []
    discarded_attempts = []
    print(
        f"competitor matrix {plan_hash}: {len(matrix)} scenarios, {profile_name}, "
        f"single={single_output}"
    )
    for scenario in matrix:
        for attempt in range(1, SCENARIO_ATTEMPTS + 1):
            activate_validation_baseline(
                scenario_media_outputs(scenario, single_output, output_names)
            )
            floor_before, compositor_before = clean_compositor_baseline(
                compositor_pid, profile
            )
            floors.append(floor_before)
            print(
                f"  [clean baseline: gpu {floor_before[0]:.1f}% "
                f"{floor_before[1]:.1f}W niri {floor_before[2]:.1f}% "
                f"rss {compositor_before['snapshot']['rss']} MiB]"
            )
            result = run_scenario(
                scenario,
                floor_before,
                profile,
                single_output,
                output_names,
                compositor_pid,
                compositor_before,
            )
            recovery_error = None
            try:
                ensure_quiet()
                compositor_after = stable_compositor_snapshot(
                    compositor_pid, compositor_before["snapshot"]
                )
                floor_after = P.idle_floor(profile, compositor_pid)
                floors.append(floor_after)
                finalize_baseline_metrics(
                    result,
                    compositor_before,
                    compositor_after,
                    floor_before,
                    floor_after,
                )
            except RuntimeError as error:
                recovery_error = str(error)
            if recovery_error and attempt < SCENARIO_ATTEMPTS:
                discarded_attempts.append(
                    {
                        "scenario": scenario.name,
                        "attempt": attempt,
                        "reason": f"baseline recovery failed: {recovery_error}",
                    }
                )
                print(
                    f"  [discarded attempt {attempt}/{SCENARIO_ATTEMPTS}: "
                    f"{recovery_error}]"
                )
                continue
            if recovery_error:
                previous = result.get("error")
                result["error"] = (
                    f"{previous}; baseline recovery failed: {recovery_error}"
                    if previous
                    else f"baseline recovery failed: {recovery_error}"
                )
            break
        results.append(result)
        if "error" in result:
            print(f"{scenario.name:<42} ERROR: {result['error']}")
        else:
            compositor = result["compositor"]
            print(
                f"{scenario.name:<42} cpu={result['cpu']:>5}% "
                f"gpu={result['gpu_mean']:>5}% gpuD={result['gpu_delta']:>5}% "
                f"W={result['power_mean']:>5} WD={result['power_delta']:>6} "
                f"rss={result['rss']:>4} pss={result['pss']:>4} vram={result['vram']:>4} "
                f"niriC={compositor['cpu']:>5}% "
                f"niriD={compositor['cpu_delta']:>+5}% "
                f"niriR={compositor['rss_delta']:>+4} "
                f"niriP={compositor['pss_delta']:>+4} "
                f"niriV={compositor['vram_delta']:>+4} "
                f"combinedP={result['combined_pss']:>4}"
            )
    qualifications = [
        result["baseline"]["qualified"]
        for result in results
        if "baseline" in result
    ]
    taints = {
        metric: any(not qualification[metric] for qualification in qualifications)
        for metric in ("compositor_memory", "gpu", "power", "compositor_cpu")
    }
    if any("baseline recovery failed" in result.get("error", "") for result in results):
        taints["compositor_memory"] = True
    payload.update(
        {
            "time": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "floors": floors,
            "discarded_attempts": discarded_attempts,
            "taints": taints,
            "tainted": any(taints.values()),
            "results": results,
        }
    )
    Path(args.out).write_text(json.dumps(payload, indent=1))
    print(f"written: {args.out}")
    if any("error" in result for result in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
