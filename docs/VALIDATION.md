# Rust development validation

Latest correctness/optimization follow-up validated locally with Rust 1.99.0 on x86_64 Linux/glibc; the earlier baseline used Rust 1.97.1. This is a public development preview, not an official release, a portability guarantee, or acceptance of every long-term DESIGN.md goal. See `docs/LOCAL-RELEASE.md` for the bounded release scope.

## Passing checks

- `cargo fmt --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked`: 48 integration tests (41 core, 4 CLI, 3 schema), plus 21 unit tests (19 TUI and 2 clipboard): 69 total
- `NO_COLOR=1 cargo test --locked tui::`: 19 TUI tests
- 32 golden snapshots at 55×20, 80×24, 120×35, 160×45, each checked in color and monochrome (64 render cases)
- `python3 tests/rust_tui_pty.py`: 20 PTY scenarios plus one terminal-protocol unit test against debug and release binaries, including resize, editing/paste, filters, conversation find/history, scrollable details, first-run consent, cancellation, synthetic resume/return, tab-preserving clipboard text, and termios restoration during a long resume review
- `python3 tests/rust_cli_recovery.py`: 2 process-level checks for SIGINT/retry, retained history, database integrity, transcript immutability, new-cache permissions, and non-mutating corrupt-cache rejection
- `python3 -m unittest discover -s tests -p 'test_*.py'`: 25 preserved prototype tests plus 5 synthetic-demo/capture and 3 package-license tests
- Release build and a temporary local host archive smoke test, including MIT and upstream dependency license notices
- `python3 tests/rust_package_smoke.py`: extracted archive indexes, searches, reads, exports, and reports disabled resume with an empty user home/configuration and `PATH=/nonexistent`
- The package smoke test uses host libraries; it is not a fresh-distribution portability test

Synthetic tests cover long UTF-8 messages, invalid/oversized records, all three adapters, zstd, Codex projection fallback/deduplication, stable source/message identity, renamed files, missing versus deleted stores, foreign/prototype database rejection, unchanged scans, cancellation retention, writer exclusion, more than 2,000 matching messages before session grouping, fuzzy metadata ranking, pagination, reader windows, CLI JSON/quoting/Unicode/path spaces, dry-run guards, and stale TUI generation rejection.

The original automated suite uses only synthetic data. The later explicitly approved real-corpus audit is described below. Resume lifecycle tests use an inert local shell helper. Later native probes used only disposable project-local state, disabled networking, and no real credentials; Codex used an invalid offline key fixture. No real conversation or authenticated session was resumed. Their corrected sandbox setup and successful guarded round trips are recorded in `docs/HARNESS-COMPATIBILITY.md`. No live config, prototype source, or existing prototype cache was migrated. All new build/dependency caches and fixtures were kept inside this project.

## Interaction regression follow-up

Fixed invisible focus, Tab cycling into a nonexistent narrow-screen preview, Enter opening an empty reader that swallowed typing, and disabled resume exiting the TUI. Search typing now recovers focus from any search-screen pane. Startup/indexing/error states are explicit; committed results refresh during a scan. Index progress reports file counts.

The PTY suite reconstructs the current VT screen rather than accepting strings from old output, and waits for fresh paint between keystroke sequences. It verifies Tab focus, typing after Tab, no-result Enter, Unicode find, cursor editing/paste, reader navigation across bounded buffers and return position, filter apply/clear, Ctrl-K help without erasing the query, Ctrl-D metadata scrolling, Ctrl-L refresh versus in-sheet filter clearing, resize, resume review cancellation, local helper launch/return, first-run consent, Ctrl-C, and terminal restoration. A pagination regression now checks three pages, returning one page at a time, and clearing page history after query changes.

The completed four-pass redesign is recorded in `docs/history/TUI-REDESIGN.md`. Golden snapshots cover results/reader/empty/error/indexing/loading/startup/details; unit tests additionally cover graphemes, filters, passage targeting, overlap handling, and monochrome style semantics. Actual light/dark terminal contrast still needs human review.

