#!/usr/bin/env python3
"""Read-only transcript adapters and a disposable, local SQLite search index."""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import sqlite3
import sys
import unicodedata
import uuid

try:
    from compression import zstd
except ImportError:
    zstd = None

READ_ERRORS = (OSError, ValueError, EOFError, sqlite3.Error) + ((zstd.ZstdError,) if zstd else ())
SCHEMA = 1
MAX_LINE = 8 * 1024 * 1024
MAX_TEXT = 32 * 1024
MAX_SESSION_TEXT = 16 * 1024 * 1024
DEFAULT_DB = Path(__file__).resolve().parent / ".cache/sessions.sqlite3"


def clean(value):
    """No terminal escapes, tabs, newlines, or bidi control characters in rows."""
    return " ".join("".join(c if not unicodedata.category(c).startswith("C") else " "
                            for c in str(value or "")).split())


def text_content(value):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(x.get("text", "") for x in value
                         if isinstance(x, dict) and x.get("type") in
                         ("text", "input_text", "output_text") and isinstance(x.get("text"), str))
    return ""


def sources(config=None):
    if config:
        values = json.loads(Path(config).read_text())
        if not isinstance(values, list):
            raise ValueError("Sources file must contain a JSON array")
    else:
        home = Path.home()
        values = [
            {"agent": "pi", "home": os.environ.get("PI_CODING_AGENT_DIR", str(home / ".pi/agent")),
             "sessions": os.environ.get("PI_CODING_AGENT_SESSION_DIR"), "resumable": True},
            {"agent": "claude", "home": os.environ.get("CLAUDE_CONFIG_DIR", str(home / ".claude")), "resumable": True},
            {"agent": "codex", "home": os.environ.get("CODEX_HOME", str(home / ".codex")), "resumable": True},
        ]
    result = []
    for value in values:
        agent = value["agent"]
        if agent not in ("pi", "claude", "codex"):
            raise ValueError(f"Unsupported agent: {agent}")
        home = Path(value["home"]).expanduser().resolve()
        roots = ([Path(value["sessions"]).expanduser().resolve()] if value.get("sessions") else
                 [home / "projects"] if agent == "claude" else
                 [home / "sessions", home / "archived_sessions"] if agent == "codex" else [home / "sessions"])
        result.append({"agent": agent, "home": str(home), "roots": [str(p) for p in roots],
                       "resumable": value.get("resumable") is True})
    return result


def discover(source):
    """Walk only explicit transcript directories, never follow directory symlinks."""
    paths, errors, available = [], [], False
    for root in source["roots"]:
        root = Path(root)
        if not root.exists():
            continue
        if not root.is_dir() or root.is_symlink():
            errors.append(f"not a regular directory: {root}")
            continue
        available = True
        def onerror(error):
            errors.append(str(error))
        for directory, dirs, files in os.walk(root, followlinks=False, onerror=onerror):
            dirs[:] = sorted(d for d in dirs if not (Path(directory) / d).is_symlink())
            for name in sorted(files):
                if not name.endswith((".jsonl", ".jsonl.zst")):
                    continue
                path = Path(directory) / name
                if not path.is_symlink() and path.is_file():
                    paths.append(path)
    # During compression transitions, prefer the uncompressed representation.
    plain = {str(p) for p in paths if p.suffix == ".jsonl"}
    paths = [p for p in paths if p.suffix != ".zst" or str(p)[:-4] not in plain]
    return paths, errors, available


