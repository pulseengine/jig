#!/usr/bin/env bash
set -eo pipefail
n=0
while IFS= read -r lock; do
  n=$((n+1)); dir=$(dirname "$lock")
  command -v npm > /dev/null || { echo "npm unavailable on this runner; $lock cannot be checked"; exit 1; }
  echo "::group::npm $lock"
  before=$(mktemp)
  cp "$lock" "$before"
  (cd "$dir" && npm install --package-lock-only --ignore-scripts --no-audit --no-fund > /dev/null)
  cmp -s "$before" "$lock" || { diff -u "$before" "$lock" || true; echo "$lock is out of sync with package.json"; exit 1; }
  (cd "$dir" && npm audit --package-lock-only --audit-level=low)
  echo "::endgroup::"
done < <(git ls-files -- '*package-lock.json' | awk -F/ '$NF == "package-lock.json"')
echo "$n package-lock.json file(s) checked"
