#!/usr/bin/env python3
"""Exercise actual rendered terminal state, not stale bytes in an output log."""
import codecs
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import tempfile
import termios
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("MASS_TEST_BINARY", ROOT / "target/debug/mass"))


class Screen:
    """Minimal VT screen for Crossterm's cursor moves, erases and SGR output."""
    def __init__(self, width, height=30, reply=None):
        self.width, self.height = width, height
        self.cells = [[" "] * width for _ in range(height)]
        self.row = self.col = 0
        self.decoder = codecs.getincrementaldecoder("utf8")("replace")
        self.pending = ""
        self.reply = reply
        self.primary = None

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        while self.pending:
            if self.pending.startswith("\x1b"):
                if len(self.pending) < 2:
                    return
                if self.pending[1] == "[":
                    match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", self.pending)
                    if not match:
                        return
                    params, _, command = match.groups()
                    self.pending = self.pending[match.end():]
                    if params == "?1049":
                        if command == "h" and self.primary is None:
                            self.primary = self.cells, self.row, self.col
                            self.cells = [[" "] * self.width for _ in range(self.height)]
                            self.row = self.col = 0
                        elif command == "l" and self.primary is not None:
                            self.cells, self.row, self.col = self.primary
                            self.primary = None
                    if params.startswith("?"):
                        continue
                    values = [int(v or 0) for v in params.split(";")]
                    n = values[0] or 1
                    if command in "Hf":
                        self.row = n - 1
                        self.col = (values[1] or 1) - 1 if len(values) > 1 else 0
                    elif command == "G": self.col = n - 1
                    elif command == "A": self.row = max(0, self.row - n)
                    elif command == "B": self.row += n
                    elif command == "C": self.col += n
                    elif command == "D": self.col = max(0, self.col - n)
                    elif command == "n" and values[0] == 6 and self.reply:
                        # Ratatui queries the real cursor when clearing after resume.
                        self.reply(f"\x1b[{min(self.row, self.height - 1) + 1};{min(self.col, self.width - 1) + 1}R".encode())
                    elif command == "J" and values[0] == 2:
                        self.cells = [[" "] * self.width for _ in range(self.height)]
                    elif command == "K" and 0 <= self.row < self.height:
                        start = 0 if values[0] in (1, 2) else self.col
                        end = self.col + 1 if values[0] == 1 else self.width
                        for col in range(max(0, start), min(end, self.width)):
                            self.cells[self.row][col] = " "
                    continue
                self.pending = self.pending[2:]
                continue
            char, self.pending = self.pending[0], self.pending[1:]
            if char == "\r": self.col = 0
            elif char == "\n":
                self.row += 1
                if self.row >= self.height:
                    self.cells.pop(0)
                    self.cells.append([" "] * self.width)
                    self.row = self.height - 1
            elif char >= " ":
                if 0 <= self.row < self.height and 0 <= self.col < self.width:
                    self.cells[self.row][self.col] = char
                self.col += 1

    @property
    def text(self):
        return "\n".join("".join(row) for row in self.cells)


class ScreenTests(unittest.TestCase):
    def test_cursor_reports_and_alternate_screen_round_trip(self):
        replies = []
        screen = Screen(10, 3, reply=replies.append)
        screen.feed(b"primary\x1b[?1049h\x1b[2;4H\x1b[6n")
        self.assertEqual(replies[-1], b"\x1b[2;4R")
        screen.feed(b"alt\x1b[?1049l\x1b[6n")
        self.assertTrue(screen.text.startswith("primary"))
        self.assertEqual(replies[-1], b"\x1b[1;8R")
        screen.feed(b"\r\n1\r\n2\r\n3\x1b[6n")
        self.assertEqual(replies[-1], b"\x1b[3;2R")


