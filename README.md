# jig

jig is the shared CI baseline for pulseengine repositories: two reusable
GitHub Actions workflows that every repo calls, pinned by commit SHA, and the
Rust checker they run. It holds the checks every repo needs and that no repo
should own alone:

- the fail-closed fork guard, so fork code never reaches self-hosted runners
  (plant guard);
- the lockfile and audit check over every `Cargo.lock` and
  `package-lock.json`, and Bazel's lockfile mode;
- the action-pin check: every `uses:` is a 40-hex SHA and every Rust
  toolchain a literal version (`jig pins`);
- the verdict's reading of sieve: a skip passes only for a class the
  caller's own sieve classify job did not select;
- a required aggregator, `required / verdict`, that is red on any failure,
  cancellation, unselected skip, empty `needs` or caller job missing from
  `needs` (`jig verdict`), and checks the caller workflow's shape
  (`jig caller`).

[plant](https://github.com/pulseengine/plant) plants a bad input for each
check to prove it can go red; jig runs its own (`ci-controls.toml`) and
shows that stand-ins which cannot fail are reported FAIL-OPEN
(`ci-controls.stand-in.toml`).

**Status: work in progress, pre-1.0.** jig v0.1.0 to v0.1.3 are signed
prereleases, and its guard uses plant v0.4.0. warte calls jig in
shadow mode since [warte#194](https://github.com/pulseengine/warte/pull/194)
(2026-11-22): jig's checks run on every pull request there but are not
required. No repository depends on jig to merge yet. jig becomes 1.0 only
once every pulseengine repository with CI uses it. Design: [docs/design.md](docs/design.md).

## How a repo calls it

```yaml
on:
  pull_request:          # never a paths filter
  merge_group:
permissions:
  contents: read

jobs:
  baseline:
    uses: pulseengine/jig/.github/workflows/baseline.yml@<40-hex-sha>
    with:
      audit: cargo-audit
      controls-only: "fmt-unformatted lock-stale required-sees-failure"

  classify:              # sieve's own classify step, as sieve documents it
    runs-on: ...
    outputs: { run_all: ..., build: ... }
    steps: [...]

  build:
    needs: classify
    if: ${{ !cancelled() && (needs.classify.result != 'success' || needs.classify.outputs.build != 'false') }}
    runs-on: ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && fromJSON('["self-hosted","linux","x64","light"]') || 'ubuntu-24.04' }}
    steps: [...]

  required:
    needs: [baseline, classify, build]
    if: always()
    uses: pulseengine/jig/.github/workflows/required.yml@<40-hex-sha>
    with:
      needs-json: ${{ toJSON(needs) }}
      never-skip: "clippy"
```

- Pin both calls to the same full commit SHA, never a tag or branch. A jig
  change reaches a repo only through that repo's own bump PR.
- The caller adds `trusted_workflows = ["pulseengine/jig/.github/workflows/*.yml"]`
  to its `plant-guard.toml`.
- Classification is the caller's: keep sieve's classify step as your own job
  (any class names; sieve check-workflows verifies the form). The verdict
  reads its outputs (`classifier-job`, default `classify`) and counts a class
  as selected unless its output is exactly `false`.
- Rust toolchains are exact versions. The one exception is an advisory
  canary on `toolchain: stable`: a job with `continue-on-error: true` that no
  other job needs (except the job calling jig's required.yml), listed in
  `advisory`. `jig pins` prints it as an advisory
  canary, and the verdict refuses any continue-on-error job outside
  `advisory`.
- The caller grants `permissions: contents: read` at the top (or
  `read-all`, or only read/none scopes); `jig caller` is red on a missing
  block or any write scope there.
- A job judged by the caller's own aggregator (for example a Kani gate over a
  matrix) is passed as `delegated: "kani-x86=kani-gate"`; the aggregator must
  be never-skip and list the job in its `needs`.

One check as a step, without the workflows:

```yaml
- uses: pulseengine/jig@<40-hex-sha>
  with:
    command: pins
```

## Local use

```sh
cargo build --release
target/release/jig pins .
target/release/jig verdict --self-test
target/release/jig docs
JIG=$PWD/target/release/jig plant ci-controls.toml
```

## License

Apache-2.0. See [LICENSE](LICENSE).
