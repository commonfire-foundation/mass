# MASS Rust development build

This is a public development preview, not an official release or a 1.0 compatibility claim. The bounded first-release checklist is in `docs/LOCAL-RELEASE.md`; broader design goals are not all first-release blockers. The Bash/Python prototype remains unchanged as a reference. `./mass` still runs that prototype; `target/release/mass` runs Rust.

## Build and run

Requires a current stable Rust toolchain (latest local validation: Rust 1.99.0; no minimum-version guarantee), a C compiler, and a normal host linker. SQLite/FTS5 and zstd are bundled. No Python, fzf, agent runtime, or network access is required by the installed executable for indexing/search/reading. Experimental, explicitly enabled resume deliberately launches the owning agent; offline native loading/return is checked for specific versions, but normal authenticated configurations are not release-verified (see `docs/HARNESS-COMPATIBILITY.md`).

```sh
cargo build --locked --release
./target/release/mass --help
./target/release/mass status
./target/release/mass index
./target/release/mass search '"redirect callback"' --json
./target/release/mass tui
```

Optional installation, performed explicitly by the user:

```sh
cargo install --locked --path .
```

For development with all Cargo caches inside this directory:

```sh
export CARGO_HOME="$PWD/.cargo-home"
```

`bash scripts/package.sh` creates a versioned host-platform archive, SHA-256 file, and a dynamic-library report in `dist/`. These are local artifacts, not published releases. Linux/glibc is the validated platform; do not assume a universally static binary or cross-platform release support.

## Storage and configuration

Defaults are `$XDG_CONFIG_HOME/mass/config.toml` and `$XDG_CACHE_HOME/mass/index.sqlite3`, with `~/.config` and `~/.cache` fallbacks. `--config FILE` and `--db FILE` override them. Paths in configuration are absolute, not shell-expanded.

Absent configuration discovers the conventional Pi, Claude Code, Codex active, and Codex archived stores. Missing optional default stores are informational. Explicit configuration **replaces** this list; list every store you want indexed. Resume is disabled by default.

```toml
exclusions = ["/excluded-project/"] # literal path substrings, not globs

[[sources]]
id = "pi-local"                  # keep this ID stable across moves/rebuilds
agent = "pi"
root = "/home/me/.pi/agent/sessions"
resume = false
optional = false

[[sources]]
id = "codex-archive"
agent = "codex"
root = "/home/me/.codex/archived_sessions"
```

Agent values: `pi`, `claude`, `codex`. Claude roots normally end in `projects`; Codex/Pi active roots normally end in `sessions`. Separate stores need distinct IDs. Removing a configured source purges its derived cache on the next completed index run; an unavailable configured root retains its cached history. Deleted/excluded files reconcile only after successful traversal. No transcript is modified.

**Privacy:** the database contains sensitive plaintext. Newly created Unix database files have mode 0600. Do not place the cache in a shared directory. Deleting the database does not guarantee secure deletion from WAL files, filesystem snapshots, or backups.

**Migration:** Rust never imports or modifies the prototype `.cache/sessions.sqlite3` or `MASS_SOURCES`. Explicitly choose a new Rust cache and run `index` to rebuild. Do not point Rust at the prototype database.

Rust schema 1 upgrades transactionally to schema 2 on `mass index` or Ctrl-L. Read-only commands request an upgrade rather than modifying the database. Cached history, session/message references, and unavailable stores survive the upgrade; adapter version 3 reparses available files on refresh to repair projection deduplication. Failed upgrades roll back the schema and data. Older binaries cannot read schema 2. Allow spare disk space for the upgrade and WAL. SQLite retains freed pages in an upgraded file, so its physical size may not immediately shrink; the benchmark's smaller sizes describe fresh indexes.

Schema 2 stores each chunk's text once in normalized message columns, uses external-content FTS over a joined view, and indexes agent/workspace/activity metadata. Public JSON remains version 1. Filtering happens before ranking and fuzzy matching; impossible scopes skip FTS traversal altogether. Excerpts and session JSON are fetched only for the selected page.

