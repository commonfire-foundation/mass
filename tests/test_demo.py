"""The public demo and capture parser must not depend on personal state."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


demo = load("mass_demo", "demo.py")
capture = load("mass_capture", "capture-preview.py")


class DemoTests(unittest.TestCase):
    def test_sources_are_synthetic_and_disabled(self):
        with demo.fixture(ROOT / "target/release/mass") as (command, env, directory):
            config = tomllib.loads(Path(command[2]).read_text())
            self.assertEqual({s["agent"] for s in config["sources"]}, {"pi", "claude", "codex"})
            for source in config["sources"]:
                self.assertFalse(source["resume"])
                path = Path(source["root"])
                self.assertTrue(path.is_relative_to(directory))
                records = [json.loads(line) for line in (path / "demo.jsonl").read_text().splitlines()]
                self.assertGreaterEqual(len(records), 3)
            self.assertTrue(Path(env["HOME"]).is_relative_to(directory))
            self.assertEqual(list(Path(env["PATH"]).iterdir()), [])
        self.assertFalse(directory.exists())

    def test_environment_does_not_inherit_credentials_or_helpers(self):
        with patch.dict(os.environ, {"ANTHROPIC_API_KEY": "invented-test-value", "DISPLAY": ":99",
                                    "WAYLAND_DISPLAY": "invented", "PI_CODING_AGENT_DIR": "/not-a-source"}):
            with demo.fixture(ROOT / "target/release/mass") as (_, env, _):
                self.assertNotIn("ANTHROPIC_API_KEY", env)
                self.assertNotIn("DISPLAY", env)
                self.assertNotIn("WAYLAND_DISPLAY", env)
                self.assertNotIn("PI_CODING_AGENT_DIR", env)

    def test_fixture_cleans_up_after_failure(self):
        with self.assertRaisesRegex(RuntimeError, "synthetic failure"):
            with demo.fixture(ROOT / "target/release/mass") as (_, _, directory):
                raise RuntimeError("synthetic failure")
        self.assertFalse(directory.exists())

    def test_missing_binary_has_actionable_error(self):
        output = subprocess.run([sys.executable, str(ROOT / "scripts/demo.py"), "--check",
                                 "--binary", str(ROOT / "target/not-a-demo-binary")],
                                capture_output=True, text=True)
        self.assertEqual(output.returncode, 2)
        self.assertIn("Build first", output.stderr)

    def test_terminal_capture_tracks_real_cells_and_styles(self):
        screen = capture.Screen(8, 2)
        screen.feed(b"\x1b[2J\x1b[1;1H\x1b[36;1mHi\x1b[0m\x1b[2;1Hcaf\xc3")
        screen.feed(b"\xa9")
        self.assertTrue(screen.text.startswith("Hi"))
        self.assertEqual(screen.cells[0][0][1], capture.PALETTE[6])
        self.assertTrue(screen.cells[0][0][3])
        self.assertIn("caf\u00e9", screen.text)
        screen.feed(b"\x1b[2;1H\x1b[2K")
        self.assertEqual(screen.text.splitlines()[1], " " * 8)


if __name__ == "__main__":
    unittest.main()
