"""Measure real Wallpaper Engine (Steam/Proton) rendering a scene, using the same metric
definitions as scripts/perf-sweep.py so the numbers sit beside a competitor-sweep row.

CPU is summed process seconds over the whole tree Steam spawned for app 431960, expressed as a
percentage of one core. RSS is summed VmRSS. GPU utilisation and power are whole-device readings
with an idle floor sampled immediately before launch, because per-process GPU accounting is not
available for the wine processes.

usage: weperf.py <scene_dir> [--seconds N] [--width W] [--height H] [--out FILE]
"""
import argparse, json, os, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import oracle

CLK = os.sysconf("SC_CLK_TCK")
APP = "431960"


def gpu():
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=utilization.gpu,power.draw", "--format=csv,noheader,nounits"],
            capture_output=True, text=True, timeout=10,
        ).stdout.strip().splitlines()[0]
        util, watts = (part.strip() for part in out.split(","))
        return float(util), float(watts)
    except Exception:
        return 0.0, 0.0


def children():
    parents = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            parts = (entry / "stat").read_text().rsplit(") ", 1)[1].split()
            parents[int(entry.name)] = int(parts[1])
        except (OSError, IndexError, ValueError):
            continue
    return parents


def reaper():
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            cmd = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
        except OSError:
            continue
        if f"AppId={APP}" in cmd and "reaper" in cmd:
            return int(entry.name)
    return None


def tree(root):
    parents = children()
    kin = {root}
    for _ in range(12):
        grew = False
        for pid, parent in parents.items():
            if parent in kin and pid not in kin:
                kin.add(pid)
                grew = True
        if not grew:
            break
    return kin


def cpu_seconds(pid):
    try:
        parts = (Path("/proc") / str(pid) / "stat").read_text().rsplit(") ", 1)[1].split()
        return (int(parts[11]) + int(parts[12])) / CLK
    except (OSError, IndexError, ValueError):
        return None


def rss_mib(pid):
    try:
        for line in (Path("/proc") / str(pid) / "status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) / 1024.0
    except OSError:
        pass
    return 0.0


def named(name):
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if (entry / "comm").read_text().strip() == name:
                return int(entry.name)
        except OSError:
            continue
    return None


def by_name(name):
    found = set()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if (entry / "comm").read_text().strip() == name:
                found.add(int(entry.name))
        except OSError:
            continue
    return found


def sample(pids):
    return {pid: cpu_seconds(pid) for pid in pids if cpu_seconds(pid) is not None}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("scene")
    ap.add_argument("--seconds", type=float, default=10.0)
    ap.add_argument("--settle", type=float, default=8.0)
    ap.add_argument("--width", type=int, default=1920)
    ap.add_argument("--height", type=int, default=1080)
    ap.add_argument("--out")
    ap.add_argument("--engine", choices=("we", "skwd"), default="we")
    ap.add_argument("--output", default="DP-3")
    ap.add_argument("--fps", type=int, default=30)
    args = ap.parse_args()

    floor_util, floor_watts = [], []
    niri = named("niri")
    niri_before = cpu_seconds(niri) if niri else None
    start_floor = time.time()
    for _ in range(6):
        util, watts = gpu()
        floor_util.append(util)
        floor_watts.append(watts)
        time.sleep(0.5)
    niri_floor = None
    if niri and niri_before is not None:
        niri_floor = 100.0 * (cpu_seconds(niri) - niri_before) / (time.time() - start_floor)

    ours = None
    if args.engine == "we":
        oracle.restart(args.scene, args.width, args.height)
        window = None
        for _ in range(90):
            time.sleep(1)
            window = oracle.we_window()
            if window:
                break
        if not window:
            sys.exit("Wallpaper Engine never opened its window")
        oracle.park_windows()
    else:
        binary = str(HERE.parent.parent / "target" / "release" / "skwd-wall-vk")
        env = dict(os.environ, SKWD_PAPER_WE_FPS=str(args.fps))
        ours = subprocess.Popen(
            [binary, args.output, args.scene, "--mute", "--scene", args.scene],
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    time.sleep(args.settle)

    live = (lambda: tree(reaper())) if args.engine == "we" else (lambda: by_name("skwd-wall-vk"))
    if args.engine == "we" and reaper() is None:
        sys.exit("could not find the Steam reaper for app 431960")
    pids = live()
    if not pids:
        sys.exit(f"no {args.engine} processes are running to measure")
    first = sample(pids)
    niri_start = cpu_seconds(niri) if niri else None
    util, watts = [], []
    started = time.time()
    while time.time() - started < args.seconds:
        u, w = gpu()
        util.append(u)
        watts.append(w)
        time.sleep(0.5)
    elapsed = time.time() - started
    current = live()
    last = sample(current)
    spent = sum(last[pid] - first.get(pid, 0.0) for pid in last)
    rss = sum(rss_mib(pid) for pid in current)
    if not current:
        sys.exit(f"the {args.engine} processes vanished during the window")
    niri_cpu = None
    if niri and niri_start is not None:
        niri_cpu = 100.0 * (cpu_seconds(niri) - niri_start) / elapsed

    mean = lambda rows: sum(rows) / max(1, len(rows))
    report = {
        "engine": "wallpaper-engine-proton" if args.engine == "we" else "skwd-wall-vk",
        "scene": os.path.basename(args.scene.rstrip("/")),
        "window": [args.width, args.height],
        "seconds": round(elapsed, 2),
        "processes": len(last),
        "cpu_percent_of_one_core": round(100.0 * spent / elapsed, 1),
        "rss_mib": round(rss),
        "gpu_percent": round(mean(util), 1),
        "gpu_percent_idle_floor": round(mean(floor_util), 1),
        "watts": round(mean(watts), 1),
        "watts_idle_floor": round(mean(floor_watts), 1),
        "niri_cpu_percent": None if niri_cpu is None else round(niri_cpu, 1),
        "niri_cpu_idle_floor": None if niri_floor is None else round(niri_floor, 1),
    }
    print(json.dumps(report, indent=2))
    if args.out:
        Path(args.out).write_text(json.dumps(report, indent=2))
    if ours is not None:
        subprocess.run(["pkill", "-x", "skwd-wall-vk"], check=False)
    else:
        subprocess.run(["pkill", "-f", "[w]allpaper64.exe -language"], check=False)


if __name__ == "__main__":
    main()
