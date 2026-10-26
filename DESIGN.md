# MASS — CLI and TUI plan

**Status:** Rust local pre-release candidate in `src/`; synthetic tests and a representative approved real-corpus audit are available. `docs/LOCAL-RELEASE.md` defines the lean first-release scope; this document also contains longer-term acceptance goals. This plan is not fully accepted or release-complete. See `docs/RUST.md` for implemented behavior and remaining gates. The Bash/fzf proof of concept remains preserved as `./mass` until replacement validation is complete.

## Product promise

> Find the conversation where you worked something out. Read the relevant passage. Return to that session if necessary.

MASS is a local conversation search tool. It is not an agent dashboard, a new harness, or an archive service.

The test for every proposed feature: **does it make finding, understanding, or reopening an existing conversation materially better?** Otherwise leave it out.

### First release

- Pi, Claude Code, and Codex.
- One search engine shared by a scriptable CLI and a keyboard-first TUI.
- Conversation-text search with readable, source-linked excerpts.
- Correct session identity, working directory, and agent-specific resume behavior.
- Explicit index freshness and coverage.
- Local, offline search with no agent execution during discovery or reading.

### Non-goals

No chat input, model picker, token/cost dashboard, agent orchestration, terminal multiplexer, repository management, transcript editing, tags/bookmarks, AI-generated summaries, embeddings, telemetry, cloud accounts, sync engine, plugin ecosystem, or QML frontend in this release.

Configured local archive roots are allowed; building or syncing those archives is not MASS's job. More agent adapters wait until the first three are trustworthy.

## What the prototype taught us

At the first real index: 5,285 transcript files, 127,331 indexed messages, a roughly 308 MiB database, and 139 partially indexed files. Example one-shot searches took approximately 130–250 ms. These are initial observations, not ongoing benchmarks or completeness guarantees.

Problems to resolve before polishing:

1. **First-run ambiguity:** demo data and an absent real index looked like poor search results.
2. **Split search modes:** fuzzy metadata and full-text search require different user workflows.
3. **Premature result limits:** the current 2,000-message cap can hide sessions before grouping.
4. **Partial history:** whole-message and per-session clipping lose searchable text.
5. **Preview mismatch:** opening a result can lose the passage that matched.
6. **Unstable references:** local SQLite row IDs are unsuitable public session identifiers.
7. **Freshness and identity:** changed files, moved stores, branches, and child sessions need explicit semantics.

## One shared data model

Keep four small entities:

- **Source:** stable configured store ID, agent, data root, availability, resume policy.
- **Session:** source-qualified native ID, title/provenance, original cwd, timestamps, archive/child/fork relationships, available actions.
- **Message:** native or deterministic entry reference, role, branch/parent reference, source locator, searchable text chunks.
- **Match:** session reference, message reference, highlight spans, excerpt, rank, coverage context.

Public references are opaque, stable across re-indexing of the same source, and independent of database row IDs. A source identifier prevents identical native IDs in different stores from colliding. Show short unambiguous references in terminal output; JSON carries complete references. Ambiguous prefixes fail rather than selecting arbitrarily.

Do not equate a model provider with an agent: a Pi conversation using OpenAI is still owned by Pi.

## CLI: a small command surface

| Command | Purpose |
| --- | --- |
| `mass` | Open the TUI when attached to a terminal |
| `mass tui [query]` | Explicit TUI entry, optionally seeded with a query |
| `mass search <query>` | Ranked session results, human-readable by default |
| `mass show <ref>` | Read a saved conversation |
| `mass export <ref> --output <file>` | Export indexed conversation/message text without overwriting |
| `mass resume <ref>` | Validate and reopen in the owning agent |
| `mass index` | Incrementally update the local index |
| `mass status` | Explain source coverage, freshness, and problems |

Do not add separate `list`, `find`, `browse`, `doctor`, `discover`, and `inspect` commands that overlap these jobs. An empty TUI query shows recent sessions; source discovery belongs in `status` and first-run setup. The prototype demo becomes a clearly labeled development fixture rather than a normal search mode.

### Search

```sh
mass search 'redirect callback'
mass search 'redirect callback' --agent pi
mass search 'palette' --project /home/me/Projects/themes
mass search 'migration' --since 2026-09-01 --limit 20
mass search 'migration' --json
```

Start with `--agent`, `--project`, `--since`, `--before`, `--limit`, `--cursor`, and `--json`. The TUI exposes the same filters. `--project` means an exact canonical workspace, not an implicit recursive scan or Git operation. Date filters apply to last conversation activity when known; explicitly report filesystem-time fallback.