## Session actions follow-up

- Clean Markdown/JSON exports reassemble verified chunk overlap, preserve Unicode/code text, and select complete messages from chunk references. Tests cover metadata-path omission/opt-in, owner-private output, existing-file/symlink refusal, and no partial destination on errors.
- Ctrl-O export/copy/source-location flows pass real PTY checks at wide and narrow sizes. Clipboard tests use an inert local helper, not the user's clipboard; missing helpers, helper errors, and bounded timeouts are covered separately.
- Reviewed-command copying does not launch an agent. POSIX quoting is tested with hostile-looking literal arguments and an inert printf executable.
- Changing native transcript identity or revoking source permission after review prevents launch in PTY tests. Normal inert-helper launch/return still works.
- A native follow-up isolated the Pi/Claude startup crashes to sandbox `nodev` device access. With a private `/dev`, all three installed harnesses passed guarded MASS → native → MASS fixture checks, including saved marker, exact arguments/cwd, native exit 0 and terminal restoration. Pi 0.85.1 and Claude Code 2.1.272 needed no credentials; Codex 0.154.0 used an invalid offline key fixture. See `docs/HARNESS-COMPATIBILITY.md`; this does not verify normal authenticated configurations.

## Importer compatibility follow-up

Read-only structural inspection identified Pi subagent-export records, Claude children sharing parent session IDs, Codex forks with embedded ancestor headers, empty placeholders, and records above the old raw-size limit. No conversation text was printed or copied into fixtures, and the live index was not rebuilt.

Adapter v2 regressions cover run/child-scoped export identities, prompt-projection deduplication, sibling/parent isolation, linked Codex ancestor headers, rejection of unrelated mixed identities, identical/prefix-copy deduplication, divergent-copy read/guard behavior, append stability after sibling removal, empty-placeholder versus truncated-file handling, large Pi images (including zstd), large Codex image URLs, retained-text limits, and preservation of diagnostics when a copy has omissions.

Adapter-version fingerprints reparse old cache entries on refresh. The approved isolated audit below now measures the formerly reported 182 diagnostics; it does not claim universal format compatibility. Unknown formats and genuine corruption still report technical diagnostics.

## Repeatable synthetic benchmark

Command: `python3 scripts/benchmark.py --sessions 200 --runs 20`

Historical baseline recorded before the visual redesign (see the correctness/optimization follow-up below for newer measurements): 200 synthetic sessions, 40 messages each, identical transcripts for both implementations. Every query is a new process; times include startup. Basic retrieval assertions pass. This is **not** an OS-cold-cache, peak-memory, or representative real-history measurement.

| Measurement | Rust release | Python prototype |
| --- | ---: | ---: |
| Initial index | 3,188.65 ms | 3,210.97 ms |
| Unchanged scan | 72.95 ms | 112.55 ms |
| First query | 122.39 ms | 116.00 ms |
| Query p50 | 118.66 ms | 126.78 ms |
| Query p95 | 140.03 ms | 159.95 ms |
| Database size | 39,481,344 bytes | 16,896,000 bytes |

Raw output: `target/benchmark-synthetic.json` (generated/ignored). This historical index-size regression motivated schema 2: the first Rust layout stored JSON chunks and FTS text rather than external-content FTS. The follow-up below measures the new layout. Rust alone is not a search-speed guarantee.

Local artifact: `dist/mass-0.1.2-x86_64-unknown-linux-gnu.tar.gz`, with adjacent `.sha256`. Library requirements are included in the archive. Nothing was published or installed globally.

## Approved local corpus audit

A separate owner-private cache under ignored `target/` indexed 5,258 sessions and 130,681 text chunks across Pi, Claude, and Codex. The live cache was not rebuilt. No conversation text, private paths, or raw corpus were copied into public fixtures or documentation.

