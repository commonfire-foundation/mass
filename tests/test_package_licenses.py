"""License collection must cover the built graph and fail closed on omissions."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class LicenseTests(unittest.TestCase):
    def collect(self, graph):
        (ROOT / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="licenses-") as tmp:
            root = Path(tmp)
            active = root / "active"
            inactive = root / "inactive"
            active.mkdir()
            inactive.mkdir()
            (active / "LICENSE").write_text("Synthetic MIT license notice")
            packages = [
                {"id": "root", "name": "mass", "version": "0.1.2"},
                {"id": "active", "name": "active", "version": "1.0.0", "license": "MIT",
                 "manifest_path": str(active / "Cargo.toml")},
                {"id": "inactive", "name": "inactive", "version": "2.0.0", "license": "MIT",
                 "manifest_path": str(inactive / "Cargo.toml")},
            ]
            tree = root / "tree.txt"
            tree.write_text(graph)
            return subprocess.run([sys.executable, str(ROOT / "scripts/package-licenses.py"),
                                   "--packages", str(tree)],
                                  input=json.dumps({"resolve": {"root": "root"}, "packages": packages}),
                                  capture_output=True, text=True)

    def test_inactive_optional_packages_are_not_shipped_dependencies(self):
        output = self.collect("mass v0.1.2 (/synthetic/source)\nactive v1.0.0\nactive v1.0.0 (*)\n")
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assertIn("Synthetic MIT license notice", output.stdout)
        self.assertNotIn("inactive", output.stdout)

    def test_missing_notice_for_selected_dependency_fails(self):
        output = self.collect("mass v0.1.2\ninactive v2.0.0\n")
        self.assertNotEqual(output.returncode, 0)
        self.assertIn("Missing dependency notices", output.stderr)

    def test_empty_malformed_or_unknown_graph_fails(self):
        for graph in ("", "not a dependency\n", "missing v9.0.0\n"):
            with self.subTest(graph=graph):
                self.assertNotEqual(self.collect(graph).returncode, 0)


if __name__ == "__main__":
    unittest.main()
