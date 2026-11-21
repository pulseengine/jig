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
- sieve's classify step, which decides which job classes a change needs;
- a required aggregator, `required / verdict`, that is red on any failure,
  cancellation, unselected skip, empty `needs` or caller job missing from
  `needs` (`jig verdict`), and checks the caller workflow's shape
  (`jig caller`).

[plant](https://github.com/pulseengine/plant) plants a bad input for each
check to prove it can go red; jig runs its own (`ci-controls.toml`) and
shows that stand-ins which cannot fail are reported FAIL-OPEN
(`ci-controls.stand-in.toml`).

**Status: v0.1.0 in preparation, work in progress.** jig becomes 1.0 only
once every pulseengine repository with CI uses it. No repository can adopt
it before the plant release that carries
[plant#33](https://github.com/pulseengine/plant/issues/33) and jig's own
first signed release. Design: [docs/design.md](docs/design.md).

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

  build:
    needs: baseline
    if: ${{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.build != 'false') }}
    runs-on: ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && fromJSON('["self-hosted","linux","x64","light"]') || 'ubuntu-24.04' }}
    steps: [...]

  required:
    needs: [baseline, build]
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
- Classes come from the caller's `sieve.toml`, named from the fleet
  vocabulary: build, unit_test, integration, emulator, coverage, mutation,
  fuzz, bench, docs (`jig classes --config sieve.toml` checks it). Until
  `enforce-skips: true` (only behind a merge queue) every class runs.
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
