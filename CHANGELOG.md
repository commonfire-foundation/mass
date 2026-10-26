# Changelog

Versioning policy: `VERSIONING.md`. Unpublished changes belong under `Unreleased`.

## Unreleased

- Update Ratatui to remove the unmaintained `paste` dependency and affected `lru` versions; keep Crossterm 0.28 via the explicit backend crate. Dependency CI treats all RustSec warnings as failures.
- Prepare the public development preview with source onboarding, a safe synthetic demo/terminal capture, contribution and private-reporting guidance, and public-release acceptance gates.

- Preserve repeated conversation turns with one-to-one, complete-message projection deduplication; adapter version 3 refreshes existing cached files.
- Treat quoted colon-containing searches as literals while retaining quoted filter values; preserve tabs and line endings in Markdown exports and clipboard text.
- Introduce transactional Rust cache schema-1 → schema-2 upgrades, normalized message storage, external-content FTS, indexed metadata filters, and deferred session hydration without changing public JSON versions.
- Apply filters before ranking/fuzzy matching, short-circuit impossible scopes, reuse message insert statements, and cache validated duplicate-file fingerprints with canonical-first refresh and safe invalidation.
- Keep multi-query reader operations in one snapshot; cancel discovery, search, and pending resume review cooperatively, skip blocked-plan transcript parsing, and restore the terminal before worker shutdown.
- Add migration rollback/search-equivalence, repeated-turn, whitespace, duplicate invalidation, reader-race, and slow-review cancellation regressions plus a repeatable targeted benchmark.

- Add explicit Markdown/JSON conversation and message exports, verified chunk reassembly, optional path metadata, and private atomic no-overwrite output.
- Add Ctrl-O session actions for export, clipboard copying, source location, and native-reopen review; optional wl-copy/xclip support has bounded helper waits and no automatic installation.
- Revalidate native reopening after confirmation, resolve executable paths, reject changed plans, and allow copying a shell-quoted reviewed command without launching.
- Diagnose sandbox device-access crashes and verify guarded offline native-session loading/return for Pi 0.85.1, Claude Code 2.1.272 and Codex 0.154.0. Codex uses an invalid offline key fixture; real authenticated configurations remain unverified.

- Keep archived conversations searchable when their project directory has disappeared.
- Generate excerpts after ranking/pagination instead of for every matching chunk; measured broad-query improvements without changing results.
- Make PageUp return to the previous result page and restore its selection; clear page history when the query or filters change.
- Remove the nonfunctional Ctrl-Y clipboard action.
- Add process-level interrupted-index recovery, corrupt-cache, and private-cache-permission checks.
- Validate importer recovery and representative retrieval against an approved local corpus using a separate private cache.
- Prepare Rust-first documentation, MIT licensing, dependency notices, and a lean local release checklist.
- Accumulate future edits here instead of bumping versions per change.

The numbered entries below are local development checkpoints, not published releases.

## 0.1.2

- Add one extra blank column on each side of the vertical results/preview divider.

## 0.1.1

- Bind keyboard help to Ctrl-K instead of Ctrl-G.
- Remove Ctrl-K's delete-to-end editing action so opening help preserves the query.
- Update keyboard hints, documentation, snapshots, and interaction coverage.

## 0.1.0 — development baseline

- Rust CLI/TUI for local Pi, Claude, and Codex conversation search, reading, and guarded resume.
- SQLite FTS5 indexing with source-qualified identities, incremental refresh, cancellation, and bounded transcript reading.
- Compact terminal-native UI, Ctrl-key actions, editable search/filters, conversation find, and scrollable details.
- Pi subagent exports, Claude child identities, Codex fork metadata, safe duplicate-history handling, and selective large-image payload parsing.
- Package-derived version in CLI output and a muted TUI title suffix.
- Synthetic regression tests, terminal snapshots, PTY scenarios, and local release archives with checksums.

The Bash/Python prototype is preserved. Real-corpus acceptance and other production-release gates remain open; see `docs/RUST.md`.
