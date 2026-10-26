#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-$PWD/.cargo-home}"
export CARGO_TARGET_DIR="$PWD/target"
cargo build --locked --release
version=$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)
host=$(rustc -vV | awk '/^host:/ { print $2 }')
name="mass-${version}-${host}"
mkdir -p dist
# A fresh stage prevents files removed from the package from lingering in it.
stage=$(mktemp -d "$PWD/dist/.package-XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
package="$stage/$name"
mkdir -p "$package/docs"
cp target/release/mass "$package/mass"
cp README.md CHANGELOG.md VERSIONING.md LICENSE DESIGN.md CONTRIBUTING.md SECURITY.md "$package/"
cp docs/RUST.md docs/VALIDATION.md docs/LOCAL-RELEASE.md docs/PROTOTYPE.md docs/HARNESS-COMPATIBILITY.md docs/PREVIEW.md "$package/docs/"
cp -R docs/assets "$package/docs/"
# Metadata can include inactive optional backends. Use the actual build graph
# (normal + build dependencies, not dev-only crates) to select required notices.
cargo tree --locked --target "$host" --edges normal,build --prefix none --format '{p}' > "$stage/dependencies.txt"
cargo metadata --locked --format-version 1 --filter-platform "$host" \
  | python3 scripts/package-licenses.py --packages "$stage/dependencies.txt" > "$package/THIRD-PARTY-LICENSES.txt"
# Record dynamic requirements rather than promising static portability.
if command -v ldd >/dev/null; then ldd target/release/mass > "$package/libraries.txt"; fi
tar -czf "$stage/$name.tar.gz" -C "$stage" "$name"
mv "$stage/$name.tar.gz" "dist/$name.tar.gz"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
printf 'Local artifact: dist/%s.tar.gz\n' "$name"
