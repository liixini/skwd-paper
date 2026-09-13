#!/usr/bin/env python3
"""Matched live performance sweep of Plasma-native wallpaper engines and the Skwd Paper Plasma bridge."""

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PERF_PATH = ROOT / "scripts" / "perf-sweep.py"
COMPETITOR_PATH = ROOT / "scripts" / "competitor-sweep.py"
MATRIX_VERSION = 1
PAPER_PLUGIN = "org.skwd.wall.plasma"
IMAGE_PLUGIN = "org.kde.image"
COLOR_PLUGIN = "org.kde.color"
SMART_VIDEO_PLUGIN = "luisbocanegra.smart.video.wallpaper.reborn"
SMART_VIDEO_PAUSE_NEVER = 3
SMART_VIDEO_BLUR_NEVER = 5
SMART_VIDEO_MUTE_ALWAYS = 5
IMAGE_FILL_STRETCH_CROP = 2
BASELINE_COLOR = "#101418"
PAPER = None
LAYER_VK = None
LAYER_STILL = None
VALIDATE_MOTION = False
PROFILES = {
    "quick": {"settle": 2.0, "window": 2.0, "hz": 5},
    "thorough": {"settle": 5.0, "window": 10.0, "hz": 2},
}
HOSTS = ("plasmashell", "kwin_wayland")
HOST_STABILITY_TOLERANCE = {"rss": 2, "pss": 2, "vram": 0}
HOST_RECOVERY_TOLERANCE = {
    "plasmashell": {"rss": 24, "pss": 24, "vram": 32},
    "kwin_wayland": {"rss": 8, "pss": 8, "vram": 16},
}
HOST_STABLE_SAMPLES = 5
HOST_STABLE_INTERVAL = 0.5
HOST_STABLE_TIMEOUT = 30.0
GPU_FLOOR_DRIFT_LIMIT = 1.0
POWER_FLOOR_DRIFT_LIMIT = 2.0
HOST_CPU_FLOOR_DRIFT_LIMIT = 2.0
SCENARIO_ATTEMPTS = 3
RENDERER_TIMEOUT = 30.0
PLAN_OUTPUTS = (
    {
        "name": "DP-3",
        "width": 1920,
        "height": 1080,
        "logical_width": 1920,
        "logical_height": 1080,
        "scale": 1.0,
        "x": 0,
        "y": 0,
        "refresh_mhz": 60000,
    },
)
STRAY_PROCESS_NAMES = (
    "skwd-paper",
    "skwd-paper-v2",
    "skwd-wall-vk",
    "skwd-wall-still",
    "skwd-paper-tini",
    "linux-wallpaper",
)


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


P = load_module("skwd_perf_sweep", PERF_PATH)
C = load_module("skwd_competitor_sweep", COMPETITOR_PATH)


class Scenario:
    def __init__(self, name, engine, workload, topology, source, fps=None, extra=None):
        self.name = name
        self.engine = engine
        self.workload = workload
        self.topology = topology
        self.source = str(source)
        self.fps = fps
        self.extra = extra or {}

    @property
    def in_process(self):
        return self.engine in {"plasma-image", "smart-video-reborn"}

    @property
    def bridged(self):
        return self.engine == "skwd-paper-plasma"

    def metadata(self, single_output):
        source = (
            {"path": self.source, "scene_id": Path(self.source).name}
            if self.workload == "we"
            else P.media_info(self.source)
        )
        return {
            "name": self.name,
            "engine": self.engine,
            "workload": self.workload,
            "topology": self.topology,
            "output": single_output if self.topology == "single" else "*",
            "source": source,
            "fps_cap": self.fps,
            "renders_inside_plasmashell": self.in_process,
            **self.extra,
        }


