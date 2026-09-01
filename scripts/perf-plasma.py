#!/usr/bin/env python3

import argparse
import json
import os
import signal
import statistics
import subprocess
import time
from pathlib import Path


def proc_stat(pid):
    text = Path(f"/proc/{pid}/stat").read_text()
    fields = text.rsplit(")", 1)[1].split()
    return {
        "cpu_ticks": int(fields[11]) + int(fields[12]),
        "threads": int(fields[17]),
    }


def proc_memory(pid):
    values = {}
    for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
        key, _, rest = line.partition(":")
        if key in {"Rss", "Pss", "Private_Clean", "Private_Dirty"}:
            values[key] = int(rest.split()[0]) / 1024.0
    return {
        "rss_mb": values.get("Rss", 0.0),
        "pss_mb": values.get("Pss", 0.0),
        "private_mb": values.get("Private_Clean", 0.0) + values.get("Private_Dirty", 0.0),
        "fds": len(list(Path(f"/proc/{pid}/fd").iterdir())),
        "threads": proc_stat(pid)["threads"],
    }


def aggregate_memory(pids):
    rows = [proc_memory(pid) for pid in pids]
    return {key: sum(row[key] for row in rows) for key in rows[0]}


def nvidia_sample():
    result = subprocess.run(
        [
            "nvidia-smi",
            "--query-gpu=utilization.gpu,utilization.decoder,power.draw,memory.used",
            "--format=csv,noheader,nounits",
        ],
        capture_output=True,
        text=True,
        timeout=5,
    )
    if result.returncode != 0:
        return {}
    values = [value.strip() for value in result.stdout.strip().split(",")]
    if len(values) != 4:
        return {}
    try:
        return {
            "gpu_pct": float(values[0]),
            "decoder_pct": float(values[1]),
            "power_w": float(values[2]),
            "gpu_memory_mb": float(values[3]),
        }
    except ValueError:
        return {}


def process_vram():
    result = subprocess.run(
        [
            "nvidia-smi",
            "--query-compute-apps=pid,used_memory",
            "--format=csv,noheader,nounits",
        ],
        capture_output=True,
        text=True,
        timeout=5,
    )
    values = {}
    for line in result.stdout.splitlines():
        try:
            pid, memory = (part.strip() for part in line.split(",", 1))
            values[int(pid)] = float(memory)
        except ValueError:
            continue
    return values


def children(parent):
    found = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            cmdline = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode()
        except (OSError, UnicodeDecodeError, IndexError):
            continue
        if int(stat[1]) == parent and "--video-stream" in cmdline:
            found.append((int(entry.name), cmdline.strip()))
    return found


def find_processes(plasma_pid, renderer_pids):
    if plasma_pid is None:
        result = subprocess.run(
            ["pgrep", "-n", "-x", "plasmashell"], capture_output=True, text=True, timeout=5
        )
        plasma_pid = int(result.stdout.strip()) if result.returncode == 0 else None
    if plasma_pid is None or not Path(f"/proc/{plasma_pid}").is_dir():
        raise RuntimeError("plasmashell is not running")
    candidates = children(plasma_pid)
    if not renderer_pids:
        renderer_pids = [pid for pid, _ in candidates]
    if not renderer_pids:
        raise RuntimeError("no Plasma video renderers found")
    for renderer_pid in renderer_pids:
        if not Path(f"/proc/{renderer_pid}").is_dir():
            raise RuntimeError(f"renderer PID {renderer_pid} is not running")
    return plasma_pid, sorted(renderer_pids)


def average(samples, key):
    values = [sample[key] for sample in samples if key in sample]
    return statistics.fmean(values) if values else 0.0


def measure_phase(name, plasma_pid, renderer_pids, window, interval):
    p0 = proc_stat(plasma_pid)["cpu_ticks"]
    r0 = {pid: proc_stat(pid)["cpu_ticks"] for pid in renderer_pids}
    started = time.monotonic()
    samples = []
    while time.monotonic() - started < window:
        samples.append(nvidia_sample())
        remaining = interval - (time.monotonic() - started) % interval
        time.sleep(min(remaining, max(0.0, window - (time.monotonic() - started))))
    elapsed = time.monotonic() - started
    p1 = proc_stat(plasma_pid)["cpu_ticks"]
    r1 = {pid: proc_stat(pid)["cpu_ticks"] for pid in renderer_pids}
    hz = os.sysconf("SC_CLK_TCK")
    vram = process_vram()
    return {
        "name": name,
        "seconds": elapsed,
        "plasmashell_cpu_pct": 100.0 * (p1 - p0) / hz / elapsed,
        "renderer_cpu_pct": 100.0 * sum(r1[pid] - r0[pid] for pid in renderer_pids) / hz / elapsed,
        "gpu_pct": average(samples, "gpu_pct"),
        "decoder_pct": average(samples, "decoder_pct"),
        "power_w": average(samples, "power_w"),
        "gpu_memory_mb": average(samples, "gpu_memory_mb"),
        "plasmashell": proc_memory(plasma_pid),
        "renderer": aggregate_memory(renderer_pids),
        "renderers": {str(pid): proc_memory(pid) for pid in renderer_pids},
        "plasmashell_vram_mb": vram.get(plasma_pid, 0.0),
        "renderer_vram_mb": sum(vram.get(pid, 0.0) for pid in renderer_pids),
        "renderer_vram_by_pid_mb": {str(pid): vram.get(pid, 0.0) for pid in renderer_pids},
    }