class TerminalTests(unittest.TestCase):
    def exercise(self, width, first_run=False, empty=False, height=30, long_history=False, resize=False, monochrome=False, resume_enabled=False, cancel=False, refresh=False, pagination=False, actions_enabled=False, resume_change=None, slow_review=None):
        with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="pty-") as tmp:
            directory = Path(tmp)
            store = directory / "sessions"
            store.mkdir()
            message_text = "unique callback passage" + ("\n```make\nall:\n\tprintf hello\n```" if actions_enabled else "")
            if not empty:
                records = [
                    {"type":"session","id":"pty-one","cwd":str(directory)},
                    {"type":"message","id":"entry","message":{"role":"user","content":message_text}},
                ]
                if long_history:
                    records += [{"type":"message","id":f"turn-{i}","message":{"role":"assistant","content":"Distant café needle" if i==70 else f"Turn {i:03d}: ordinary history"}} for i in range(80)]
                (store / "one.jsonl").write_text("\n".join(map(json.dumps, records)) + "\n")
            if pagination:
                for i in range(120):
                    records = [
                        {"type":"session","id":f"page-{i}","cwd":str(directory)},
                        {"type":"message","id":"entry","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":f"Paging item {i:03d}"}},
                    ]
                    (store / f"page-{i}.jsonl").write_text("\n".join(map(json.dumps, records)) + "\n")
            if first_run:
                (store / "unsupported.jsonl").write_text('{"type":"message","message":{"role":"user","content":"fixture without session identity"}}\n')
            config = directory / "config.toml"
            config.write_text(f'[[sources]]\nid="test"\nagent="pi"\nroot={json.dumps(str(store))}\n')
            extra_env = {}
            if resume_enabled:
                config.write_text(config.read_text() + "resume=true\n")
                bin_dir = directory / "bin"
                bin_dir.mkdir()
                stub = bin_dir / "pi"
                stub.write_text('#!/bin/sh\nprintf launched > "$MASS_TEST_MARKER"\n')
                stub.chmod(0o700)
                extra_env = {"PATH":str(bin_dir), "MASS_TEST_MARKER":str(directory / "launched")}
            if actions_enabled:
                bin_dir = directory / "clipboard-bin"
                bin_dir.mkdir()
                helper = bin_dir / "wl-copy"
                helper.write_text('#!/bin/sh\n/bin/cat > "$MASS_CLIPBOARD_FILE"\n')
                helper.chmod(0o700)
                extra_env.update({"PATH":str(bin_dir) + (os.pathsep + extra_env["PATH"] if "PATH" in extra_env else ""), "WAYLAND_DISPLAY":"synthetic", "DISPLAY":"", "MASS_CLIPBOARD_FILE":str(directory / "clipboard.txt")})
            argv = [str(BINARY), "--config", str(config), "--db", str(directory / "index.sqlite3")]
            if not first_run:
                subprocess.run(argv + ["index"], check=True, capture_output=True)
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
            before = termios.tcgetattr(slave)
            proc = subprocess.Popen(argv + ["tui"], stdin=slave, stdout=slave, stderr=slave, env={**os.environ,"TERM":"xterm-256color",**({"NO_COLOR":"1"} if monochrome else {}),**extra_env})
            screen = Screen(width, height, reply=lambda data: os.write(master, data))
            def wait_for(text):
                deadline = time.monotonic() + 10
                while text not in screen.text:
                    self.assertIsNone(proc.poll(), screen.text)
                    self.assertLess(time.monotonic(), deadline, screen.text)
                    if select.select([master], [], [], .1)[0]:
                        screen.feed(os.read(master, 65536))
            def keys(data):
                os.write(master, data)
                # Observe fresh paint before checking labels also present in an older frame.
                # This also separates standalone Esc from a following Alt-key sequence.
                deadline = time.monotonic() + .08
                while time.monotonic() < deadline:
                    if select.select([master], [], [], .02)[0]:
                        screen.feed(os.read(master, 65536))
            try:
                version = subprocess.check_output([str(BINARY), "--version"], text=True).strip().split()[-1]
                wait_for(f"M.A.S.S. - Multi Agent Session Search v{version}")
                if first_run:
                    wait_for("Enter: Build index")
                    self.assertFalse((directory / "index.sqlite3").exists())
                    if cancel:
                        keys(b"\x03")
                        self.assertEqual(proc.wait(timeout=5), 130)
                        self.assertEqual(termios.tcgetattr(slave), before)
                        self.assertFalse((directory / "index.sqlite3").exists())
                        return
                    keys(b"\r")
                wait_for("No conversations yet" if empty else "callback passage")
                wait_for("Search ›")
                if pagination:
                    wait_for("1–50 conversations")
                    keys(b"\x1b[6~")
                    wait_for("51–100 conversations")
                    keys(b"\x1b[B\x1b[B")
                    selected = next(line.split("│")[0].strip() for line in screen.text.splitlines() if line.strip().startswith("› Paging item"))
                    keys(b"\x1b[6~")
                    wait_for("101–121 conversations")
                    keys(b"\x1b[5~")
                    wait_for("51–100 conversations")
                    wait_for(selected)
                    keys(b"\x1b[5~")
                    wait_for("1–50 conversations")
                    keys(b"\x1b[6~")
                    wait_for("51–100 conversations")
                    keys(b"callback")
                    wait_for("1–1 conversations")
                    keys(b"\x1b[5~")
                    wait_for("1–1 conversations")
                    keys(b"\x15")
                    wait_for("1–50 conversations")
                    keys(b"\x1b[5~")
                    wait_for("1–50 conversations")
                elif empty:
                    # Enter without a selection must not lock us into a reader.
                    keys(b"\rxxyzz")
                    wait_for("xxyzz")
                    wait_for("No matches")
                    self.assertNotIn("Reader ·", screen.text)
                else:
                    keys(b"\t\t")
                    # Typing must work after Tab, including preview focus.
                    keys(b"xxyzz")
                    wait_for("No matches")
                    wait_for("Search ›")
                    keys(b"\runique")
                    wait_for("xxyzzunique")
                    self.assertNotIn("Reader ·", screen.text)
                    keys(b"\x7f" * len("xxyzzunique") + b"callback")
                    wait_for("callback passage")
                    keys(b"\x01\x0b")  # Home, Ctrl-K: help must not erase the query suffix
                    wait_for("Keyboard help")
                    wait_for("Ctrl-L Refresh index")
                    keys(b"\x1b")
                    wait_for("Search › callback")
                    keys(b"\x05")  # Restore cursor to end
                    if actions_enabled:
                        destination = directory / "export with spaces.json"
                        keys(b"\x0f")
                        wait_for("Session actions")
                        self.assertFalse(destination.exists())
                        keys(b"\x1b[B\r")  # Export conversation as JSON
                        wait_for("Destination filename:")
                        keys(b"\x1b[200~" + str(destination).encode() + b"\x1b[201~\r")
                        wait_for("Export saved")
                        exported = json.loads(destination.read_text())
                        self.assertEqual(exported["messages"][0]["text"], message_text)
                        self.assertNotIn("source_path", exported["session"])
                        keys(b"\x1b")
                        wait_for("Search › callback")
                        keys(b"\x0f")
                        wait_for("Session actions")
                        keys(b"\x1b[B" * 5 + b"\r")  # Copy current message through inert helper
                        wait_for("Copied to clipboard")
                        self.assertEqual((directory / "clipboard.txt").read_text(), message_text)
                        keys(b"\x1b")
                        wait_for("Search › callback")
                        keys(b"\x0f")
                        wait_for("Session actions")
                        keys(b"\x1b[B" * 8 + b"\r")
                        wait_for("Containing directory")
                        wait_for("one.jsonl")
                        keys(b"\x1b")
                        wait_for("Search › callback")
                    if refresh:
                        with (store / "one.jsonl").open("a") as transcript:
                            transcript.write(json.dumps({"type":"session_info","name":"unique callback passage — refreshed"}) + "\n")
                        keys(b"\x0c")  # Ctrl-L outside filters refreshes the index
                        wait_for("callback passage — refreshed")
                    keys(b"\x04")  # Ctrl-D: full metadata remains readable in a scrollable sheet
                    wait_for("Session details")
                    keys(b"\x1b[6~")
                    wait_for("one.jsonl")
                    keys(b"\x1b")
                    wait_for("Search ›")
                    if slow_review:
                        # Index only the small fixture; make the subsequent native
                        # identity review slow using discarded synthetic image data.
                        with (store / "one.jsonl").open("a") as transcript:
                            transcript.write('{"type":"message","message":{"role":"assistant","content":[{"type":"image","data":"')
                            for _ in range(64): transcript.write("x" * (1024 * 1024))
                            transcript.write('"}]}}\n')
                    keys(b"\x12")  # Disabled resume should explain, not exit.
                    if slow_review:
                        wait_for("Checking source identity")
                        time.sleep(.1)  # Allow the worker to enter the streaming parse.
                        started = time.monotonic()
                        if slow_review == "escape":
                            keys(b"\x1b")
                            keys(b"\x15nevermatchesaftercancel")
                            wait_for("No matches")  # Requires a new worker result, not just repaint.
                        keys(b"\x03")
                        self.assertEqual(proc.wait(timeout=2), 130)
                        self.assertLess(time.monotonic() - started, 2)
                        self.assertEqual(termios.tcgetattr(slave), before)
                        self.assertFalse((directory / "launched").exists())
                        return
                    wait_for("Y Launch agent" if resume_enabled else "Resume unavailable")
                    self.assertFalse((directory / "launched").exists())
                    keys(b"\x1b")
                    wait_for("Search ›")
                    if resume_enabled:
                        self.assertFalse((directory / "launched").exists())
                        keys(b"\x12")
                        wait_for("Y Launch agent")
                        if actions_enabled:
                            keys(b"c")
                            wait_for("Copied to clipboard")
                            self.assertIn("--session", (directory / "clipboard.txt").read_text())
                            self.assertFalse((directory / "launched").exists())
                            keys(b"\x12")
                            wait_for("Y Launch agent")
                        if resume_change == "identity":
                            transcript = store / "one.jsonl"
                            transcript.write_text(transcript.read_text().replace('"pty-one"', '"different-session"'))
                        elif resume_change == "permission":
                            config.write_text(config.read_text().replace("resume=true", "resume=false"))
                        keys(b"y")
                        wait_for("Resume changed" if resume_change else "Agent finished")
                        wait_for("Search ›")
                        if resume_change:
                            self.assertFalse((directory / "launched").exists())
                        else:
                            self.assertEqual((directory / "launched").read_text(), "launched")
                    # Normal cursor editing and bracketed paste.
                    keys(b"\x01X\x1b[3~\x05")  # Home, insert, Delete, End
                    wait_for("Search › Xallback")
                    keys(b"\x15\x1b[200~callback\x1b[201~")
                    wait_for("Search › callback")
                    wait_for("callback passage")
                    keys(b"\r")
                    wait_for("↑↓ Scroll")
                    wait_for("USER")
                    if long_history:
                        keys("/distant café needle\r".encode())
                        wait_for("Distant café needle")
                        keys(b"\x1b[5~")  # Previous history automatically
                        wait_for("Turn 069")
                        keys(b"\x1b[6~")  # Continue forward automatically
                        wait_for("Distant café needle")
                        keys(b"\x1b")
                        wait_for("Search ›")
                        keys(b"\r")
                        wait_for("↑↓ Scroll")
                        wait_for("Distant café needle")
                    if resize:
                        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 20, 55, 0, 0))
                        screen = Screen(55, 20)
                        import signal
                        proc.send_signal(signal.SIGWINCH)
                        wait_for("↑↓ Scroll")
                        wait_for("callback")
                    keys(b"\x1b")
                    wait_for("Search ›")
                    keys(b"\x06")
                    wait_for("Clear filters")
                    keys(b"\x1b[C\x1b[C\r")  # Claude, then apply
                    wait_for("No matches")
                    keys(b"\x06")
                    wait_for("Clear filters")
                    keys(b"\x0c\r")  # Clear filters, preserve query
                    wait_for("All agents")
                    wait_for("Search › callback")
                    wait_for("callback passage")
                if first_run:
                    report = json.loads(subprocess.check_output(argv + ["status", "--json"]))
                    self.assertTrue(report["coverage"]["warnings"])
                    self.assertNotIn("Missing native session identity", screen.text)
                    self.assertNotIn("warnings", screen.text)
                    self.assertNotIn("Not yet fully indexed", screen.text)
                keys(b"\x1b")
                self.assertEqual(proc.wait(timeout=5), 0)
                self.assertEqual(termios.tcgetattr(slave), before)
            finally:
                if proc.poll() is None:
                    proc.kill(); proc.wait()
                os.close(master); os.close(slave)

    def test_cancel_slow_resume_review_keeps_worker_responsive(self): self.exercise(120, resume_enabled=True, slow_review="escape")
    def test_exit_during_slow_resume_review_restores_terminal(self): self.exercise(120, resume_enabled=True, slow_review="quit")
    def test_session_actions_export_copy_and_source(self): self.exercise(120, actions_enabled=True)
    def test_reviewed_command_copy_does_not_launch(self): self.exercise(120, actions_enabled=True, resume_enabled=True)
    def test_session_actions_narrow(self): self.exercise(55, height=20, actions_enabled=True)
    def test_results_previous_page_and_query_reset(self): self.exercise(120, pagination=True)
    def test_wide(self): self.exercise(120)
    def test_narrow(self): self.exercise(55)
    def test_first_run_consent(self): self.exercise(100, first_run=True)
    def test_empty_index_enter_does_not_trap_input(self): self.exercise(55, empty=True)
    def test_compact_monochrome(self): self.exercise(55, height=20, monochrome=True)
    def test_medium(self): self.exercise(80, height=24)
    def test_large(self): self.exercise(160, height=45)
    def test_reader_find_and_continuous_history(self): self.exercise(100, long_history=True)
    def test_resize_while_reading(self): self.exercise(120, resize=True)
    def test_ctrl_refresh(self): self.exercise(100, refresh=True)
    def test_cancel_before_index(self): self.exercise(80, first_run=True, cancel=True)
    def test_resume_confirmation_and_return(self): self.exercise(100, resume_enabled=True)
    def test_resume_rechecks_identity_after_review(self): self.exercise(120, resume_enabled=True, resume_change="identity")
    def test_resume_rechecks_permission_after_review(self): self.exercise(120, resume_enabled=True, resume_change="permission")


if __name__ == "__main__": unittest.main()