def build_matrix(floor_video, mid_video, max_video, still, scene):
    matrix = []
    for topology in ("single", "all"):
        matrix.extend(
            [
                Scenario(f"static {topology} plasma-image", "plasma-image", "static", topology, still),
                Scenario(
                    f"static {topology} skwd-paper-plasma",
                    "skwd-paper-plasma",
                    "static",
                    topology,
                    still,
                ),
                Scenario(
                    f"static {topology} skwd-paper-layer",
                    "skwd-paper-layer",
                    "static",
                    topology,
                    still,
                ),
            ]
        )
        for label, source in (("floor", floor_video), ("mid", mid_video), ("max", max_video)):
            matrix.extend(
                [
                    Scenario(
                        f"video {label} {topology} smart-video-reborn",
                        "smart-video-reborn",
                        "video",
                        topology,
                        source,
                    ),
                    Scenario(
                        f"video {label} {topology} skwd-paper-plasma",
                        "skwd-paper-plasma",
                        "video",
                        topology,
                        source,
                        fps=30,
                    ),
                    Scenario(
                        f"video {label} {topology} skwd-paper-layer",
                        "skwd-paper-layer",
                        "video",
                        topology,
                        source,
                    ),
                ]
            )
        for fps in (30, 60):
            matrix.extend(
                [
                    Scenario(
                        f"we {fps}fps {topology} skwd-paper-plasma",
                        "skwd-paper-plasma",
                        "we",
                        topology,
                        scene,
                        fps=fps,
                    ),
                    Scenario(
                        f"we {fps}fps {topology} skwd-paper-layer",
                        "skwd-paper-layer",
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


def plasma_script(source):
    result = subprocess.run(
        [
            "qdbus6",
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell.evaluateScript",
            source,
        ],
        capture_output=True,
        text=True,
        timeout=20,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "Plasma scripting failed")
    return result.stdout.strip()


def parse_kscreen_outputs(text):
    outputs = []
    for output in json.loads(text).get("outputs", []):
        if not output.get("connected", True) or output.get("enabled") is False:
            continue
        name = output.get("name")
        pos = output.get("pos") or {}
        modes = {mode.get("id"): mode for mode in output.get("modes", [])}
        mode = modes.get(output.get("currentModeId")) or {}
        size = mode.get("size") or output.get("size") or {}
        scale = float(output.get("scale") or 1.0)
        width = int(size.get("width") or 0)
        height = int(size.get("height") or 0)
        if not name or not width or not height:
            continue
        outputs.append(
            {
                "name": name,
                "width": width,
                "height": height,
                "logical_width": int(round(width / scale)),
                "logical_height": int(round(height / scale)),
                "scale": scale,
                "x": int(pos.get("x", 0)),
                "y": int(pos.get("y", 0)),
                "refresh_mhz": int(round(float(mode.get("refreshRate") or 0) * 1000)),
            }
        )
    return sorted(outputs, key=lambda output: output["name"])


def plasma_outputs():
    result = subprocess.run(
        ["kscreen-doctor", "-j"], capture_output=True, text=True, timeout=10
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "kscreen-doctor failed")
    return parse_kscreen_outputs(result.stdout)


def desktop_geometries():
    output = plasma_script(
        "var rows=[];for(var i=0;i<screenCount;i++){var g=screenGeometry(i);var d=desktopForScreen(i);"
        "rows.push({screen:i,id:d?d.id:null,x:g.x,y:g.y,width:g.width,height:g.height});}"
        "print(JSON.stringify(rows));"
    )
    return json.loads(output.splitlines()[-1])


def screen_map(outputs, geometries):
    by_position = {(geometry["x"], geometry["y"]): geometry for geometry in geometries}
    mapping = {}
    for output in outputs:
        geometry = by_position.get((output["x"], output["y"]))
        if geometry is None or geometry.get("id") is None:
            raise RuntimeError(f"no Plasma desktop at the position of {output['name']}")
        output["logical_width"] = geometry["width"]
        output["logical_height"] = geometry["height"]
        mapping[output["name"]] = geometry["id"]
    return mapping


def color_config():
    return {"plugin": COLOR_PLUGIN, "config": {"Color": BASELINE_COLOR}}


def image_config(still):
    return {
        "plugin": IMAGE_PLUGIN,
        "config": {
            "Image": Path(still).resolve().as_uri(),
            "FillMode": IMAGE_FILL_STRETCH_CROP,
        },
    }


def smart_video_config(video):
    entry = {
        "filename": Path(video).resolve().as_uri(),
        "enabled": True,
        "duration": 0,
        "customDuration": 0,
        "playbackRate": 0.0,
        "alternativePlaybackRate": 0.0,
        "loop": True,
    }
    return {
        "plugin": SMART_VIDEO_PLUGIN,
        "config": {
            "VideoUrls": json.dumps([entry]),
            "PauseMode": SMART_VIDEO_PAUSE_NEVER,
            "BlurMode": SMART_VIDEO_BLUR_NEVER,
            "MuteMode": SMART_VIDEO_MUTE_ALWAYS,
            "CheckWindowsActiveScreen": False,
            "ScreenOffPausesVideo": False,
            "BatteryPausesVideo": False,
            "CrossfadeEnabled": False,
            "RandomMode": False,
            "ResumeLastVideo": False,
            "ChangeWallpaperMode": 0,
        },
    }


def paper_assignment(scenario, output):
    kind = {"static": "static", "video": "video", "we": "we"}[scenario.workload]
    source = {"kind": kind, "path": scenario.source}
    if kind == "video":
        source["engine"] = "default"
    return {
        "assignment": {
            "outputs": [output["name"]],
            "source": source,
            "mute": True,
            "volume": 0,
        },
        "paper": PAPER,
        "width": max(16, output["logical_width"]),
        "height": max(16, output["logical_height"]),
        "fps": scenario.fps or 30,
        "paused": False,
        "manualPaused": False,
    }


def paper_config(scenario, targets):
    assignments = {output["name"]: paper_assignment(scenario, output) for output in targets}
    return {"plugin": PAPER_PLUGIN, "config": {"Assignments": json.dumps(assignments)}}


def scenario_targets(scenario, single_output, outputs):
    if scenario.topology == "single":
        return [output for output in outputs if output["name"] == single_output]
    return list(outputs)


def desktop_assignments(scenario, single_output, outputs, mapping):
    assignments = {str(screen): color_config() for screen in mapping.values()}
    targets = scenario_targets(scenario, single_output, outputs)
    if scenario.engine == "plasma-image":
        wanted = image_config(scenario.source)
    elif scenario.engine == "smart-video-reborn":
        wanted = smart_video_config(scenario.source)
    elif scenario.engine == "skwd-paper-plasma":
        wanted = paper_config(scenario, targets)
    else:
        return assignments
    for output in targets:
        assignments[str(mapping[output["name"]])] = wanted
    return assignments


def apply_assignments(assignments):
    payload = json.dumps(assignments)
    plasma_script(
        f"var assignments = {payload};"
        "desktops().forEach(function(d) {"
        "var wanted = assignments[String(d.id)];"
        "if (!wanted) return;"
        f'd.wallpaperPlugin = "{COLOR_PLUGIN}";'
        'd.currentConfigGroup = ["Wallpaper", wanted.plugin, "General"];'
        "Object.keys(wanted.config).forEach(function(key) {"
        "d.writeConfig(key, wanted.config[key]);"
        "});"
        "d.wallpaperPlugin = wanted.plugin;"
        "});"
    )


def snapshot_desktops(keys):
    output = plasma_script(
        f"var keys = {json.dumps(sorted(keys))};"
        "print(JSON.stringify(desktops().map(function(d) {"
        "var plugin = d.wallpaperPlugin;"
        'd.currentConfigGroup = ["Wallpaper", plugin, "General"];'
        "var config = {};"
        'keys.forEach(function(key) { var v = d.readConfig(key, ""); if (v !== "") config[key] = v; });'
        "return {id: d.id, plugin: plugin, config: config};"
        "})));"
    )
    return json.loads(output.splitlines()[-1])


def restore_desktops(saved):
    apply_assignments(
        {str(item["id"]): {"plugin": item["plugin"], "config": item["config"]} for item in saved}
    )


def layer_command(scenario, single_output, outputs):
    output = single_output if scenario.topology == "single" else "*"
    if scenario.engine == "skwd-paper-layer":
        if scenario.workload == "static":
            return [LAYER_STILL, output, scenario.source, "--fill-mode", "fill", "--persist"]
        command = [LAYER_VK, output, scenario.source, "--mute"]
        if scenario.workload == "we":
            command.extend(["--scene", scenario.source])
        return command
    if scenario.engine == "linux-wallpaperengine":
        selected = [single_output] if scenario.topology == "single" else [o["name"] for o in outputs]
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
    raise ValueError(f"{scenario.engine} does not run as a layer-shell client")


def process_table(proc_root=Path("/proc")):
    table = []
    for entry in proc_root.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            cmdline = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
        except (OSError, IndexError):
            continue
        table.append((int(entry.name), int(stat[1]), cmdline.strip()))
    return table


def bridge_roots(table, plasma_pid):
    return sorted(
        pid
        for pid, parent, cmdline in table
        if parent == plasma_pid and ("--video-stream" in cmdline or "--frame-stream" in cmdline)
    )


def bridge_streams(table, plasma_pid):
    streams = 0
    for pid, parent, cmdline in table:
        if parent != plasma_pid or ("--video-stream" not in cmdline and "--frame-stream" not in cmdline):
            continue
        streams += max(1, cmdline.count("--stream-fd "), cmdline.count("--frame-stream "))
    return streams


def wait_bridge_roots(plasma_pid, expected, timeout=RENDERER_TIMEOUT):
    deadline = time.monotonic() + timeout
    streams = 0
    while time.monotonic() < deadline:
        table = process_table()
        streams = bridge_streams(table, plasma_pid)
        if streams == expected:
            return bridge_roots(table, plasma_pid)
        time.sleep(0.25)
    raise RuntimeError(f"expected {expected} Plasma bridge streams, found {streams}")


def bridge_tree(roots):
    pids = set()
    for root in roots:
        pids.update(C.process_tree(root))
    return sorted(pids)


def zombie(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] == "Z"
    except (OSError, IndexError):
        return True


def stray_processes():
    processes = {}
    for name in STRAY_PROCESS_NAMES:
        pids = [pid for pid in P.process_pids(name) if not zombie(pid)]
        if pids:
            processes[name] = pids
    return processes


def kill_strays(timeout=10.0):
    deadline = time.monotonic() + timeout
    processes = stray_processes()
    if processes:
        C.signal_wallpaper_processes(processes, signal.SIGTERM)
    while processes and time.monotonic() < deadline:
        time.sleep(0.1)
        processes = stray_processes()
    if processes:
        C.signal_wallpaper_processes(processes, signal.SIGKILL)
        deadline = time.monotonic() + 3.0
        while processes and time.monotonic() < deadline:
            time.sleep(0.1)
            processes = stray_processes()
    if processes:
        detail = ", ".join(f"{name}={','.join(map(str, pids))}" for name, pids in processes.items())
        raise RuntimeError(f"wallpaper processes survived cleanup: {detail}")


def host_pids():
    return {name: P.require_compositor(name) for name in HOSTS}


def host_recovered(name, before, after):
    tolerance = HOST_RECOVERY_TOLERANCE[name]
    return all(abs(after[metric] - before[metric]) <= tolerance[metric] for metric in tolerance)


def stable_host_snapshot(name, pid, reference=None):
    deadline = time.monotonic() + HOST_STABLE_TIMEOUT
    samples = []
    latest = None
    while time.monotonic() < deadline:
        samples.append(P.process_snapshot(pid))
        samples = samples[-HOST_STABLE_SAMPLES:]
        if len(samples) == HOST_STABLE_SAMPLES:
            latest = C.summarize_snapshots(samples)
            stable = all(
                latest["spread"][metric] <= HOST_STABILITY_TOLERANCE[metric]
                for metric in HOST_STABILITY_TOLERANCE
            )
            recovered = reference is None or host_recovered(name, reference, latest["snapshot"])
            if stable and recovered:
                return latest
        time.sleep(HOST_STABLE_INTERVAL)
    detail = latest or C.summarize_snapshots(samples)
    raise RuntimeError(f"{name} memory did not stabilize: {detail}")


def stable_hosts(pids, reference=None):
    return {
        name: stable_host_snapshot(name, pid, None if reference is None else reference[name]["snapshot"])
        for name, pid in pids.items()
    }


def host_cpu(pids):
    return {name: P.proc_cpu(pid) for name, pid in pids.items()}


def idle_floor(profile, pids):
    cpu_start = host_cpu(pids)
    started = time.monotonic()
    utils, powers = [], []
    for _ in range(max(2, int(2 * profile["hz"]))):
        util, power = P.gpu_sample()
        utils.append(util)
        powers.append(power)
        time.sleep(1.0 / profile["hz"])
    elapsed = time.monotonic() - started
    cpu_end = host_cpu(pids)
    cpu = {
        name: round(max(0.0, (cpu_end[name] or 0.0) - (cpu_start[name] or 0.0)) / elapsed * 100.0, 1)
        for name in pids
    }
    return {"gpu": sum(utils) / len(utils), "power": sum(powers) / len(powers), "cpu": cpu}


def clean_baseline(pids, profile, baseline_assignments, attempts=3):
    for _ in range(attempts):
        apply_assignments(baseline_assignments)
        kill_strays()
        hosts = stable_hosts(pids)
        floor = idle_floor(profile, pids)
        if not stray_processes():
            return floor, hosts
    raise RuntimeError("wallpaper processes returned during baseline sampling")


def show_desktop(enabled):
    state = subprocess.run(
        ["qdbus6", "org.kde.KWin", "/KWin", "org.kde.KWin.showingDesktop"],
        capture_output=True,
        text=True,
        timeout=5,
    )
    showing = state.stdout.strip() == "true"
    if showing == enabled:
        return
    subprocess.run(
        [
            "qdbus6",
            "org.kde.kglobalaccel",
            "/component/kwin",
            "org.kde.kglobalaccel.Component.invokeShortcut",
            "Show Desktop",
        ],
        check=True,
        timeout=5,
    )
    time.sleep(1.0)


def capture_output(output, path):
    captured = subprocess.run(
        ["grim", "-o", output, "-t", "ppm", str(path)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=20,
    )
    if captured.returncode != 0:
        raise RuntimeError(captured.stderr.strip() or f"screenshot of {output} failed")
    C.wait_for_stable_file(path)


def validate_motion(outputs, workdir, moving):
    checks = {}
    show_desktop(True)
    try:
        for output in outputs:
            hashes, means, deviations = [], [], []
            samples = C.VIDEO_VALIDATION_SAMPLES if moving else 1
            for index in range(samples):
                frame = workdir / f"motion-{output.replace('/', '_')}-{index}.ppm"
                capture_output(output, frame)
                digest, mean, deviation = C.frame_content_signature(frame)
                hashes.append(digest)
                means.append(mean)
                deviations.append(deviation)
                if index + 1 < samples:
                    time.sleep(C.VIDEO_VALIDATION_INTERVAL)
            unique = len(set(hashes))
            if max(means) < C.MIN_VIDEO_MEAN_SIGNAL or max(deviations) < C.MIN_VIDEO_SIGNAL_DEVIATION:
                raise RuntimeError(
                    f"validation captured blank or near-black content on {output} "
                    f"(mean={max(means):.4f}, deviation={max(deviations):.4f})"
                )
            if moving and unique < 2:
                raise RuntimeError(
                    f"validation captured no motion on {output} ({unique}/{len(hashes)} unique frames)"
                )
            checks[output] = {
                "hashes": [value[:16] for value in hashes],
                "unique_frames": unique,
                "mean_signal": [round(value, 6) for value in means],
                "signal_deviation": [round(value, 6) for value in deviations],
            }
    finally:
        show_desktop(False)
    return checks


def launch_layer(scenario, single_output, outputs, workdir):
    env = dict(os.environ)
    env["XDG_CACHE_HOME"] = str(workdir / "cache")
    if scenario.workload == "we" and scenario.engine == "skwd-paper-layer":
        env["SKWD_PAPER_WE_FPS"] = str(scenario.fps)
    error_log = open(workdir / "stderr.log", "w+b")
    proc = subprocess.Popen(
        layer_command(scenario, single_output, outputs),
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=error_log,
        start_new_session=True,
    )
    return proc, error_log


def host_metrics(pids, before, after, cpu_start, cpu_end, elapsed, floor):
    return {
        name: P.compositor_metrics(
            name,
            pid,
            before[name]["snapshot"],
            after[name]["snapshot"],
            cpu_start[name],
            cpu_end[name],
            elapsed,
            floor["cpu"][name],
        )
        for name, pid in pids.items()
    }


def run_scenario(scenario, floor, profile, single_output, outputs, mapping, pids, hosts_before):
    metadata = {
        **scenario.metadata(single_output),
        "idle_gpu": round(floor["gpu"], 1),
        "idle_power": round(floor["power"], 1),
    }
    targets = [output["name"] for output in scenario_targets(scenario, single_output, outputs)]
    proc = None
    error_log = None
    with tempfile.TemporaryDirectory(prefix="skwd-plasma-") as temporary:
        workdir = Path(temporary)
        try:
            try:
                if scenario.in_process or scenario.bridged:
                    apply_assignments(desktop_assignments(scenario, single_output, outputs, mapping))
                else:
                    proc, error_log = launch_layer(scenario, single_output, outputs, workdir)
                if scenario.bridged:
                    roots = wait_bridge_roots(pids["plasmashell"], len(targets))
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                return {**metadata, "error": str(error)}
            settle = profile["settle"]
            if scenario.workload == "we":
                settle = max(settle, 6.0)
            time.sleep(settle)
            if proc is not None and proc.poll() is not None:
                return {**metadata, "error": C.error_tail(error_log) or "engine died at spawn"}
            if error_log is not None:
                fatal = C.fatal_engine_log(error_log)
                if fatal:
                    return {**metadata, "error": f"engine log contains: {fatal}"}
            if VALIDATE_MOTION:
                try:
                    metadata["motion_validation_outputs"] = targets
                    metadata["motion_frame_hashes"] = validate_motion(
                        targets, workdir, scenario.workload != "static"
                    )
                except (RuntimeError, subprocess.TimeoutExpired) as error:
                    return {**metadata, "error": str(error)}
            if scenario.bridged:
                renderer_pids = bridge_tree(roots)
            elif proc is not None:
                renderer_pids = C.process_tree(proc.pid)
            else:
                renderer_pids = []
            cpu_start = C.cpu_total(renderer_pids)
            hosts_cpu_start = host_cpu(pids)
            start = time.monotonic()
            utils, powers = [], []
            for _ in range(max(1, int(profile["window"] * profile["hz"]))):
                util, power = P.gpu_sample()
                utils.append(util)
                powers.append(power)
                time.sleep(1.0 / profile["hz"])
            elapsed = time.monotonic() - start
            if scenario.bridged:
                renderer_pids = bridge_tree(bridge_roots(process_table(), pids["plasmashell"]))
            elif proc is not None:
                renderer_pids = C.process_tree(proc.pid)
            hosts_cpu_end = host_cpu(pids)
            if proc is not None and proc.poll() is not None:
                return {**metadata, "error": C.error_tail(error_log) or "engine died mid-window"}
            if scenario.bridged and bridge_streams(process_table(), pids["plasmashell"]) != len(targets):
                return {**metadata, "error": "Plasma bridge renderer died mid-window"}
            if any(value is None for value in hosts_cpu_end.values()):
                return {**metadata, "error": "a Plasma host process died mid-window"}
            cpu = max(0.0, C.cpu_total(renderer_pids) - cpu_start) / elapsed * 100.0
            gpu_mean = sum(utils) / len(utils)
            power_mean = sum(powers) / len(powers)
            try:
                hosts_active = stable_hosts(pids)
            except RuntimeError as error:
                return {**metadata, "error": str(error)}
            hosts = host_metrics(
                pids, hosts_before, hosts_active, hosts_cpu_start, hosts_cpu_end, elapsed, floor
            )
            for name in hosts:
                hosts[name]["baseline_samples"] = hosts_before[name]
                hosts[name]["active_samples"] = hosts_active[name]
            renderer_pss = sum(P.proc_pss(pid) for pid in renderer_pids)
            return {
                **metadata,
                "cpu": round(cpu, 1),
                "gpu_mean": round(gpu_mean, 1),
                "gpu_delta": round(gpu_mean - floor["gpu"], 1),
                "gpu_max": max(utils),
                "power_mean": round(power_mean, 1),
                "power_delta": round(power_mean - floor["power"], 1),
                "power_max": round(max(powers), 1),
                "processes": len(renderer_pids),
                "process_names": sorted(filter(None, (C.process_name(pid) for pid in renderer_pids))),
                "rendered_outputs": targets,
                "vram": C.vram_total(renderer_pids),
                "rss": sum(P.proc_rss(pid) for pid in renderer_pids),
                "pss": renderer_pss,
                "combined_pss": renderer_pss + hosts["plasmashell"]["pss_delta"],
                "compositor": hosts["plasmashell"],
                "kwin": hosts["kwin_wayland"],
            }
        finally:
            if proc is not None:
                C.stop_process(proc)
            if error_log is not None:
                error_log.close()
            time.sleep(0.7)


def finalize_host(block, before, after, floor_before, floor_after, name):
    for metric in ("rss", "pss", "vram"):
        baseline = C.midpoint(before["snapshot"][metric], after["snapshot"][metric])
        block[f"{metric}_before"] = before["snapshot"][metric]
        block[f"{metric}_after_cleanup"] = after["snapshot"][metric]
        block[f"{metric}_baseline"] = baseline
        block[f"{metric}_delta"] = round(block[metric] - baseline, 1)
    block["post_cleanup_samples"] = after
    block["cpu_idle_before"] = round(floor_before["cpu"][name], 1)
    block["cpu_idle_after"] = round(floor_after["cpu"][name], 1)
    block["cpu_idle"] = round(C.midpoint(floor_before["cpu"][name], floor_after["cpu"][name]), 1)
    block["cpu_delta"] = round(block["cpu"] - block["cpu_idle"], 1)


def finalize_baseline_metrics(result, before, after, floor_before, floor_after):
    host_cpu_drift = {
        name: round(abs(floor_after["cpu"][name] - floor_before["cpu"][name]), 1) for name in HOSTS
    }
    floor_drift = {
        "gpu": round(abs(floor_after["gpu"] - floor_before["gpu"]), 1),
        "power": round(abs(floor_after["power"] - floor_before["power"]), 1),
        "compositor_cpu": host_cpu_drift["plasmashell"],
        "kwin_cpu": host_cpu_drift["kwin_wayland"],
    }
    qualification = {
        "compositor_memory": all(
            host_recovered(name, before[name]["snapshot"], after[name]["snapshot"]) for name in HOSTS
        ),
        "gpu": floor_drift["gpu"] <= GPU_FLOOR_DRIFT_LIMIT,
        "power": floor_drift["power"] <= POWER_FLOOR_DRIFT_LIMIT,
        "compositor_cpu": host_cpu_drift["plasmashell"] <= HOST_CPU_FLOOR_DRIFT_LIMIT,
        "kwin_cpu": host_cpu_drift["kwin_wayland"] <= HOST_CPU_FLOOR_DRIFT_LIMIT,
    }
    result["baseline"] = {
        "before": before,
        "after_cleanup": after,
        "floor_before": floor_before,
        "floor_after": floor_after,
        "floor_drift": floor_drift,
        "qualified": qualification,
    }
    if "error" in result or "compositor" not in result:
        return result
    finalize_host(
        result["compositor"], before["plasmashell"], after["plasmashell"], floor_before, floor_after, "plasmashell"
    )
    finalize_host(
        result["kwin"], before["kwin_wayland"], after["kwin_wayland"], floor_before, floor_after, "kwin_wayland"
    )
    result["idle_gpu"] = round(C.midpoint(floor_before["gpu"], floor_after["gpu"]), 1)
    result["idle_power"] = round(C.midpoint(floor_before["power"], floor_after["power"]), 1)
    result["gpu_delta"] = round(result["gpu_mean"] - result["idle_gpu"], 1)
    result["power_delta"] = round(result["power_mean"] - result["idle_power"], 1)
    result["combined_pss"] = round(result["pss"] + result["compositor"]["pss_delta"], 1)
    return result


def matrix_id(matrix, single_output):
    payload = [scenario.metadata(single_output) for scenario in matrix]
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()[:16]


def sibling_binary(paper, name):
    candidate = Path(paper).resolve().parent / name
    if candidate.is_file():
        return str(candidate)
    found = shutil.which(name)
    if not found:
        sys.exit(f"missing {name} next to {paper} and on PATH")
    return found


def binary_version(path):
    try:
        result = subprocess.run([path, "--version"], capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None
    text = (result.stdout or result.stderr).strip()
    return text.splitlines()[0] if text else None


def ensure_available(matrix):
    engines = {scenario.engine for scenario in matrix}
    required = {"nvidia-smi", "qdbus6", "kscreen-doctor"}
    if "linux-wallpaperengine" in engines:
        required.add("linux-wallpaperengine")
    if VALIDATE_MOTION:
        required.update(("grim", "magick"))
    missing = [command for command in sorted(required) if not shutil.which(command)]
    if missing:
        sys.exit(f"missing tools: {', '.join(missing)}")
    installed = Path("/usr/share/plasma/wallpapers")
    for engine, plugin in (
        ("skwd-paper-plasma", PAPER_PLUGIN),
        ("smart-video-reborn", SMART_VIDEO_PLUGIN),
        ("plasma-image", IMAGE_PLUGIN),
    ):
        if engine in engines and not any(
            (base / plugin / "metadata.json").is_file()
            for base in (installed, Path.home() / ".local/share/plasma/wallpapers")
        ):
            sys.exit(f"Plasma wallpaper plugin {plugin} is not installed")


def main():
    global PAPER, LAYER_VK, LAYER_STILL, VALIDATE_MOTION
    parser = argparse.ArgumentParser()
    parser.add_argument("--quick", action="store_true")
    parser.add_argument("--window", type=float)
    parser.add_argument("--out", default=str(ROOT / "plasma-sweep-latest.json"))
    parser.add_argument("--single-output")
    parser.add_argument("--floor-video")
    parser.add_argument("--scene")
    parser.add_argument("--paper", help="skwd-paper executable used by the Plasma plugin and layer rows")
    parser.add_argument("--no-validate-motion", action="store_true")
    parser.add_argument("--only", action="append")
    parser.add_argument("--engine", action="append")
    parser.add_argument("--workload", action="append", choices=("static", "video", "we"))
    parser.add_argument("--plan", action="store_true")
    parser.add_argument("--restore", help="re-apply the desktop snapshot a previous run saved and exit")
    args = parser.parse_args()
    if args.restore:
        restore_desktops(json.loads(Path(args.restore).read_text()))
        return
    PAPER = args.paper or shutil.which("skwd-paper-v2") or shutil.which("skwd-paper")
    if not PAPER:
        sys.exit("missing skwd-paper-v2 on PATH; pass --paper")
    LAYER_VK = sibling_binary(PAPER, "skwd-wall-vk")
    LAYER_STILL = sibling_binary(PAPER, "skwd-wall-still")
    VALIDATE_MOTION = not args.no_validate_motion
    on_plasma = os.environ.get("XDG_CURRENT_DESKTOP", "").casefold() == "kde"
    if not on_plasma and not args.plan:
        sys.exit("run this sweep inside a KDE Plasma Wayland session")
    outputs = plasma_outputs() if on_plasma else list(PLAN_OUTPUTS)
    if not outputs:
        sys.exit("could not discover Plasma outputs")
    ranked = P.ranked_outputs(outputs, "")
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
        Path.home() / ".local/share/Steam/steamapps/workshop/content/431960/2165290843"
    )
    if not (Path(scene) / "scene.pkg").is_file():
        sys.exit(f"Wallpaper Engine scene is unavailable: {scene}")
    matrix = build_matrix(floor_videos["vp9"], mid_video, max_video, still, scene)
    if args.only:
        terms = [term.casefold() for term in args.only]
        matrix = [s for s in matrix if all(term in s.name.casefold() for term in terms)]
    if args.engine:
        matrix = [s for s in matrix if s.engine in set(args.engine)]
    if args.workload:
        matrix = [s for s in matrix if s.workload in set(args.workload)]
    if not matrix:
        sys.exit("no Plasma scenarios match the requested filters")
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
        "resolution_label": None,
        "baseline": {
            "preexisting_wallpaper_engines": "terminated before sampling",
            "scenario_cleanup": f"all desktops reset to {COLOR_PLUGIN} before every baseline",
            "host_samples": HOST_STABLE_SAMPLES,
            "host_sample_interval_seconds": HOST_STABLE_INTERVAL,
            "host_stability_tolerance_mib": HOST_STABILITY_TOLERANCE,
            "host_recovery_tolerance_mib": HOST_RECOVERY_TOLERANCE,
            "gpu_floor_drift_limit_points": GPU_FLOOR_DRIFT_LIMIT,
            "power_floor_drift_limit_watts": POWER_FLOOR_DRIFT_LIMIT,
            "host_cpu_floor_drift_limit_points": HOST_CPU_FLOOR_DRIFT_LIMIT,
        },
        "paper": {
            "executable": PAPER,
            "version": binary_version(PAPER),
            "layer_vk": LAYER_VK,
            "layer_still": LAYER_STILL,
        },
        "visual_validation": {
            "enabled": VALIDATE_MOTION,
            "samples": C.VIDEO_VALIDATION_SAMPLES if VALIDATE_MOTION else 0,
            "interval_seconds": C.VIDEO_VALIDATION_INTERVAL if VALIDATE_MOTION else None,
            "crop": "center 75%",
            "minimum_mean_signal": C.MIN_VIDEO_MEAN_SIGNAL,
            "minimum_signal_deviation": C.MIN_VIDEO_SIGNAL_DEVIATION,
            "capture": "grim while KWin shows the desktop",
        },
        "active_outputs": outputs,
        "compositor": {"name": "plasma", "hosts": list(HOSTS)},
        "scenarios": [scenario.metadata(single_output) for scenario in matrix],
    }
    if args.plan:
        print(json.dumps(payload, indent=2))
        return
    ensure_available(matrix)
    if P.process_pids("skwd-walld"):
        sys.exit("stop skwd-walld before sweeping; Deck re-applies its wallpapers over the matrix")
    pids = host_pids()
    payload["compositor"]["pid"] = pids["plasmashell"]
    payload["compositor"]["process_name"] = "plasmashell"
    payload["compositor"]["pids"] = pids
    mapping = screen_map(outputs, desktop_geometries())
    payload["screen_map"] = mapping
    config_keys = {
        "Assignments", "Assignment", "Paper", "StreamWidth", "StreamHeight", "StreamFps", "Paused",
        "Image", "FillMode", "Color", "VideoUrls", "PauseMode", "BlurMode", "MuteMode",
        "CheckWindowsActiveScreen", "ScreenOffPausesVideo", "BatteryPausesVideo", "CrossfadeEnabled",
        "RandomMode", "ResumeLastVideo", "ChangeWallpaperMode",
    }
    saved = snapshot_desktops(config_keys)
    saved_path = Path(args.out).with_suffix(".saved.json")
    saved_path.write_text(json.dumps(saved, indent=1))
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    baseline_assignments = {str(screen): color_config() for screen in mapping.values()}
    if VALIDATE_MOTION:
        with tempfile.TemporaryDirectory(prefix="skwd-plasma-warmup-") as temporary:
            show_desktop(True)
            try:
                capture_output(single_output, Path(temporary) / "warm.ppm")
            finally:
                show_desktop(False)
        payload["visual_validation"]["capture_warmed_before_sampling"] = True
    floors, results, discarded = [], [], []
    print(f"plasma matrix {plan_hash}: {len(matrix)} scenarios, {profile_name}, single={single_output}")
    try:
        for scenario in matrix:
            for attempt in range(1, SCENARIO_ATTEMPTS + 1):
                floor_before, hosts_before = clean_baseline(pids, profile, baseline_assignments)
                floors.append(floor_before)
                print(
                    f"  [clean baseline: gpu {floor_before['gpu']:.1f}% {floor_before['power']:.1f}W "
                    f"plasmashell {floor_before['cpu']['plasmashell']:.1f}% kwin {floor_before['cpu']['kwin_wayland']:.1f}% "
                    f"rss {hosts_before['plasmashell']['snapshot']['rss']}/{hosts_before['kwin_wayland']['snapshot']['rss']} MiB]"
                )
                result = run_scenario(
                    scenario, floor_before, profile, single_output, outputs, mapping, pids, hosts_before
                )
                recovery_error = None
                try:
                    apply_assignments(baseline_assignments)
                    kill_strays()
                    hosts_after = stable_hosts(pids, hosts_before)
                    floor_after = idle_floor(profile, pids)
                    floors.append(floor_after)
                    finalize_baseline_metrics(result, hosts_before, hosts_after, floor_before, floor_after)
                except RuntimeError as error:
                    recovery_error = str(error)
                if recovery_error and attempt < SCENARIO_ATTEMPTS:
                    discarded.append(
                        {"scenario": scenario.name, "attempt": attempt, "reason": f"baseline recovery failed: {recovery_error}"}
                    )
                    print(f"  [discarded attempt {attempt}/{SCENARIO_ATTEMPTS}: {recovery_error}]")
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
                shell = result["compositor"]
                kwin = result["kwin"]
                print(
                    f"{scenario.name:<42} cpu={result['cpu']:>5}% gpuD={result['gpu_delta']:>5}% "
                    f"WD={result['power_delta']:>6} rss={result['rss']:>4} vram={result['vram']:>4} "
                    f"shellC={shell['cpu_delta']:>+5}% shellR={shell['rss_delta']:>+5} shellV={shell['vram_delta']:>+5} "
                    f"kwinC={kwin['cpu_delta']:>+5}% kwinR={kwin['rss_delta']:>+5} kwinV={kwin['vram_delta']:>+5}"
                )
    finally:
        try:
            apply_assignments(baseline_assignments)
            kill_strays()
            restore_desktops(saved)
        except RuntimeError as error:
            print(f"could not restore the original Plasma wallpapers: {error}; rerun with --restore {saved_path}")
    qualifications = [result["baseline"]["qualified"] for result in results if "baseline" in result]
    taints = {
        metric: any(not qualification[metric] for qualification in qualifications)
        for metric in ("compositor_memory", "gpu", "power", "compositor_cpu", "kwin_cpu")
    }
    if any("baseline recovery failed" in result.get("error", "") for result in results):
        taints["compositor_memory"] = True
    payload.update(
        {
            "time": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "floors": floors,
            "discarded_attempts": discarded,
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
