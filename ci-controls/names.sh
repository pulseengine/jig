#!/usr/bin/env bash
set -eo pipefail
case "$NAMES_PATTERNS" in
  "")
    echo "private-name guard has no patterns: pass names-patterns: <tracked shapes file>, or names-patterns: none"
    exit 1 ;;
  none)
    echo "::warning title=names: off::names-patterns: none; the private-name guard is off for this repository"
    if [ -n "$GITHUB_STEP_SUMMARY" ]; then echo "names: off (names-patterns: none)" >> "$GITHUB_STEP_SUMMARY"; fi
    exit 0 ;;
esac
git ls-files --error-unmatch -- "$NAMES_PATTERNS" > /dev/null 2>&1 \
  || { echo "names-patterns: $NAMES_PATTERNS is not a tracked file"; exit 1; }
if [ -n "$NAMES_ALLOW" ]; then
  jig names . --patterns "$NAMES_PATTERNS" --allow "$NAMES_ALLOW"
else
  jig names . --patterns "$NAMES_PATTERNS"
fi
