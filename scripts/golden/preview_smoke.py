#!/usr/bin/env python3
"""Preview smoke tier: render each Workshop scene headless and compare with the author preview.
ImageMagick reports SSIM as a distance (0 = identical); scores here are 1 - distance, higher is better.

usage: preview_smoke.py --workshop DIR --vk PATH [--out CSV] [--baseline CSV] [--size WxH] [--limit N]
Exit status 1 when any scene drops more than --tolerance SSIM against the baseline or stops rendering.
"""
import argparse, csv, json, os, shutil, subprocess, sys, tempfile
from PIL import Image, ImageSequence

def load_preview(path):
    im = Image.open(path)
    if getattr(im, "n_frames", 1) > 1:
        frames = [f.convert("RGB") for f in ImageSequence.Iterator(im)]
        return frames[min(2, len(frames) - 1)]
    return im.convert("RGB")

def ssim(a, b, tmp):
    pa, pb = os.path.join(tmp, "a.png"), os.path.join(tmp, "b.png")
    a.save(pa); b.save(pb)
    r = subprocess.run(["magick", "compare", "-metric", "SSIM", pa, pb, "null:"], capture_output=True, text=True)
    text = r.stderr.strip()
    try:
        distance = float(text.split("(")[1].split(")")[0]) if "(" in text else float(text.split()[0]) / 65535.0
    except Exception:
        return 0.0
    return 1.0 - distance

def best_ssim(frame_path, preview_path, tmp, size=192):
    render = Image.open(frame_path).convert("RGB")
    preview = load_preview(preview_path).resize((size, size), Image.BOX)
    w, h = render.size
    best = -1.0
    for s in (1.0, 0.85, 0.7):
        side = int(min(w, h) * s)
        for fx in (0.0, 0.25, 0.5, 0.75, 1.0):
            for fy in (0.0, 0.5, 1.0):
                x, y = int((w - side) * fx), int((h - side) * fy)
                crop = render.crop((x, y, x + side, y + side)).resize((size, size), Image.BOX)
                best = max(best, ssim(crop, preview, tmp))
    return best

def find_preview(scene_dir):
    for name in ("preview.jpg", "preview.png", "preview.gif"):
        p = os.path.join(scene_dir, name)
        if os.path.isfile(p):
            return p
    return None

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workshop", required=True)
    ap.add_argument("--vk", required=True)
    ap.add_argument("--out", default="preview_smoke.csv")
    ap.add_argument("--baseline")
    ap.add_argument("--size", default="960x540")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--tolerance", type=float, default=0.02)
    args = ap.parse_args()
    baseline = {}
    if args.baseline and os.path.isfile(args.baseline):
        with open(args.baseline) as f:
            for row in csv.DictReader(f):
                baseline[row["scene"]] = row
    rows = []
    scenes = sorted(d for d in os.listdir(args.workshop) if os.path.isfile(os.path.join(args.workshop, d, "scene.pkg")))
    if args.limit:
        scenes = scenes[: args.limit]
    regressions = []
    with tempfile.TemporaryDirectory() as tmp:
        for sid in scenes:
            scene_dir = os.path.join(args.workshop, sid)
            out = os.path.join(tmp, sid)
            shutil.rmtree(out, ignore_errors=True)
            try:
                r = subprocess.run([args.vk, "scene-dump", scene_dir, "--out", out, "--size", args.size, "--frames", "1", "--time0", "1.0", "--daytime", "0.5"], capture_output=True, text=True, timeout=180)
                status = "ok" if r.returncode == 0 else f"exit{r.returncode}"
            except subprocess.TimeoutExpired:
                r = None
                status = "timeout"
            row = {"scene": sid, "status": status, "ssim": "", "chains": "", "particles": "", "animated": "", "render_ms": ""}
            manifest = os.path.join(out, "manifest.json")
            if status == "ok" and os.path.isfile(manifest):
                m = json.load(open(manifest))
                row.update(chains=m.get("effect_chains"), particles=m.get("particle_systems"), animated=m.get("animated"), render_ms=round(m.get("render_ms", 0), 1))
                preview = find_preview(scene_dir)
                frame = os.path.join(out, m["frames"][0]) if m.get("frames") else None
                if preview and frame:
                    row["ssim"] = round(best_ssim(frame, preview, tmp), 4)
            rows.append(row)
            prev = baseline.get(sid)
            if prev:
                if prev["status"] == "ok" and row["status"] != "ok":
                    regressions.append((sid, "stopped rendering", prev["status"], row["status"]))
                elif prev.get("ssim") and row.get("ssim") != "" and float(row["ssim"]) < float(prev["ssim"]) - args.tolerance:
                    regressions.append((sid, "ssim drop", prev["ssim"], row["ssim"]))
            print(f"{sid} {row['status']} ssim={row['ssim']} chains={row['chains']} particles={row['particles']} ms={row['render_ms']}", flush=True)
            shutil.rmtree(out, ignore_errors=True)
    with open(args.out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=["scene", "status", "ssim", "chains", "particles", "animated", "render_ms"])
        w.writeheader(); w.writerows(rows)
    ok = sum(1 for r in rows if r["status"] == "ok")
    scored = [float(r["ssim"]) for r in rows if r["ssim"] != ""]
    print(f"scenes={len(rows)} rendered={ok} mean_ssim={sum(scored)/len(scored):.4f}" if scored else f"scenes={len(rows)} rendered={ok}")
    for reg in regressions:
        print("REGRESSION", *reg)
    sys.exit(1 if regressions else 0)

if __name__ == "__main__":
    main()
