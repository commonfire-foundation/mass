#!/usr/bin/env python3
"""Rasterize a real synthetic MASS PTY screen, without accessing the desktop."""
import argparse
import codecs
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import termios
import time

ROOT = Path(__file__).resolve().parents[1]
BACKGROUND = (20, 24, 29)
FOREGROUND = (222, 226, 231)
PALETTE = [
    (30, 35, 41), (216, 108, 108), (145, 191, 133), (221, 192, 123),
    (128, 165, 210), (183, 151, 205), (111, 195, 202), (210, 216, 222),
    (132, 143, 155), (239, 143, 143), (177, 215, 158), (242, 215, 153),
    (167, 196, 233), (211, 183, 230), (152, 219, 224), (242, 245, 247),
]


def indexed_color(n):
    if n < 16:
        return PALETTE[n]
    if n < 232:
        n -= 16
        levels = [0, 95, 135, 175, 215, 255]
        return levels[n // 36], levels[(n // 6) % 6], levels[n % 6]
    return (8 + (n - 232) * 10,) * 3


class Screen:
    """The cursor/erase/SGR subset emitted by this TUI; not a general emulator."""
    def __init__(self, width, height):
        self.width, self.height = width, height
        self.row = self.col = 0
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf8")("replace")
        self.reset_style()
        self.cells = [[self.cell(" ") for _ in range(width)] for _ in range(height)]

    def reset_style(self):
        self.fg, self.bg = FOREGROUND, BACKGROUND
        self.bold = self.dim = self.reverse = self.underline = False

    def cell(self, text):
        fg, bg = (self.bg, self.fg) if self.reverse else (self.fg, self.bg)
        if self.dim:
            fg = tuple(round(a * .6 + b * .4) for a, b in zip(fg, bg))
        return text, fg, bg, self.bold, self.underline

    def sgr(self, values):
        i = 0
        while i < len(values):
            code = values[i]
            if code == 0: self.reset_style()
            elif code == 1: self.bold = True
            elif code == 2: self.dim = True
            elif code == 4: self.underline = True
            elif code == 7: self.reverse = True
            elif code == 22: self.bold = self.dim = False
            elif code == 24: self.underline = False
            elif code == 27: self.reverse = False
            elif code == 39: self.fg = FOREGROUND
            elif code == 49: self.bg = BACKGROUND
            elif 30 <= code <= 37: self.fg = PALETTE[code - 30]
            elif 40 <= code <= 47: self.bg = PALETTE[code - 40]
            elif 90 <= code <= 97: self.fg = PALETTE[code - 90 + 8]
            elif 100 <= code <= 107: self.bg = PALETTE[code - 100 + 8]
            elif code in (38, 48) and i + 2 < len(values):
                if values[i + 1] == 5:
                    color = indexed_color(values[i + 2]); i += 2
                elif values[i + 1] == 2 and i + 4 < len(values):
                    color = tuple(values[i + 2:i + 5]); i += 4
                else:
                    i += 1
                    continue
                if code == 38: self.fg = color
                else: self.bg = color
            i += 1

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
                    elif command == "m": self.sgr(values)
                    elif command == "J" and values[0] == 2:
                        self.cells = [[self.cell(" ") for _ in range(self.width)] for _ in range(self.height)]
                    elif command == "K" and 0 <= self.row < self.height:
                        start = 0 if values[0] in (1, 2) else self.col
                        end = self.col + 1 if values[0] == 1 else self.width
                        for col in range(max(0, start), min(end, self.width)):
                            self.cells[self.row][col] = self.cell(" ")
                    continue
                self.pending = self.pending[2:]
                continue
            char, self.pending = self.pending[0], self.pending[1:]
            if char == "\r": self.col = 0
            elif char == "\n": self.row += 1
            elif char >= " ":
                if 0 <= self.row < self.height and 0 <= self.col < self.width:
                    self.cells[self.row][self.col] = self.cell(char)
                self.col += 1

    @property
    def text(self):
        return "\n".join("".join(cell[0] for cell in row) for row in self.cells)


def rasterize(screen, font_path, output):
    from PIL import Image, ImageDraw, ImageFont
    font = ImageFont.truetype(str(font_path), 17)
    bold_path = font_path.with_name(font_path.name.replace("-Regular", "-Bold")) if "-Regular" in font_path.name else font_path.with_name(font_path.stem + "-Bold.ttf")
    bold_font = ImageFont.truetype(str(bold_path), 17) if bold_path.is_file() else font
    width, height, padding = round(font.getlength("M")), 25, 24
    image = Image.new("RGB", (screen.width * width + padding * 2,
                             screen.height * height + padding * 2), BACKGROUND)
    draw = ImageDraw.Draw(image)
    for row, cells in enumerate(screen.cells):
        for col, (char, fg, bg, bold, underline) in enumerate(cells):
            x, y = padding + col * width, padding + row * height
            draw.rectangle((x, y, x + width - 1, y + height - 1), fill=bg)
            draw.text((x, y + 2), char, font=bold_font if bold else font, fill=fg)
            if underline:
                draw.line((x, y + height - 3, x + width - 1, y + height - 3), fill=fg)
    output.parent.mkdir(parents=True, exist_ok=True)
    image.save(output)


def main():
    from demo import fixture, run_json
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/mass")
    parser.add_argument("--font", type=Path, default=Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"))
    parser.add_argument("--output", type=Path, default=ROOT / "docs/assets/preview.png")
    args = parser.parse_args()
    if not args.binary.is_file() or not args.font.is_file():
        parser.error("A built --binary and installed monospace --font are required")
    with fixture(args.binary) as (command, env, _):
        env.pop("NO_COLOR", None)
        env["TERM"] = "xterm-256color"
        run_json(command, env, "index")
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 140, 0, 0))
        before = termios.tcgetattr(slave)
        proc = subprocess.Popen(command + ["tui", "callback"], env=env,
                                stdin=slave, stdout=slave, stderr=slave)
        screen = Screen(140, 32)
        try:
            deadline, ready, last_move = time.monotonic() + 15, None, 0.0
            while True:
                if proc.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("TUI did not render the synthetic preview:\n" + screen.text)
                if select.select([master], [], [], .05)[0]:
                    screen.feed(os.read(master, 65536))
                if "Pi · website" in screen.text and "› Fix the OAuth callback" not in screen.text and time.monotonic() - last_move > .2:
                    os.write(master, b"\x1b[B")
                    last_move = time.monotonic()
                if all(text in screen.text for text in ("› Fix the OAuth callback", "redirect_uri", "Claude", "Codex")):
                    if ready is None:
                        ready = time.monotonic()
                    elif time.monotonic() - ready > .3:
                        break
            rasterize(screen, args.font, args.output)
            os.write(master, b"\x03")
            assert proc.wait(timeout=5) == 130
            assert termios.tcgetattr(slave) == before, "Terminal settings were not restored"
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()
            os.close(master)
            os.close(slave)
    print(f"Saved real synthetic TUI capture: {args.output}")


if __name__ == "__main__":
    main()
