#!/usr/bin/env bash
# Plays two git revisions of the engine against each other over UHP.
# Usage: scripts/version-match.sh <rev-a> <rev-b> [uhp-match options...]
#   e.g. scripts/version-match.sh worktree v0.4.0 --games 200 --time-ms 100
# Prints the uhp-match summary; record notable results in CLAUDE.md.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
a="$("$root/scripts/build-version.sh" "${1:?rev-a}")"
b="$("$root/scripts/build-version.sh" "${2:?rev-b}")"
shift 2
(cd "$root/engine" && cargo build --release --bin uhp-match >&2)
"$root/engine/target/release/uhp-match" --a "$a" --b "$b" "$@"