## CLI contract

- Bare `mass` starts the TUI only on a terminal; redirected bare invocation exits 2.
- `search QUERY`: phrase/token/prefix FTS plus weaker one-edit title/project matches, grouped by source-qualified session without a pre-group message cap.
- Filters: `--agent`, `--project`, `--since`, `--before`, `--limit`, `--cursor`; query syntax supports `agent:` and `project:`. Dates are YYYY-MM-DD, since-inclusive/before-exclusive. Activity falls back explicitly to file modification time. Fully quoted tokens containing colons are literals, including `"agent:pi"`; an unquoted prefix such as `project:"path with spaces"` still introduces a filter.
- `--json` produces version-1 data, no ANSI/progress. Index progress goes to stderr. Zero hits are successful; missing/invalid indexes and SQL budget failures are errors, not empty results.
- Search has a two-second SQLite execution budget. Exhaustion fails the operation; no lossy truncated page is returned. Narrow the query after an interruption. Pagination tokens bind query, filters, scan state, and a transactional revision. Queries use a consistent read snapshot; concurrent index commits invalidate old cursors.
- `show REF [--at MESSAGE_REF] [--json]` streams the indexed history from that passage without a pager. References are SHA-256 source/native-ID identities; any unambiguous prefix of at least four hex characters is accepted.
- `export REF --format markdown|json --output FILE [--message MESSAGE_REF] [--include-paths]` writes a clean indexed-text document. A selected chunk exports its complete logical message; verified adjacent chunk overlap is removed. Source/cwd metadata is omitted unless requested, but title/text content is not redacted. New files are owner-private and committed atomically without overwriting existing files/symlinks. Output is not a native harness backup or import format.
- `resume REF --dry-run` reports argv, cwd, store overrides, supported status, and warnings without launching anything. Real resume requires a terminal and confirmation, even with source permission. `--yes` skips only confirmation. Child, archived/custom-layout, missing-source, changed-identity, invalid-cwd, and unavailable-executable cases are blocked. Live-session status remains unknown. The executable is resolved to an absolute path. After confirmation, MASS reloads configuration and rechecks current indexed/source identity and plan equality before launching. Installed-harness verification blockers are recorded in `docs/HARNESS-COMPATIBILITY.md`.
- `status [--json]` reports configured roots, availability, cache location, and last attempted/successful scan plus coverage warnings.

Terminal labels remove terminal/bidi control characters. CLI transcript output, Markdown exports, and clipboard text retain tabs and line endings while stripping other terminal/bidi controls. The TUI expands tabs to four spaces for display only. JSON preserves transcript content as escaped data; consumers must sanitize it before terminal rendering. The CLI emits no color.

## TUI

The title displays `M.A.S.S. - Multi Agent Session Search` followed by a muted `vMAJOR.MINOR.PATCH` suffix. The suffix follows the package version in `Cargo.toml` automatically, as does `mass --version`. See `VERSIONING.md` and `CHANGELOG.md` for release tracking.

The redesigned interface uses a single divider, bold titles, quieter metadata, terminal-native cyan emphasis, and a selected-title marker. `NO_COLOR` keeps bold/underline/reverse cues without setting foreground/background colors. Results show disambiguated short project names and relative dates; Ctrl-D reveals full paths, identity, and timestamp provenance. Up/Down and PageUp/PageDown scroll details, help, and resume-review sheets.

Type to search; arrows select; Enter reads the selected passage; Esc returns without clearing the query or losing the reading position. Wide terminals show a preview; narrow terminals use the reader. Tab/Shift-Tab move between visible panes, with a cursor or › marker indicating focus. Typing or Backspace returns to search. Left/Right, Home/End, Delete, Ctrl-A/E/U, Unicode grapheme editing, horizontal scrolling, and bracketed paste work in the search field.

