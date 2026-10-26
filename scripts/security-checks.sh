#!/usr/bin/env bash
# Download checksum-pinned audit tools into this checkout, never into the system.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]]; then
  printf 'Tool bootstrap supports Linux x86_64; use Gitleaks/cargo-audit directly on other hosts.\n' >&2
  exit 2
fi
mode="${1:-}"
if [[ "$mode" != secrets && "$mode" != dependencies ]]; then
  printf 'Usage: bash scripts/security-checks.sh {secrets|dependencies} [tool arguments]\n' >&2
  exit 2
fi
shift
tools="$PWD/target/security-tools"
mkdir -p "$tools"
export CARGO_HOME="${CARGO_HOME:-$PWD/.cargo-home}"
if [[ "$mode" == secrets ]]; then
  curl --fail --location --silent --show-error --retry 3 \
    https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz \
    -o "$tools/gitleaks.tar.gz"
  printf '%s  %s\n' 551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb "$tools/gitleaks.tar.gz" | sha256sum -c
  tar -xzf "$tools/gitleaks.tar.gz" -C "$tools" gitleaks
  exec "$tools/gitleaks" git --redact --no-banner "$@" "$PWD"
else
  curl --fail --location --silent --show-error --retry 3 \
    https://github.com/rustsec/rustsec/releases/download/cargo-audit/v0.22.2/cargo-audit-x86_64-unknown-linux-gnu-v0.22.2.tgz \
    -o "$tools/audit.tgz"
  printf '%s  %s\n' ab28a1bdb54db4d5d8ad5981cf1f959410370b3d28250dbd35f6a44248620e39 "$tools/audit.tgz" | sha256sum -c
  tar -xzf "$tools/audit.tgz" -C "$tools" --strip-components=1 cargo-audit-x86_64-unknown-linux-gnu-v0.22.2/cargo-audit
  exec "$tools/cargo-audit" audit --db "$tools/advisory-db" --deny warnings "$@"
fi