def connect(path, write=False):
    path = Path(path)
    if write:
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        if path.is_symlink():
            raise ValueError("Refusing a symlink database")
        db = sqlite3.connect(path)
        os.chmod(path, 0o600)
    else:
        if not path.is_file():
            raise ValueError("No index yet. Run ./mass index first (or ./mass demo).")
        db = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    db.execute("PRAGMA busy_timeout=5000")
    if write:
        db.execute("PRAGMA journal_mode=WAL")
        db.execute("PRAGMA foreign_keys=ON")
        version = db.execute("PRAGMA user_version").fetchone()[0]
        if version not in (0, SCHEMA):
            raise ValueError("Unsupported index version; choose a new --db path and re-index")
        db.executescript("""
        CREATE TABLE IF NOT EXISTS sessions (
          id INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT UNIQUE NOT NULL,
          agent TEXT NOT NULL, home TEXT NOT NULL, session_root TEXT NOT NULL,
          path TEXT NOT NULL, session_id TEXT NOT NULL, cwd TEXT NOT NULL,
          title TEXT NOT NULL, modified REAL NOT NULL, fingerprint TEXT NOT NULL,
          resumable INTEGER NOT NULL, warning TEXT NOT NULL, child INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS messages (
          id INTEGER PRIMARY KEY, session INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
          role TEXT NOT NULL, text TEXT NOT NULL, ref TEXT NOT NULL, parent TEXT NOT NULL, line INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS messages_session ON messages(session);
        CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
          title, project, body, content='messages_view', content_rowid='id', prefix='2 3');
        CREATE VIEW IF NOT EXISTS messages_view AS
          SELECT m.id, s.title AS title, s.cwd AS project, m.text AS body
          FROM messages m JOIN sessions s ON s.id=m.session;
        CREATE TRIGGER IF NOT EXISTS messages_ad BEFORE DELETE ON messages BEGIN
          INSERT INTO search(search,rowid,title,project,body)
          SELECT 'delete',old.id,s.title,s.cwd,old.text FROM sessions s WHERE s.id=old.session;
        END;
        CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
          INSERT INTO search(rowid,title,project,body)
          SELECT new.id,s.title,s.cwd,new.text FROM sessions s WHERE s.id=new.session;
        END;
        """)
        db.execute(f"PRAGMA user_version={SCHEMA}")
    return db


def parse(path, agent):
    metadata = {"session_id": "", "cwd": "", "title": "", "child": int("subagents" in path.parts)}
    messages, fallback, warnings = [], [], Counter()
    total = 0
    stream = None
    if path.suffix == ".zst":
        if zstd is None:
            raise ValueError("Compressed Codex history needs Python 3.14+ with compression.zstd")
        stream = zstd.open(path, "rb")
    else:
        stream = path.open("rb")
    with stream:
        line_no = 0
        while True:
            raw = stream.readline(MAX_LINE + 1)
            if not raw:
                break
            line_no += 1
            if len(raw) > MAX_LINE:
                while raw and not raw.endswith(b"\n"):
                    raw = stream.readline(MAX_LINE + 1)
                warnings["oversized records skipped"] += 1
                continue
            try:
                entry = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                warnings["invalid or unfinished records skipped"] += 1
                continue
            if not isinstance(entry, dict):
                continue
            kind = entry.get("type")
            role, text, is_fallback = "", "", False
            if agent == "pi":
                if kind == "session":
                    metadata["session_id"] = str(entry.get("id") or "")
                    metadata["cwd"] = str(entry.get("cwd") or "")
                    if entry.get("version", 1) not in (1, 2, 3):
                        warnings["unrecognized Pi version"] += 1
                elif kind == "session_info" and entry.get("name"):
                    metadata["title"] = str(entry["name"])
                elif kind == "message" and isinstance(entry.get("message"), dict):
                    message = entry["message"]
                    role = message.get("role")
                    if role in ("user", "assistant"):
                        text = text_content(message.get("content"))
                elif kind in ("compaction", "branch_summary"):
                    role, text = "summary", entry.get("summary", "")
            elif agent == "claude":
                if entry.get("sessionId"):
                    metadata["session_id"] = str(entry["sessionId"])
                if entry.get("cwd"):
                    metadata["cwd"] = str(entry["cwd"])
                if entry.get("isSidechain"):
                    metadata["child"] = 1
                if kind == "custom-title" and entry.get("customTitle"):
                    metadata["title"] = str(entry["customTitle"])
                elif kind == "summary" and entry.get("summary") and not metadata["title"]:
                    metadata["title"] = str(entry["summary"])
                elif kind in ("user", "assistant") and not entry.get("isMeta"):
                    message = entry.get("message", {})
                    if isinstance(message, dict):
                        role, text = kind, text_content(message.get("content"))
                        if entry.get("isCompactSummary"):
                            role = "summary"
            else:
                payload = entry.get("payload", {})
                if not isinstance(payload, dict):
                    continue
                if kind == "session_meta":
                    metadata["session_id"] = str(payload.get("id") or "")
                    metadata["cwd"] = str(payload.get("cwd") or "")
                    metadata["child"] = int(isinstance(payload.get("source"), dict) or bool(payload.get("parent_thread_id")))
                elif kind == "turn_context" and payload.get("cwd"):
                    metadata["cwd"] = str(payload["cwd"])
                elif kind == "response_item" and payload.get("type") == "message":
                    role = payload.get("role")
                    if role in ("user", "assistant"):
                        text = text_content(payload.get("content"))
                elif kind == "event_msg" and payload.get("type") in ("user_message", "agent_message"):
                    role = "user" if payload["type"] == "user_message" else "assistant"
                    text, is_fallback = payload.get("message", ""), True
                elif kind == "compacted":
                    role, text = "summary", payload.get("message", "")
            if not isinstance(text, str) or not text.strip():
                continue
            if len(text) > MAX_TEXT:
                warnings["messages truncated"] += 1
                text = text[:MAX_TEXT]
            if total + len(text) > MAX_SESSION_TEXT:
                warnings["session text limit reached"] = 1
                continue
            total += len(text)
            item = (role, text, str(entry.get("id") or entry.get("uuid") or line_no),
                    str(entry.get("parentId") or entry.get("parentUuid") or ""), line_no)
            (fallback if is_fallback else messages).append(item)
    # Codex may persist both response items and their user/agent event projections.
    seen = {(m[0], m[1]) for m in messages}
    messages.extend(m for m in fallback if (m[0], m[1]) not in seen)
    messages.sort(key=lambda m: m[4])
    if not metadata["session_id"] and agent == "claude":
        try:
            metadata["session_id"] = str(uuid.UUID(path.stem))
        except ValueError:
            pass
    if not metadata["title"]:
        metadata["title"] = next((clean(m[1])[:140] for m in messages if m[0] == "user"), "Untitled session")
    metadata["warning"] = "; ".join(f"{name}: {count}" for name, count in warnings.items())
    return metadata, messages


