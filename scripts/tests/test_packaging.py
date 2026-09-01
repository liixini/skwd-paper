import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/package-stage.sh"
MANIFEST = ROOT / "packaging/manifest.txt"
FFMPEG_LICENSE = ROOT / "LICENSES/ffmpeg-sys-the-third-WTFPL.txt"
FFMPEG_LICENSE_DESTINATION = (
    "usr/share/licenses/skwd-paper/third-party/ffmpeg-sys-the-third-LICENSE"
)
BINARIES = (
    "usr/bin/skwd-paper-v2",
    "usr/bin/skwd-wall-still",
    "usr/bin/skwd-wall-vk",
    "usr/lib/skwd-paper/skwd-paper-tinier",
)
LICENSES = (
    "usr/share/licenses/skwd-paper/LICENSE",
    "usr/share/licenses/skwd-paper/third-party/OpenH264-BSD-2-Clause.txt",
    "usr/share/licenses/skwd-paper/third-party/dav1d-BSD-2-Clause.txt",
    "usr/share/licenses/skwd-paper/third-party/ffmpeg-sys-the-third-LICENSE",
    "usr/share/licenses/skwd-paper/third-party/libyuv-BSD-3-Clause.txt",
)
EXPECTED = BINARIES + LICENSES


def release_binaries(directory: Path, names=BINARIES) -> None:
    directory.mkdir(parents=True)
    for entry in names:
        name = "skwd-paper" if entry == "usr/bin/skwd-paper-v2" else Path(entry).name
        path = directory / name
        path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        path.chmod(0o755)


class PackagingTests(unittest.TestCase):
    def test_manifest_is_the_exact_sorted_base_package_contract(self):
        entries = tuple(MANIFEST.read_text(encoding="utf-8").splitlines())
        self.assertEqual(entries, EXPECTED)
        self.assertEqual(entries, tuple(sorted(entries)))

    def test_stage_contains_exact_manifest_with_stable_names_and_modes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "release"
            destination = root / "stage"
            release_binaries(binaries)
            environment = {**os.environ, "SKWD_PAPER_BIN_DIR": str(binaries)}
            subprocess.run(
                ["sh", str(SCRIPT), str(destination)],
                cwd=root,
                env=environment,
                check=True,
                capture_output=True,
                text=True,
            )

            files = tuple(
                sorted(
                    str(path.relative_to(destination))
                    for path in destination.rglob("*")
                    if path.is_file()
                )
            )
            self.assertEqual(files, EXPECTED)
            for entry in BINARIES:
                mode = stat.S_IMODE((destination / entry).stat().st_mode)
                self.assertEqual(mode, 0o755)
            for entry in LICENSES:
                mode = stat.S_IMODE((destination / entry).stat().st_mode)
                self.assertEqual(mode, 0o644)
            self.assertEqual(FFMPEG_LICENSE.stat().st_size, 432)
            self.assertEqual(
                (destination / FFMPEG_LICENSE_DESTINATION).read_bytes(),
                FFMPEG_LICENSE.read_bytes(),
            )

    def test_stage_fails_before_writing_when_the_public_cli_is_missing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "release"
            destination = root / "stage"
            release_binaries(binaries, tuple(entry for entry in BINARIES if entry != "usr/bin/skwd-paper-v2"))
            environment = {**os.environ, "SKWD_PAPER_BIN_DIR": str(binaries)}
            result = subprocess.run(
                ["sh", str(SCRIPT), str(destination)],
                cwd=root,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("skwd-paper", result.stderr)
            self.assertFalse(destination.exists())

    def test_stage_fails_before_writing_when_a_compatibility_worker_is_missing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "release"
            destination = root / "stage"
            release_binaries(binaries, tuple(entry for entry in BINARIES if entry != "usr/bin/skwd-wall-vk"))
            environment = {**os.environ, "SKWD_PAPER_BIN_DIR": str(binaries)}
            result = subprocess.run(
                ["sh", str(SCRIPT), str(destination)],
                cwd=root,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("skwd-wall-vk", result.stderr)
            self.assertFalse(destination.exists())

    def test_stage_fails_before_writing_when_the_private_worker_is_missing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "release"
            destination = root / "stage"
            release_binaries(
                binaries,
                tuple(entry for entry in BINARIES if entry != "usr/lib/skwd-paper/skwd-paper-tinier"),
            )
            environment = {**os.environ, "SKWD_PAPER_BIN_DIR": str(binaries)}
            result = subprocess.run(
                ["sh", str(SCRIPT), str(destination)],
                cwd=root,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("skwd-paper-tinier", result.stderr)
            self.assertFalse(destination.exists())

    def test_stage_refuses_a_nonempty_destination_without_mutation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "release"
            destination = root / "stage"
            destination.mkdir()
            sentinel = destination / "unowned-file"
            sentinel.write_text("keep\n", encoding="utf-8")
            release_binaries(binaries)
            environment = {**os.environ, "SKWD_PAPER_BIN_DIR": str(binaries)}
            result = subprocess.run(
                ["sh", str(SCRIPT), str(destination)],
                cwd=root,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("not empty", result.stderr)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep\n")
            self.assertEqual(tuple(destination.iterdir()), (sentinel,))


if __name__ == "__main__":
    unittest.main()
