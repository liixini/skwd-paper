import importlib.util
import json
import pathlib
import unittest
from unittest import mock


def load_module():
    path = pathlib.Path(__file__).resolve().parents[1] / "perf-plasma-mixed.py"
    spec = importlib.util.spec_from_file_location("perf_plasma_mixed", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PerfPlasmaMixedTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.module = load_module()

    def test_display_tool_is_explicit_and_uses_deck_interface(self):
        outputs = [
            {"name": "DP-1", "width": 1920, "height": 1080},
            {"name": "DP-2", "width": 2560, "height": 1440},
            {"name": "DP-3", "width": 3840, "height": 2160},
        ]
        completed = mock.Mock(returncode=0, stdout=json.dumps(outputs), stderr="")
        with mock.patch.object(
            self.module.subprocess, "run", return_value=completed
        ) as run:
            self.assertEqual(self.module.displays("/opt/skwd-helm"), outputs)
        run.assert_called_once_with(
            ["/opt/skwd-helm", "displays", "--json"],
            capture_output=True,
            text=True,
            timeout=10,
        )

    def test_display_tool_requires_three_outputs(self):
        completed = mock.Mock(returncode=0, stdout="[]", stderr="")
        with mock.patch.object(self.module.subprocess, "run", return_value=completed):
            with self.assertRaisesRegex(RuntimeError, "three outputs required"):
                self.module.displays("skwd-helm")


if __name__ == "__main__":
    unittest.main()
