#!/usr/bin/env python3
"""Synthetic process-level recovery checks; never touch live stores or caches."""
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import sqlite3
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("MASS_TEST_BINARY", ROOT / "target/debug/mass"))


class RecoveryTests(unittest.TestCase):
    def test_interrupted_update_retains_history_and_can_retry(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="recovery-") as tmp:
            root = Path(tmp)
            store = root / "sessions"
            store.mkdir()
            transcript = store / "one.jsonl"
            transcript.write_text('\n'.join(map(json.dumps, [
                {"type": "session", "id": "recovery", "cwd": str(root)},
                {"type": "message", "id": "original", "message": {"role": "user", "content": "original needle"}},
            ])) + '\n')
            config = root / "config.toml"
            config.write_text(f'[[sources]]\nid="test"\nagent="pi"\nroot={json.dumps(str(store))}\n')
            database = root / "index.sqlite3"
            argv = [str(BINARY), "--config", str(config), "--db", str(database)]
            subprocess.run(argv + ["index"], check=True, capture_output=True)
            self.assertEqual(database.stat().st_mode & 0o777, 0o600)
            with transcript.open("a") as out:
                for i in range(20_000):
                    out.write(json.dumps({"type": "message", "id": str(i), "message": {"role": "user", "content": "replacement marker"}}) + '\n')
            original_hash = hashlib.sha256(transcript.read_bytes()).digest()
            proc = subprocess.Popen(argv + ["index", "--json"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                self.assertTrue(select.select([proc.stderr], [], [], 10)[0], "No index progress")
                self.assertIn(b"transcript files", proc.stderr.readline())
                proc.send_signal(signal.SIGINT)
                stdout, stderr = proc.communicate(timeout=15)
                self.assertEqual(proc.returncode, 130, stderr.decode())
                self.assertIn("Indexing cancelled", str(json.loads(stdout)))
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.communicate()
            with sqlite3.connect(database) as db:
                self.assertEqual(db.execute("pragma integrity_check").fetchone()[0], "ok")
            page = json.loads(subprocess.check_output(argv + ["search", "original needle", "--json"]))
            self.assertEqual(len(page["results"]), 1)
            subprocess.run(argv + ["index"], check=True, capture_output=True)
            page = json.loads(subprocess.check_output(argv + ["search", "replacement marker", "--json"]))
            self.assertEqual(len(page["results"]), 1)
            self.assertEqual(hashlib.sha256(transcript.read_bytes()).digest(), original_hash)

    def test_corrupt_cache_is_rejected_without_modification(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="corrupt-") as tmp:
            root = Path(tmp)
            config = root / "config.toml"
            config.write_text("sources = []\n")
            database = root / "index.sqlite3"
            original = b"not a database\n" * 4096
            database.write_bytes(original)
            argv = [str(BINARY), "--config", str(config), "--db", str(database)]
            for command in (["search", "needle"], ["index"]):
                result = subprocess.run(argv + command, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"not a database", result.stderr)
                self.assertEqual(database.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
