"""Exercise real fzf in a pseudo-terminal; no agent executable is launched."""
import fcntl
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import struct
import sys
import termios
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import test_backend as fixtures


@unittest.skipUnless(shutil.which("fzf"), "fzf required")
class PickerTests(unittest.TestCase):
    setUp = fixtures.BackendTests.setUp
    tearDown = fixtures.BackendTests.tearDown

    def test_picker_modes_safe_query_and_resume_guard(self):
        fixtures.BackendTests.index(self)
        marker = self.root / "must-not-exist"
        pid, fd = pty.fork()
        if pid == 0:
            os.environ.update(TERM="xterm-256color", MASS_DB=str(self.db),
                              MASS_SOURCES=str(self.config), MASS_PYTHON=sys.executable)
            os.execv(str(ROOT / "mass"), [str(ROOT / "mass")])
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 170, 0, 0))
        output = bytearray()
        reaped = False

        def wait_for(text, timeout=10):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if text.encode() in output:
                    return
                if select.select([fd], [], [], 0.1)[0]:
                    try:
                        output.extend(os.read(fd, 65536))
                    except OSError:
                        break
            self.fail(f"Missing UI text {text!r}; terminal tail: {bytes(output[-800:])!r}")

        try:
            wait_for("Search >")
            os.write(fd, b'callback')
            wait_for("Fix OAuth redirect")
            output.clear()
            os.write(fd, b'\x06')
            wait_for("Fuzzy >")
            output.clear()
            os.write(fd, b'\x13')
            wait_for("Search >")
            # A query is passed as one quoted argument, never executed as shell code.
            output.clear()
            os.write(fd, f'$(touch {marker})'.encode())
            wait_for("0/0")
            self.assertFalse(marker.exists())
            output.clear()
            os.write(fd, b'\x15callback')  # Ctrl-U, then a real query.
            # Drain the PTY until the replacement list arrives; sleeping here can
            # fill the PTY buffer and block fzf before it processes the reload.
            wait_for("1/1")
            output.clear()
            os.write(fd, b'\x0f')  # Ctrl-O: demo is preview-only.
            wait_for("Preview-only source")
            os.write(fd, b'\r')
            output.clear()
            wait_for("Search >")
            os.write(fd, b'\x1b')
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                found, status = os.waitpid(pid, os.WNOHANG)
                if found:
                    reaped = True
                    self.assertEqual(os.waitstatus_to_exitcode(status), 0)
                    break
                time.sleep(0.05)
            else:
                self.fail("Picker did not exit")
        finally:
            if not reaped:
                try:
                    os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                os.waitpid(pid, 0)
            os.close(fd)


if __name__ == "__main__":
    unittest.main()
