# jig design

Distilled from the proposal "Fleet CI unification" (2026-11-21), where jig is
called ci-baseline. Numbers come from sieve's dry run and a read of every
repo's workflow files; those marked as estimates are estimates.

**Read "v0.1.0 as built" first.** The sections after it are the proposal as
written. Where they disagree with it, the as-built section is current. Above
all: baseline.yml has **no classify job and no outputs**. The fixed-output
interface below was dropped because the first real adopter, gale, failed
its class list on all six classes. Classification stays in each caller's
own sieve classify job.

## v0.1.0 as built: where it departs from the proposal

Agreed with the neighbouring sessions on 2026-11-21 (sieve, plant, warte,
relay, temper, varve) and recorded in artifacts/ (REQ-015..020, DD-011..013):

- **No `pool`, `fallback` or `sieve-config` input.** plant guard refuses a
  runner taken from an input, and baseline's jobs are light work, so every
  job runs on the light pool behind the fail-closed fork guard.
- **Classification stays in the caller** (owner, 2026-11-21; REQ-016,
  DD-011). baseline.yml has no classify job and no outputs. Each caller keeps
  sieve's classify step as its own job, in the form sieve check-workflows
  verifies today (REQ-018), with its own class names; the verdict reads the
  selections from that job (`classifier-job`, default `classify`). A fixed
  vocabulary was tried first and dropped: gale, the first real adopter,
  failed it on all six classes. Shadow mode, enforce-skips and the
  `check-graph --control graph-inputs` refusal therefore live in the
  caller's classify job, as sieve documents them.
- **The verdict fails closed on classes** (REQ-003): a class counts as
  selected unless the classifier's output is exactly 'false', and all
  classes count as selected when the classifier is absent or did not
  succeed.
- **`delegated` input**: a job judged by a caller aggregator (relay's Kani and
  Verus gates keep their reason-gated skips) is not judged by the verdict;
  the aggregator must be never-skip, succeed and list the job in its needs
  (REQ-015, DD-012).
- **Completeness reads the caller workflow from the checkout** (path from
  `github.workflow_ref`), so no `actions: read` is needed; `jig caller`
  also checks the triggers, filters and the required job's shape (REQ-017).
- **supply covers npm**: package-lock.json sync and `npm audit`, no waivers. It installs node 26.13.1 (pinned setup-node) when a lock exists; jig tracks a clean fixture lock so its own supply runs the npm path (DD-016).
- **jig cuts signed releases** (owner, 2026-11-21) and the workflows fetch
  its binary by recorded digest (docs/releasing.md); `action.yml` offers
  single checks as a step. Version 1.0 means fleet-wide use (REQ-020).
