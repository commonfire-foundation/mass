#!/usr/bin/env python3
"""Exercise the local archive with an empty user environment on the build host."""
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


class PackageTests(unittest.TestCase):
    def test_packaged_search_without_helpers_or_user_configuration(self):
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        candidates = list((ROOT / "dist").glob(f"mass-{version}-*.tar.gz"))
        self.assertTrue(candidates, "Run scripts/package.sh first")
        archive = Path(os.environ.get("MASS_TEST_ARCHIVE", max(candidates, key=lambda p: p.stat().st_mtime)))
        with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="package-smoke-") as tmp:
            root = Path(tmp)
            with tarfile.open(archive) as tar:
                tar.extractall(root, filter="data")
            package = root / archive.name.removesuffix(".tar.gz")
            binary = package / "mass"
            self.assertTrue((package / "LICENSE").read_text().startswith("MIT License"))
            self.assertIn("License expression:", (package / "THIRD-PARTY-LICENSES.txt").read_text())
            self.assertTrue((package / "docs/LOCAL-RELEASE.md").is_file())
            self.assertTrue((package / "SECURITY.md").is_file())
            self.assertTrue((package / "docs/PREVIEW.md").is_file())
            self.assertTrue((package / "docs/assets/preview.png").is_file())
            self.assertFalse((package / "docs/SESSION-ACTIONS-PLAN.md").exists())
            self.assertFalse((package / ".cargo-home").exists())
            self.assertFalse((package / "target").exists())
            home = root / "empty-home"
            home.mkdir()
            env = {"HOME": str(home), "XDG_CONFIG_HOME": str(home / "config"),
                   "XDG_CACHE_HOME": str(home / "cache"), "PATH": "/nonexistent", "LANG": "C.UTF-8"}
            self.assertEqual(subprocess.check_output([str(binary), "--version"], env=env, text=True).strip(), f"mass {version}")
            sources = root / "sessions"
            sources.mkdir()
            (sources / "one.jsonl").write_text('\n'.join(map(json.dumps, [
                {"type": "session", "id": "package", "cwd": str(root)},
                {"type": "message", "id": "entry", "message": {"role": "user", "content": "packaged callback passage"}},
            ])) + '\n')
            config = root / "config.toml"
            config.write_text(f'[[sources]]\nid="test"\nagent="pi"\nroot={json.dumps(str(sources))}\n')
            args = [str(binary), "--config", str(config)]
            subprocess.run(args + ["index"], env=env, check=True, capture_output=True)
            page = json.loads(subprocess.check_output(args + ["search", '"packaged callback"', "--json"], env=env))
            self.assertEqual(len(page["results"]), 1)
            hit = page["results"][0]
            reference = hit["session"]["reference"]
            read = json.loads(subprocess.check_output(args + ["show", reference, "--at", hit["message_ref"], "--json"], env=env))
            self.assertEqual(read["messages"][0]["text"], "packaged callback passage")
            destination = root / "export.json"
            subprocess.run(args + ["export", reference, "--format", "json", "--output", str(destination)], env=env, check=True, capture_output=True)
            exported = json.loads(destination.read_text())
            self.assertEqual(exported["messages"][0]["text"], "packaged callback passage")
            self.assertNotIn("source_path", exported["session"])
            self.assertFalse(exported["native_backup"])
            plan = json.loads(subprocess.check_output(args + ["resume", reference, "--dry-run"], env=env))
            self.assertFalse(plan["supported"])
            self.assertTrue((home / "cache/mass/index.sqlite3").is_file())


if __name__ == "__main__":
    unittest.main()
