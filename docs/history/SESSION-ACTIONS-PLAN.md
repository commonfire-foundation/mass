# Session actions — historical implementation record

This records an earlier local-development checkpoint. Its test counts, publication restrictions, and artifact status are historical; current behavior and release gates are in `docs/RUST.md`, `docs/VALIDATION.md`, and `docs/LOCAL-RELEASE.md`.

Approved scope: make a found conversation useful without turning MASS into a harness or archive manager. Work remains local; do not publish, bump the version, install globally, mutate original transcripts, or access credentials for testing.

## 1. Clean export

- Add `mass export REF --format markdown|json --output FILE`.
- Default scope is the whole indexed conversation; `--message MESSAGE_REF` exports the complete message containing that matched chunk.
- Reassemble verified adjacent chunks into messages without duplicate overlap. Stream the conversation rather than retaining its entire history.
- Include agent, title, activity date and non-path provenance. Omit source/cwd paths by default; offer an explicit include-paths option. Conversation text itself is not secret/path-redacted.
- Exports describe indexed coverage and are not native harness backups or restorable sessions.
- Require a destination; create owner-private output atomically without overwriting an existing file or symlink. Do not leave partially written destination files.
- Test Markdown/code preservation, JSON structure, long/Unicode messages, message scoping, privacy metadata, and overwrite refusal.

## 2. Compact session actions

- Ctrl-O opens one keyboard-navigable menu for the selected result/current reader message; existing Ctrl-R resume review remains available.
- Export conversation or current message as Markdown/JSON through an explicit path-entry/confirmation sheet.
- Copy the matching excerpt, complete current message, session reference, source path, or reviewed resume command.
- Show the original transcript's full path and containing directory; no file-manager launch is necessary for this first version.
- Detect optional local wl-copy/xclip helpers. Explain unavailable actions; never install helpers or silently use OSC52/remote clipboard escapes.
- Keep slow exports/clipboard operations off the rendering thread and surface errors in the action sheet.
- Warn that exports and clipboard contents may contain secrets. Do not copy or export simply by opening the menu.

## 3. Native reopening

- Clearly distinguish reopening an existing native session from importing an export. No transcript import, restoration, cross-harness conversion, or session deletion.
- Retain source opt-in, identity/cwd/child/archive guards, and explicit launch review.
- Revalidate the source/configuration/indexed identity immediately before launch after confirmation; reject a changed plan instead of silently changing the command.
- Inspect installed versions/help through direct executables, avoiding installer wrappers. Test disposable native sessions only with project-local homes/configuration and no credentials or prompts/model requests.
- Record what was actually verified. Authentication or unsupported disposable formats are verification blockers, not proof of successful reopening. Unverified harnesses stay experimental/default-off.

## 4. Verification and local packaging

- Add synthetic unit/CLI and PTY action-flow regressions, plus inert clipboard/resume helpers.
- Run existing formatting, Clippy, Rust, PTY, recovery, prototype, and package smoke checks.
- Update README, detailed usage, changelog (Unreleased), and validation evidence.
- Rebuild/check the local archive without publishing or changing the package version.

## Completion record

- **Export implemented:** `src/export.rs` and the CLI provide conversation/message Markdown/JSON export, verified chunk joining, explicit path metadata, and private atomic no-overwrite files.
- **Actions implemented:** Ctrl-O opens the compact menu; exports, local-helper clipboard copying, source location, and reopen review work without changing the underlying query or reader selection. Ctrl-R review also supports C to copy a reviewed command without launching it.
- **Reopen safeguards implemented:** executable paths are resolved; current configuration and session identity are checked after confirmation; changed plans are rejected. Inert-helper tests verify launch/return and post-review identity/permission revocation.
- **Offline native acceptance advanced:** the Pi/Claude crashes were caused by sandbox `nodev` device access and fixed with a private `/dev`. Pi 0.85.1, Claude 2.1.272 and Codex 0.154.0 now pass guarded MASS-to-native-to-MASS fixture checks: saved marker, exact arguments/cwd, native exit 0 and terminal restoration. Codex used an invalid, non-secret offline key fixture; no real credentials or conversations were used. Normal authenticated configurations remain unverified. See `docs/HARNESS-COMPATIBILITY.md`; reopening stays experimental/default-off.
- **Verification passed:** 60 Rust tests, 19 monochrome TUI tests, 18 PTY scenarios against debug/release binaries, 2 recovery checks against both builds, 25 prototype tests, formatting, Clippy, extracted-package smoke (including export), and archive checksum.
- The local executable/archive were rebuilt. Version remains **0.1.2**, changes stay under **Unreleased**, and nothing was published or installed globally.
