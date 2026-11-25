#!/usr/bin/env bash
set -eo pipefail
n=0
while IFS= read -r lock; do
  n=$((n+1))
  echo "::group::$AUDIT $lock"
  if [ "$AUDIT" = cargo-audit ]; then
    cargo audit --file "$lock"
  else
    cargo deny --manifest-path "$(dirname "$lock")/Cargo.toml" check advisories
  fi
  echo "::endgroup::"
done < <(git ls-files -- '*Cargo.lock' | awk -F/ '$NF == "Cargo.lock"')
echo "$n Cargo.lock file(s) audited, no waivers"
