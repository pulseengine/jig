//! `jig docs [topic]`: short embedded docs (PulseEngine CLI convention).

const VERDICT: &str = "\
jig verdict: the required / verdict decision (REQ-001, REQ-002, REQ-005)

  jig verdict --needs needs.json --workflow .github/workflows/ci.yml \\
      [--self-job required] [--baseline-job baseline] \\
      [--never-skip 'a b'] [--advisory c,d] [--delegated job=aggregator] \\
      [--format text|json]
  jig verdict --self-test

needs.json is the JSON of ${{ toJSON(needs) }}. The built-in decision table
always runs first; a disagreeing row is exit 1 (\"self-test row N disagrees\").

Red (exit 1), one reason line each, if:
  - needs is empty;
  - the baseline job is absent or not success;
  - a non-advisory job failed or was cancelled;
  - any result is not success/failure/cancelled/skipped;
  - a non-advisory job was skipped and it is never-skip, run_all is true, its
    class (needs.<baseline>.outputs.<class> in its if:) is selected, or no
    class can be read from its if:;
  - a workflow job (except the self job and advisory jobs) is missing from
    needs, or a needs key is not a workflow job.
A class is selected unless the baseline's output for it is exactly \"false\".

--delegated <job>=<aggregator> (repeatable; comma or space separated pairs,
REQ-015): the delegated job's own result is not judged (not failure, not
skip, not completeness; it may be absent from needs; an unknown result string
is still red). Red instead if: the aggregator is absent from needs or not
success; the aggregator is not also in --never-skip; the aggregator's job in
the workflow does not list the delegated job in its needs: (scalar or
sequence); the delegated job is not a workflow job, is the baseline or self
job, or is delegated to itself; the aggregator is itself delegated. A
malformed pair is exit 2.

Exit: 0 passed, 1 failed or nothing compared, 2 could not run (bad args,
unreadable or unparseable input, workflow without jobs, YAML merge key).
";

const PINS: &str = "\
jig pins: every action and toolchain is pinned (REQ-008)

  jig pins [DIR] [--format text|json]

Scans DIR/.github/workflows/*.yml|yaml and every action.yml|yaml below DIR
(not target/ or .git/), as parsed YAML. Violations:
  - uses: not ./local, docker://image@sha256:<64 hex>, or
    owner/repo[/path]@<40 lowercase hex> (tags, branches, short or uppercase
    SHAs and ${{ }} expressions are refused);
  - dtolnay/rust-toolchain without with.toolchain of the form N.N[.N] or
    nightly-YYYY-MM-DD (stable, beta, nightly are refused);
  - a run: with `cargo +nightly` not followed directly by -YYYY-MM-DD;
  - a run: with pip install / pip3 install / python -m pip install lacking
    --require-hashes;
  - a local uses: ./path naming no file the scan read (missing, symlinked,
    in the skipped root target/, or outside the repository);
  - a file that does not parse or uses a YAML merge key.
Findings are file:line. No file found is exit 1 (nothing was compared).

Blind to: the run: checks read shell text, not a parsed shell. Indirection
(`p=pip; $p install`, eval, a script file the step calls) is not seen.
When in doubt they fail closed (unbalanced quotes, option values).

Exit: 0 passed, 1 failed or nothing compared, 2 could not run.
";

const CLASSES: &str = "\
jig classes: the sieve config uses only the fleet class vocabulary (REQ-016)

  jig classes --config sieve.toml [--format text|json]
  jig classes --list

The vocabulary (also `jig classes --list`, one per line):
  build unit_test integration emulator coverage mutation fuzz bench docs

The config is TOML with schema = \"sieve.config.v1\" and one [class.<name>]
table per class. Red (exit 1): a class name outside the vocabulary (each is
named), or no [class.*] at all (nothing was compared). Green prints the
vocabulary classes the config defines.

Exit: 0 passed, 1 failed or nothing compared, 2 could not run (unreadable
file, not TOML, schema missing or not sieve.config.v1, class not a table of
tables).
";

const CALLER: &str = "\
jig caller: the shape of a caller workflow (REQ-017)

  jig caller --workflow .github/workflows/ci.yml \\
      [--required-job required] [--baseline-job baseline] [--format text|json]

Red (exit 1), one reason line each, if:
  - on: (scalar, sequence or mapping form) lacks pull_request or merge_group;
  - pull_request or merge_group has paths, paths-ignore, branches,
    branches-ignore or types (merge_group types: [checks_requested] is the
    one allowed value);
  - on: has pull_request_target at all;
  - the required job is missing, its if: is not exactly always() or
    ${{ always() }} (whitespace ignored), or its needs: misses any other job;
  - the baseline job is missing or does not use
    <path>/.github/workflows/baseline.yml@<40 lowercase hex>, the required job
    does not use <path>/.github/workflows/required.yml@<40 hex>, or the two
    SHAs differ.

Exit: 0 passed, 1 failed, 2 could not run (unreadable or unparseable
workflow, YAML merge key, no jobs mapping).
";

pub const TOPICS: [(&str, &str, &str); 4] = [
    (
        "verdict",
        "the required / verdict decision and its self-test",
        VERDICT,
    ),
    ("pins", "action, toolchain and pip pin check", PINS),
    ("classes", "sieve.toml class vocabulary check", CLASSES),
    ("caller", "caller workflow shape check", CALLER),
];

pub fn list() -> String {
    let mut s = String::from("jig docs topics:\n");
    for (name, about, _) in TOPICS {
        s += &format!("  {name:<8} {about}\n");
    }
    s += "Run `jig docs <topic>` to print one.\n";
    s
}

pub fn topic(name: &str) -> Option<&'static str> {
    TOPICS.iter().find(|(n, _, _)| *n == name).map(|t| t.2)
}
