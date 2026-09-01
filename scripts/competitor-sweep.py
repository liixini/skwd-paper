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
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PERF_PATH = ROOT / "scripts" / "perf-sweep.py"
VK = ROOT / "target" / "release" / "skwd-wall-vk"
STILL = ROOT / "target" / "release" / "skwd-wall-still"
MATRIX_VERSION = 10
PHONTO = "phonto"
WPAPERD = "wpaperd"
HYPRPAPER = "hyprpaper"
LIVE_PAPER = "live-paper"
KACAU = "kacau-wall"
TINIER = ROOT / "target" / "release" / "skwd-paper-tinier"
TINIER_CACHE = Path.home() / ".cache/skwd-paper-v2/engine-sweep"
KACAU_REVISION = "f84e70c48238efd9e9cd3b23dfe8f565b873f1e9"
VALIDATE_MOTION = False
YIN = "yin"
YINCTL = "yinctl"
YIN_HOME = None
PROFILES = {
    "quick": {"settle": 2.0, "window": 2.0, "hz": 5},
    "thorough": {"settle": 5.0, "window": 10.0, "hz": 2},
}


def load_perf_module():
    spec = importlib.util.spec_from_file_location("skwd_perf_sweep", PERF_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


P = load_perf_module()


class Scenario:
    def __init__(self, name, engine, workload, topology, source, fps=None, tuned=False):
        self.name = name
        self.engine = engine
        self.workload = workload
        self.topology = topology
        self.source = str(source)
        self.fps = fps
        self.tuned = tuned

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
        }
        if self.engine == "kacau-wall":
            metadata["revision"] = KACAU_REVISION
            metadata["video_path"] = (
                f"cached Vulkan Video ({self.fps} fps "
                f"{'matched' if self.tuned else 'default'})"
                if self.workload == "video"
                else "cached image"
            )
        if self.engine == "live-paper":
            metadata["output_targeting"] = "compositor default after niri focus-monitor"
        return metadata


def tinier_fixtures():
    entries = {
        "floor": (TINIER_CACHE / "matrix-floor-1366x768-tokyo-qp20.ivf", 30),
        "mid": (TINIER_CACHE / "matrix-mid-2560x1440-5mGuCdlCcNM-qp20.ivf", 60),
        "max": (TINIER_CACHE / "matrix-max-3840x2160-tokyo-qp20.ivf", 30),
    }
    return {k: (str(v), fps) for k, (v, fps) in entries.items() if v.exists()}