- The previous live cache reported 182 diagnostics. The isolated rebuild reported zero, with one empty placeholder counted separately.
- All 170 previously rejected nonempty files were indexed. Independent source-text witnesses matched original JSON in those files plus all 11 previously partial files (181 witnesses). This is a sample per file, not a complete text-by-text equivalence proof; the latter 11 files did not add message chunks.
- 36 known-hit checks covered quoted phrases, partial terms with project/agent filters, and date bounds; all passed after fixing deleted-project filtering. All 12 corresponding matched-passage reads reached matching text. This is representative retrieval evidence, not exhaustive recall measurement.
- Initial index: 213.90 seconds; approximately 600.10 MiB on disk; observed index-child peak RSS 15.91 MiB. Incremental passes took approximately 0.22–0.33 seconds with 5,257 unchanged files and one active transcript reparsed.
- Private evidence and one-off audit scripts stay under `target/`. They are not part of the release package.

### Measured search improvement

The original and updated binaries ran against the same private index, alternating order. Each query had one warmup and three measured runs per binary. Times include process startup; these are warm-filesystem-cache host measurements, not OS-cold-cache claims. All 40 old/new comparisons returned identical results and excerpts.

| Broad query | Before median | After median |
| --- | ---: | ---: |
| theme | 708.96 ms | 374.62 ms |
| configuration | 237.68 ms | 62.25 ms |
| session | 506.00 ms | 104.20 ms |
| error | 388.65 ms | 83.24 ms |
| callback | 62.53 ms | 33.25 ms |
| README | 275.37 ms | 72.20 ms |
| Hyprland | 268.21 ms | 74.14 ms |
| index | 226.78 ms | 55.04 ms |
| Ctrl | 61.26 ms | 28.84 ms |
| Rust | 166.82 ms | 68.76 ms |

The change defers excerpt generation until after ranking and paging. It does not change the index format or ranking. Observed child peak RSS was at most 22.75 MiB in these runs; process-startup/launcher overhead is included, so this is not an allocator-level memory profile or worst-case bound.

## Correctness and optimization follow-up

The automated checks listed under Passing checks were rerun against the corrected implementation. Historical real-corpus and native-probe findings were not rerun. This follow-up did not access or upgrade live stores or caches. A fresh, temporary archive containing the current release binary, documentation, and generated dependency notices passed the package smoke test without replacing existing `dist/` artifacts. CI now also specifies release-binary PTY/recovery checks; remote execution remains unverified.

New regressions cover:

- Quoted colon literals, quoted project-filter values, and tab/line-ending fidelity in exports, CLI output sanitization, reader display, and inert-helper clipboard copying.
- One-to-one projection pairing across complete chunk sequences, repeated prompts, reverse-order pairs, and identical prefixes with different tails.
- Explicit schema-1 upgrades, non-mutating read-only rejection, transactional rollback on malformed legacy data, preserved references/history, and FTS integrity after refresh/deletion. Independent internal-content FTS queries match new ranking, excerpts, and filtering on synthetic histories.
- Impossible scopes return without preparing an FTS read (enforced by a denying SQLite authorizer), without bypassing cursor validation.
- Cached duplicate detection and invalidation after canonical deletion, exclusion, rewrite, copy append, and configuration removal, including canonical paths sorting after their copies.
- A deterministic separate-connection commit between reader queries, proving a window retains one snapshot; existing caller transactions remain composable.
- Cancellable discovery/search/parsing, blocked-plan parsing avoidance, and Escape/Ctrl-C during a 64 MiB synthetic resume review. Tests require prompt worker recovery, no launch, and restored termios. Cancellation remains cooperative, not a guarantee against blocked kernel I/O.

### Fresh-index benchmark

Command: `python3 scripts/benchmark.py --sessions 200 --runs 20`

| Measurement | Review baseline | After fixes |
| --- | ---: | ---: |
| Initial index | ~3,950 ms | 3,510.75 ms |
| Unchanged scan | 65.66 ms | 53.21 ms |
| Query p50 | 51.3 ms | 42.02 ms |
| Query p95 | 58.7 ms | 46.43 ms |
| Database size | ~39.5 MB | 24,629,248 bytes |