Human output: short reference, title, agent/project/date, and one useful excerpt. Show a bounded page with a clear indication when more results exist. No ASCII table that forces long titles into narrow columns.

```text
8 matches · indexed 2 minutes ago

p:7k2m  Fix OAuth redirect
        Pi · website · yesterday
        “…the callback URI was encoded twice…”

c:9x4a  Investigate login failures
        Codex · api · Sep 12
        “…validate the redirect before setting the cookie…”
```

The references above are illustrative, not a finalized encoding.

### Reading and resuming

```sh
mass show p:7k2m
mass show p:7k2m --at <message-ref>
mass show p:7k2m --json
mass resume p:7k2m --dry-run
mass resume p:7k2m
```

`show` opens at the selected match when supplied. CLI output is plain text unless JSON is requested; it does not silently launch a pager, editor, agent, or clipboard helper. The TUI owns interactive reading.

`resume --dry-run` reports executable/argv, cwd, store overrides, warnings, and whether the operation is currently supported, without invoking the agent. Actual resume requires a terminal and confirmation; `--yes` may skip that confirmation, not missing-source or identity checks.

Never add permission-bypass flags. Restored agent settings may still authorize actions, so describe resume as launching an executable, not merely viewing history. Unknown live-session status is reported as unknown, not guessed from transcript mtime.

### Command contract

- Human output and machine data on stdout; diagnostics/progress on stderr.
- `--json` emits a versioned object with results, pagination, warnings, and coverage. No ANSI or progress text mixed into it.
- Zero matches are a successful search. Errors are not disguised as empty results.
- Exit codes: `0` success, `1` operation failure, `2` invalid usage, `130` cancellation.
- Search replies explicitly distinguish a complete page, more available results, and incomplete coverage. Indexing returns a failure status when configured readable sources fail to process; missing optional default stores are informational.
- Respect `NO_COLOR`, terminal width, and redirected output.
- Bare `mass` without a TTY prints concise usage and exits with a usage error; it never waits for input or launches a hidden TUI.
- CLI search/show do not silently start indexing. An absent index gets an actionable error; stale but usable results carry freshness information.

## TUI: search first, not dashboard first

```text
 MASS                                      indexed 2m ago
 Search conversations…                                    
 All agents · All projects                                 
 ────────────────────────────┬─────────────────────────────
 Fix OAuth redirect          │ Fix OAuth redirect          
 Pi · website · yesterday    │                             
 “…encoded twice…”           │ User                        
                             │ Why does the callback fail? 
 Investigate login failures  │                             
 Codex · api · Sep 12         │ Assistant                   
 “…validate the redirect…”   │ The URI was encoded twice…  
 ────────────────────────────┴─────────────────────────────
 Enter read   Ctrl-R resume   Ctrl-F filters   Ctrl-K help
```

### Layout rules

- One primary search field, focused at launch.
- One row group per session, not one row per matching message.
- An adjacent preview on wide terminals; a dedicated reader on narrow ones.
- Title first. Agent, project, and age are secondary. IDs and full paths belong in details, not every result row.
- Highlight matching text, not entire colorful rows. Use terminal colors and a minimal semantic palette; no agent logos or decorative charts.
- Keep keyboard selection stable as results refresh. Discard stale query responses.
- Preserve query, filters, selection, and reader position when navigating back.
- Partial-source/index warnings use one quiet status line with details on demand.

### Keys

| Key | Action |
| --- | --- |
| Type | Search |
| Up / Down | Move through results |
| Tab / Shift-Tab | Move focus between search, results, and preview |
| Enter | Open the reader at the selected matching passage |
| Esc | Back from reader/dialog; exit from the search screen |
| Ctrl-F | Small filter sheet: agent, project, date |
| Ctrl-R | Review and confirm resume |
| Ctrl-L | Request an incremental index refresh; clear filters inside the filter sheet |
| Ctrl-D | Session details |
| Ctrl-K | Keyboard help |
| Ctrl-O | Session actions |

Reader: scrolling, `/` to find within the conversation, and next/previous match. Plain letters never become navigation shortcuts while the search field has focus. No mandatory mouse use, no global Vim mode, no command palette for six actions.

Ctrl-O provides deliberate export, clipboard, source-location, and reopen actions. Clipboard support is optional through existing local helpers; do not advertise a nonfunctional shortcut, install helpers automatically, or silently use remote clipboard escape sequences. Exports are indexed text documents, not restorable native-session backups.

