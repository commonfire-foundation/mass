#!/usr/bin/env python3
"""Generate synthetic, preview-only history. Never touch real agent stores."""
import json
import os
from pathlib import Path
import sys


def create(root):
    root = Path(root).resolve()
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    config = root / "sources.json"
    if config.exists():
        return config
    project = root / "example-project"
    project.mkdir(exist_ok=True)
    samples = {
        "pi/sessions/demo/pi-session.jsonl": [
            {"type": "session", "version": 3, "id": "11111111-1111-4111-8111-111111111111", "cwd": str(project)},
            {"type": "message", "id": "p1", "parentId": None, "message": {"role": "user", "content": "Why does the OAuth callback redirect fail?"}},
            {"type": "message", "id": "p2", "parentId": "p1", "message": {"role": "assistant", "content": [{"type": "text", "text": "The callback URI was encoded twice. Encode it once before redirecting."}]}},
            {"type": "message", "id": "p3", "parentId": "p1", "message": {"role": "user", "content": "Alternate branch: try a localhost callback instead."}},
            {"type": "session_info", "name": "Fix OAuth redirect"},
        ],
        "claude/projects/demo/22222222-2222-4222-8222-222222222222.jsonl": [
            {"type": "user", "uuid": "c1", "sessionId": "22222222-2222-4222-8222-222222222222", "cwd": str(project), "message": {"content": "Make the terminal theme easier to read."}},
            {"type": "assistant", "uuid": "c2", "parentUuid": "c1", "message": {"content": [{"type": "text", "text": "Raise foreground contrast and make inactive borders quieter. The palette should stay warm."}]}},
            {"type": "custom-title", "customTitle": "Terminal theme contrast"},
        ],
        "codex/sessions/2026/09/rollout-demo.jsonl": [
            {"type": "session_meta", "payload": {"id": "33333333-3333-4333-8333-333333333333", "cwd": str(project), "source": "cli"}},
            {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Speed up startup by avoiding repeated database scans."}]}},
            {"type": "event_msg", "payload": {"type": "user_message", "message": "Speed up startup by avoiding repeated database scans."}},
            {"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Use an incremental SQLite index. Cache unchanged records and keep parsing off the UI thread."}]}},
        ],
    }
    for relative, entries in samples.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.exists():
            raise ValueError(f"Refusing to overwrite existing fixture: {path}")
        path.write_text("".join(json.dumps(entry) + "\n" for entry in entries))
    (root / "codex/archived_sessions").mkdir(exist_ok=True)
    config.write_text(json.dumps([{"agent": agent, "home": str(root / agent), "resumable": False}
                                  for agent in ("pi", "claude", "codex")], indent=2) + "\n")
    return config


if __name__ == "__main__":
    os.umask(0o077)
    create(sys.argv[1])
