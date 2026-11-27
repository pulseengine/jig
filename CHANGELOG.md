# Changelog

jig is below 1.0 (REQ-020). Callers pin the record commit that follows each
signed tag, never the tag commit (docs/releasing.md). Releases before v0.1.6
are described in their GitHub release notes.

## Unreleased

- CI runs jig's wasm32-wasip2 build under wasmtime 51.0.0 and compares
  stdout, stderr and exit code with the native binary on four inputs,
  one of them a planted unpinned `uses:` (REQ-028). The plant control
  `wasm-native-differs` shows the comparison goes red on one changed line.
- wasm32-wasip2: exit code 2 (cannot judge) now reaches the host as 2, not
  1. On WASI, jig flushes stdout and stderr and calls
  `std::process::exit`, which needs Rust 1.100's exit-with-code import.
- `jig docs caller`: a caller must require `required / verdict` and
  `baseline / guard`, or it merges past a red verdict (warte#227); two
  `gh api` commands check it from a maintainer shell. design.md adds the
  same line to the adoption checklist (owner, 2026-11-27).

## v0.1.8 (2026-11-27)

- `jig confined`: judges runner confinement by what the kernel refuses (a
  write outside the grants, a read of a denied path, on targets the job's
  own user owns), after a positive control; `ZWINGER_*` env vars are
  labels only (REQ-027, `jig docs confined`).
- baseline: the guard job carries the confinement probe on zwinger's decoy
  paths, inert while `JIG_CONFINED` is "false". It turns on in a later
  change together with the `confined` runner label, once a confined runner
  is online (DD-020; owner approved the joint zwinger proposal 2026-11-27).
  Nothing changes for callers today.
- `jig docs names`: pre-check a shapes file with `jig names`, not grep -E.

## v0.1.7 (2026-11-26)

- `jig names`: refuses a tracked file that matches one of the caller's
  private-name patterns, and prints the file, line and pattern index, never
  the match (REQ-026, jig#17, `jig docs names`).
- baseline: **new required input `names-patterns`** (breaking on bump). The
  pins job runs `jig names` with it: a tracked file of shape patterns that
  name no real host, or `none` to opt out (logged). Empty, or a path that is
  not tracked, is red. `none` raises a `names: off` warning annotation and
  a job summary line, so an opted-out repo shows in the run. `names-allow`
  names a tracked allow file. Exact names are not accepted as an input:
  GitHub prints a step's env values in the log before the step runs
  (owner, 2026-11-25: shape patterns only, fail closed).
- A grouped monthly Dependabot config (github-actions and cargo; planted
  fixtures under ci-controls/fixtures are excluded).

## v0.1.6 (2026-11-25)

- supply: every Cargo workspace found from the tracked Cargo.toml files
  needs a committed Cargo.lock, unless the new `lockfile-census` input lists
  it with an issue and a reason (REQ-025, `jig lockfiles`, `jig docs
  lockfiles`). Before, a nested workspace whose lock was never committed
  passed.
- supply: fixed a vacuous pass. Three tests used `cmd | grep -q .` under
  pipefail; on a long list the writer's SIGPIPE made the test false and the
  check was skipped, including the `audit: none` guard. They now capture
  the list and test it.