### First run and empty states

On first launch, discover file counts without starting agents:

```text
Search your local agent conversations

Pi           3,002 transcript files
Claude Code     98 transcript files
Codex        2,185 transcript files

MASS will create a private, plaintext search index.
It will not launch agents or change their transcripts.

[Build index]   [Cancel]
```

These are illustrative counts from the initial scan, never hard-coded UI values.

After consent, show progress and allow search over committed results while indexing continues. Clearly label indexing/partial availability. No synthetic results in this path.

Distinguish: no sources found, no index yet, indexing in progress, no query matches, unavailable source, and partial/unsupported history. Each gets one appropriate recovery action. A broad filter-reset action must not silently change the user's query.

Once initial indexing is approved, the TUI may perform a lightweight freshness check and run a bounded incremental refresh in a worker while presenting cached results. Never run multiple writers; another indexing process yields an “indexing elsewhere” state. No permanent daemon or startup service in v1.

## Search quality without search-mode switches

One query combines:

1. Exact phrases and token/prefix matches in message text.
2. Exact and prefix matches in titles and project names.
3. Fuzzy title/project candidates, with weaker matches ranked below direct hits.
4. A small recency tiebreaker, never enough to bury a clearly relevant old conversation.

Keep syntax small: ordinary words, quoted phrases, `agent:`, and `project:`. Dates live in CLI flags or the filter sheet. Unknown filter keys receive a readable error, not silent interpretation. No regex/SQL/Boolean query language in v1.

All query terms must be satisfied within a passage and its session metadata. Terms scattered across unrelated turns are not silently considered a phrase or a passage match. Code-aware tokenization should make snake_case, kebab-case, and common path fragments useful without indexing JSON field names.

Rank messages, group by source-qualified session, and return the best excerpt plus a match count when cheaply available. **Do not apply an arbitrary message cap before finding a page of distinct sessions.** Bounded execution is still required; budget exhaustion must be visible and continuation must not lose candidates.

Default results include available archived sessions. Child/fork relationships are labeled; inherited duplicate text should not overwhelm parent results. A lightweight expand/drill-in is enough—no graphical session-tree editor.

## Indexing and privacy

- The index is disposable derived data, not the archival authority.
- Version each adapter and index schema. Parser changes invalidate affected sources, not unrelated adapters.
- Replace silent text clipping with bounded streaming extraction and message chunking. Large nontext payloads must not force whole records to vanish. Unsupported records get explicit coverage warnings.
- Preserve native entry IDs and source locations so the reader can reach the original match.
- Tail append-only files only when identity and prefix validation prove it is safe; otherwise reparse. Handle truncation, replacement, incomplete trailing records, and compressed/plain transitions.
- Track last successful scan separately from last attempted scan, source availability, and parser errors.
- Purge deleted/excluded content from searchable results after confirmed reconciliation. Never infer deletion from an unavailable mount. Removing an explicitly configured source must have a clear cache-removal policy.
- Keep a single writer, transactional updates, and concurrent readers. Cancelling an index update leaves a valid usable index.
- Do not promise secure deletion from SQLite/WAL/backups. Explain that the cache contains sensitive plaintext.
- Do not read credentials, execute transcript content, render terminal escape sequences, load agent extensions, or start agent runtimes during search.

Default storage for the installed app follows XDG:

```text
$XDG_CONFIG_HOME/mass/config.toml
$XDG_CACHE_HOME/mass/index.sqlite3
```

Use normal `~/.config` / `~/.cache` fallbacks. Source configuration stays small: enabled agents, explicit extra stores, exclusions, and resume permission. No arbitrary shell-command templates in configuration.

**Migration is opt-in:** the prototype's project-local database and `MASS_SOURCES` JSON are not silently moved or reinterpreted. Offer to rebuild in the installed location and clearly explain where data will be stored. No live configuration changes are part of this planning task.

## Implementation direction

**Chosen stack: Rust + SQLite FTS5 + Ratatui/Crossterm, with Clap for the CLI.**

| Component | Choice |
| --- | --- |
| CLI arguments and help | Clap |
| Terminal rendering and input | Ratatui + Crossterm |
| Index and full-text queries | rusqlite with bundled SQLite and FTS5 verified in CI |
| Transcript and JSON protocol parsing | serde + serde_json |
| Compressed Codex transcripts | zstd |