Ctrl-F opens labeled Agent, Project, Since, and Before fields. Use Tab between fields, Left/Right for agents, and type/Up/Down for project suggestions. Enter applies; Ctrl-L clears filters without changing the search query; Esc cancels. Ctrl-K shows keyboard help (and does not delete query text); Ctrl-L outside the filter dialog requests an incremental scan; Ctrl-C exits with 130. Function keys are not used.

Ctrl-O opens session actions for the selected result, or the message at the top of the reader. Export, explicit local clipboard copying, source location, and native-reopen review share this menu. Clipboard actions detect wl-copy/xclip and run off the rendering thread with a three-second helper timeout; no automatic installation or OSC52 fallback is used. Exports stay off the rendering thread and keep the destination absent until complete. Opening the menu does not export, copy, or launch anything.

Ctrl-R opens an in-TUI resume review. Esc cancels without launching anything. C copies an available reviewed command when a clipboard helper exists; this does not launch anything. Only Y on a supported, freshly revalidated plan launches the agent, after restoring the terminal. MASS returns to the same screen after the process exits. Source permissions and identity/cwd/executable guards remain in force.

PageDown advances result pages; PageUp returns to the previous page and restores its selection. Changing queries/filters or completing a refresh clears page history. In the reader, arrows and PageUp/PageDown scroll actual wrapped display rows, fetching adjacent history automatically at the edges. Left/Right jump to adjacent history. The reader shows speaker separation, Markdown headings and fenced-code structure, preserved indentation, search-term emphasis, approximate history position, and nearby-match indicators. Selected excerpts locate the initial passage within its text chunk. `/` performs literal conversation find; Enter searches beyond loaded history when necessary; n/N navigate matches and wrap through the conversation. Find runs on the worker with a two-second budget and explicit errors, not silent incomplete results. Highlighting is presentation-only; search ranking/semantics are unchanged.

Rendering uses bounded 32-chunk buffers, not an entire transcript in memory. Adjacent chunk overlap is removed when verified; formatting is deliberately basic (not a full Markdown renderer). CLI `show` still streams all chunks. Clipboard actions now live in Ctrl-O; the former nonfunctional Ctrl-Y shortcut remains removed.

The normal TUI header is neutral: `Local conversation search`. Importer diagnostics are retained in `mass status`, not shown as a raw warning count or a “not fully indexed” alarm. A completed refresh with per-record diagnostics is not converted into a failed-refresh notification. Actual refresh/search errors still surface; this presentation change does not imply complete import coverage or fix adapter limitations.

First run uses the same cancellable, exclusion-aware filename discovery as indexing and shows an explicit Enter-to-build prompt before creating the cache. Empty/loading/error states appear in the main panel, not just a clipped status line. Query/reader work and indexing use separate worker threads/connections; committed results refresh during a long scan. Old query generations are rejected. Normal reader windows, find, and streaming reads share a consistent database snapshot across their component queries. Escape cancels a pending resume review; Ctrl-C cancels worker work and restores the terminal before joining workers. Cancellation is cooperative and cannot interrupt a blocked kernel filesystem call. Already-blocked resume plans do not parse the transcript; the review states that identity was not checked. Disabled resume reports its reason without closing the TUI. There is no startup daemon or silent CLI indexing.

## Implemented adapter boundary

Adapter version 3 supports Pi v1–3 message/summary records, Pi subagent export v1 message records, Claude message/custom-title records, and Codex rollout response/event/compaction records, including `.jsonl.zst`. Pi exports use run/child-scoped identities and are read-only. Claude child identities include `agentId`, keeping siblings distinct from their shared parent session ID. Codex forks retain their owning header; explicitly linked ancestor headers cannot overwrite its identity or working directory. Unrelated mixed identities still fail safely.

Identical normalized histories and exact prefixes are deduplicated, retaining the fuller history when identity and context agree and neither parse has omissions. Divergent copies remain separate readable histories; automatic resume is blocked for the ambiguous family. Copy references are file-qualified and retained on append; renaming an ambiguous copy can change its reference. Conservative read-only guards remain until a fresh cache rebuild rather than silently re-enabling resume.

