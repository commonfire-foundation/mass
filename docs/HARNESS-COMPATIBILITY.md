# Native reopening: evidence and limits

MASS reopens an existing transcript through its original harness. It does **not** restore Markdown/JSON exports, import conversations, reconstruct deleted native sessions, or convert between harnesses.

Resume remains experimental and `resume = false` by default. No live configuration, real transcript, real credential store, or authenticated agent session was used for these checks.

## Local native checks

Installed executables were used directly, not installer wrappers. Checks used disabled networking, a read-only host filesystem, a private device filesystem, and writable project-local disposable homes/configuration/caches/workspaces. Synthetic conversations contained distinctive saved markers. No model prompts were submitted.

| Harness | Reported version | Offline saved-session result | Native exit and terminal |
| --- | --- | --- | --- |
| Pi | 0.85.1 | Saved marker displayed without credentials | Exit 0; terminal restored |
| Claude Code | 2.1.272 | Saved marker displayed in bare/safe/restricted mode without credentials | Exit 0; terminal restored |
| Codex | 0.154.0 | Saved marker displayed using a deliberately invalid, non-secret API-key fixture with networking disabled | `/quit` exited 0; terminal restored |

Codex without the dummy authentication fixture still stops at authentication. The fixture only permits offline UI initialization; it is **not** evidence of successful authentication or model access.

### Original crash cause: sandbox device access

The first Pi/Claude checks crashed before help/version, with exits 132/134. A seven-variant, two-harness matrix isolated the cause:

- The original root read-only bind also left `/dev` mounted with `nodev`. Opening `/dev/null`, `/dev/zero`, or `/dev/urandom` returned `Permission denied`, despite their normal file permissions.
- Changing user environment variables, making `/tmp` writable, making shared memory writable, or mounting a fresh `/proc` individually did not fix startup.
- Adding **`--dev /dev`** to the Bubblewrap invocation did fix both harnesses. This supplies private usable devices; it does not expose writable host files or enable networking.

Keep `--unshare-net`, `--clearenv`, the read-only host bind, and project-local home/configuration overrides. Do not solve this by removing isolation or borrowing real credentials. No MASS resume-argument change was required.

The PTY driver also needed a controlling terminal, honest keyboard-capability responses, and separately timed key events. Advertising an unimplemented keyboard protocol or sending an entire slash command plus Enter as one burst produced misleading input failures.

## Verified MASS-to-native round trips

All three installed harnesses also passed a guarded **MASS TUI → review → native session → MASS TUI** check:

- Only a disposable fixture source had `resume = true`; live configuration and the live index were untouched.
- Test-only executable wrappers recorded MASS's exact arguments and working directory, then invoked the real executable with additional isolation options. They were not fake agents.
- Pi disabled tools, extensions, skills, templates, context discovery and project approval. Claude used bare/safe/restricted mode, empty tools and MCP configuration. Codex used read-only sandboxing, on-request approval, disabled update checks and the offline dummy-key fixture.
- Exact native session arguments and the fixture working directory matched expectations for each harness.
- Wrapper-delimited native output contained the saved marker, excluding MASS's own search-result text from that assertion.
- Each native child exited 0; MASS displayed its successful-return status. The test then deliberately exited MASS with Ctrl-C (130), and terminal attributes matched their initial values.

These are controlled, version-specific fixture checks, not tests of the user's normal plugins, hooks, permissions, login or provider configuration. Only the empty disposable workspaces were trusted during onboarding.

Diagnostic scripts and raw evidence remain in ignored, owner-private `target/` directories. `target/latest-sandbox-diagnosis.txt` points to the matrix, fixture and round-trip evidence; `mass-roundtrip-results.json` contains the three integration outcomes. These are local audit artifacts, not portable CI fixtures or package contents.

## Additional automated safeguards

Synthetic/inert-helper regressions cover:

- Disabled sources and read-only/ambiguous/child guards.
- Review cancellation without starting a process.
- Explicit confirmation, terminal restoration, helper exit, and return to MASS.
- Source identity and configuration permission checked again **after** confirmation.
- Changed review plans rejected rather than silently launching a different command.
- Executable resolution to an absolute path; arguments passed directly, without a shell.
- Reviewed-command copying with POSIX shell quoting. Copied commands are snapshots: executing one outside MASS bypasses subsequent MASS revalidation.

## Remaining native acceptance

Normal authenticated configurations, resumed model/tool activity, live-session concurrency, broader interruption cases and other installed versions remain unverified. Any further test needs appropriate authorization for credentials, network use and the harness's own configuration/state writes. Record the actual versions and outcomes rather than generalizing these offline results into universal support.

Do not enable resume globally, resume real conversations without consent, enable automatic approval, or allow hidden model requests to broaden this evidence.
