#!/usr/bin/env bash
set -eo pipefail
"$JIG_PROBE" confined --granted "$RUNNER_TEMP" --write "$CONFINED_WRITE" --read "$CONFINED_READ"
