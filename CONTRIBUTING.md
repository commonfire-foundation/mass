# Contributing

MASS is a public development preview, not a stable release. Small fixes, clear
reproductions, and synthetic adapter fixtures are welcome. For a substantial
feature or interface change, open an issue before investing in implementation.
The scope is local conversation search and reading, not a new agent harness.

## Protect conversation privacy

- Never commit real transcripts, databases, credentials, private project paths,
  or captured terminal output from real histories. Use invented fixtures.
- A Markdown export is **not** a redaction tool. Text and titles may contain
  secrets even when metadata paths are omitted.
- Inspect diagnostics before sharing them: `mass status`, errors, screenshots,
  and test logs can reveal paths or conversation contents.
- Report vulnerabilities privately as described in `SECURITY.md`, not in a
  public issue. Do not include working credentials in any report.

## Start here

Use current stable Rust, a C compiler/linker, and Python 3.12+ for development
checks and packaging. `fzf` is only needed to exercise the preserved prototype's
terminal test; the test otherwise reports a skip. It is not a Rust dependency.

```sh
cargo build --locked --release
python3 scripts/demo.py
```

The demo isolates sources/configuration/cache and disables native launch and
clipboard helpers. The Rust executable is `target/release/mass`; `./mass` is the
historical Bash/fzf prototype. See `README.md` for normal use and `docs/RUST.md`
for configuration and behavior.

## Check a change

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 tests/rust_tui_pty.py
python3 tests/rust_cli_recovery.py
python3 -m unittest discover -s tests -p 'test_*.py'
cargo build --release --locked
MASS_TEST_BINARY="$PWD/target/release/mass" python3 tests/rust_tui_pty.py
MASS_TEST_BINARY="$PWD/target/release/mass" python3 tests/rust_cli_recovery.py
python3 scripts/demo.py --check
bash scripts/package.sh
python3 tests/rust_package_smoke.py
```

CI also checks secrets and dependencies. On Linux x86_64, the following downloads
checksum-pinned tools into ignored `target/` (never into the system):

```sh
bash scripts/security-checks.sh secrets --staged
bash scripts/security-checks.sh dependencies
```

The first command scans the staged diff; without `--staged` it scans Git history.
Dependency checking fails on all RustSec warnings, including unsound and
unmaintained crates. Do not suppress a finding simply to get a green check:
explain its impact and fix it, or propose a narrowly scoped, reviewed exception
with a reason and expiry.

These checks use synthetic stores. Do not substitute a live store or enable a
real agent in an automated test. Native compatibility experiments require
explicit permission and an isolated, credential-free environment.

## Tests and snapshots

Add a regression near the affected behavior. Adapter changes should include a
small invented record demonstrating identity and text preservation. Keep cache
migration and search-equivalence tests when changing storage or ranking.

Update snapshots only deliberately, then inspect every changed screen:

```sh
MASS_UPDATE_SNAPSHOTS=1 cargo test --locked tui::snapshots
cargo test --locked tui::snapshots
```

Golden snapshots do not replace human terminal review. `docs/PREVIEW.md`
describes the synthetic screenshot and its optional capture dependencies.

## Pull requests and versions

Keep a PR focused. Explain the problem, behavior change, validation performed,
and remaining limits. Include synthetic before/after terminal captures for
visual changes. List checks you could not run rather than claiming success.

Leave individual changes under **Unreleased** in `CHANGELOG.md`. Do not bump a
version, tag a release, or upload binaries as part of an ordinary fix. Follow
`VERSIONING.md` when the maintainer decides to release. Cargo publication is
intentionally disabled until a distribution decision is made.

Contributions are under the repository's MIT license. Preserve third-party
notices and explain the origin/license of any imported code or assets.
