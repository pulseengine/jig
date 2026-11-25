# Changelog

jig is below 1.0 (REQ-020). Callers pin the record commit that follows each
signed tag, never the tag commit (docs/releasing.md). Releases before v0.1.6
are described in their GitHub release notes.

## Unreleased

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
