# Preserved Bash/fzf prototype

Run these commands from the repository root. This is the separate historical prototype, not the Rust application documented in README.md.

**Multi Agent Session Search** — a Bash/fzf proof of concept for finding old Pi,
Claude Code, and Codex conversations.

A single search line, a result list, and a transcript preview. Search conversation
text, switch to fuzzy title/project matching, or deliberately reopen a session
in its original agent. No model calls are needed for search.

## Try it

From this directory:

```sh
./mass demo
```

The demo generates three synthetic conversations in `.cache/mass-demo/`. They are
**preview-only**: the resume action cannot start an agent.

Search for `callback`, `foreground`, or `SQLite`. Press **Ctrl-F** to try fuzzy
matching against session titles and project names instead.

## Search your history

```sh
./mass discover     # Filename counts only; no transcript content read
./mass index        # Explicitly build the local text index
./mass              # Open the picker
```

The first index can take time and disk space: this machine had about 12 GiB of
raw transcripts during initial research. MASS prints progress every 100 checked
files. Later runs skip unchanged files and reparse changed files.

Indexing is synchronous and interruptible. Ctrl-C preserves completed updates;
rerun `./mass index` to continue. Nothing automatically indexes on opening the
picker. Rerun the index command when you want fresh results.

### Keys

| Key | Action |
| --- | --- |
| Type | Search user/assistant text, summaries, titles, and project paths |
| Up / Down | Select a session; the side preview follows |
| Ctrl-F | Fuzzy title/project/agent matching using fzf |
| Ctrl-S | Return to full-text search |
| Enter | Read a bounded transcript preview in `less`; `q` returns |
| Ctrl-O | Show the resume command and ask for confirmation |
| Esc | Exit |

Mode switches clear the query. The side pane previews messages around a text
match; the separate Enter view currently shows recent indexed messages.

### Search syntax

```sh
./mass search 'callback redirect'
./mass search 'agent:pi callback'
./mass search 'agent:claude contrast'
./mass search 'agent:codex incremental'
./mass status
```

Full-text mode uses case-insensitive token-prefix matching: `increm` matches
`incremental`. Multiple words must match the same indexed message (its title
and project fields also participate). It is not semantic search or typo
correction. Punctuation separates tokens; quotation marks are not phrase-query
operators. Queries are treated as text, not executable SQL or shell code.

Fuzzy mode searches metadata, not whole transcript bodies. Empty full-text
queries show the 500 most recently modified sessions. Matching results are
ranked with SQLite FTS5, then grouped into one result per session file. The POC
considers at most 2,000 matching messages and displays up to 500 sessions;
fuzzy mode loads at most 100,000 session rows.

## Requirements

- Bash
- Python 3.10+ with SQLite FTS5
- A recent fzf; tested with **0.74.4**
- `less` for the separate preview, optional
- Python **3.14+ with `compression.zstd`** for compressed Codex transcripts

No Python packages are required. Tested locally with Python 3.14.7 and SQLite
3.53.4. The agent executables are needed only for explicit resume actions.
MASS does not install dependencies or execute agent launchers during discovery,
indexing, or previews.

## Sources

| Agent | Default transcript directories | Overrides |
| --- | --- | --- |
| Pi | `~/.pi/agent/sessions/` | `PI_CODING_AGENT_DIR`, `PI_CODING_AGENT_SESSION_DIR` |
| Claude Code | `~/.claude/projects/` | `CLAUDE_CONFIG_DIR` |
| Codex | `~/.codex/sessions/`, `~/.codex/archived_sessions/` | `CODEX_HOME` |

Only explicit transcript roots are scanned; directory and file symlinks are
skipped. The scanner does not open credentials, settings, or prompt-history
files elsewhere in an agent's home directory.

To search a copied archive or non-default stores, set `MASS_SOURCES` to an
explicit JSON array:

```json
[
  {
    "agent": "pi",
    "home": "/absolute/path/to/copied/.pi/agent",
    "sessions": "/absolute/path/to/custom-sessions",
    "resumable": false
  },
  {
    "agent": "claude",
    "home": "/absolute/path/to/copied/.claude",
    "resumable": false
  },
  {
    "agent": "codex",
    "home": "/absolute/path/to/copied/.codex",
    "resumable": false
  }
]
```