def source_key(source, path):
    logical = str(path)[:-4] if path.suffix == ".zst" else str(path)
    return f"{source['agent']}:{logical}"


def index(args):
    configured = sources(args.sources)
    if args.limit is not None and args.limit < 1:
        raise ValueError("--limit must be positive")
    db = connect(args.db, write=True)
    stats = Counter()
    processed = 0
    for source in configured:
        paths, errors, available = discover(source)
        print(f"{source['agent']}: {len(paths):,} transcript files" + (" (store unavailable)" if not available else ""), file=sys.stderr)
        for error in errors:
            print(f"  Discovery warning: {clean(error)}", file=sys.stderr)
        seen = set()
        stopped = False
        for path in paths:
            if args.limit is not None and processed >= args.limit:
                stopped = True
                break
            processed += 1
            key = source_key(source, path)
            seen.add(key)
            try:
                stat = path.stat()
                config_hash = hashlib.sha256(json.dumps(source, sort_keys=True).encode()).hexdigest()[:16]
                fingerprint = f"{SCHEMA}:{stat.st_size}:{stat.st_mtime_ns}:{stat.st_ino}:{config_hash}"
                old = db.execute("SELECT id,fingerprint FROM sessions WHERE source=?", (key,)).fetchone()
                if old and old["fingerprint"] == fingerprint:
                    stats["unchanged"] += 1
                    continue
                meta, messages = parse(path, source["agent"])
                after = path.stat()
                if (stat.st_size, stat.st_mtime_ns, stat.st_ino) != (after.st_size, after.st_mtime_ns, after.st_ino):
                    stats["changing; retry next index"] += 1
                    continue
                root = next(root for root in source["roots"] if path.is_relative_to(root))
                with db:
                    if old:
                        sid = old["id"]
                        # Delete before updating session metadata: the FTS trigger needs old values.
                        db.execute("DELETE FROM messages WHERE session=?", (sid,))
                        db.execute("DELETE FROM sessions WHERE id=?", (sid,))
                    else:
                        sid = None
                    cursor = db.execute("""INSERT INTO sessions
                      (id,source,agent,home,session_root,path,session_id,cwd,title,modified,fingerprint,resumable,warning,child)
                      VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
                      (sid, key, source["agent"], source["home"], root, str(path), meta["session_id"],
                       meta["cwd"], clean(meta["title"]), stat.st_mtime, fingerprint,
                       int(source["resumable"]), meta["warning"], meta["child"]))
                    sid = cursor.lastrowid
                    # Metadata-only sessions must also be searchable.
                    db.executemany("INSERT INTO messages(session,role,text,ref,parent,line) VALUES (?,?,?,?,?,?)",
                                   [(sid, *m) for m in messages] or [(sid, "metadata", "", "", "", 0)])
                stats["indexed"] += 1
                stats["partial"] += bool(meta["warning"])
            except READ_ERRORS as error:
                stats["failed"] += 1
                print(f"  Skipped {clean(path.name)}: {clean(error)}", file=sys.stderr)
            if processed % 100 == 0:
                print(f"  Checked {processed:,} files…", file=sys.stderr)
        # Purge only after a complete scan. Unavailable roots never imply deletion.
        if not stopped and not errors:
            scanned_roots = {r for r in source["roots"] if Path(r).is_dir() and not Path(r).is_symlink()}
            existing = db.execute("SELECT id,source,session_root FROM sessions WHERE agent=? AND home=?", (source["agent"], source["home"])).fetchall()
            with db:
                for row in existing:
                    if row["session_root"] in scanned_roots and row["source"] not in seen:
                        db.execute("DELETE FROM messages WHERE session=?", (row["id"],))
                        db.execute("DELETE FROM sessions WHERE id=?", (row["id"],))
                        stats["removed"] += 1
        if stopped:
            break
    print("Index: " + ", ".join(f"{v:,} {k}" for k, v in stats.items()) + ".", file=sys.stderr)
    print("No agent was launched. Source transcripts were not modified.", file=sys.stderr)
    db.close()
    return 1 if stats["failed"] else 0


def emit_row(row, hit=""):
    date = datetime.fromtimestamp(row["modified"], timezone.utc).strftime("%Y-%m-%d")
    project = Path(row["cwd"]).name or "unknown project"
    flags = (" [child]" if row["child"] else "") + (" [partial]" if row["warning"] else "")
    values = [row["id"], row["agent"], date, project, row["title"] + flags, hit]
    print("\t".join(clean(v) for v in values))


def search(args):
    db = connect(args.db)
    query = args.query.strip()
    agent = args.agent
    filters = re.findall(r"(?:^|\s)agent:(pi|claude|codex)(?=\s|$)", query)
    if filters:
        agent = filters[-1]
        query = re.sub(r"(?:^|\s)agent:(pi|claude|codex)(?=\s|$)", " ", query)
    tokens = re.findall(r"[^\W_]+", query, re.UNICODE)[:24]
    if not tokens:
        sql = "SELECT * FROM sessions WHERE (?='' OR agent=?) ORDER BY modified DESC LIMIT ?"
        for row in db.execute(sql, (agent, agent, args.limit)):
            emit_row(row)
    else:
        # Quote every term: input is text, never raw MATCH syntax or SQL.
        match = " AND ".join('"' + token.replace('"', '""') + '"*' for token in tokens)
        hits = db.execute("""SELECT s.*, snippet(search,2,'[',']',' … ',24) AS hit,
          bm25(search,6.0,3.0,1.0) AS score
          FROM search JOIN messages m ON m.id=search.rowid JOIN sessions s ON s.id=m.session
          WHERE search MATCH ? AND (?='' OR s.agent=?) ORDER BY score LIMIT 2000""", (match, agent, agent))
        seen = set()
        for row in hits:
            if row["id"] in seen:
                continue
            seen.add(row["id"])
            emit_row(row, row["hit"])
            if len(seen) >= args.limit:
                break
    db.close()


def session(db, ident):
    row = db.execute("SELECT * FROM sessions WHERE id=?", (ident,)).fetchone()
    if row is None:
        raise ValueError("This result is no longer in the index; refresh the picker")
    return row


def preview(args):
    db = connect(args.db)
    row = session(db, args.id)
    print(f"{clean(row['title'])}\n{row['agent']} · {clean(row['cwd']) or 'unknown directory'}")
    print(f"Source: {clean(row['path'])}")
    print("All recorded branches · user/assistant text and summaries only")
    print("Resume opens the saved session, not necessarily the matching branch.\n")
    if row["warning"]:
        print(f"Partial coverage: {row['warning']}\n")
    terms = [t.casefold() for t in re.findall(r"[^\W_]+", args.query) if t not in ("agent", "pi", "claude", "codex")]
    rows = db.execute("SELECT * FROM messages WHERE session=? ORDER BY line", (args.id,)).fetchall()
    matches = [i for i, m in enumerate(rows) if terms and all(t in m["text"].casefold() for t in terms)]
    center = matches[0] if matches else max(0, len(rows) - 6)
    chosen = rows[max(0, center - 1):center + 7]
    for message in chosen:
        print(f"── {message['role']} · line {message['line']} · {clean(message['ref'])} ──")
        text = message["text"]
        if terms:
            positions = [text.casefold().find(t) for t in terms if t in text.casefold()]
            offset = max(0, min(positions, default=0) - 300)
        else:
            offset = 0
        shown = text[offset:offset + 4500]
        print(("… " if offset else "") + "\n".join(clean(line) for line in shown.splitlines()))
        if offset + len(shown) < len(text):
            print("… [preview clipped]")
        print()
    if len(chosen) < len(rows):
        print(f"Showing {len(chosen)} of {len(rows)} indexed messages.")
    db.close()


def launch_plan(row):
    if not row["resumable"]:
        raise ValueError("Preview-only source: resuming is disabled")
    if row["child"]:
        raise ValueError("Child transcript: resume its parent in the original agent; direct resume is disabled")
    path = Path(row["path"])
    if not path.is_file() or path.is_symlink():
        raise ValueError("Source transcript is missing or is now a symlink; refresh the index")
    cwd = Path(row["cwd"])
    if not row["cwd"] or not cwd.is_absolute() or not cwd.is_dir():
        raise ValueError("Original working directory is unavailable; automatic resume is disabled")
    agent = row["agent"]
    env = {}
    if agent == "pi":
        env = {"PI_CODING_AGENT_DIR": row["home"], "PI_CODING_AGENT_SESSION_DIR": row["session_root"]}
        argv = ["pi", "--session", str(path)]
    else:
        ident = str(uuid.UUID(row["session_id"]))
        env["CLAUDE_CONFIG_DIR" if agent == "claude" else "CODEX_HOME"] = row["home"]
        argv = [agent, "--resume" if agent == "claude" else "resume", ident]
    return {"cwd": str(cwd), "env": env, "argv": argv}


def plan(args):
    db = connect(args.db)
    result = launch_plan(session(db, args.id))
    db.close()
    if args.command == "resume":
        if not args.confirm:
            raise ValueError("Resume requires --confirm; use the picker for a confirmation prompt")
        if not shutil.which(result["argv"][0]):
            raise ValueError(f"{result['argv'][0]} is not on PATH; nothing was installed or launched")
        os.chdir(result["cwd"])
        os.execvpe(result["argv"][0], result["argv"], {**os.environ, **result["env"]})
    else:
        print(f"Directory: {clean(result['cwd'])}")
        print("Command:   " + clean(shlex.join(result["argv"])))
        print("Store:     " + clean(row_env(result["env"])))
        print("This may invoke an Omarchy/mise installer. The agent loads its own settings,\npermissions, hooks and extensions. This tool adds no auto-approval flags.")


def row_env(env):
    return " ".join(f"{key}={value}" for key, value in env.items())


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", default=os.environ.get("MASS_DB", str(DEFAULT_DB)))
    parser.add_argument("--sources", default=os.environ.get("MASS_SOURCES"))
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("index")
    p.add_argument("--limit", type=int)
    sub.add_parser("discover")
    p = sub.add_parser("search")
    p.add_argument("--query", default="")
    p.add_argument("--agent", choices=("pi", "claude", "codex", ""), default="")
    p.add_argument("--limit", type=int, default=500)
    p = sub.add_parser("preview")
    p.add_argument("id", type=int)
    p.add_argument("--query", default="")
    sub.add_parser("plan").add_argument("id", type=int)
    p = sub.add_parser("resume")
    p.add_argument("id", type=int)
    p.add_argument("--confirm", action="store_true")
    sub.add_parser("status")
    args = parser.parse_args()
    try:
        if args.command == "index":
            return index(args)
        if args.command == "discover":
            for source in sources(args.sources):
                paths, errors, available = discover(source)
                print(json.dumps({"agent": source["agent"], "roots": source["roots"], "files": len(paths),
                                  "available": available, "warnings": errors, "resumable": source["resumable"]}))
        elif args.command == "search":
            search(args)
        elif args.command == "preview":
            preview(args)
        elif args.command in ("plan", "resume"):
            plan(args)
        elif args.command == "status":
            db = connect(args.db)
            for row in db.execute("SELECT agent,count(*) AS n,sum(warning!='') AS partial FROM sessions GROUP BY agent"):
                print(f"{row['agent']}: {row['n']:,} sessions, {row['partial']:,} partial")
            print(f"Index: {args.db}")
            db.close()
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error) as error:
        print(f"MASS: {clean(error)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\nCancelled. Completed index updates are preserved.", file=sys.stderr)
        raise SystemExit(130)