- Build one Rust application with a shared library core behind both CLI and TUI. Search semantics, adapters, reading, and resume validation must not diverge between interfaces.
- Port the Python adapters into typed, tested Rust modules. Retain the Python prototype and synthetic fixtures as a behavior reference during development, not a runtime dependency or an unquestioned correctness oracle.
- Use buffered streaming extraction and bounded allocations; a Rust parser that loads entire transcripts is not an acceptable replacement for proper resource limits.
- Keep indexing and database queries off the UI thread. Start with bounded worker threads, separate SQLite connections, a single writer, cancellation, and query-generation IDs. No async runtime or daemon unless a measured requirement justifies one.
- Use Ratatui/Crossterm for persistent selection, a proper reader, and responsive refresh states. Restore terminal state on exit, error, cancellation, and before handing control to a resumed agent.
- Retire the Bash/fzf frontend after the replacement reaches parity; do not maintain two production TUIs.
- Keep a thin command layer and versioned JSON output so a future QML surface can reuse the CLI without requiring a socket API or MCP server today.
- Ship a single `mass` executable with no Python/fzf runtime requirement. Provide a Cargo build/install path and versioned release binaries with checksums; document platform library requirements rather than promising universal static linking.
- Do not couple installation to Omarchy or modify system keybindings automatically.

Rust is chosen for distribution, predictable resource use, and low startup overhead—not an assumed multiplier on SQLite query speed. Benchmark the Python baseline and Rust implementation on identical fixtures and an approved corpus: cold startup, warm query p50/p95, initial and incremental indexing throughput, peak memory, and index size. Check retrieval correctness alongside speed. Index layout and query design remain first-order performance concerns in either language.

Suggested structure (one Cargo package, not a multi-crate framework):

```text
Cargo.toml
Cargo.lock
src/
  main.rs
  lib.rs
  cli.rs
  tui/            # state, rendering, input
  adapters/       # pi, claude, codex
  index.rs
  search.rs
  reader.rs
  resume.rs
  config.rs
  models.rs
tests/
  fixtures/
  adapters.rs
  index.rs
  search.rs
  cli.rs
  tui.rs
```

Use `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` as baseline gates, plus PTY interaction tests and repeatable benchmarks. No generic plugin framework. Keep agent-specific knowledge in adapters and reuse normalized types everywhere else.

## Delivery order and acceptance gates

### 1. Trustworthy history and identity

Establish the Cargo package and shared Rust core. Port the three adapters against synthetic fixtures, then implement stable source/session/message references, chunked extraction, missing/deleted-source semantics, and accurate coverage reporting. Preserve the working prototype until the replacement is validated.

Gate: a phrase beyond today's text caps is findable; unchanged scans do not reparse; deletion reconciles correctly; branch/child identity survives a rebuild; sources are never modified. Capture adapter/version support explicitly rather than promising every historical format.

### 2. Production CLI and one search engine

Implement the small command set, shared filters/ranking, stable JSON, distinct-session pagination, passage-linked reading, and resume dry-run/guards.

Gate: CLI tests cover quoting, Unicode, path spaces, redirected output, errors versus zero matches, pagination, and stable references. A curated set of synthetic questions finds its known answers. Resume planning invokes no agents.

### 3. Dedicated TUI

Build the Ratatui/Crossterm interface: first-run consent, search/results/reader screens, unified matching, filter sheet, freshness/progress states, and guarded resume. Replace fzf only after these work.

Gate: selecting a result opens the matching passage; Back retains state; old responses never overwrite a new query; narrow terminals remain usable; keyboard-only flows work; cancelling indexing or resume is safe.

### 4. Package and validate on the real corpus

Release the Rust `mass` executable with a Cargo install path, versioned binaries/checksums, XDG paths, opt-in prototype migration, concise help/docs, and a repeatable performance/recall benchmark against the Python baseline. Verify it runs without Python or fzf. No new features in this step.

Targets to validate, not claims:

- Warm query p95 at or below 150 ms on the current corpus, excluding typing debounce; results visible within 250 ms.
- Cached recent sessions visible within 300 ms of TUI startup on the test machine.
- Incremental updates do not freeze input or previews.
- No source-transcript writes and no network access during indexing/search.
- Full-text coverage losses and query-budget limits are visible, never disguised as exhaustive zero matches.
- Measure cold start, indexing rate, peak memory, and index size before fixing resource budgets.

### Release stop line

Ship when a person can find, read, and reopen old conversations reliably, and scripts can use the same results. Do not add dashboards, semantic search, remote transport, or more agents to make the release feel larger.