JSONL is streamed with selective field retention: unneeded image/base64 and other payload fields are discarded before allocation, so a record exceeding 16 MiB solely because of such fields can still contribute text. A 16 MiB retained-JSON budget still bounds text/metadata. Malformed records or retained-budget exhaustion produce persistent diagnostics with a file, reason, count, and first affected line. Empty new placeholders are counted separately, without a missing-identity warning; truncating an already indexed transcript retains its cached history and reports the issue. `mass status --json` includes `duplicates`, `duplicates_unchanged` (the fingerprint-cache subset), and `ignored_empty` counters. Duplicate discovery records retain both copy and canonical fingerprints. Skipping a copy requires an unchanged canonical file that is still eligible in the current scan. Canonical files refresh before their copies; deletion, exclusion, divergence, appends, and source removal invalidate or reconcile the cache.

Text uses 16 KiB UTF-8-safe chunks with 512-byte overlap, without a whole-session text cap. Overlap is visible in full chunk-oriented `show` output. Codex event projections and Pi initial-prompt projections pair one-to-one with adjacent normalized message occurrences only when their complete chunk sequences agree. A normalized occurrence cannot consume repeated later prompts, and matching prefixes cannot delete parts of longer messages. Unmatched or ambiguous projections are retained rather than globally deduplicated by text. Unchanged files skip parsing; changed files reparse transactionally with a reused insert statement. Adapter-version fingerprints automatically trigger reimport on the next refresh. One writer uses an OS lock; committed history remains readable. Unknown historical formats are not claimed to be fully supported.

## Validation

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 tests/rust_tui_pty.py
python3 tests/rust_cli_recovery.py
cargo build --release --locked
python3 scripts/benchmark.py --sessions 200 --runs 20
python3 scripts/benchmark-regressions.py --runs 5
```

Automated tests/benchmarks use synthetic stores under the project directory. An additional explicitly approved real-corpus audit used a separate private project-local cache; only aggregate evidence is recorded in `docs/VALIDATION.md`. Python is a development-only dependency for PTY tests and the baseline benchmark. The benchmark checks basic retrieval on identical fixtures and reports startup-inclusive query p50/p95, first query, initial/unchanged indexing, and index size. It does not claim OS-cold-cache or peak-memory measurements.

## Known limits and deferred work

The first public release targets local search and reading on the validated Linux host platform. These limits must be documented, not all engineered away before a pre-1.0 release:

- Searchable text/metadata above the 16 MiB retained-record budget still skips with diagnostics. Arbitrarily large text-string support is deferred. The local corpus audit includes observed indexing/query peak RSS, not a worst-case memory proof.
- Per-source successful-scan history, detailed freshness checks, and finer coverage taxonomy need hardening; current persistent scan reporting is aggregate.
- Additional missing-ID formats, inherited fork-text deduplication across sessions, and historical adapter variants need a curated real-corpus audit. Known Pi exports and Claude/Codex child identities are supported; genuinely ambiguous/missing identities still fail safely instead of guessing.
- Refresh remains explicit via Ctrl-L or `mass index`; no startup watcher is included. Clipboard helpers are optional external programs, never bundled or installed by MASS. Reader formatting is basic. Broad human theme/accessibility review remains outside the synthetic color/NO_COLOR checks.
- Terminal restoration is tested for normal exits, Ctrl-C cancellation, an inert resume helper, and guarded offline native round trips for the three versions in `docs/HARNESS-COMPATIBILITY.md`. Signals/panics, broader native interruption/concurrency cases and normal authenticated configurations need more coverage.
- Representative real-corpus retrieval, performance, and observed process RSS are now measured locally. This is not an exhaustive recall audit, cold-cache benchmark, syscall trace, installed-agent resume verification, or portability claim. Real resume stays experimental and disabled by default.

See `docs/VALIDATION.md` for evidence and `docs/LOCAL-RELEASE.md` for the deliberately smaller release bar. Do not equate a local candidate with a completed production/1.0 acceptance claim.
