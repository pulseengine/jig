# jig

jig is the shared CI baseline for pulseengine repositories: a pair of reusable
GitHub Actions workflows that every repo calls, pinned by commit SHA. It holds
the checks every repo needs and that no repo should own alone:

- the fail-closed fork guard, so fork code never reaches self-hosted runners;
- the lockfile and audit check over every `Cargo.lock` (and Bazel lockfile mode);
- the action-pin check: every `uses:` is a 40-hex SHA;
- sieve's classify step, which decides which job classes a change needs;
- a required aggregator, `required / verdict`, that turns red on any failure,
  cancellation, unselected skip or empty `needs`.

[plant](https://github.com/pulseengine/plant) supplies planted controls per job
class to prove each of these checks can go red.

**Status: bootstrap.** No workflow in this repo is callable yet. The design is
in [docs/design.md](docs/design.md).

## How a repo will call it

A sketch of the intended interface, not yet implemented:

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
      pool: light
      audit: cargo-deny
      controls-only: "fmt-unformatted lock-stale required-sees-failure"

  build:
    needs: baseline
    if: ${{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.build != 'false') }}
    runs-on: ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && fromJSON('["self-hosted","linux","x64","light"]') || 'ubuntu-latest' }}
    steps: [...]

  required:
    needs: [baseline, build]
    if: always()
    uses: pulseengine/jig/.github/workflows/required.yml@<40-hex-sha>
    with:
      needs-json: ${{ toJSON(needs) }}
      classes: ${{ needs.baseline.outputs.classes }}
```

Pin by full commit SHA, never by tag or branch. A jig change reaches a repo
only through that repo's own bump PR and merge queue.

## License

Apache-2.0. See [LICENSE](LICENSE).
