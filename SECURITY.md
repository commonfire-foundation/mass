# Security

## Reporting a vulnerability

Use GitHub's private vulnerability reporting for this repository:

https://github.com/commonfire-foundation/mass/security/advisories/new

Do not open a public issue for an undisclosed vulnerability. Include the commit
or version, affected platform, impact, and a minimal **invented** reproduction.
Never submit real conversations, cache databases, credentials, or private
configuration. If GitHub's private reporting is unavailable, open an issue
requesting a private contact channel **without vulnerability details**.

This is a small, pre-release project. Maintainers will triage reports as they
can; there is no guaranteed response time or supported stable release line.
Fixes currently target `main`. No security audit or sandbox guarantee is claimed.

## Trust boundaries

- Indexing, search, and reading should not modify native transcripts, execute
  agents, or send conversation data over the network.
- The local SQLite cache contains plaintext conversations and may contain
  secrets. New Unix caches are owner-private; filesystem permissions are not
  encryption. Deletion does not securely erase WAL files, snapshots, or backups.
- Exports and clipboard actions are explicit, but their text is not secret-
  redacted. JSON preserves original content; terminal consumers must sanitize it.
- Native reopening is experimental and disabled by default. If deliberately
  enabled and confirmed, the native agent may use its own credentials, hooks,
  plugins, tools, permissions, and network. MASS is not its sandbox.
- Symlinks are skipped during discovery. Malformed input, unsupported formats,
  record limits, and ambiguous identities should fail safely or report coverage
  diagnostics rather than silently claim complete history.

Issues involving unintended disclosure/execution, transcript mutation, unsafe
export destinations, permission bypasses, or malformed-input crashes are
particularly useful to report privately. Ordinary usability bugs can use the
public bug-report form with synthetic data.
