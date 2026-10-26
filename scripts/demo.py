#!/usr/bin/env python3
"""Try MASS with invented conversations, never with your own history."""
import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def create(directory):
    """Create only synthetic sources, with native reopening explicitly disabled."""
    stamp = datetime.now(timezone.utc).isoformat()
    projects = directory / "projects"
    for name in ("website", "queue", "handbook"):
        (projects / name).mkdir(parents=True)
    stories = {
        "pi": [
            {"type": "session", "id": "demo-pi", "cwd": str(projects / "website")},
            {"type": "session_info", "name": "Fix the OAuth callback"},
            {"type": "message", "id": "pi-user", "message": {"role": "user", "content":
                "Why does the callback land on an error page after sign-in?"}},
            {"type": "message", "id": "pi-answer", "message": {"role": "assistant", "content":
                "The callback URL was encoded twice. Build the redirect URI once, "
                "then let the query encoder escape it.\n\n"
                "```python\nredirect_uri = origin + '/auth/callback'\n"
                "params = {'redirect_uri': redirect_uri}\n```\n\n"
                "Keep one callback regression for a path containing spaces. "
                "The callback should arrive unchanged after decoding."}},
        ],
        "claude": [
            {"type": "custom-title", "sessionId": "demo-claude", "cwd": str(projects / "queue"),
             "customTitle": "Test the callback retry path"},
            {"type": "user", "sessionId": "demo-claude", "uuid": "claude-user",
             "message": {"content": "Add a callback retry test using a fake clock."}},
            {"type": "assistant", "sessionId": "demo-claude", "uuid": "claude-answer",
             "message": {"content": "Inject a clock and retry only transient failures. "
                         "Assert the attempt count and stop after the configured deadline."}},
        ],
        "codex": [
            {"type": "session_meta", "payload": {"id": "demo-codex", "cwd": str(projects / "handbook")}},
            {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "Document the callback recovery workflow"}]}},
            {"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [
                {"type": "output_text", "text": "Explain the error, preserve the pending request, "
                 "and offer a deliberate retry. Do not silently repeat a completed action."}]}},
        ],
    }
    config = []
    for agent, records in stories.items():
        store = directory / "sources" / agent
        store.mkdir(parents=True)
        for record in records:
            record["timestamp"] = stamp
        (store / "demo.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records))
        config.append(f'[[sources]]\nid="demo-{agent}"\nagent="{agent}"\n'
                      f'root={json.dumps(str(store))}\nresume=false\n')
    path = directory / "config.toml"
    path.write_text("\n".join(config))
    return path


@contextmanager
def fixture(binary):
    """Yield an explicitly isolated CLI invocation and a credential-free environment."""
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="demo-") as tmp:
        directory = Path(tmp)
        config = create(directory)
        home, empty_path = directory / "home", directory / "empty-bin"
        home.mkdir()
        empty_path.mkdir()
        env = {"HOME": str(home), "XDG_CONFIG_HOME": str(home / "config"),
               "XDG_CACHE_HOME": str(home / "cache"), "PATH": str(empty_path),
               "TERM": os.environ.get("TERM", "xterm-256color"), "LANG": "C.UTF-8"}
        for key in ("NO_COLOR", "COLORTERM"):
            if key in os.environ:
                env[key] = os.environ[key]
        command = [str(Path(binary).resolve()), "--config", str(config),
                   "--db", str(directory / "index.sqlite3")]
        yield command, env, directory


def run_json(command, env, *args):
    output = subprocess.run(command + list(args) + ["--json"], env=env,
                            check=True, capture_output=True, text=True)
    return json.loads(output.stdout)


def check(command, env):
    coverage = run_json(command, env, "index")["coverage"]
    assert coverage["indexed"] == 3 and not coverage["warnings"], coverage
    for agent in ("pi", "claude", "codex"):
        page = run_json(command, env, "search", "callback", "--agent", agent)
        assert len(page["results"]) == 1, agent
        hit = page["results"][0]
        reference = hit["session"]["reference"]
        reading = run_json(command, env, "show", reference, "--at", hit["message_ref"])
        assert any("callback" in m["text"].lower() for m in reading["messages"]), agent
        plan = json.loads(subprocess.check_output(command + ["resume", reference, "--dry-run"],
                                                 env=env, text=True))
        assert not plan["supported"], agent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/mass")
    parser.add_argument("--query", default="callback", help="initial TUI query")
    parser.add_argument("--check", action="store_true", help="verify the demo without opening a terminal")
    args = parser.parse_args()
    if not args.binary.is_file():
        parser.error("Build first: cargo build --locked --release (or select --binary PATH)")
    if not args.check and not (sys.stdin.isatty() and sys.stdout.isatty()):
        parser.error("The demo needs a terminal; use --check for noninteractive verification")
    try:
        with fixture(args.binary) as (command, env, _):
            if args.check:
                check(command, env)
                print("Demo check passed: Pi, Claude Code, Codex; isolated data; reopening disabled.")
                return 0
            run_json(command, env, "index")
            print("Synthetic demo · no personal history · reopening and clipboard disabled", flush=True)
            return subprocess.run(command + ["tui", args.query], env=env).returncode
    except subprocess.CalledProcessError as error:
        print(f"Demo failed: {error.stderr or error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
