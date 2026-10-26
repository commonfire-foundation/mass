import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import backend
import demo


class BackendTests(unittest.TestCase):
    def setUp(self):
        (ROOT / ".cache").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / ".cache")
        self.root = Path(self.temp.name)
        self.config = demo.create(self.root)
        self.db = self.root / "index.sqlite3"
        self.pi = self.root / "pi/sessions/demo/pi-session.jsonl"
        self.codex = self.root / "codex/sessions/2026/09/rollout-demo.jsonl"

    def tearDown(self):
        self.temp.cleanup()

    def index(self, limit=None):
        with contextlib.redirect_stderr(io.StringIO()):
            return backend.index(SimpleNamespace(db=self.db, sources=self.config, limit=limit))

    def search(self, query="", agent=""):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            backend.search(SimpleNamespace(db=self.db, query=query, agent=agent, limit=500))
        return out.getvalue()

    def append(self, path, value):
        with path.open("a") as stream:
            stream.write(json.dumps(value) + "\n")

    def row(self, agent):
        with sqlite3.connect(self.db) as db:
            db.row_factory = sqlite3.Row
            return dict(db.execute("SELECT * FROM sessions WHERE agent=?", (agent,)).fetchone())

    def test_indexes_all_agents_and_conversation_content(self):
        self.assertEqual(self.index(), 0)
        self.assertIn("pi", self.search("callback"))
        self.assertIn("claude", self.search("foreground"))
        self.assertIn("codex", self.search("incremental"))
        self.assertEqual(len(self.search().splitlines()), 3)

    def test_prefix_and_agent_filter(self):
        self.index()
        self.assertIn("codex", self.search("increm agent:codex"))
        self.assertEqual(self.search("incremental agent:claude"), "")
        self.assertEqual(len(self.search("agent:pi").splitlines()), 1)

    def test_arbitrary_query_is_not_match_syntax(self):
        self.index()
        self.assertEqual(self.search('" OR * ; DROP TABLE sessions; --'), "")
        self.assertEqual(len(self.search().splitlines()), 3)
        self.assertIn("pi", self.search("-callback"))

    def test_index_never_mutates_source_transcripts(self):
        paths = list(self.root.rglob("*.jsonl"))
        before = {p: (p.stat().st_mtime_ns, hashlib.sha256(p.read_bytes()).hexdigest()) for p in paths}
        self.index()
        self.assertEqual(before, {p: (p.stat().st_mtime_ns, hashlib.sha256(p.read_bytes()).hexdigest()) for p in paths})

    def test_incremental_skips_unchanged_files(self):
        self.index()
        with patch.object(backend, "parse", side_effect=AssertionError("reparsed")):
            self.assertEqual(self.index(), 0)

    def test_changed_file_replaces_fts_content_and_keeps_id(self):
        self.index()
        ident = self.row("pi")["id"]
        self.append(self.pi, {"type": "message", "message": {"role": "assistant", "content": "uniquenewterm"}})
        self.append(self.pi, {"type": "session_info", "name": "Renamed oauth"})
        self.index()
        self.assertEqual(self.row("pi")["id"], ident)
        self.assertIn("Renamed oauth", self.search("uniquenewterm"))
        self.assertEqual(len(self.search("callback").splitlines()), 1)

    def test_deleted_file_is_removed_from_fts(self):
        self.index()
        self.pi.unlink()
        self.index()
        self.assertEqual(self.search("callback"), "")
        self.assertEqual(len(self.search().splitlines()), 2)

    def test_missing_optional_archive_does_not_block_active_deletion(self):
        self.index()
        (self.root / "codex/archived_sessions").rmdir()
        self.codex.unlink()
        self.index()
        self.assertEqual(self.search("incremental"), "")

    def test_missing_store_does_not_delete_cached_history(self):
        self.index()
        original = self.root / "pi/sessions"
        original.rename(self.root / "pi/offline")
        self.index()
        self.assertIn("pi", self.search("callback"))

    def test_limited_scan_does_not_purge(self):
        self.index()
        self.pi.unlink()
        self.index(limit=1)
        # Pi's empty directory was fully scanned, so its deletion is real;
        # the later unscanned Codex store must remain intact.
        self.assertIn("codex", self.search("incremental"))

    def test_pi_all_branches_are_searchable(self):
        self.index()
        self.assertIn("pi", self.search("localhost"))
        metadata, messages = backend.parse(self.pi, "pi")
        self.assertEqual(metadata["title"], "Fix OAuth redirect")
        self.assertEqual(next(m for m in messages if m[2] == "p3")[3], "p1")

    def test_tools_thinking_and_images_are_excluded(self):
        self.append(self.pi, {"type": "message", "message": {"role": "assistant", "content": [
            {"type": "thinking", "thinking": "private_thinking_marker"},
            {"type": "toolCall", "arguments": {"secret": "private_tool_marker"}},
            {"type": "image", "data": "private_image_marker"}]}})
        self.append(self.pi, {"type": "message", "message": {"role": "toolResult", "content": "private_output_marker"}})
        self.index()
        self.assertEqual(self.search("private"), "")

    def test_codex_projection_is_not_duplicated(self):
        _, messages = backend.parse(self.codex, "codex")
        self.assertEqual(len(messages), 2)

    def test_codex_events_only_fallback(self):
        self.codex.write_text(json.dumps({"type": "event_msg", "payload": {"type": "agent_message", "message": "eventonly"}}) + "\n")
        _, messages = backend.parse(self.codex, "codex")
        self.assertEqual(messages[0][1], "eventonly")

    def test_unfinished_line_marks_partial_and_recovers(self):
        with self.pi.open("a") as f:
            f.write('{"type":')
        self.index()
        self.assertIn("unfinished", self.row("pi")["warning"])
        with self.pi.open("a") as f:
            f.write('"message","message":{"role":"user","content":"recoveredrecord"}}\n')
        self.index()
        self.assertEqual(self.row("pi")["warning"], "")
        self.assertIn("pi", self.search("recoveredrecord"))

    def test_oversized_records_are_bounded(self):
        path = self.root / "oversized.jsonl"
        path.write_bytes(b'x' * 300 + b'\n' + json.dumps({"type": "message", "message": {"role": "user", "content": "small"}}).encode() + b'\n')
        with patch.object(backend, "MAX_LINE", 100):
            meta, messages = backend.parse(path, "pi")
        self.assertIn("oversized", meta["warning"])
        self.assertEqual(messages[0][1], "small")

    @unittest.skipUnless(backend.zstd, "Python 3.14 zstd required")
    def test_compressed_codex_and_plain_precedence(self):
        zipped = self.codex.with_suffix(".jsonl.zst")
        zipped.write_bytes(backend.zstd.compress(self.codex.read_bytes()))
        source = backend.sources(self.config)[2]
        self.assertEqual(len(backend.discover(source)[0]), 1)
        self.codex.unlink()
        self.index()
        self.assertIn("codex", self.search("incremental"))

    @unittest.skipUnless(backend.zstd, "Python 3.14 zstd required")
    def test_corrupt_compression_is_reported_not_crashed(self):
        self.codex.with_suffix(".jsonl.zst").write_bytes(b'not zstd')
        self.codex.unlink()
        self.assertEqual(self.index(), 1)
        self.assertIn("pi", self.search("callback"))

    def test_control_characters_cannot_reach_terminal(self):
        self.assertEqual(backend.clean("one\x1b\t\n\u202etwo"), "one two")
        self.append(self.pi, {"type": "session_info", "name": "danger\x1b[2J\nnext"})
        self.index()
        self.assertNotIn("\x1b", self.search())
        self.assertEqual(len(self.search().splitlines()), 3)

    def test_demo_cannot_resume(self):
        self.index()
        with self.assertRaisesRegex(ValueError, "Preview-only"):
            backend.launch_plan(self.row("pi"))

    def test_resume_plan_uses_original_agent_store_and_argv(self):
        self.index()
        for agent in ("pi", "claude", "codex"):
            row = self.row(agent)
            row["resumable"] = 1
            plan = backend.launch_plan(row)
            self.assertEqual(plan["argv"][0], agent)
            self.assertEqual(plan["cwd"], str(self.root / "example-project"))
            self.assertNotIn("--dangerously-skip-permissions", plan["argv"])
            self.assertNotIn("--approve-for-me", plan["argv"])

    def test_child_and_missing_cwd_cannot_resume(self):
        self.index()
        row = self.row("claude")
        row.update(resumable=1, child=1)
        with self.assertRaisesRegex(ValueError, "Child transcript"):
            backend.launch_plan(row)
        row.update(child=0, cwd=str(self.root / "missing"))
        with self.assertRaisesRegex(ValueError, "working directory"):
            backend.launch_plan(row)

    def test_private_database_permissions(self):
        self.index()
        self.assertEqual(self.db.stat().st_mode & 0o777, 0o600)

    def test_environment_overrides(self):
        with patch.dict(os.environ, {"PI_CODING_AGENT_DIR": str(self.root / "custompi"),
                                    "PI_CODING_AGENT_SESSION_DIR": str(self.root / "sessionsoverride"),
                                    "CLAUDE_CONFIG_DIR": str(self.root / "customclaude"),
                                    "CODEX_HOME": str(self.root / "customcodex")}):
            values = backend.sources()
            self.assertEqual(values[0]["roots"], [str(self.root / "sessionsoverride")])
            self.assertEqual(values[1]["roots"], [str(self.root / "customclaude/projects")])
            self.assertEqual(values[2]["roots"][1], str(self.root / "customcodex/archived_sessions"))


if __name__ == "__main__":
    unittest.main()
