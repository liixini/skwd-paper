import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock


def load_module():
    path = pathlib.Path(__file__).resolve().parents[1] / "competitor-sweep.py"
    spec = importlib.util.spec_from_file_location("competitor_sweep", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CompetitorSweepTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.module = load_module()

    def matrix(self):
        return self.module.build_matrix(
            "/video/floor-vp9.webm",
            "/video/mid.mp4",
            "/video/max.mp4",
            "/image/still.webp",
            "/scene/2165290843",
        )

    def test_matrix_has_matched_workloads_and_topologies(self):
        matrix = self.matrix()
        self.assertEqual(len(matrix), 65)
        self.assertEqual({scenario.topology for scenario in matrix}, {"single", "all"})
        for workload, engines in {
            "static": {
                "skwd-wall-still",
                "awww",
                "hyprpaper",
                "wpaperd",
                "kacau-wall",
                "wallr",
                "yin",
            },
            "video": {
                "skwd-wall-vk",
                "mpvpaper",
                "phonto",
                "kacau-wall",
                "wallr",
                "yin",
            },
            "we": {"skwd-wall-vk", "linux-wallpaperengine"},
        }.items():
            self.assertEqual(
                {scenario.engine for scenario in matrix if scenario.workload == workload},
                engines,
            )

    def test_mpvpaper_matrix_has_default_and_hw_decode(self):
        mpv = [scenario for scenario in self.matrix() if scenario.engine == "mpvpaper"]
        self.assertEqual(len(mpv), 12)
        self.assertEqual({scenario.tuned for scenario in mpv}, {False, True})
        command = self.module.scenario_command(
            next(scenario for scenario in mpv if scenario.tuned),
            "DP-3",
            ["DP-3", "DP-1", "DP-2"],
        )
        self.assertIn("hwdec=auto", command[2])

    def test_resolution_matrix_has_current_video_products_once(self):
        matrix = self.module.build_resolution_matrix(
            "1080p", "/video/matched-1080p.av1.mp4"
        )
        self.assertEqual(len(matrix), 6)
        self.assertEqual({scenario.topology for scenario in matrix}, {"single"})
        self.assertEqual(
            {scenario.engine for scenario in matrix},
            {
                "skwd-wall-vk",
                "mpvpaper",
                "phonto",
                "wallr",
                "kacau-wall",
                "yin",
            },
        )
        self.assertEqual(sum(scenario.engine == "mpvpaper" for scenario in matrix), 1)
        mpvpaper = next(
            scenario for scenario in matrix if scenario.engine == "mpvpaper"
        )
        self.assertTrue(mpvpaper.tuned)
        for scenario in matrix:
            self.assertEqual(scenario.source, "/video/matched-1080p.av1.mp4")
            self.assertEqual(scenario.fps, 30)
            self.assertEqual(scenario.extra["comparison"], "matched-resolution")
            self.assertEqual(scenario.extra["resolution_label"], "1080p")

    def test_wallr_uses_isolated_config_and_explicit_targeting(self):
        scenario = self.module.build_resolution_matrix(
            "1080p", "/video/matched-1080p.av1.mp4"
        )[3]
        with tempfile.TemporaryDirectory() as directory:
            workdir = pathlib.Path(directory)
            config_path = self.module.write_wallr_config(workdir)
            config = config_path.read_text()
            command = self.module.wallr_apply_command(scenario, "DP-3", config_path)
        self.assertIn("hw_decode: auto", config)
        self.assertIn("provider: none", config)
        self.assertIn(str(workdir / "wallr.sock"), config)
        self.assertEqual(command[-2:], ["--monitor", "DP-3"])
        self.assertIn("--no-theme", command)

    def test_wpaperd_uses_isolated_matched_static_config(self):
        scenarios = [
            scenario for scenario in self.matrix() if scenario.engine == "wpaperd"
        ]
        self.assertEqual(len(scenarios), 1)
        self.assertEqual(scenarios[0].topology, "all")
        self.assertEqual(
            self.module.scenario_command(scenarios[0], "DP-3", ["DP-3"]),
            [self.module.WPAPERD],
        )
        with tempfile.TemporaryDirectory() as directory:
            config = self.module.write_wpaperd_config(
                scenarios[0], pathlib.Path(directory)
            ).read_text()
            self.assertIn("[any]", config)
            self.assertIn('path = "/image/still.webp"', config)
            self.assertIn('mode = "center"', config)
            self.assertIn("initial-transition = false", config)

    def test_hyprpaper_uses_explicit_matched_static_config(self):
        scenarios = [
            scenario for scenario in self.matrix() if scenario.engine == "hyprpaper"
        ]
        self.assertEqual(len(scenarios), 2)
        single = next(scenario for scenario in scenarios if scenario.topology == "single")
        all_outputs = next(scenario for scenario in scenarios if scenario.topology == "all")
        self.assertEqual(
            self.module.scenario_command(single, "DP-3", ["DP-3"]),
            [self.module.HYPRPAPER],
        )
        with tempfile.TemporaryDirectory() as directory:
            workdir = pathlib.Path(directory)
            single_config = self.module.write_hyprpaper_config(
                single, "DP-3", ["DP-3", "DP-1", "DP-2"], workdir
            ).read_text()
            self.assertEqual(single_config.count("wallpaper {"), 1)
            self.assertIn("monitor = DP-3", single_config)
            self.assertIn("path = /image/still.webp", single_config)
            self.assertIn("fit_mode = cover", single_config)
            self.assertIn("splash = false", single_config)
            self.assertIn("ipc = false", single_config)
            all_config = self.module.write_hyprpaper_config(
                all_outputs, "DP-3", ["DP-3", "DP-1", "DP-2"], workdir
            ).read_text()
            self.assertEqual(all_config.count("wallpaper {"), 3)
            for output in ("DP-3", "DP-1", "DP-2"):
                self.assertIn(f"monitor = {output}", all_config)

    def test_lwe_all_output_command_names_every_screen(self):
        scenario = next(
            scenario
            for scenario in self.matrix()
            if scenario.engine == "linux-wallpaperengine"
            and scenario.topology == "all"
            and scenario.fps == 60
        )
        command = self.module.scenario_command(
            scenario,
            "DP-3",
            ["DP-3", "DP-1", "DP-2"],
        )
        self.assertEqual(command.count("--screen-root"), 3)
        self.assertEqual(command[-1], "2165290843")

    def test_phonto_uses_explicit_outputs_and_reuses_the_source(self):
        scenarios = [
            scenario for scenario in self.matrix() if scenario.engine == "phonto"
        ]
        self.assertEqual(len(scenarios), 6)
        single = next(scenario for scenario in scenarios if scenario.topology == "single")
        single_command = self.module.scenario_command(
            single,
            "DP-3",
            ["DP-3", "DP-1", "DP-2"],
        )
        self.assertEqual(single_command.count("--display"), 1)
        self.assertIn("DP-3", single_command)

        all_outputs = next(
            scenario for scenario in scenarios if scenario.topology == "all"
        )
        all_command = self.module.scenario_command(
            all_outputs,
            "DP-3",
            ["DP-3", "DP-1", "DP-2"],
        )
        self.assertEqual(all_command.count("--display"), 3)
        self.assertEqual(all_command.count(all_outputs.source), 3)

    def test_yin_uses_cuda_daemon_and_distinct_floor_fixture(self):
        matrix = self.module.build_matrix(
            "/video/floor-vp9.webm",
            "/video/mid.mp4",
            "/video/max.mp4",
            "/image/still.webp",
            "/scene/2165290843",
            yin_floor_video="/video/floor-h264.mp4",
        )
        floor = next(
            scenario
            for scenario in matrix
            if scenario.engine == "yin"
            and scenario.workload == "video"
            and "floor" in scenario.name
        )
        self.assertEqual(floor.source, "/video/floor-h264.mp4")
        self.assertEqual(
            self.module.scenario_command(floor, "DP-3", ["DP-3"]),
            [self.module.YIN, "--use-cuda-copy"],
        )

    def test_kacau_uses_pinned_default_cached_gpu_video_path(self):
        scenarios = [
            scenario for scenario in self.matrix() if scenario.engine == "kacau-wall"
        ]
        self.assertEqual(len(scenarios), 10)
        self.assertEqual(
            {scenario.fps for scenario in scenarios if scenario.workload == "video"},
            {30, 60},
        )
        video = next(
            scenario
            for scenario in scenarios
            if scenario.workload == "video" and scenario.topology == "single"
        )
        self.assertEqual(
            self.module.scenario_command(video, "DP-3", ["DP-3"]),
            [self.module.KACAU],
        )
        with mock.patch.object(
            self.module.P,
            "media_info",
            return_value={"path": video.source},
        ):
            metadata = video.metadata("DP-3")
        self.assertEqual(metadata["revision"], self.module.KACAU_REVISION)
        self.assertIn("Vulkan Video", metadata["video_path"])
        matched = next(
            scenario
            for scenario in scenarios
            if scenario.workload == "video" and scenario.fps == 60
        )
        self.assertEqual(
            self.module.kacau_apply_command(matched, "DP-3")[-2:],
            ["--frame-rate", "60"],
        )

    def test_yin_socket_cleanup_only_removes_sockets(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "yin"
            path.write_text("not a socket")
            with self.assertRaisesRegex(RuntimeError, "non-socket"):
                self.module.remove_stale_yin_socket(path)
            self.assertTrue(path.is_file())
        ipc = mock.Mock()
        ipc.lstat.return_value = mock.Mock(st_mode=self.module.stat.S_IFSOCK | 0o600)
        self.module.remove_stale_yin_socket(ipc)
        ipc.unlink.assert_called_once_with()

    def test_kacau_socket_cleanup_only_removes_sockets(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "kacau-wall.sock"
            path.write_text("not a socket")
            with self.assertRaisesRegex(RuntimeError, "non-socket"):
                self.module.remove_stale_kacau_socket(path)
            self.assertTrue(path.is_file())
        ipc = mock.Mock()
        ipc.lstat.return_value = mock.Mock(st_mode=self.module.stat.S_IFSOCK | 0o600)
        self.module.remove_stale_kacau_socket(ipc)
        ipc.unlink.assert_called_once_with()

    def test_kacau_fatal_decoder_logs_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "stderr.log"
            with path.open("w+b") as error_log:
                error_log.write(b"worker panicked at gpu_video.rs:123\n")
                error_log.flush()
                self.assertEqual(
                    self.module.fatal_engine_log(error_log),
                    "panicked at",
                )

    def test_frame_content_signature_measures_and_hashes_central_crop(self):
        measured = mock.Mock(returncode=0, stdout="0.31 0.14", stderr="")
        pixels = mock.Mock(returncode=0, stdout=b"cropped pixels", stderr=b"")
        with mock.patch.object(
            self.module.subprocess, "run", side_effect=[measured, pixels]
        ) as run:
            digest, mean, deviation = self.module.frame_content_signature(
                pathlib.Path("/tmp/frame.ppm")
            )
        self.assertEqual(mean, 0.31)
        self.assertEqual(deviation, 0.14)
        self.assertEqual(len(digest), 64)
        for call in run.call_args_list:
            self.assertIn("-crop", call.args[0])
            self.assertIn("75%x75%+0+0", call.args[0])

    def test_video_validation_rejects_changing_black_frames(self):
        captured = mock.Mock(returncode=0, stdout="", stderr="")
        signatures = [
            ("first", 0.002, 0.001),
            ("second", 0.003, 0.002),
            ("third", 0.002, 0.002),
            ("fourth", 0.003, 0.001),
        ]
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(
            self.module.subprocess, "run", return_value=captured
        ), mock.patch.object(
            self.module, "wait_for_stable_file"
        ), mock.patch.object(
            self.module, "frame_content_signature", side_effect=signatures
        ), mock.patch.object(
            self.module.time, "sleep"
        ):
            with self.assertRaisesRegex(RuntimeError, "blank or near-black"):
                self.module.validate_video_motion(
                    ["DP-3"], pathlib.Path(directory)
                )

    def test_video_validation_accepts_motion_after_initial_duplicate(self):
        captured = mock.Mock(returncode=0, stdout="", stderr="")
        signatures = [
            ("first", 0.30, 0.14),
            ("first", 0.30, 0.14),
            ("second", 0.31, 0.15),
            ("third", 0.29, 0.13),
        ]
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(
            self.module.subprocess, "run", return_value=captured
        ), mock.patch.object(
            self.module, "wait_for_stable_file"
        ), mock.patch.object(
            self.module, "frame_content_signature", side_effect=signatures
        ), mock.patch.object(
            self.module.time, "sleep"
        ):
            result = self.module.validate_video_motion(
                ["DP-3"], pathlib.Path(directory)
            )
        self.assertEqual(result["DP-3"]["unique_frames"], 3)
        self.assertEqual(len(result["DP-3"]["hashes"]), 4)

    def test_visual_validation_requires_empty_active_workspace(self):
        workspaces = [
            {
                "output": "DP-3",
                "is_active": True,
                "active_window_id": 42,
            }
        ]
        queried = mock.Mock(
            returncode=0,
            stdout=self.module.json.dumps(workspaces),
            stderr="",
        )
        with mock.patch.object(self.module.subprocess, "run", return_value=queried):
            with self.assertRaisesRegex(RuntimeError, "empty active workspace"):
                self.module.require_empty_active_workspace("DP-3")

    def test_scenario_returns_visual_validation_failure_as_result(self):
        scenario = self.module.build_resolution_matrix(
            "1080p", "/video/matched-1080p.av1.mp4"
        )[0]
        process = mock.Mock(pid=123)
        process.poll.return_value = None
        error_log = mock.Mock()
        error_log.name = "/tmp/unused-stderr.log"
        floor = (1.0, 40.0)
        profile = {"settle": 0.0, "window": 1.0, "hz": 1.0}
        with mock.patch.object(
            self.module.P,
            "media_info",
            return_value={"path": "/video/matched-1080p.av1.mp4"},
        ), mock.patch.object(
            self.module.P, "process_snapshot", return_value={}
        ), mock.patch.object(
            self.module, "launch_scenario", return_value=(process, error_log, {})
        ), mock.patch.object(
            self.module, "rendered_outputs", return_value=["DP-3"]
        ), mock.patch.object(
            self.module, "fatal_engine_log", return_value=None
        ), mock.patch.object(
            self.module, "require_empty_active_workspace"
        ), mock.patch.object(
            self.module,
            "validate_video_motion",
            side_effect=RuntimeError("video validation failed"),
        ), mock.patch.object(
            self.module, "stop_process"
        ), mock.patch.object(
            self.module.time, "sleep"
        ):
            previous = self.module.VALIDATE_MOTION
            self.module.VALIDATE_MOTION = True
            try:
                result = self.module.run_scenario(
                    scenario, floor, profile, "DP-3", ["DP-3"], 456
                )
            finally:
                self.module.VALIDATE_MOTION = previous
        self.assertEqual(result["error"], "video validation failed")

    def test_pmon_vram_includes_graphics_and_compute_processes(self):
        output = """# gpu pid type fb ccpm command
    0 120 G 529 0 linux-wallpaper
    0 121 C+G 112 0 skwd-wall-vk
    0 999 G 400 0 browser
"""
        self.assertEqual(self.module.parse_pmon_vram(output, {120, 121}), 641)

    def test_rendered_outputs_filters_by_engine_namespace(self):
        layers = [
            {"namespace": "phonto", "output": "DP-2"},
            {"namespace": "skwd-wall-vk", "output": "DP-1"},
            {"namespace": "phonto", "output": "DP-1"},
            {"namespace": "hyprpaper", "output": "DP-3"},
            {"namespace": "wallr", "output": "DP-3"},
            {"namespace": "yin-wallpaper", "output": "DP-3"},
            {"namespace": "wpaperd-DP-2", "output": "DP-2"},
            {"namespace": "wpaperd-DP-1", "output": "DP-1"},
            {"namespace": "kacau:wall:DP-2", "output": "DP-2"},
            {"namespace": "kacau:wall:DP-1", "output": "DP-1"},
        ]
        completed = mock.Mock(returncode=0, stdout=self.module.json.dumps(layers))
        with mock.patch.object(self.module.subprocess, "run", return_value=completed):
            self.assertEqual(
                self.module.rendered_outputs("phonto"),
                ["DP-1", "DP-2"],
            )
            self.assertIsNone(self.module.rendered_outputs("awww"))
            self.assertEqual(self.module.rendered_outputs("hyprpaper"), ["DP-3"])
            self.assertEqual(self.module.rendered_outputs("wallr"), ["DP-3"])
            self.assertEqual(
                self.module.rendered_outputs("wpaperd"), ["DP-1", "DP-2"]
            )
            self.assertEqual(self.module.rendered_outputs("yin"), ["DP-3"])
            self.assertEqual(
                self.module.rendered_outputs("kacau-wall"),
                ["DP-1", "DP-2"],
            )


if __name__ == "__main__":
    unittest.main()
