#!/usr/bin/env bash
set -eo pipefail
jig confined --granted "$RUNNER_TEMP" --write "$CONFINED_WRITE" --read "$CONFINED_READ"
