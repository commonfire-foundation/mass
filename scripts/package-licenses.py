#!/usr/bin/env python3
"""Collect upstream dependency notices from Cargo metadata; no network requests."""
import argparse
import json
from pathlib import Path
import re
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--packages", type=Path, help="cargo tree --prefix none --format '{p}' output for the built dependency graph")
args = parser.parse_args()
metadata = json.load(sys.stdin)
root_id = metadata["resolve"]["root"]
selected = None
if args.packages:
    selected = set()
    for line in args.packages.read_text().splitlines():
        match = re.fullmatch(r"([^\s]+) v([^\s]+)(?: .*)?", line)
        if not match:
            sys.exit("Unrecognized dependency graph line; inspect before packaging")
        selected.add(match.groups())
    available = {(p["name"], p["version"]) for p in metadata["packages"]}
    if not selected or not selected <= available:
        sys.exit("Dependency graph is empty or disagrees with Cargo metadata; inspect before packaging")
blocks = ["MASS dependency license notices\n\n"
          "Upstream notices are reproduced below, including alternative licenses and "
          "vendored-library notices. Package license expressions describe the upstream terms.\n"]
missing = []
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["id"] == root_id:
        continue
    if selected is not None and (package["name"], package["version"]) not in selected:
        continue
    root = Path(package["manifest_path"]).parent
    notices = sorted(p for p in root.rglob("*") if p.is_file() and
                     p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE")))
    if package.get("license_file"):
        declared = root / package["license_file"]
        if declared.is_file() and declared not in notices:
            notices.append(declared)
    if not notices:
        missing.append(f'{package["name"]} {package["version"]}')
        continue
    blocks.append(f'\n{"=" * 72}\n{package["name"]} {package["version"]}\n'
                  f'License expression: {package.get("license") or "see declared license file"}\n')
    for path in notices:
        blocks.append(f'\n--- {path.relative_to(root)} ---\n{path.read_text(errors="replace")}\n')
if missing:
    sys.exit("Missing dependency notices; inspect before packaging: " + ", ".join(missing))
sys.stdout.write("".join(blocks))
