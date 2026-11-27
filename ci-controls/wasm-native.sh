#!/usr/bin/env bash
set -eo pipefail
# jig's wasm32-wasip2 build under $WASMTIME and the native $JIG must give the
# same output and exit code on the same input (REQ-028); the planted workflow
# with a tag ref must turn both red. A could-not-run input must give exit 2
# through the component too (wasip2 turns a returned code 2 into 1; jig exits
# through std::process::exit). The OS errno inside an error message is
# platform detail (2 natively, 44 under WASI) and is dropped before comparing.
wasm=$(cd "$(dirname "$JIG_WASM")" && pwd)/$(basename "$JIG_WASM")
both() {
  dir=$1 want=$2; shift 2
  n=$(cd "$dir" && "$JIG" "$@" 2>&1) && nc=0 || nc=$?
  w=$(cd "$dir" && "$WASMTIME" run --dir=. "$wasm" "$@" 2>&1) && wc=0 || wc=$?
  n=$(printf '%s\n' "$n" | sed -E 's/ \(os error [0-9]+\)//')
  w=$(printf '%s\n' "$w" | sed -E 's/ \(os error [0-9]+\)//')
  if [ "$n" != "$w" ] || [ "$nc" != "$wc" ]; then
    echo "wasm differs from native: jig $* (native exit $nc, wasm exit $wc)"
    diff <(printf '%s\n' "$n") <(printf '%s\n' "$w") || true
    exit 1
  fi
  [ "$nc" = "$want" ] || { printf '%s\n' "$n"; echo "jig $*: exit $nc, expected $want"; exit 1; }
  echo "same under wasmtime and native (exit $nc): jig $*"
}
both . 0 pins .
both . 0 verdict --self-test
both . 2 verdict --needs absent-needs.json --workflow absent-workflow.yml
p=$(mktemp -d)
mkdir -p "$p/.github/workflows"
printf 'on: push\njobs:\n  a:\n    runs-on: ubuntu-24.04\n    steps:\n      - uses: actions/checkout@v4\n' > "$p/.github/workflows/planted.yml"
both "$p" 1 pins .
