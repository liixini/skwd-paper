import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock


def load_module():
    path = pathlib.Path(__file__).resolve().parents[1] / "perf-sweep.py"
    spec = importlib.util.spec_from_file_location("perf_sweep", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PerfSweepMatrixTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.module = load_module()
        cls.original_media_info = cls.module.media_info
        cls.module.media_info = cls.media_info

    @classmethod
    def tearDownClass(cls):
        cls.module.media_info = cls.original_media_info

    @staticmethod
    def media_info(path):
        path = str(path)
        width, height = (1366, 768) if "floor" in path else (2560, 1440)
        codec = next(
            (candidate for candidate in ("av1", "vp9", "h264", "hevc") if candidate in path),
            "av1",
        )
        return {
            "path": path,
            "width": width,
            "height": height,
            "codec": codec,
            "frame_rate": "30/1",
            "size": 1,
            "mtime_ns": 1,
        }

    def matrix(self, single_output="DP-1", max_output="*"):
        outputs = [
            {"name": "DP-3", "width": 1920, "height": 1080},
            {"name": "DP-1", "width": 2560, "height": 1440},
            {"name": "DP-2", "width": 3840, "height": 2160},
        ]
        scenes = ["/scene/reference", "/scene/stress"]
        return self.module.build_matrix(
            False,
            {
                "av1": "/video/floor-av1.mp4",
                "vp9": "/video/floor-vp9.webm",
                "h264": "/video/floor-h264.mp4",
                "hevc": "/video/floor-hevc.mp4",
            },
            "/video/mid.mp4",
            "/video/max.mp4",
            "/video/max2.mp4",
            "/image/a.webp",
            "/image/b.webp",
            single_output,
            max_output,
            "/scene/reference",
            outputs,
            scenes,
        )

    def test_default_matrix_always_covers_low_mid_max_vulkan_and_transitions(self):
        matrix = self.matrix()
        self.module.validate_matrix(
            matrix,
            {
                "av1": "/video/floor-av1.mp4",
                "vp9": "/video/floor-vp9.webm",
                "h264": "/video/floor-h264.mp4",
                "hevc": "/video/floor-hevc.mp4",
            },
            (1366, 768),
            "DP-1",
            "*",
            [
                {"name": "DP-3", "width": 1920, "height": 1080},
                {"name": "DP-1", "width": 2560, "height": 1440},
                {"name": "DP-2", "width": 3840, "height": 2160},
            ],
            ["/scene/reference", "/scene/stress"],
        )
        self.assertEqual({scenario.tier for scenario in matrix}, {"low", "mid", "max"})
        self.assertEqual(self.module.MATRIX_VERSION, 18)
        self.assertEqual(len(matrix), 75)
        for codec in ("av1", "vp9", "h264", "hevc"):
            source = next(path for path in {
                "av1": "/video/floor-av1.mp4",
                "vp9": "/video/floor-vp9.webm",
                "h264": "/video/floor-h264.mp4",
                "hevc": "/video/floor-hevc.mp4",
            }.values() if codec in path)
            self.assertTrue(any(
                scenario.env.get("SKWD_VK_PATH", "dmabuf-present") == "dmabuf-present"
                for scenario in matrix
                if scenario.tier == "low" and scenario.src == source
            ))
        self.assertTrue(any(scenario.tier == "max" and scenario.swap_to for scenario in matrix))

    def test_matrix_metadata_records_source_output_and_render_paths(self):
        metadata = [scenario.metadata() for scenario in self.matrix()]
        floor = next(item for item in metadata if item["name"] == "low vk av1 floor")
        self.assertEqual(floor["tier"], "low")
        self.assertEqual(floor["output"], "DP-1")
        self.assertEqual(floor["source"]["width"], 1366)
        self.assertEqual(floor["source"]["height"], 768)
        self.assertEqual(floor["path"], "dmabuf-present")
        self.assertEqual(floor["decode"], "auto")
        self.assertEqual(floor["present"], "dmabuf")
        linear = next(
            item for item in metadata if item["name"] == "low vk av1 floor nv12 linear"
        )
        self.assertEqual(linear["nv12_modifier"], "linear")
        self.assertEqual(linear["nv12_backend"], "presenter")
        scene = next(item for item in metadata if item["name"] == "mid WE scene native")
        self.assertEqual(scene["workload"], "we-scene")
        self.assertEqual(scene["source"]["type"], "we-scene")
        mixed = next(item for item in metadata if item["name"] == "max mixed video static WE")
        self.assertEqual(mixed["topology"], "mixed")
        self.assertEqual(
            {placement["kind"] for placement in mixed["placements"]},
            {"video", "static", "we-scene"},
        )

    def test_matrix_signature_changes_with_output_topology(self):
        single = self.module.matrix_id(self.matrix())
        changed = self.module.matrix_id(self.matrix(single_output="eDP-1"))
        self.assertNotEqual(single, changed)

    def test_steady_static_uses_real_static_renderer(self):
        steady = self.module.Scenario(
            "static",
            "mid",
            "/image/a.webp",
            "DP-1",
        )
        transition = self.module.Scenario(
            "transition",
            "max",
            "/image/a.webp",
            "DP-1",
            swap_to="/image/b.webp",
        )
        self.assertEqual(
            self.module.scenario_commands(steady, {"swap_ms": 1000})[0][0],
            str(self.module.STILL),
        )
        self.assertEqual(
            self.module.scenario_commands(transition, {"swap_ms": 1000})[0][0],
            str(self.module.VK),
        )

    def test_startup_transition_stays_alive_for_measurement(self):
        scenario = self.module.Scenario(
            "WE to static",
            "max",
            "/image/a.webp",
            "DP-1",
            start_from="/scene/preview.gif",
        )
        command = self.module.scenario_commands(scenario, {"swap_ms": 1000})[0]
        self.assertIn("--transition-from", command)
        self.assertIn("--persist", command)

    def test_mixed_content_routes_each_real_renderer(self):
        scenario = self.module.Scenario(
            "mixed",
            "max",
            "/video/max.mp4",
            "mixed",
            placements=[
                self.module.Placement("DP-1", "/video/max.mp4"),
                self.module.Placement("DP-2", "/image/a.webp"),
                self.module.Placement("DP-3", "/scene/reference", scene=True),
            ],
        )
        commands = self.module.scenario_commands(scenario, {"swap_ms": 1000})
        self.assertEqual([command[0] for command in commands].count(str(self.module.VK)), 2)
        self.assertEqual([command[0] for command in commands].count(str(self.module.STILL)), 1)

    def test_independent_same_video_uses_one_renderer_per_output(self):
        scenario = next(
            item for item in self.matrix() if item.name == "max same video independent outputs"
        )
        commands = self.module.scenario_commands(scenario, {"swap_ms": 1000})
        self.assertEqual(len(commands), 3)
        self.assertEqual({command[2] for command in commands}, {"/video/max.mp4"})
        self.assertEqual({command[1] for command in commands}, {"DP-1", "DP-2", "DP-3"})

    def test_same_video_matrix_compares_adaptive_native_and_max_exports(self):
        scenarios = {scenario.name: scenario for scenario in self.matrix()}
        adaptive = scenarios["max same video adaptive all outputs"]
        xr24 = scenarios["max same video adaptive xr24 steady"]
        native = scenarios["max same video shared native exports"]
        largest = scenarios["max same video shared max export"]
        self.assertNotIn("SKWD_VK_REUSE_EXPORT", adaptive.env)
        self.assertEqual(xr24.env["SKWD_VK_HYBRID_NV12"], "0")
        self.assertEqual(native.env["SKWD_VK_REUSE_EXPORT"], "native")
        self.assertEqual(largest.env["SKWD_VK_REUSE_EXPORT"], "max")

    def test_nv12_presenter_ab_pair_is_matched_except_for_backend(self):
        pair = [
            scenario
            for scenario in self.matrix()
            if "nv12 presenter A/B" in scenario.name
        ]
        self.assertEqual(len(pair), 2)
        by_backend = {
            scenario.env["SKWD_VK_NV12_BACKEND"]: scenario for scenario in pair
        }
        self.assertEqual(set(by_backend), {"presenter", "renderer"})
        presenter = by_backend["presenter"]
        renderer = by_backend["renderer"]
        self.assertIn("transfer-only", presenter.name)
        self.assertIn("full-renderer", renderer.name)
        self.assertEqual(presenter.src, "/video/floor-h264.mp4")
        presenter_env = {
            key: value
            for key, value in presenter.env.items()
            if key != "SKWD_VK_NV12_BACKEND"
        }
        renderer_env = {
            key: value
            for key, value in renderer.env.items()
            if key != "SKWD_VK_NV12_BACKEND"
        }
        self.assertEqual(presenter_env, renderer_env)
        presenter_metadata = presenter.metadata()
        renderer_metadata = renderer.metadata()
        for metadata in (presenter_metadata, renderer_metadata):
            metadata.pop("name")
            metadata.pop("nv12_backend")
        self.assertEqual(presenter_metadata, renderer_metadata)
        self.assertEqual(presenter.metadata()["nv12_modifier"], "negotiated")

    def test_existing_nv12_rows_are_pinned_to_presenter(self):
        scenarios = {scenario.name: scenario for scenario in self.matrix()}
        for name in (
            "low vk av1 floor nv12",
            "low vk av1 floor nv12 linear",
            "mid video hw nv12",
        ):
            self.assertEqual(
                scenarios[name].env["SKWD_VK_NV12_BACKEND"],
                "presenter",
            )

    def test_missing_low_vulkan_codec_fails_validation(self):
        matrix = [
            scenario
            for scenario in self.matrix()
            if scenario.name != "low vk av1 floor"
        ]
        with self.assertRaises(SystemExit):
            self.module.validate_matrix(
                matrix,
                {
                    "av1": "/video/floor-av1.mp4",
                    "vp9": "/video/floor-vp9.webm",
                    "h264": "/video/floor-h264.mp4",
                    "hevc": "/video/floor-hevc.mp4",
                },
                (1366, 768),
                "DP-1",
                "*",
                [
                    {"name": "DP-3", "width": 1920, "height": 1080},
                    {"name": "DP-1", "width": 2560, "height": 1440},
                    {"name": "DP-2", "width": 3840, "height": 2160},
                ],
                ["/scene/reference", "/scene/stress"],
            )

    def test_process_pids_matches_exact_comm_names(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            for pid, name in (("12", "niri"), ("34", "niri"), ("56", "niri-session")):
                process = root / pid
                process.mkdir()
                (process / "comm").write_text(f"{name}\n")
            (root / "self").mkdir()
            self.assertEqual(self.module.process_pids("niri", root), [12, 34])

    def test_compositor_prefers_systemd_main_pid_for_renamed_binary(self):
        with mock.patch.object(self.module, "user_service_main_pid", return_value=1137):
            with mock.patch.object(self.module, "process_pids", return_value=[]):
                self.assertEqual(self.module.require_compositor("niri"), 1137)

    def test_pmon_parser_and_compositor_deltas_are_explicit(self):
        output = """# gpu pid type fb ccpm command
    0 120 G 529 0 niri
    0 121 C+G 112 0 skwd-wall-vk
    0 999 G 400 0 browser
"""
        self.assertEqual(self.module.parse_pmon_vram(output, {120, 121}), 641)
        metrics = self.module.compositor_metrics(
            "niri",
            120,
            {"rss": 100, "pss": 80, "vram": 500},
            {"rss": 130, "pss": 95, "vram": 540},
            10.0,
            10.25,
            2.0,
            4.0,
        )
        self.assertEqual(metrics["cpu"], 12.5)
        self.assertEqual(metrics["cpu_idle"], 4.0)
        self.assertEqual(metrics["cpu_delta"], 8.5)
        self.assertEqual(metrics["rss_delta"], 30)
        self.assertEqual(metrics["pss_delta"], 15)
        self.assertEqual(metrics["vram_delta"], 40)

    def test_drm_fdinfo_vram_units_are_normalized(self):
        text = "\n".join(
            [
                "drm-memory-vram: 2048 KiB",
                "drm-memory-gtt: 2 MiB",
                "drm-memory-total: 1048576 bytes",
            ]
        )
        self.assertEqual(self.module.drm_vram_kib(text), 5120)

    def test_sysfs_sampler_supports_amd_without_nvidia_smi(self):
        with tempfile.TemporaryDirectory() as directory:
            device = pathlib.Path(directory)
            (device / "gpu_busy_percent").write_text("37\n")
            hwmon = device / "hwmon/hwmon0"
            hwmon.mkdir(parents=True)
            (hwmon / "power1_average").write_text("12500000\n")
            original_devices = self.module.DRM_DEVICES
            original_smi = self.module.NVIDIA_SMI
            self.module.DRM_DEVICES = [{"path": device, "vendor": "0x1002"}]
            self.module.NVIDIA_SMI = None
            try:
                self.assertEqual(self.module.gpu_sample(), (37.0, 12.5))
                metadata = self.module.gpu_sampler_metadata()
                self.assertEqual(metadata["backend"], "drm-sysfs")
                self.assertEqual(metadata["vendors"], ["0x1002"])
                self.assertTrue(metadata["gpu_utilization"])
                self.assertTrue(metadata["board_power"])
            finally:
                self.module.DRM_DEVICES = original_devices
                self.module.NVIDIA_SMI = original_smi

    def test_drm_fdinfo_fallback_uses_one_client_total(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fdinfo = root / "42/fdinfo"
            fdinfo.mkdir(parents=True)
            (fdinfo / "3").write_text("drm-memory-vram: 2048 KiB\n")
            (fdinfo / "4").write_text("drm-memory-vram: 2048 KiB\n")
            original = self.module.NVIDIA_SMI
            self.module.NVIDIA_SMI = None
            try:
                self.assertEqual(self.module.proc_vram(42, root), 2)
            finally:
                self.module.NVIDIA_SMI = original


if __name__ == "__main__":
    unittest.main()