Custom sources default to preview-only. `sessions` is an optional explicit root;
otherwise directories are derived from `home` as shown above. Only mark a source
`resumable: true` if it is a live store you deliberately want to reopen.

```sh
MASS_SOURCES=/absolute/path/to/sources.json ./mass index
```

`MASS_DB` overrides the default `.cache/sessions.sqlite3` database.
`MASS_PYTHON` selects the Python executable used by the Bash launcher.
Choose a separate database when using a different source configuration:
removing an entire source from configuration does not purge its cached records.

## Resume behavior

MASS uses structured argv and the stored working directory, not a shell command
built from transcript text:

- Pi: `pi --session /absolute/path/to/session.jsonl`
- Claude Code: `claude --resume <uuid>`
- Codex: `codex resume <uuid>`

The owning agent's data-root environment is restored. Missing directories,
missing transcripts, preview-only sources, and recognized child-agent
transcripts cannot be resumed automatically. The POC does not attach to existing
live terminals or detect concurrent writers. Do not resume a session already
running elsewhere.

**Resuming is not a read-only action.** The agent loads its own permissions,
project configuration, extensions, hooks, and possibly active goals. MASS adds
no auto-approval flags, but does not override settings the agent restores.
On Omarchy, invoking an agent may run a lazy mise installer/update wrapper.
MASS shows this warning and asks before proceeding.

Resume opens the saved session, **not the specific historical message or branch
that matched**. Child results are labeled but not grouped into a parent tree yet.

## Coverage and privacy

This is a bounded, best-effort reader, not a universal archival format converter:

- Pi's recorded branches and compaction summaries are searchable. It never uses
  `SessionManager.open()`, which can migrate source files.
- Claude's recorded user/assistant entries are indexed, including entries still
  present before compaction. Internal formats may change between releases.
- Codex supports response items, event-only fallback, summaries, archived roots,
  and `.jsonl.zst`. Duplicate event projections are suppressed. Names from its
  separate metadata database and referenced/future history layouts are not
  reconstructed; the first user message is the fallback title.
- Tool results, tool arguments, reasoning, images, and extension-private entries
  are not indexed. Thus this is conversation-text search, not every byte of
  recorded agent activity.
- Records over 8 MiB are skipped; individual text messages are clipped at 32,768
  characters; accumulated extracted text is capped at 16,777,216 characters per
  file. These limits and malformed/incomplete records produce a `[partial]`
  label. Previews are separately bounded.
- Changed files are reparsed, not tailed. Files changing during parsing are
  deferred until the next run. No background watcher or automatic refresh yet.
- Successful complete scans remove deleted files from search results. Missing
  stores and failed/limited scans preserve cached history to avoid treating an
  offline mount as deletion. A failed update can therefore leave older cached
  contents available; index warnings must not be mistaken for full coverage.
- Removing rows is **not secure erasure** of SQLite pages or WAL history.
- The index contains **plaintext conversation text**, even though it excludes
  credentials files. Messages themselves may contain secrets. Cache files are
  owner-private, but not encrypted; use disk encryption and exclude the cache
  from backups/sync if appropriate. `.cache/` is git-ignored.
- Already-deleted transcripts, ephemeral sessions, and cloud-only histories
  cannot be recovered by this tool. No network source connectors are included.

## Development

```sh
python3 -m unittest discover -s tests -v
bash -n mass
shellcheck mass
```

Tests use synthetic data and project-local temporary directories. They exercise
all three adapters, incremental updates and deletion, compression, truncation,
read-only source behavior, query handling, launch planning, and a real fzf
pseudo-terminal flow including the resume guard. They never launch an agent.

Files:

- `mass` — Bash/fzf interface and confirmation flow
- `backend.py` — adapters, SQLite index, search, previews, and launch planning
- `demo.py` — synthetic, non-resumable examples
- `tests/` — standard-library unittest suite

The backend is kept separate so a later Omarchy QML plugin can replace the
fzf surface without replacing the transcript adapters.
