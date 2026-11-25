# Changelog

jig is below 1.0 (REQ-020). Callers pin the record commit that follows each
signed tag, never the tag commit (docs/releasing.md). Releases before v0.1.6
are described in their GitHub release notes.

## Unreleased

- `jig names`: refuses a tracked file that matches one of the caller's
  private-name patterns, and prints the file, line and pattern index, never
  the match (REQ-026, jig#17, `jig docs names`). baseline.yml does not run
  it yet: where the exact names come from in CI is an owner decision.

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
