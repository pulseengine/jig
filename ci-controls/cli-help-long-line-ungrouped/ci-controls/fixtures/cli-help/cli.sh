#!/bin/sh
# rivet: verifies REQ-031
# A fixture CLI whose help `jig cli-help` accepts. Each cli-help-* plant in
# ci-controls.toml replaces this file with a copy that breaks one rule.
CMDS="check report c3 c4 c5 c6 c7 c8 c9"

short() {
  cat <<'TXT'
demo: a fixture CLI for jig cli-help

Usage: demo <COMMAND>

Commands:
  check   Check the thing
  report  Print a report
  c3      Three
  c4      Four
  c5      Five
  c6      Six
  c7      Seven
  c8      Eight
  c9      Nine

Options:
  -h, --help  Print help
  xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
TXT
}

long() {
  short
  cat <<'TXT'

Examples:
  demo check
  demo report
TXT
}

sub() {
  cat <<TXT
demo $1: one fixture command

Usage: demo $1

Options:
  -h, --help  Print help
TXT
}

case "$*" in
  -h) short ;;
  --help) long ;;
  *)
    if [ $# -eq 2 ] && { [ "$2" = -h ] || [ "$2" = --help ]; }; then
      for c in $CMDS; do
        if [ "$c" = "$1" ]; then sub "$1"; exit 0; fi
      done
    fi
    echo "error: unexpected argument '$*'" >&2
    exit 2 ;;
esac
