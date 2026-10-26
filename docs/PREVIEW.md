# Synthetic demo and terminal preview

The README image is a capture of the Rust TUI running against invented Pi,
Claude Code, and Codex conversations. No live history, desktop window, or
credential store is captured. It is not a mock interface or evidence of a
released binary.

## Try it

From a source checkout, with Python 3 available:

```sh
cargo build --locked --release
python3 scripts/demo.py
```

The demo creates a disposable directory under `target/`, explicitly selects only
its invented sources, and gives MASS a disposable HOME/config/cache. Resume is
disabled and PATH contains no agent or clipboard executables. Quit to clean up.
An abrupt SIGKILL can leave the synthetic directory under ignored `target/`;
no live history is involved. No application files are installed globally.

For an automated smoke check, without opening a terminal:

```sh
python3 scripts/demo.py --check
```

This indexes the fixtures, verifies retrieval for all three agents, reads a
matching passage, and checks that native resume is blocked. `--binary PATH`
selects a different locally built executable; `--query TEXT` seeds the TUI.

## Reproduce the image

Capture requires Python with Pillow and a monospace TrueType font, in addition
to the built Rust binary. These are optional documentation tools, not MASS
runtime dependencies. The capture script starts the same demo fixture in a
pseudo-terminal, reads actual terminal cells/styles, and rasterizes that screen.
It does not access the compositor, other windows, or the user's terminal history.

```sh
python3 scripts/capture-preview.py --output docs/assets/preview.png
```

Use `--font PATH` if DejaVu Sans Mono is not installed at the default Linux
location. Capture is fixed at 140 columns × 32 rows; inspect the image after a
UI change. The palette/font are presentation choices for the terminal capture,
not a promise that every user's terminal looks identical. The checked-in image
was captured with IBM Plex Mono (regular/bold), rasterized from the system font;
no font files are bundled.

The application version shown in the image comes from the actual executable.
Do not hand-edit screen text or use real conversations to make the preview
look more convincing.
