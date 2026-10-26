#!/usr/bin/env python3
"""Synthetic duplicate-refresh and filtered-query timings; never use live stores."""
import argparse
import json
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, default=ROOT / "target/release/mass")
parser.add_argument("--runs", type=int, default=5)
args = parser.parse_args()
if args.runs < 1:
    parser.error("--runs must be positive")
(ROOT / "target").mkdir(exist_ok=True)
result = {"scope": "synthetic; warm filesystem cache; process startup included", "runs": args.runs}
with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="regression-bench-") as tmp:
    root = Path(tmp)
    store = root / "sessions"
    store.mkdir()
    config = root / "config.toml"
    config.write_text(f'[[sources]]\nid="bench"\nagent="pi"\nroot={json.dumps(str(store))}\n')
    prefix = [str(args.binary.resolve()), "--config", str(config), "--db", str(root / "copies.sqlite3")]

    def run(*command):
        start = time.perf_counter()
        output = subprocess.run(prefix + list(command), capture_output=True, check=True)
        return (time.perf_counter() - start) * 1000, json.loads(output.stdout)

    def median(*command):
        run(*command)  # warmup
        samples = [run(*command) for _ in range(args.runs)]
        return round(statistics.median(t for t, _ in samples), 2), samples[-1][1]

    records = [{"type": "session", "id": "same", "cwd": str(root)},
               {"type": "session_info", "name": "duplicate performance fixture"}]
    records += [{"type": "message", "id": str(i), "message": {
        "role": "assistant", "content": "callback " + "repeatable background context " * 100
    }} for i in range(2000)]
    content = "".join(json.dumps(r) + "\n" for r in records)
    (store / "a.jsonl").write_text(content)
    run("index", "--json")
    alone, _ = median("index", "--json")
    (store / "b.jsonl").write_text(content)
    run("index", "--json")
    duplicate, report = median("index", "--json")
    assert len(run("search", "callback", "--json")[1]["results"]) == 1
    result["duplicate_refresh"] = {
        "transcript_bytes": len(content.encode()), "messages": 2000,
        "without_copy_median_ms": alone, "with_copy_median_ms": duplicate,
        "duplicates": report["coverage"]["duplicates"],
        "duplicates_unchanged": report["coverage"].get("duplicates_unchanged", 0),
    }
    for path in store.iterdir():
        path.unlink()
    prefix[-1] = str(root / "filters.sqlite3")
    for i in range(400):
        records = [{"type": "session", "id": str(i), "cwd": str(root / f"project-{i % 20}")},
                   {"type": "session_info", "name": f"Fixture {i}"}]
        records += [{"type": "message", "id": str(j), "message": {
            "role": "assistant", "content": "ordinary callback background context " * 30
        }} for j in range(40)]
        (store / f"{i:04}.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records))
    run("index", "--json")
    result["filtered_search"] = {"sessions": 400, "messages": 16000}
    for label, query, filters, expected in [
        ("common_unfiltered", "callback", [], 20),
        ("common_absent_agent", "callback", ["--agent", "claude"], 0),
        ("common_one_project", "callback", ["--project", str(root / "project-1")], 20),
        ("empty_absent_agent", "", ["--agent", "claude"], 0),
        ("rare_absent_agent", "zzznomatch", ["--agent", "claude"], 0),
    ]:
        elapsed, page = median("search", query, *filters, "--json")
        assert len(page["results"]) == expected, label
        result["filtered_search"][label + "_median_ms"] = elapsed
print(json.dumps(result, indent=2))
