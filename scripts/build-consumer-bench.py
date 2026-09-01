#!/usr/bin/env python3
"""Build a deterministic, storage-efficient cross-VM wallpaper test bench."""

import argparse
import json
import os
import shutil
from pathlib import Path

STATIC_EXTS = {".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif", ".avif"}
VIDEO_EXTS = {".mp4", ".webm", ".mkv", ".mov", ".m4v", ".avi"}


def files(root: Path, extensions: set[str], minimum_size: int) -> list[Path]:
    return sorted(
        (
            path
            for path in root.rglob("*")
            if path.is_file()
            and path.suffix.lower() in extensions
            and path.stat().st_size >= minimum_size
        ),
        key=lambda path: str(path.relative_to(root)).casefold(),
    )


def scenes(root: Path) -> list[Path]:
    candidates = []
    for project in root.glob("*/project.json"):
        directory = project.parent
        if not (directory.joinpath("scene.pkg").is_file() or directory.joinpath("gifscene.pkg").is_file()):
            continue
        try:
            metadata = json.loads(project.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        if str(metadata.get("type", "scene")).lower() != "scene":
            continue
        if not any(path.name.lower().startswith("preview.") for path in directory.iterdir()):
            continue
        candidates.append(directory)
    return sorted(candidates, key=lambda path: path.name)


def evenly(candidates: list[Path], count: int) -> list[Path]:
    if len(candidates) < count:
        raise SystemExit(f"need {count} candidates, found {len(candidates)}")
    if count == 1:
        return [candidates[len(candidates) // 2]]
    return [candidates[index * (len(candidates) - 1) // (count - 1)] for index in range(count)]


def link_or_copy(source: str, destination: str) -> str:
    try:
        os.link(source, destination)
        return destination
    except OSError:
        return shutil.copy2(source, destination)


def materialize_files(root: Path, selected: list[Path], destination: Path) -> list[dict]:
    records = []
    for index, source in enumerate(selected, 1):
        relative = source.relative_to(root)
        target = destination / f"{index:03d}" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        link_or_copy(str(source.resolve()), str(target))
        records.append(
            {
                "index": index,
                "source": str(source),
                "path": str(target.relative_to(destination.parent)),
                "bytes": source.stat().st_size,
            }
        )
    return records


def materialize_scenes(selected: list[Path], destination: Path) -> list[dict]:
    records = []
    for source in selected:
        target = destination / source.name
        shutil.copytree(source, target, copy_function=link_or_copy, symlinks=False)
        records.append(
            {
                "id": source.name,
                "source": str(source),
                "path": str(target.relative_to(destination.parent)),
            }
        )
    return records


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--static-root", type=Path, required=True)
    parser.add_argument("--video-root", type=Path, required=True)
    parser.add_argument("--we-root", type=Path, required=True)
    parser.add_argument("--we-assets-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--static-count", type=int, default=300)
    parser.add_argument("--video-count", type=int, default=20)
    parser.add_argument("--we-count", type=int, default=10)
    args = parser.parse_args()

    if args.output.exists():
        raise SystemExit(f"refusing to overwrite existing bench: {args.output}")
    args.output.mkdir(parents=True)

    static_candidates = files(args.static_root, STATIC_EXTS, 64 * 1024)
    video_candidates = files(args.video_root, VIDEO_EXTS, 1024 * 1024)
    scene_candidates = scenes(args.we_root)
    selected_static = evenly(static_candidates, args.static_count)
    selected_video = evenly(video_candidates, args.video_count)
    selected_scenes = evenly(scene_candidates, args.we_count)

    manifest = {
        "schema": 1,
        "selection": "casefolded-path-even-sampling",
        "counts": {
            "static": len(selected_static),
            "video": len(selected_video),
            "we": len(selected_scenes),
        },
        "candidate_counts": {
            "static": len(static_candidates),
            "video": len(video_candidates),
            "we": len(scene_candidates),
        },
        "static": materialize_files(args.static_root, selected_static, args.output / "static"),
        "video": materialize_files(args.video_root, selected_video, args.output / "video"),
        "we": materialize_scenes(selected_scenes, args.output / "we"),
    }
    if args.we_assets_root:
        assets_target = args.output / "we-assets"
        shutil.copytree(
            args.we_assets_root,
            assets_target,
            copy_function=link_or_copy,
            symlinks=False,
        )
        manifest["we_assets"] = {
            "source": str(args.we_assets_root),
            "path": "we-assets",
            "files": sum(path.is_file() for path in assets_target.rglob("*")),
        }
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"output": str(args.output), **manifest["counts"]}))


if __name__ == "__main__":
    main()
