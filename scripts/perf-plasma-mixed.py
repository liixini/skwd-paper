#!/usr/bin/env python3

import argparse
import json
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
PERF = ROOT / "scripts" / "perf-plasma.py"
CONFIG_KEYS = [
    "Kind",
    "Source",
    "Renderer",
    "StreamWidth",
    "StreamHeight",
    "StreamFps",
    "Image",
    "FillMode",
]


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
        timeout=15,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "Plasma scripting failed")
    return result.stdout.strip()


def snapshot():
    keys = json.dumps(CONFIG_KEYS)
    source = f"""
var keys = {keys};
var result = desktops().map(function(d) {{
    var plugin = d.wallpaperPlugin;
    d.currentConfigGroup = [\"Wallpaper\", plugin, \"General\"];
    var config = {{}};
    keys.forEach(function(key) {{ config[key] = d.readConfig(key, \"\"); }});
    return {{screen: d.screen, plugin: plugin, config: config}};
}});
print(JSON.stringify(result));
"""
    output = plasma_script(source)
    return json.loads(output.splitlines()[-1])


def apply(assignments):
    payload = json.dumps(assignments)
    source = f"""
var assignments = {payload};
desktops().forEach(function(d) {{
    var wanted = assignments[String(d.screen)];
    if (!wanted) return;
    d.wallpaperPlugin = \"org.kde.color\";
    d.currentConfigGroup = [\"Wallpaper\", wanted.plugin, \"General\"];
    Object.keys(wanted.config).forEach(function(key) {{
        d.writeConfig(key, wanted.config[key]);
    }});
    d.wallpaperPlugin = wanted.plugin;
}});
"""
    plasma_script(source)


def restore(saved):
    assignments = {
        str(item["screen"]): {"plugin": item["plugin"], "config": item["config"]}
        for item in saved
    }
    apply(assignments)


