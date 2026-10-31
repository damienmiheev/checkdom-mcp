#!/bin/sh
# Release facts the release workflows read, in one place (RELEASE.md).
# Usage: scripts/release-info.sh version [<rev>]      package version in Cargo.toml
#        scripts/release-info.sh signing-key [<rev>]  the minisign public key (minisign.pub)
# <rev> reads the file at that git revision instead of the work tree. Needs no
# Rust toolchain, so the workflows can call it before installing one.
set -eu
cd "$(dirname "$0")/.."

fail() { echo "release-info: $*" >&2; exit 1; }
read_file() {
    if [ -n "${2:-}" ]; then git show "$2:$1"; else cat "$1"; fi
}

case ${1:-} in
version)
    version=$(read_file Cargo.toml "${2:-}" | awk '
        /^\[package\]/ { on = 1; next }
        /^\[/ { on = 0 }
        on && /^version[ \t]*=/ { sub(/^[^"]*"/, ""); sub(/".*/, ""); print; exit }')
    echo "$version" | grep -Eqx '[0-9]+\.[0-9]+\.[0-9]+' || fail "no plain MAJOR.MINOR.PATCH version in [package]"
    echo "$version"
    ;;
signing-key)
    # minisign.pub: an "untrusted comment:" line, then the key (RW…).
    key=$(read_file minisign.pub "${2:-}" 2>/dev/null | grep -E '^RW[A-Za-z0-9+/=]+$' | head -1) || true
    [ -n "$key" ] || fail "minisign.pub has no public key (RELEASE.md § One-time setup)"
    echo "$key"
    ;;
*)
    fail "usage: $0 version|signing-key [<rev>]"
    ;;
esac
