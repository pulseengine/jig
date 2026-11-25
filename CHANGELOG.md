# Changelog

jig is below 1.0 (REQ-020). Callers pin the record commit that follows each
signed tag, never the tag commit (docs/releasing.md). Releases before v0.1.6
are described in their GitHub release notes.

## v0.1.6 (unreleased)

- supply: every Cargo workspace found from the tracked Cargo.toml files
  needs a committed Cargo.lock, unless the new `lockfile-census` input lists
  it with an issue and a reason (REQ-025, `jig lockfiles`, `jig docs
  lockfiles`). Before, a nested workspace whose lock was never committed
  passed.
- supply: fixed a vacuous pass. Three tests used `cmd | grep -q .` under
  pipefail; on a long list the writer's SIGPIPE made the test false and the
  check was skipped, including the `audit: none` guard. They now capture
  the list and test it.
