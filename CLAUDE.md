# CLAUDE.md for jig

## What this is

jig is the shared CI baseline: reusable workflows (`baseline.yml`,
`required.yml`) that every pulseengine repo calls, pinned by SHA. It is mostly
glue: workflow files, a pin list and self-tests. It consumes plant and sieve as
signed release binaries (version + sha256, or through the varve layer); it does
not vendor or build them. Design: `docs/design.md`.

## Read first

- `docs/design.md`: interface, checks, plant controls per class, rollout.
- `artifacts/`: STPA losses, hazards, constraints and draft requirements.
  `rivet validate` must stay green; every new check traces to a requirement.

## Rules

- Project name is lowercase: jig.
- Bootstrap phase, CI hold: one light workflow only (rivet validate + lint).
  No pilot wiring into other repos until the pulseengine.eu session signals
  that the hold is lifted.
- Every `uses:` in this repo is pinned to a 40-hex SHA; jig must pass its own
  pin check.
- Self-hosted jobs use the fail-closed fork guard expression; no self-hosted
  job in a `pull_request_target` workflow.
- jig has no org secrets and publishes nothing. Callers grant `contents: read`
  (plus `actions: read` for the completeness step) and pass no secrets.
- A check counts only once a planted bad input has turned it red (plant).
- No new Python: checkers are Rust, as WASI components where possible.
- rivet comes from the varve pin (`varve.toml`, realm covalent); run it as
  `rivet` through the shim, never an ambient binary.
- Main is protected: signed commits, PRs only, merge on green required checks.
