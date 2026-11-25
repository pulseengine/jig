#!/usr/bin/env bash
set -eo pipefail
# Workspaces are found from the tracked Cargo.toml files (outside
# target/), so a nested workspace whose lock was never committed is
# red unless the census lists it with an issue and a reason.
git rev-parse --show-toplevel > /dev/null
# Capture, then test: grep -q exits at the first match, and under
# pipefail the writer's SIGPIPE would make the test false and skip
# the check.
manifests=$(git ls-files -- '*Cargo.toml' | awk -F/ '$NF == "Cargo.toml"')
if [ -n "$manifests" ]; then
  if [ -n "$CENSUS" ]; then jig lockfiles . --census "$CENSUS"; else jig lockfiles .; fi
fi
n=0
while IFS= read -r lock; do
  n=$((n+1)); dir=$(dirname "$lock")
  echo "::group::cargo metadata --locked in $dir"
  cargo metadata --locked --format-version 1 --manifest-path "$dir/Cargo.toml" > /dev/null
  echo "::endgroup::"
done < <(git ls-files -- '*Cargo.lock' | awk -F/ '$NF == "Cargo.lock"')
echo "$n Cargo.lock file(s) checked"
if [ "$n" -eq 0 ] && [ -n "$manifests" ]; then
  echo "Cargo.toml is tracked but no Cargo.lock is: commit the lockfile"; exit 1
fi