The fresh cache is about 38% smaller and query median about 18% lower on this recipe. These are separate host runs, not interleaved trials or representative real-corpus guarantees. Physical sizes after in-place upgrades may retain SQLite free pages. The prototype remains smaller at 16,896,000 bytes. Raw follow-up: `target/benchmark-review-after.json` (generated/ignored).

### Targeted repeatable benchmark

Command: `python3 scripts/benchmark-regressions.py --runs 5`

A 2,000-message, approximately 6.2 MB transcript refreshed in 48.67 ms without a copy and 48.78 ms with an unchanged duplicate. Coverage confirmed one `duplicates_unchanged` skip and zero reparsed files. The review's earlier one-off duplicate scan took about 260 ms; the cache now avoids that parse/insert/delete work entirely.

On a separate 400-session/16,000-message fixture, median startup-inclusive searches were 2.52 ms for a common term scoped to an absent agent, 30.91 ms for one of twenty projects, and 141.07 ms unfiltered. These targeted timings vary with fixture layout and host conditions; they do not establish an across-the-board broad-query improvement. Impossible scopes no longer traverse FTS postings, while nonempty scopes still need matching/grouping work. Raw follow-up: `target/benchmark-regressions-after.json` (generated/ignored).

## Public-preview preparation

The public source tree excludes local caches, raw histories, private audit evidence,
and binary archives. `scripts/demo.py` uses three invented agent histories with an
isolated HOME/config/cache and no agent/clipboard helpers on PATH. Its CLI check
and interactive cancellation/cleanup passed. The README capture comes from this
real synthetic TUI, not a live desktop or a mock screen.

The dependency audit found `RUSTSEC-2026-0002` and `RUSTSEC-2026-0253` in the old
transitive `lru` 0.12.5, and the unmaintained `paste` advisory
`RUSTSEC-2024-0436`. Ratatui 0.30.2 with `lru` 0.18.5 removes those findings;
`cargo-audit` 0.22.2 with `--deny warnings` passed without exceptions. The explicit
`ratatui-crossterm` backend retains Crossterm 0.28. All Rust tests, unchanged golden
snapshots, strict Clippy, and both debug/release PTY/recovery suites passed.
The PTY emulator now implements cursor-position reports and alternate-screen
round trips required by Ratatui's clear-after-resume operation, with a protocol
regression test; no native-launch checks were removed or weakened.

A clean source export with an initially empty Cargo cache passed the release
build, demo check, 69 Rust tests, strict Clippy/formatting, 33 Python tests, archive
checksum verification, and the empty-home package smoke test. Existing local
`dist/` artifacts were not replaced. Packaging now uses a fresh staging directory
and collects notices for the actual normal/build dependency graph: Cargo metadata
can include inactive optional backends. Regressions require missing active
notices and malformed dependency selections to fail rather than be silently
omitted. Gitleaks 8.30.1 found no secrets in the publishable source export; this is
not a claim that automated scanning can prove the absence of all secrets.

Hosted CI is configured for Ubuntu 22.04 and 24.04, fresh-home package behavior,
Gitleaks, and warnings-as-errors dependency auditing. Actions are SHA-pinned and
workflow tokens are explicitly read-only. A configured check is not proof of a
successful hosted run; GitHub's checks on the exact commit are authoritative.
Human terminal acceptance, distribution support promises, and exact release
artifact approval remain separate gates in `docs/LOCAL-RELEASE.md`.

## Not verified / not claimed

Exhaustive real-corpus recall, cold-cache latency, worst-case memory bounds, network/filesystem syscall tracing, normal authenticated-agent configurations and broader native resume compatibility, other operating systems/distributions, broad human theme/accessibility review, and remote CI execution. Resume remains explicitly opt-in and disabled by default. The preserved prototype has not been removed. These limits are documented rather than expanded into first-release feature work.
