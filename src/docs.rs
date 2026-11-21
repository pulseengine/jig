//! `jig docs [topic]`: short embedded docs (PulseEngine CLI convention).

const VERDICT: &str = "\
jig verdict: the required / verdict decision (REQ-001, REQ-002, REQ-005)

  jig verdict --needs needs.json --workflow .github/workflows/ci.yml \\
      [--self-job required] [--baseline-job baseline] \\
      [--never-skip 'a b'] [--advisory c,d] [--format text|json]
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
  - a file that does not parse or uses a YAML merge key.
Findings are file:line. No file found is exit 1 (nothing was compared).

Exit: 0 passed, 1 failed or nothing compared, 2 could not run.
";

pub const TOPICS: [(&str, &str, &str); 2] = [
    (
        "verdict",
        "the required / verdict decision and its self-test",
        VERDICT,
    ),
    ("pins", "action, toolchain and pip pin check", PINS),
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