def build_matrix(
    floor_video, mid_video, max_video, still, scene, yin_floor_video=None, tinier=None
):
    yin_floor_video = yin_floor_video or floor_video
    tinier = tinier or {}
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
            if topology == "single":
                matrix.append(
                    Scenario(
                        f"video {label} single live-paper default",
                        "live-paper",
                        "video",
                        topology,
                        source,
                        tuned=True,
                    )
                )
            if label in tinier:
                matrix.append(
                    Scenario(
                        f"video {label} {topology} skwd-tinier",
                        "skwd-paper-tinier",
                        "video",
                        topology,
                        tinier[label][0],
                        fps=tinier[label][1],
                    )
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


def scenario_command(scenario, single_output, outputs):
    output = single_output if scenario.topology == "single" else "*"
    if scenario.engine == "skwd-wall-still":
        return [str(STILL), output, scenario.source, "--persist"]
    if scenario.engine == "skwd-paper-tinier":
        command = [str(TINIER)]
        if scenario.topology == "single":
            command.extend(["--output", single_output])
        command.extend([scenario.source, str(scenario.fps)])
        return command
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
    if scenario.engine == "live-paper":
        return [LIVE_PAPER, scenario.source]
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
    raise RuntimeError(f"Kacau did not write validation frame {path.name}")


def validate_kacau_motion(outputs, workdir):
    output_hashes = {}
    for output in outputs:
        hashes = []
        safe_output = output.replace("/", "_")
        for index in range(2):
            frame = workdir / f"motion-{safe_output}-{index}.ppm"
            captured = subprocess.run(
                ["grim", "-o", output, "-t", "ppm", str(frame)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=20,
            )
            if captured.returncode != 0:
                raise RuntimeError(captured.stderr.strip() or "Kacau screenshot failed")
            wait_for_stable_file(frame)
            hashes.append(hashlib.sha256(frame.read_bytes()).hexdigest())
            time.sleep(0.5)
        if hashes[0] == hashes[1]:
            raise RuntimeError(
                f"Kacau video validation captured two identical frames on {output}"
            )
        output_hashes[output] = [value[:16] for value in hashes]
    return output_hashes


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


def write_live_paper_config(workdir):
    config = workdir / "live-paper.toml"
    config.write_text(
        'backend = "mpv"\n'
        "\n[player]\n"
        "mute = true\n"
        'hwdec = "auto"\n'
        "fill = true\n"
        "\n[player.mpv_options]\n"
        'ao = "null"\n'
        "\n[layer]\n"
        'layer = "background"\n'
        "exclusive_zone = -1\n"
        "\n[pause]\n"
        "on_fullscreen = false\n"
        "on_maximized = false\n"
        "on_gamemode = false\n"
        "on_screen_off = false\n"
    )
    return config


def focus_niri_output(output):
    result = subprocess.run(
        ["niri", "msg", "action", "focus-monitor", output],
        capture_output=True,
        text=True,
        timeout=5,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or f"could not focus {output}")


def launch_scenario(scenario, single_output, outputs, workdir):
    env = dict(os.environ)
    env["XDG_CACHE_HOME"] = str(workdir / "cache")
    if scenario.engine == "yin":
        home = Path(YIN_HOME) if YIN_HOME else workdir / "home"
        home.mkdir(parents=True, exist_ok=True)
        env["HOME"] = str(home)
        remove_stale_yin_socket()
    elif scenario.engine == "kacau-wall":
        home = workdir / "home"
        home.mkdir(parents=True, exist_ok=True)
        env["HOME"] = str(home)
        env["XDG_CONFIG_HOME"] = str(workdir / "config")
        remove_stale_kacau_socket(kacau_socket(env))
    elif scenario.engine == "wpaperd":
        env["XDG_STATE_HOME"] = str(workdir / "state")
    if scenario.workload == "we" and scenario.engine == "skwd-wall-vk":
        env["SKWD_PAPER_WE_FPS"] = str(scenario.fps)
    if scenario.engine == "live-paper":
        focus_niri_output(single_output)
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
    elif scenario.engine == "live-paper":
        command.extend(["--config-path", str(write_live_paper_config(workdir))])
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
            error_log.close()
            raise RuntimeError("Yin daemon did not create /tmp/yin")
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
            error_log.close()
            raise RuntimeError("Kacau daemon did not create its IPC socket")

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
        if scenario.workload == "video" and VALIDATE_MOTION:
            try:
                # Use the configured single-output monitor as the stable probe for
                # both topologies. The caller must leave its wallpaper unobstructed.
                extra["motion_validation_output"] = single_output
                extra["motion_frame_hashes"] = validate_kacau_motion(
                    [single_output], workdir
                )
            except RuntimeError as error:
                fatal_log = fatal_engine_log(error_log)
                stop_process(proc)
                error_log.close()
                if fatal_log:
                    raise RuntimeError(
                        f"{error}; engine log contains: {fatal_log}"
                    ) from error
                raise
    return proc, error_log, extra


def error_tail(error_log):
    error_log.flush()
    error_log.seek(0)
    return error_log.read().decode(errors="replace")[-2000:].strip()


def rendered_outputs(engine):
    namespace = {
        "skwd-wall-vk": "skwd-wall-vk",
        "mpvpaper": "mpvpaper",
        "live-paper": "live-paper",
        "phonto": "phonto",
        "hyprpaper": "hyprpaper",
        "yin": "yin-wallpaper",
    }.get(engine)
    namespace_prefix = {
        "kacau-wall": "kacau:wall:",
        "wpaperd": "wpaperd-",
    }.get(engine)
    if namespace is None and namespace_prefix is None:
        return None
    try:
        result = subprocess.run(
            ["niri", "msg", "-j", "layers"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        layers = json.loads(result.stdout) if result.returncode == 0 else []
    except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError):
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


def run_scenario(scenario, floor, profile, single_output, outputs, compositor_pid):
    metadata = {
        **scenario.metadata(single_output),
        "idle_gpu": round(floor[0], 1),
        "idle_power": round(floor[1], 1),
    }
    compositor_before = P.process_snapshot(compositor_pid)
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
                if scenario.engine in ("yin", "kacau-wall")
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
            compositor_after = P.process_snapshot(compositor_pid)
            compositor = P.compositor_metrics(
                P.COMPOSITOR_NAME,
                compositor_pid,
                compositor_before,
                compositor_after,
                compositor_cpu_start,
                compositor_cpu_end,
                elapsed,
                floor[2],
            )
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
    if "live-paper" in engines:
        required.add(LIVE_PAPER)
    if "skwd-paper-tinier" in engines and not Path(TINIER).is_file():
        sys.exit(f"missing skwd-paper-tinier build at {TINIER}")
    if "wpaperd" in engines:
        required.add(WPAPERD)
    if "hyprpaper" in engines:
        required.add(HYPRPAPER)
    if "linux-wallpaperengine" in engines:
        required.add("linux-wallpaperengine")
    if "phonto" in engines:
        required.add(PHONTO)
    if "kacau-wall" in engines:
        required.add(KACAU)
        if VALIDATE_MOTION:
            required.add("grim")
    if "yin" in engines:
        required.update((YIN, YINCTL))
    missing = [command for command in sorted(required) if not executable_available(command)]
    if missing:
        sys.exit(f"missing competitor tools: {', '.join(missing)}")
    if {"skwd-wall-vk", "skwd-wall-still"} & engines:
        P.require_shared_device_binary()


def ensure_quiet():
    names = (
        "skwd-wall-vk",
        "skwd-wall-still",
        "awww-daemon",
        "wpaperd",
        "hyprpaper",
        "phonto",
        "mpvpaper",
        "live-paper",
        "linux-wallpaper",
        "yin",
        "kacau-wall",
    )
    running = []
    for name in names:
        result = subprocess.run(["pgrep", "-x", name], capture_output=True, text=True)
        if result.returncode == 0:
            running.append(name)
    if running:
        sys.exit(f"wallpaper engines already running: {', '.join(running)}")


def matrix_id(matrix, single_output):
    payload = [scenario.metadata(single_output) for scenario in matrix]
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()[:16]


def main():
    global PHONTO, WPAPERD, HYPRPAPER, LIVE_PAPER, KACAU, VALIDATE_MOTION
    global YIN, YINCTL, YIN_HOME
    parser = argparse.ArgumentParser()
    parser.add_argument("--quick", action="store_true")
    parser.add_argument("--window", type=float)
    parser.add_argument("--out", default=str(ROOT / "competitor-sweep-latest.json"))
    parser.add_argument("--single-output")
    parser.add_argument("--floor-video")
    parser.add_argument("--scene")
    parser.add_argument(
        "--phonto",
        default=PHONTO,
        help="Phonto executable (default: resolve phonto from PATH)",
    )
    parser.add_argument("--wpaperd", default=WPAPERD, help="wpaperd executable")
    parser.add_argument(
        "--hyprpaper", default=HYPRPAPER, help="hyprpaper executable"
    )
    parser.add_argument(
        "--live-paper", default=LIVE_PAPER, help="live-paper executable"
    )
    parser.add_argument("--kacau", default=KACAU, help="Kacau Wall executable")
    parser.add_argument(
        "--validate-motion",
        action="store_true",
        help="Capture the Kacau probe output twice and require changing video frames",
    )
    parser.add_argument("--yin", default=YIN, help="Yin daemon executable")
    parser.add_argument("--yinctl", default=YINCTL, help="Yin client executable")
    parser.add_argument(
        "--yin-home",
        help="Persistent HOME for pre-warmed Yin output-sized video caches",
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
    LIVE_PAPER = args.live_paper
    KACAU = args.kacau
    VALIDATE_MOTION = args.validate_motion
    YIN = args.yin
    YINCTL = args.yinctl
    YIN_HOME = args.yin_home
    outputs_info = P.active_outputs()
    if not outputs_info:
        sys.exit("could not discover active outputs")
    ranked = P.ranked_outputs(outputs_info, "")
    output_names = [output["name"] for output in ranked]
    single_output = args.single_output or ranked[0]["name"]
    if single_output not in output_names:
        sys.exit(f"unknown single output {single_output}; available: {', '.join(output_names)}")
    videos = sorted(P.VIDEO_DIR.glob("*.mp4"))
    stills = sorted(list(P.STILL_DIR.glob("*.webp")) + list(P.STILL_DIR.glob("*.png")))
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
        tinier=tinier_fixtures(),
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
        "schema": 2,
        "matrix_version": MATRIX_VERSION,
        "matrix_id": plan_hash,
        "scenario_count": len(matrix),
        "profile": profile_name,
        "single_output": single_output,
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
    floor = P.idle_floor(profile, compositor_pid)
    floors = [floor]
    results = []
    print(
        f"competitor matrix {plan_hash}: {len(matrix)} scenarios, {profile_name}, "
        f"single={single_output}, idle={floor[0]:.1f}%/{floor[1]:.1f}W/"
        f"niri {floor[2]:.1f}%"
    )
    for index, scenario in enumerate(matrix):
        if index and (profile_name == "thorough" or index % 4 == 0):
            time.sleep(2.0)
            floor = P.idle_floor(profile, compositor_pid)
            floors.append(floor)
            print(
                f"  [idle re-sample: gpu {floor[0]:.1f}% {floor[1]:.1f}W "
                f"niri {floor[2]:.1f}%]"
            )
        result = run_scenario(
            scenario,
            floor,
            profile,
            single_output,
            output_names,
            compositor_pid,
        )
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
    drift = max(value[0] for value in floors) - min(value[0] for value in floors)
    payload.update(
        {
            "time": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "floors": floors,
            "tainted": drift > 10.0,
            "results": results,
        }
    )
    Path(args.out).write_text(json.dumps(payload, indent=1))
    print(f"written: {args.out}")
    if any("error" in result for result in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
