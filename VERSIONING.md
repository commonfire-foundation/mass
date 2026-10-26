# Versioning

M.A.S.S. uses Semantic Versioning: `MAJOR.MINOR.PATCH`.

`[package].version` in `Cargo.toml` is the single source of truth. Cargo supplies it at build time to the Rust API, `mass --version`, and the TUI's muted `v…` suffix. The packaging script uses the same value in archive names. There is no separate VERSION file to synchronize.

Accumulate individual fixes and UI tweaks under **Unreleased**. Do not bump the version after each edit, test run, or local package build. Change it only when the maintainer explicitly decides to cut a release. Local candidate artifacts can therefore share a version; their SHA-256 checksums identify the actual binaries.

When cutting a release while the project is pre-1.0:

- Increment PATCH for compatible fixes and small presentation changes.
- Increment MINOR for substantial features or breaking CLI/API behavior. Document breaking changes explicitly.
- Reserve 1.0.0 for a stable product/compatibility commitment. A first public pre-1.0 release needs the bounded checklist in `docs/LOCAL-RELEASE.md`, not every long-term aspiration in `DESIGN.md`.

## Preparing a version

1. Record changes under `Unreleased` in `CHANGELOG.md`.
2. Update the package version in `Cargo.toml`. Run `CARGO_HOME="$PWD/.cargo-home" cargo check --offline` to update the root package entry in `Cargo.lock`; review any other lockfile changes.
3. Move the release notes into a matching version heading. Add a release date only when actually releasing.
4. Update version-bearing TUI snapshots with `MASS_UPDATE_SNAPSHOTS=1 CARGO_HOME="$PWD/.cargo-home" cargo test --locked tui::snapshots`, inspect them, then run normal tests, formatting, Clippy, and PTY checks.
5. Run `CARGO_HOME="$PWD/.cargo-home" bash scripts/package.sh`. Verify the new archive checksum and run the release-binary PTY suite using `MASS_TEST_BINARY="$PWD/target/release/mass"`.

The initial tracked Rust version is 0.1.0, a development baseline—not a production-release claim. The preserved Bash/Python prototype remains separate.

Application versions are distinct from JSON response versions, adapter versions, and SQLite schema versions. Bump those only when their respective contracts change; an application patch must not arbitrarily invalidate the index or alter JSON envelopes.