- **plant v0.4.0** accepts workflow_call workflows and reusable-workflow
  calls listed in `trusted_workflows` (plant#34); v0.3.0 rejected every jig
  caller.

## Why a separate repo

plant is the checker: sieve's false-skip controls live in plant so the decider
and its check stay apart. plant is pre-1.0 and ships often. The baseline is
glue (two workflow files, a pin list, self-tests) and consumes plant and sieve
as signed release binaries, by version and sha256 or through the varve layer.

## Interface: two reusable workflows

Two, because the aggregator must `needs:` the caller's own jobs.

### `baseline.yml` (workflow_call)

Inputs: `pool` (default `light`), `fallback` (`ubuntu-24.04`), `sieve-config`
(`sieve.toml`), `guard-config` (`plant-guard.toml`), `controls`
(`ci-controls.toml`), `controls-only` (cheap control ids per PR), `audit`
(`cargo-deny` | `cargo-audit` | `none`), `bazel` (bool), `enforce-skips`
(bool, default false = shadow mode, every class true).

**Superseded (v0.1.0 has no outputs; see "as built"):**
Outputs: `run_all`, `reason`, `classes` (JSON), and one fixed output per fleet
class: `build`, `unit_test`, `integration`, `emulator`, `coverage`,
`mutation`, `fuzz`, `bench`, `docs`. Proofs, plant controls, guard and audit
are never-skip and have no output.

Jobs:

| job | what it does |
|---|---|
| guard | `plant guard` (pinned binary) against the caller's workflows |
| supply | for every `git ls-files '*Cargo.lock'` named exactly `Cargo.lock`: `cargo metadata --locked`, then audit each lock; with `bazel`, every invocation and `.bazelrc` carry `--lockfile_mode=error` |
| pins | every `uses:` is a 40-hex SHA (local `./` excepted); dtolnay `toolchain:` is a version (or `stable` in an advisory canary job: `continue-on-error: true`, needed by no job but the verdict job, listed in the verdict's `advisory`); no `cargo +nightly` without a date; no `pip install` without hashes |
| controls | `plant --only <controls-only>`, same-repo runners only |
| classify (superseded: lives in the caller) | refuse a `sieve.toml` whose globs or paths match nothing at HEAD (extend `sieve check-graph`), then `sieve classify --format github-output`. Shadow mode, a refused config or any failure means every class is true |

### `required.yml` (workflow_call)

Inputs: `needs-json` (`${{ toJSON(needs) }}`), `classes`, `never-skip` (job
ids), `advisory` (job ids allowed to fail, by name).

One job, `verdict` (required context `required / verdict`):

- self-test: a decision table over planted needs JSON;
- completeness: parse the caller workflow; every job except `required` and the
  advisory ones must be in `needs`;
- judge: red if `needs` is empty, on any failure or cancellation, if baseline
  is not success, or if a skipped job's class was selected by sieve or the job
  is never-skip.

Callers use `needs: [baseline, …every job…]` with `if: always()`, never
`success()`. One required context per repo replaces lists of up to 22.

### Class declaration

**As built:** a job's `if:` reads `needs.<classifier>.outputs.<class>` from
the caller's own classify job, so the `uses:` recognition below is not
needed. Proposal text:

A job's `if:` reads `needs.baseline.outputs.<class>` in sieve's exact form,
plus sieve's `[workflows.job_outputs]` map; `sieve check-workflows` verifies
both. Tool changes needed:

- `sieve check-workflows` must recognise a `uses: …/baseline.yml@sha` job as
  the classifier;
- plant's manifest needs a `class` key. Until then the class goes in the
  `check` string as `"<class>: <job name>"`.

## plant controls per class

Cheap controls run per PR. Expensive ones rerun the check twice and run
nightly and whenever `.github/**`, `ci-controls.toml` or lockfiles change.

| class | planted bad input | when |
|---|---|---|
| fork guard | misspelled self-hosted label; fork-flag form (`guard-bad-label`, `guard-unguarded-site`) | PR |
| aggregator | needs JSON with one failure; one cancelled; an unselected skip of a never-skip job; empty needs | PR |
| sieve classify | plant's 10 false-skip controls on the repo's own `sieve.toml` | PR |
| pins | `actions/checkout@v4`; `dtolnay/rust-toolchain@stable` | PR |
| audit | nested `Cargo.lock` with a known advisory | PR |
| lockfile freshness | `Cargo.toml` bump without a lock update; `MODULE.bazel` edit without a lock update | PR |
| lint | one unformatted file | PR |
| rivet validate | an artifact with a broken link | PR |
| build | compile error in a claimed crate | nightly |
| unit test | broken function body a named test must catch | nightly |
| integration / oracle | known-wrong output bytes | nightly |
| proof check | Lean `sorry`/axiom; Kani narrowing assume and ledger growth; Rocq `Admitted.`; Verus `assume(false)` | nightly |
| fuzz smoke | target panicking on a corpus seed; empty target list | nightly |
| mutation | deleted assertion so a mutant survives; missing `outcomes.json` | nightly |
| coverage / MC/DC | removed test, so the ratchet drops | nightly |
| emulator | an image that must fault | nightly |
| release | tag/version mismatch | release PRs |

Each `ci-controls.toml` entry has `id`, `check`, `run`, `plant` (a directory
under `ci-controls/`) and an optional `timeout`.

## Call pattern

- `on: pull_request` (never a paths filter) plus `merge_group`;
  `permissions: contents: read`; `concurrency` cancels in progress only on
  `pull_request`.
- Gated jobs (as built): `needs: classify`, `if: !cancelled() &&
  (needs.classify.result != 'success' || needs.classify.outputs.<class> !=
  'false')`, runs-on in the
  fail-closed fork-guard form.
- Cheap jobs (clippy, rivet) are never gated.
- `required`: needs all jobs, `if: always()`.
- Bazel callers set `bazel: true`; `.bazelrc` gets `common --lockfile_mode=error`.

`merge_group`: sieve selects everything on non-PR events, so every queue run is
a full run, and a miss can only delay a red, never merge one. That holds only
with a queue, so `enforce-skips` stays false until a repo has one.

## Rollout

- **Phase 0**: jig's own CI calls itself, with plant controls for aggregator,
  pins, supply and guard. Stand-ins show an always-green aggregator and an
  always-skip sieve go FAIL-OPEN. Then a signed v0.1.0.
- **Phase 1, shadow** (2 weeks or 100 queue runs, whichever is later):
  baseline and required run beside existing jobs, everything runs, the old
  required contexts stay. Pilots: warte (cargo-only, correctness pilot, about
  0 minutes saved; shadow since warte#194, b8ece52, 2026-11-22), then relay
  (Bazel plus 4 provers, existing queue). Before relay starts, required.yml
  uploads each run's needs JSON (classify outputs and every job's result)
  with the head SHA as a 90-day artifact, so every pilot records the same
  evidence; a plant control proves the record is written.
- **Phase 2, enforce**: relay sets `enforce-skips: true` and moves protection
  to `required / verdict`, old contexts kept one more week. warte gets a queue
  before it enforces.
- **Phase 3**: the queue repos, then the largest code-only repos, then the
  rest; the Bazel rules repos last.

Success criteria (owner, 2026-11-22):

- every pilot: guard, pins, supply, controls and the verdict green and
  correct for 2 weeks or 100 runs, whichever is later; every plant control
  PASS and every stand-in FAIL-OPEN; runner-minutes per PR run and per merged
  PR (2 weeks before vs after, per pool); queue wait p50/p90 (runner wait on
  a repo without a merge queue);
- relay only: sieve misses 0 of at least 200 compared skipped jobs.

warte proves correctness only, because sieve skips nothing there. A replay
of sieve v0.3.0 with warte's sieve.toml at b8ece52 over its last 80 merged
PRs (2026-11-04 to 2026-11-22, the files of each squash commit, event
pull_request) gave 53 run_all, 27 with all three classes selected, and 0
would-skip jobs: every class lists the root Cargo.toml, which claims every
file, and the binary embeds docs/, templates/ and migrations/. The replay
used today's config and tree for older diffs, one classification per PR (not
per push), and no merge_group or push runs.

Rollback: revert the `uses:` SHA per repo; fleet-wide, `enforce-skips: false`
or a repo variable whose default fails closed to run everything.

Blast radius: a jig commit reaches a repo only through that repo's bump PR and
queue, pilots first with a 48 h soak. Everything below jig is pinned too: plant
and sieve by version and sha256, actions by SHA. No secrets are passed.

Stays repo-specific: build, test, proof, oracle and emulator steps; toolchain
versions; heavy pools; `sieve.toml`; `ci-controls.toml`; never-skip and
advisory lists; release, sign and deploy pipelines; nightly schedules.

Risks: check names become `"<caller job> / <called job>"`; plant guard must
also check jig's own workflows, and today it rejects every job that calls a
reusable workflow from another repository (plant `src/guard.rs`), which is
how every caller uses jig (plant#33; a `trusted_workflows` fix awaits a
plant release, which the pilot needs); reusable-workflow nesting caps (2 levels is
fine); self-hosted runners have no `gh`, so use `curl`.

Estimate: if gating removed half of the gated-class minutes in code-only PRs,
about 10–15% of roughly 120k runner-minutes per 2 weeks. Build-once,
dropping push re-runs in queue repos and moving hosted jobs to self-hosted
pools each save more.
