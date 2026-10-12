#!/usr/bin/env bash
# Builds the UHP engine (hive-uhp) at a git revision and prints its path.
# Binaries are cached in .versions/<short-sha>/hive-uhp.
# Usage: scripts/build-version.sh <rev>   (rev: tag, branch, sha, or "worktree")
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
rev="${1:?usage: build-version.sh <rev>}"
mkdir -p "$root/.versions"

if [ "$rev" = "worktree" ]; then
  # Current uncommitted tree.
  (cd "$root/engine" && cargo build --release --bin hive-uhp >&2)
  echo "$root/engine/target/release/hive-uhp"
  exit 0
fi

sha="$(git -C "$root" rev-parse --short=10 "$rev^{commit}")"
out="$root/.versions/$sha"
if [ ! -x "$out/hive-uhp" ]; then
  src="$root/.versions/src-$sha"
  rm -rf "$src"
  git -C "$root" worktree add --detach --force "$src" "$sha" >&2
  if [ ! -f "$src/engine/src/bin/hive-uhp.rs" ]; then
    git -C "$root" worktree remove --force "$src"
    echo "revision $rev ($sha) has no UHP engine (added in v0.4.0)" >&2
    exit 1
  fi
  (cd "$src/engine" && cargo build --release --bin hive-uhp --target-dir "$root/.versions/target" >&2)
  mkdir -p "$out"
  cp "$root/.versions/target/release/hive-uhp" "$out/hive-uhp"
  git -C "$root" worktree remove --force "$src"
fi
echo "$out/hive-uhp"
