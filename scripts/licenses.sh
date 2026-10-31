#!/bin/sh
# Writes the third-party licence notices for the crates compiled into the
# release binaries (cargo-about, about.toml + about.hbs). Fails if a crate has
# a licence not accepted in about.toml.
#
# Usage: scripts/licenses.sh [output]   (default: THIRD_PARTY_NOTICES.txt)
# Needs: cargo install cargo-about --version 0.9.2 --locked --features cli
set -eu
cd "$(dirname "$0")/.."
OUT="${1:-THIRD_PARTY_NOTICES.txt}"
cargo about generate --fail --locked about.hbs > "$OUT.tmp"
mv "$OUT.tmp" "$OUT"
echo "Wrote $OUT ($(wc -l < "$OUT" | tr -d ' ') lines)"