def median_phases(phases, name, key):
    return statistics.median(phase[key] for phase in phases if phase["name"] == name)


def soak(plasma_pid, renderer_pids, seconds, interval):
    started = time.monotonic()
    samples = []
    while True:
        elapsed = time.monotonic() - started
        vram = process_vram()
        samples.append(
            {
                "seconds": elapsed,
                "plasmashell": proc_memory(plasma_pid),
                "renderer": aggregate_memory(renderer_pids),
                "plasmashell_vram_mb": vram.get(plasma_pid, 0.0),
                "renderer_vram_mb": sum(vram.get(pid, 0.0) for pid in renderer_pids),
            }
        )
        if elapsed >= seconds:
            break
        time.sleep(min(interval, seconds - elapsed))
    duration_minutes = max(samples[-1]["seconds"] - samples[0]["seconds"], 0.001) / 60.0
    slopes = {}
    for process in ["plasmashell", "renderer"]:
        for key in ["rss_mb", "pss_mb", "private_mb", "fds", "threads"]:
            slopes[f"{process}_{key}_per_min"] = (
                samples[-1][process][key] - samples[0][process][key]
            ) / duration_minutes
    for key in ["plasmashell_vram_mb", "renderer_vram_mb"]:
        slopes[f"{key}_per_min"] = (samples[-1][key] - samples[0][key]) / duration_minutes
    return {"seconds": samples[-1]["seconds"], "slopes": slopes, "samples": samples}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--plasmashell-pid", type=int)
    parser.add_argument("--renderer-pid", type=int, action="append")
    parser.add_argument("--window", type=float, default=10.0)
    parser.add_argument("--interval", type=float, default=1.0)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--soak-seconds", type=float, default=0.0)
    parser.add_argument("--soak-interval", type=float, default=10.0)
    parser.add_argument("--soak-paused", action="store_true")
    parser.add_argument("--out", default="/tmp/skwd-plasma-perf.json")
    args = parser.parse_args()
    plasma_pid, renderer_pids = find_processes(args.plasmashell_pid, args.renderer_pid)
    phases = []
    stopped = False
    try:
        for index in range(args.rounds):
            if stopped:
                for renderer_pid in renderer_pids:
                    os.kill(renderer_pid, signal.SIGCONT)
                stopped = False
                time.sleep(2.0)
            active = measure_phase("active", plasma_pid, renderer_pids, args.window, args.interval)
            phases.append(active)
            print(
                f"active {index + 1}/{args.rounds}: renderer {active['renderer_cpu_pct']:.2f}% "
                f"plasma {active['plasmashell_cpu_pct']:.2f}% gpu {active['gpu_pct']:.1f}% "
                f"power {active['power_w']:.1f}W"
            )
            for renderer_pid in renderer_pids:
                os.kill(renderer_pid, signal.SIGSTOP)
            stopped = True
            time.sleep(2.0)
            paused = measure_phase("paused", plasma_pid, renderer_pids, args.window, args.interval)
            phases.append(paused)
            print(
                f"paused {index + 1}/{args.rounds}: plasma {paused['plasmashell_cpu_pct']:.2f}% "
                f"gpu {paused['gpu_pct']:.1f}% power {paused['power_w']:.1f}W"
            )
    finally:
        if stopped:
            for renderer_pid in renderer_pids:
                if Path(f"/proc/{renderer_pid}").is_dir():
                    os.kill(renderer_pid, signal.SIGCONT)
    summary = {
        "plasmashell_pid": plasma_pid,
        "renderer_pids": renderer_pids,
        "rounds": args.rounds,
        "window_seconds": args.window,
        "active": {
            key: median_phases(phases, "active", key)
            for key in [
                "plasmashell_cpu_pct",
                "renderer_cpu_pct",
                "gpu_pct",
                "decoder_pct",
                "power_w",
                "gpu_memory_mb",
            ]
        },
        "paused": {
            key: median_phases(phases, "paused", key)
            for key in [
                "plasmashell_cpu_pct",
                "renderer_cpu_pct",
                "gpu_pct",
                "decoder_pct",
                "power_w",
                "gpu_memory_mb",
            ]
        },
        "active_memory": next(phase for phase in reversed(phases) if phase["name"] == "active"),
        "phases": phases,
    }
    summary["delta"] = {
        key: summary["active"][key] - summary["paused"][key]
        for key in ["plasmashell_cpu_pct", "gpu_pct", "decoder_pct", "power_w"]
    }
    if args.soak_seconds > 0.0:
        soak_stopped = False
        try:
            if args.soak_paused:
                for renderer_pid in renderer_pids:
                    os.kill(renderer_pid, signal.SIGSTOP)
                soak_stopped = True
                time.sleep(2.0)
            label = "paused" if args.soak_paused else "active"
            print(f"{label} soak: {args.soak_seconds:.0f}s")
            summary["soak"] = soak(
                plasma_pid, renderer_pids, args.soak_seconds, args.soak_interval
            )
            summary["soak"]["state"] = label
        finally:
            if soak_stopped:
                for renderer_pid in renderer_pids:
                    if Path(f"/proc/{renderer_pid}").is_dir():
                        os.kill(renderer_pid, signal.SIGCONT)
        print(json.dumps(summary["soak"]["slopes"], indent=2))
    Path(args.out).write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps({key: summary[key] for key in ["active", "paused", "delta"]}, indent=2))
    print(args.out)


if __name__ == "__main__":
    main()
