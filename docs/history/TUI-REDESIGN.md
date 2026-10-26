# TUI usability and visual redesign — historical record

This records an earlier implementation checkpoint, not current test counts or release status. See `docs/VALIDATION.md` for subsequent verification and `docs/LOCAL-RELEASE.md` for remaining gates.

Status: implemented and locally validated. This records the four-pass plan approved in conversation and its execution evidence. Broader Rust production-release gates remain open; see docs/RUST.md.

## Direction

A quiet conversation finder, not a dashboard: clear typography, restrained terminal-native color, useful excerpts, obvious actions, and keyboard-first operation. Preserve search semantics, offline operation, source privacy, and resume guards. Keep the prototype intact.

## Pass 1 — Visual foundation

- Replace nested boxes with spacing and one pane divider.
- Bold conversation titles, quieter metadata, readable body text.
- One terminal-native accent for focus/matches; selection remains clear with NO_COLOR.
- Visible input cursor and visual focus instead of `[focused]` labels.
- Results show title, agent, short/disambiguated project, relative date, and useful excerpt.
- Highlight matches; move full paths and references into details.
- Show page position/more-results state; preserve selection and scroll during refresh.
- Responsive contextual footer; wide preview and narrow reader.

Acceptance: focus and selection are obvious; results are understandable without opening each one.

## Pass 2 — Interaction

- Labelled filter fields: agent selector, searchable project selector, since/before dates.
- Active filters beneath search; clear filters without clearing the query.
- Normal text editing: cursor movement, Home/End, Delete, paste, horizontal scrolling.
- Resume review dialog; cancel restores the screen without launching anything.
- Separate query loading, indexing progress, warnings, and transient feedback; keep cached results during refresh.
- First-run source counts and a clear build-index action; actionable empty/error states.

Acceptance: ordinary use needs no hidden syntax or invisible focus modes.

## Pass 3 — Reading

- Conversation header, distinct speaker labels, basic Markdown structure, intact code indentation.
- Open at the matching passage within its chunk, with visible highlighting.
- Scroll position and next/previous-match indicators.
- Continuous, bounded reader navigation rather than exposed storage windows.
- Preserve reader position and query when returning to search.

Acceptance: opening a result immediately explains why it matched; navigation does not lose context.

## Pass 4 — Validation

- Rendering snapshots at 55×20, 80×24, 120×35, 160×45.
- Long titles, Unicode, code blocks, empty results, errors, indexing, and NO_COLOR coverage.
- PTY tests for actual keyboard interaction, resizing, cancellation, and terminal restoration.
- Run fmt, Clippy, Rust tests, prototype regressions, release build, and release-binary PTY tests.
- Rebuild the local archive/checksum; do not install globally or index a real corpus.

## Execution record

### Delivered

1. **Visual foundation:** sparse layout, one divider, shortened/disambiguated metadata, relative dates, Unicode cell clipping, title-only selection emphasis, display-only match highlighting, visible focus/cursor, responsive styled key hints, persistent list selection/scroll, and scrollable full details. Terminal-native accent; no fixed background and no color dependency.
2. **Interaction:** grapheme-aware cursor editing and horizontal viewport, Home/End/Delete/Ctrl-A/E/U/K, bracketed paste, labeled filter sheet with project suggestions/date validation/clear, persistent coverage-warning count separate from query/index/transient state, cached results during refresh, and in-TUI resume review/cancel/return. Long review/help/details sheets scroll rather than hiding information on small screens.
3. **Reading:** speaker headings, Markdown headings/fences, preserved code indentation, width-aware rows, passage targeting, verified adjacent-chunk overlap suppression, automatic previous/next history, bounded 32-chunk render buffers, worker-backed conversation find with wrap, resize anchors, and reading-position preservation on return.
4. **Validation:** completed the checks below and rebuilt the local release executable/archive/checksum. No global install or prototype migration.

### Evidence

- `cargo fmt --check` and locked all-target Clippy with `-D warnings`: pass.
- `cargo test --locked`: **16 unit tests + 19 core + 2 CLI** pass.
- `NO_COLOR=1 cargo test --locked tui::`: all **16** TUI tests pass.
- **32 golden terminal snapshots**, rendered in color and monochrome (**64 cases**): 55×20, 80×24, 120×35, 160×45; results, reader, empty, error, indexing, query loading, startup, and details. Fixtures include Unicode, long titles, project-name collisions, and code.
- **11 PTY scenarios pass against both debug and release binaries:** narrow/wide and all target sizes, editing/paste, scrollable metadata, filter apply/clear, no-result Enter, reader/find across buffers/back-position retention, resize, NO_COLOR, first-run consent, Ctrl-C cancellation, disabled resume, review cancellation, and launch/return using a local inert helper. Terminal attributes restored.
- **25 preserved prototype tests** pass.
- Artifact: `dist/mass-0.1.0-x86_64-unknown-linux-gnu.tar.gz` and adjacent `.sha256`.

Snapshots live in `tests/snapshots/tui/`; the matrix is `src/tui/snapshots.rs`. Intentional updates use `MASS_UPDATE_SNAPSHOTS=1 cargo test terminal_size_and_state_matrix`, followed by a normal test run and snapshot inspection. Use `CARGO_HOME="$PWD/.cargo-home"` for repository-local dependency caching.

### Boundaries and remaining review

All transcripts were synthetic. No real agent was launched, no live configuration changed, and the Bash/Python prototype remains intact. Human contrast/readability review in actual light/dark terminal themes is still needed; terminal-native colors and monochrome behavior are covered structurally, not by a claim of subjective visual approval. Reader formatting is intentionally basic, not complete Markdown. Real-corpus and installed-agent release gates remain documented separately.
