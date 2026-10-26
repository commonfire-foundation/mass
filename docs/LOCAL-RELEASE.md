# Lean first public release checklist

Scope: **find an existing conversation and read the matching passage**, locally, on x86_64 Linux/glibc. Native resume is experimental, disabled by default, and not advertised as release-verified support.

Source is available as a public development preview. This is not a release announcement or a promise of compatibility. The checklist separates opening development from publishing a supported binary; not every future goal in DESIGN.md blocks the first pre-1.0 release.

## Public-preview bar

- Clear development status, supported platform, source-build instructions, and an isolated synthetic demo.
- MIT licensing, privacy warnings, contribution guidance, and private vulnerability reporting.
- Secret-reviewed source tree without real conversations, caches, or private audit evidence.
- Passing hosted checks for source, synthetic terminal/recovery behavior, demo, packaging, and dependencies.
- A small public issue checklist for human acceptance, Linux compatibility, and exact release-artifact verification.

The source preview does not require a release tag, binary upload, crates.io package, or version bump.

## Locally verified

- [x] Approved corpus rebuilt in a separate owner-private project-local cache, without rebuilding the live cache or changing transcripts.
- [x] Previously rejected nonempty files recovered; representative indexed text checked against original JSON. Known empty placeholder ignored without a false identity diagnostic.
- [x] Representative phrase, prefix/project/agent, and activity-date retrieval checks across all three agents; opening returned message references reaches matching text.
- [x] Deleted project directories remain filterable, with a synthetic regression test.
- [x] Previous-page navigation and query-reset behavior tested through a real pseudo-terminal.
- [x] Search optimization measured against the old binary on the same cache; returned results/excerpts compared for equality.
- [x] Interrupted indexing retains old history, leaves a valid database, and can be retried. Corrupt databases are rejected without changing their bytes.
- [x] Resume remains disabled by default. Inert-helper launch/revalidation tests and guarded offline native round trips pass for the three tested versions, documented in `docs/HARNESS-COMPATIBILITY.md`. No real session or credentials were used; Codex used an invalid offline key fixture.
- [x] Nonfunctional Ctrl-Y shortcut removed. Subsequently approved session actions add working optional clipboard support under Ctrl-O, rather than restoring a dead shortcut.
- [x] Rust-first usage/configuration/privacy documentation, MIT license, and upstream dependency notices prepared.
- [x] Packaged executable indexes/searches/reads with a fresh user environment and no helpers on PATH; archive checksum verifies on the build host.

Final package verification and exact test results are recorded in `docs/VALIDATION.md`.

## Deliberately deferred

Automatic/background freshness refresh, unlimited individual text records, every historical agent format, full Markdown rendering, bundled/automatically installed clipboard helpers, additional adapters, non-Linux platform support, and a broad human terminal-theme/accessibility matrix.

Schema 2's normalized storage and external-content FTS are implemented; further storage optimization is not a release gate. Use Ctrl-L or `mass index` for freshness. Use `mass status` for technical coverage details. Missing/ambiguous histories must fail safely rather than guessing identity.

## Before an actual public release

- Review the local candidate in the maintainer's terminal; automated snapshots do not replace this judgment.
- Explicitly choose a release version and move Unreleased notes into that version. Do not increment versions for individual edits.
- Rerun regression/package checks for the exact artifact being released.
- Explicitly authorize publication and choose a destination. Nothing in the local preparation process uploads, pushes, tags, installs globally, or publishes an artifact.

Offline disposable-session loading and return are now checked against specific installed versions. A broader supported native-resume claim still requires authorized testing of normal authenticated configurations and more interruption/concurrency cases. Resume stays opt-in/experimental.
