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
        self.assertEqual(len(matrix), 60)
        self.assertEqual({scenario.topology for scenario in matrix}, {"single", "all"})
        for workload, engines in {
            "static": {
                "skwd-wall-still",
                "awww",
                "hyprpaper",
                "wpaperd",
                "kacau-wall",
                "yin",
            },
            "video": {
                "skwd-wall-vk",
                "live-paper",
                "mpvpaper",
                "phonto",
                "kacau-wall",
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

    def test_live_paper_matrix_is_single_output_with_isolated_defaults(self):
        scenarios = [
            scenario for scenario in self.matrix() if scenario.engine == "live-paper"
        ]
        self.assertEqual(len(scenarios), 3)
        self.assertEqual({scenario.topology for scenario in scenarios}, {"single"})
        scenario = scenarios[0]
        self.assertEqual(
            self.module.scenario_command(scenario, "DP-3", ["DP-3"]),
            [self.module.LIVE_PAPER, scenario.source],
        )
        with tempfile.TemporaryDirectory() as directory:
            config = self.module.write_live_paper_config(
                pathlib.Path(directory)
            ).read_text()
        self.assertIn('hwdec = "auto"', config)
        self.assertIn('ao = "null"', config)
        self.assertIn("on_fullscreen = false", config)
        with mock.patch.object(self.module.subprocess, "run") as run:
            run.return_value = mock.Mock(returncode=0)
            self.module.focus_niri_output("DP-3")
        self.assertEqual(
            run.call_args.args[0],
            ["niri", "msg", "action", "focus-monitor", "DP-3"],
        )

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
            {"namespace": "live-paper", "output": "DP-2"},
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
            self.assertEqual(self.module.rendered_outputs("live-paper"), ["DP-2"])
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
