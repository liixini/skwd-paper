import importlib.util
import json
import pathlib
import unittest
from unittest import mock


def load_module():
    path = pathlib.Path(__file__).resolve().parents[1] / "plasma-sweep.py"
    spec = importlib.util.spec_from_file_location("plasma_sweep", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


OUTPUTS = [
    {
        "name": "DP-3",
        "width": 1920,
        "height": 1080,
        "logical_width": 1920,
        "logical_height": 1080,
        "scale": 1.0,
        "x": 0,
        "y": 0,
        "refresh_mhz": 144000,
    },
    {
        "name": "DP-1",
        "width": 2560,
        "height": 1440,
        "logical_width": 2560,
        "logical_height": 1440,
        "scale": 1.0,
        "x": 1920,
        "y": 0,
        "refresh_mhz": 165000,
    },
    {
        "name": "DP-2",
        "width": 3840,
        "height": 2160,
        "logical_width": 1920,
        "logical_height": 1080,
        "scale": 2.0,
        "x": 4480,
        "y": 0,
        "refresh_mhz": 60000,
    },
]
MAPPING = {"DP-3": 3, "DP-1": 21, "DP-2": 2}
NAMES = [output["name"] for output in OUTPUTS]


class PlasmaSweepTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.module = load_module()
        cls.module.PAPER = "/usr/bin/skwd-paper-v2"
        cls.module.LAYER_VK = "/usr/bin/skwd-wall-vk"
        cls.module.LAYER_STILL = "/usr/bin/skwd-wall-still"

    def matrix(self):
        return self.module.build_matrix(
            "/video/floor-vp9.webm",
            "/video/mid.mp4",
            "/video/max.mp4",
            "/image/still.webp",
            "/scene/2165290843",
        )

    def scenario(self, name):
        return next(scenario for scenario in self.matrix() if scenario.name == name)

    def test_matrix_pairs_every_plasma_engine_with_the_bridge_and_layer_rows(self):
        matrix = self.matrix()
        self.assertEqual(len(matrix), 36)
        self.assertEqual({scenario.topology for scenario in matrix}, {"single", "all"})
        for workload, engines in {
            "static": {"plasma-image", "skwd-paper-plasma", "skwd-paper-layer"},
            "video": {"smart-video-reborn", "skwd-paper-plasma", "skwd-paper-layer"},
            "we": {"skwd-paper-plasma", "skwd-paper-layer", "linux-wallpaperengine"},
        }.items():
            self.assertEqual(
                {scenario.engine for scenario in matrix if scenario.workload == workload},
                engines,
            )
        self.assertEqual(
            {scenario.fps for scenario in matrix if scenario.workload == "we"}, {30, 60}
        )
        self.assertTrue(self.scenario("static single plasma-image").in_process)
        self.assertTrue(self.scenario("video mid all skwd-paper-plasma").bridged)
        self.assertFalse(self.scenario("we 30fps all lwe").in_process)

    def test_smart_video_config_plays_one_muted_video_without_pausing(self):
        config = self.module.smart_video_config("/video/mid.mp4")["config"]
        videos = json.loads(config["VideoUrls"])
        self.assertEqual(len(videos), 1)
        self.assertEqual(videos[0]["filename"], "file:///video/mid.mp4")
        self.assertTrue(videos[0]["enabled"])
        self.assertTrue(videos[0]["loop"])
        self.assertEqual(config["PauseMode"], self.module.SMART_VIDEO_PAUSE_NEVER)
        self.assertEqual(config["MuteMode"], self.module.SMART_VIDEO_MUTE_ALWAYS)
        self.assertEqual(config["BlurMode"], self.module.SMART_VIDEO_BLUR_NEVER)
        self.assertFalse(config["CheckWindowsActiveScreen"])
        self.assertFalse(config["CrossfadeEnabled"])

    def test_plasma_image_config_uses_a_file_uri_and_crop_fill(self):
        wanted = self.module.image_config("/image/still.webp")
        self.assertEqual(wanted["plugin"], "org.kde.image")
        self.assertEqual(wanted["config"]["Image"], "file:///image/still.webp")
        self.assertEqual(wanted["config"]["FillMode"], 2)

    def test_paper_assignments_mirror_deck_payloads_per_output(self):
        scenario = self.scenario("we 60fps all skwd-paper-plasma")
        wanted = self.module.paper_config(scenario, OUTPUTS)
        self.assertEqual(wanted["plugin"], "org.skwd.wall.plasma")
        assignments = json.loads(wanted["config"]["Assignments"])
        self.assertEqual(sorted(assignments), sorted(NAMES))
        entry = assignments["DP-2"]
        self.assertEqual(entry["assignment"]["outputs"], ["DP-2"])
        self.assertEqual(entry["assignment"]["source"], {"kind": "we", "path": "/scene/2165290843"})
        self.assertTrue(entry["assignment"]["mute"])
        self.assertEqual((entry["width"], entry["height"]), (1920, 1080))
        self.assertEqual(entry["fps"], 60)
        self.assertEqual(entry["paper"], "/usr/bin/skwd-paper-v2")
        self.assertFalse(entry["paused"])
        video = self.module.paper_assignment(self.scenario("video mid single skwd-paper-plasma"), OUTPUTS[0])
        self.assertEqual(video["assignment"]["source"], {"kind": "video", "path": "/video/mid.mp4", "engine": "default"})
        self.assertEqual(video["fps"], 30)
        still = self.module.paper_assignment(self.scenario("static single skwd-paper-plasma"), OUTPUTS[0])
        self.assertEqual(still["assignment"]["source"], {"kind": "static", "path": "/image/still.webp"})

    def test_single_topology_targets_one_desktop_and_colours_the_rest(self):
        scenario = self.scenario("video floor single smart-video-reborn")
        assignments = self.module.desktop_assignments(scenario, "DP-3", OUTPUTS, MAPPING)
        self.assertEqual(assignments["3"]["plugin"], "luisbocanegra.smart.video.wallpaper.reborn")
        self.assertEqual(assignments["21"]["plugin"], "org.kde.color")
        self.assertEqual(assignments["2"]["plugin"], "org.kde.color")
        everywhere = self.module.desktop_assignments(
            self.scenario("static all plasma-image"), "DP-3", OUTPUTS, MAPPING
        )
        self.assertEqual({item["plugin"] for item in everywhere.values()}, {"org.kde.image"})
        layer = self.module.desktop_assignments(
            self.scenario("we 30fps all skwd-paper-layer"), "DP-3", OUTPUTS, MAPPING
        )
        self.assertEqual({item["plugin"] for item in layer.values()}, {"org.kde.color"})
        bridge = self.module.desktop_assignments(
            self.scenario("video max single skwd-paper-plasma"), "DP-3", OUTPUTS, MAPPING
        )
        self.assertEqual(list(json.loads(bridge["3"]["config"]["Assignments"])), ["DP-3"])

    def test_screen_map_matches_desktop_ids_by_geometry_not_priority(self):
        geometries = [
            {"screen": 0, "id": 21, "x": 1920, "y": 0, "width": 2560, "height": 1440},
            {"screen": 1, "id": 2, "x": 4480, "y": 0, "width": 1309, "height": 2327},
            {"screen": 2, "id": 3, "x": 0, "y": 0, "width": 1920, "height": 1080},
        ]
        outputs = [dict(output) for output in OUTPUTS]
        self.assertEqual(
            self.module.screen_map(outputs, geometries), {"DP-3": 3, "DP-1": 21, "DP-2": 2}
        )
        rotated = next(output for output in outputs if output["name"] == "DP-2")
        self.assertEqual((rotated["logical_width"], rotated["logical_height"]), (1309, 2327))
        with self.assertRaises(RuntimeError):
            self.module.screen_map(outputs, geometries[:2])
        with self.assertRaises(RuntimeError):
            self.module.screen_map(outputs, [{**geometries[0], "id": None}, *geometries[1:]])

    def test_kscreen_outputs_use_the_current_mode_and_skip_disabled_outputs(self):
        text = json.dumps(
            {
                "outputs": [
                    {
                        "name": "DP-2",
                        "connected": True,
                        "enabled": True,
                        "pos": {"x": 4480, "y": 0},
                        "scale": 2.0,
                        "currentModeId": "2",
                        "modes": [
                            {"id": "1", "size": {"width": 1920, "height": 1080}, "refreshRate": 60.0},
                            {"id": "2", "size": {"width": 3840, "height": 2160}, "refreshRate": 59.997},
                        ],
                    },
                    {
                        "name": "HDMI-A-1",
                        "connected": True,
                        "enabled": False,
                        "pos": {"x": 0, "y": 0},
                        "currentModeId": "1",
                        "modes": [{"id": "1", "size": {"width": 1920, "height": 1080}}],
                    },
                ]
            }
        )
        outputs = self.module.parse_kscreen_outputs(text)
        self.assertEqual(len(outputs), 1)
        self.assertEqual(outputs[0]["name"], "DP-2")
        self.assertEqual((outputs[0]["width"], outputs[0]["height"]), (3840, 2160))
        self.assertEqual((outputs[0]["logical_width"], outputs[0]["logical_height"]), (1920, 1080))
        self.assertEqual(outputs[0]["refresh_mhz"], 59997)

    def test_bridge_roots_are_stream_children_of_plasmashell_only(self):
        table = [
            (100, 1, "plasmashell"),
            (200, 100, "/usr/bin/skwd-paper-v2 plasma --video-stream /video/mid.mp4"),
            (201, 100, "/usr/bin/skwd-paper-v2 plasma --frame-stream 1920x1080"),
            (202, 200, "/usr/bin/skwd-wall-vk --stream-fd 7"),
            (300, 100, "/usr/bin/plasma-discover"),
            (400, 1, "/usr/bin/skwd-wall-vk DP-3 /video/mid.mp4 --mute"),
        ]
        self.assertEqual(self.module.bridge_roots(table, 100), [200, 201])
        self.assertEqual(self.module.bridge_streams(table, 100), 2)
        shared = [
            (100, 1, "plasmashell"),
            (200, 100, "skwd-wall-vk --video-stream /v.mp4 --stream-size 1x1 --stream-fd 3 --stream-size 2x2 --stream-fd 4 --stream-size 3x3 --stream-fd 5"),
            (201, 100, "skwd-wall-still * /a.png --frame-stream 1x1 --frame-fd 4 --frame-stream 2x2 --frame-fd 6"),
        ]
        self.assertEqual(self.module.bridge_streams(shared, 100), 5)
        self.assertEqual(self.module.bridge_roots(shared, 100), [200, 201])

    def test_layer_commands_match_the_niri_sweep_shape(self):
        video = self.module.layer_command(self.scenario("video mid single skwd-paper-layer"), "DP-3", OUTPUTS)
        self.assertEqual(video, ["/usr/bin/skwd-wall-vk", "DP-3", "/video/mid.mp4", "--mute"])
        scene = self.module.layer_command(self.scenario("we 60fps all skwd-paper-layer"), "DP-3", OUTPUTS)
        self.assertEqual(scene[:2], ["/usr/bin/skwd-wall-vk", "*"])
        self.assertIn("--scene", scene)
        still = self.module.layer_command(self.scenario("static single skwd-paper-layer"), "DP-3", OUTPUTS)
        self.assertEqual(still[0], "/usr/bin/skwd-wall-still")
        self.assertIn("--persist", still)
        lwe = self.module.layer_command(self.scenario("we 30fps all lwe"), "DP-3", OUTPUTS)
        self.assertEqual(lwe.count("--screen-root"), 3)
        self.assertEqual(lwe[-1], "2165290843")
        self.assertIn("30", lwe)
        with self.assertRaises(ValueError):
            self.module.layer_command(self.scenario("static single plasma-image"), "DP-3", OUTPUTS)

    def test_host_recovery_tolerates_plasmashell_more_than_kwin(self):
        before = {"rss": 300, "pss": 280, "vram": 100}
        self.assertTrue(self.module.host_recovered("plasmashell", before, {"rss": 320, "pss": 300, "vram": 130}))
        self.assertFalse(self.module.host_recovered("kwin_wayland", before, {"rss": 320, "pss": 300, "vram": 130}))
        self.assertTrue(self.module.host_recovered("kwin_wayland", before, {"rss": 306, "pss": 286, "vram": 112}))

    def test_baseline_metrics_qualify_both_hosts_and_bracket_idle_floors(self):
        def summary(rss, pss, vram):
            return {"snapshot": {"rss": rss, "pss": pss, "vram": vram}, "spread": {"rss": 0, "pss": 0, "vram": 0}, "samples": []}

        before = {"plasmashell": summary(300, 280, 100), "kwin_wayland": summary(200, 180, 60)}
        after = {"plasmashell": summary(310, 290, 100), "kwin_wayland": summary(202, 182, 60)}
        floor_before = {"gpu": 1.0, "power": 40.0, "cpu": {"plasmashell": 0.5, "kwin_wayland": 1.0}}
        floor_after = {"gpu": 1.4, "power": 41.0, "cpu": {"plasmashell": 0.7, "kwin_wayland": 1.2}}
        result = {
            "gpu_mean": 5.2,
            "power_mean": 50.0,
            "pss": 40,
            "compositor": {"rss": 360, "pss": 340, "vram": 180, "cpu": 6.0},
            "kwin": {"rss": 205, "pss": 185, "vram": 70, "cpu": 3.1},
        }
        self.module.finalize_baseline_metrics(result, before, after, floor_before, floor_after)
        qualified = result["baseline"]["qualified"]
        self.assertTrue(all(qualified.values()))
        self.assertEqual(result["compositor"]["rss_delta"], 55.0)
        self.assertEqual(result["compositor"]["vram_delta"], 80.0)
        self.assertEqual(result["compositor"]["cpu_delta"], 5.4)
        self.assertEqual(result["kwin"]["rss_delta"], 4.0)
        self.assertEqual(result["kwin"]["cpu_delta"], 2.0)
        self.assertEqual(result["gpu_delta"], 4.0)
        self.assertEqual(result["power_delta"], 9.5)
        self.assertEqual(result["combined_pss"], 95.0)
        drifted = {"gpu": 3.0, "power": 41.0, "cpu": {"plasmashell": 0.7, "kwin_wayland": 4.0}}
        self.module.finalize_baseline_metrics(dict(result), before, after, floor_before, drifted)
        unqualified = self.module.finalize_baseline_metrics(dict(result), before, after, floor_before, drifted)
        self.assertFalse(unqualified["baseline"]["qualified"]["gpu"])
        self.assertTrue(unqualified["baseline"]["qualified"]["compositor_cpu"])
        self.assertFalse(unqualified["baseline"]["qualified"]["kwin_cpu"])

    def test_apply_script_resets_to_colour_before_switching_plugins(self):
        with mock.patch.object(self.module, "plasma_script") as script:
            self.module.apply_assignments({"3": self.module.color_config()})
        source = script.call_args.args[0]
        self.assertIn("assignments[String(d.id)]", source)
        self.assertIn('d.wallpaperPlugin = "org.kde.color";', source)
        self.assertIn("d.writeConfig(key, wanted.config[key]);", source)
        self.assertIn("d.wallpaperPlugin = wanted.plugin;", source)


if __name__ == "__main__":
    unittest.main()