def displays(display_tool):
    try:
        result = subprocess.run(
            [display_tool, "displays", "--json"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except FileNotFoundError as error:
        raise RuntimeError(f"display tool not found: {display_tool}") from error
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "Wayland output discovery failed")
    found = json.loads(result.stdout)
    if len(found) < 3:
        raise RuntimeError(f"three outputs required, found {found}")
    return found[:3]


def wait_renderers(expected, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = subprocess.run(
            ["pgrep", "-P", str(plasma_pid())], capture_output=True, text=True, timeout=5
        )
        pids = []
        for value in result.stdout.split():
            path = Path(f"/proc/{value}/cmdline")
            try:
                command = path.read_bytes().replace(b"\0", b" ").decode()
            except OSError:
                continue
            if "--video-stream" in command:
                pids.append(int(value))
        if len(pids) == expected:
            return sorted(pids)
        time.sleep(0.5)
    raise RuntimeError(f"expected {expected} renderers")


def plasma_pid():
    result = subprocess.run(
        ["pgrep", "-n", "-x", "plasmashell"], capture_output=True, text=True, timeout=5
    )
    if result.returncode != 0:
        raise RuntimeError("plasmashell is not running")
    return int(result.stdout.strip())


def video(source, output, renderer, fps):
    return {
        "plugin": "org.skwd.wall.plasma",
        "config": {
            "Kind": "video",
            "Source": source,
            "Renderer": renderer,
            "StreamWidth": output["width"],
            "StreamHeight": output["height"],
            "StreamFps": fps,
        },
    }


def scene(source, output, renderer, fps):
    value = video(source, output, renderer, fps)
    value["config"]["Kind"] = "we"
    return value


def static(source):
    return {
        "plugin": "org.kde.image",
        "config": {"Image": Path(source).resolve().as_uri(), "FillMode": 2},
    }


def screenshot(name, outputs, directory):
    directory.mkdir(parents=True, exist_ok=True)
    paths = []
    errors = []
    state = subprocess.run(
        ["qdbus6", "org.kde.KWin", "/KWin", "org.kde.KWin.showingDesktop"],
        capture_output=True,
        text=True,
        timeout=5,
    )
    toggle = state.stdout.strip() != "true"
    shortcut = [
        "qdbus6",
        "org.kde.kglobalaccel",
        "/component/kwin",
        "org.kde.kglobalaccel.Component.invokeShortcut",
        "Show Desktop",
    ]
    if toggle:
        subprocess.run(shortcut, check=True, timeout=5)
        time.sleep(1.0)
    try:
        for output in outputs:
            path = directory / f"{name}-{output['name']}.png"
            command = ["grim", "-o", output["name"], str(path)]
            result = subprocess.run(command, capture_output=True, text=True, timeout=15)
            if result.returncode == 0:
                paths.append(str(path))
            else:
                errors.append({"output": output["name"], "error": result.stderr.strip()})
    finally:
        if toggle:
            subprocess.run(shortcut, check=True, timeout=5)
    return {"files": paths, "errors": errors}


def measure(name, assignments, outputs, args):
    apply(assignments)
    expected = sum(item["plugin"] == "org.skwd.wall.plasma" for item in assignments.values())
    renderer_pids = wait_renderers(expected)
    time.sleep(args.settle)
    output = Path(args.out_dir) / f"{name}.json"
    command = [
        "python3",
        str(PERF),
        "--plasmashell-pid",
        str(plasma_pid()),
        "--window",
        str(args.window),
        "--rounds",
        str(args.rounds),
        "--out",
        str(output),
    ]
    for pid in renderer_pids:
        command.extend(["--renderer-pid", str(pid)])
    subprocess.run(command, check=True)
    report = json.loads(output.read_text())
    report["name"] = name
    report["assignments"] = assignments
    report["screenshots"] = screenshot(name, outputs, Path(args.out_dir) / "screenshots")
    output.write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--video", action="append", required=True)
    parser.add_argument("--scene", required=True)
    parser.add_argument("--static", required=True)
    parser.add_argument(
        "--display-tool",
        default="skwd-helm",
        help="Deck CLI or compatible command implementing 'displays --json'",
    )
    parser.add_argument("--renderer", default=str(ROOT / "target" / "release" / "skwd-wall-vk"))
    parser.add_argument("--window", type=float, default=10.0)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--settle", type=float, default=8.0)
    parser.add_argument("--out-dir", default="/tmp/skwd-plasma-mixed")
    parser.add_argument(
        "--leave-profile", choices=["all-same-video", "mixed-videos", "video-we-static"]
    )
    args = parser.parse_args()
    if len(args.video) < 3:
        parser.error("pass three --video paths")
    Path(args.out_dir).mkdir(parents=True, exist_ok=True)
    outputs = displays(args.display_tool)
    saved = snapshot()
    profiles = {
        "all-same-video": {
            str(index): video(args.video[0], outputs[index], args.renderer, 30)
            for index in range(3)
        },
        "mixed-videos": {
            str(index): video(args.video[index], outputs[index], args.renderer, [30, 30, 60][index])
            for index in range(3)
        },
        "video-we-static": {
            "0": video(args.video[0], outputs[0], args.renderer, 30),
            "1": scene(args.scene, outputs[1], args.renderer, 30),
            "2": static(args.static),
        },
    }
    reports = []
    try:
        for name, assignments in profiles.items():
            print(f"== {name} ==", flush=True)
            reports.append(measure(name, assignments, outputs, args))
    finally:
        restore(saved)
    if args.leave_profile:
        apply(profiles[args.leave_profile])
        expected = sum(
            item["plugin"] == "org.skwd.wall.plasma"
            for item in profiles[args.leave_profile].values()
        )
        wait_renderers(expected)
    summary = {
        "outputs": outputs,
        "profiles": reports,
        "restored": saved,
    }
    path = Path(args.out_dir) / "summary.json"
    path.write_text(json.dumps(summary, indent=2) + "\n")
    print(path)


if __name__ == "__main__":
    main()
