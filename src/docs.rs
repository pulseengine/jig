//! `jig docs [topic]`: short embedded docs (PulseEngine CLI convention).

const VERDICT: &str = "\
jig verdict: the required / verdict decision (REQ-001, REQ-002, REQ-005)

  jig verdict --needs needs.json --workflow .github/workflows/ci.yml \\
      [--self-job required] [--baseline-job baseline] \\
      [--classifier-job classify] \\
      [--never-skip 'a b'] [--advisory c,d] [--delegated job=aggregator] \\
      [--record record.json] [--format text|json]
  jig verdict --self-test

needs.json is the JSON of ${{ toJSON(needs) }}. The built-in decision table
always runs first; a disagreeing row is exit 1 (\"self-test row N disagrees\").

Red (exit 1), one reason line each, if:
  - needs is empty;
  - the baseline job is absent or not success (with --baseline-job \"\" no
    baseline is judged: another required verdict must judge it, REQ-024);
  - a non-advisory job failed or was cancelled;
  - any result is not success/failure/cancelled/skipped;
  - a non-advisory job was skipped and it is never-skip, run_all is true, its
    class (needs.<classifier>.outputs.<class> in its if:) is selected, or no
    class can be read from its if:;
  - the classifier job is in needs but skipped, failed or cancelled;
  - a workflow job (except the self job and advisory jobs) is missing from
    needs, or a needs key is not a workflow job;
  - a job with continue-on-error (anything but a literal false) is not in
    --advisory: its result reads success even when it fails (REQ-021).

Classes: each caller keeps its own classify job (any id, default `classify`,
--classifier-job) whose outputs are the adopter's own sieve class names plus
run_all. There is no fixed class vocabulary and the baseline job has no class
outputs; a gated job's if: reads needs.<classifier>.outputs.<class> (the old
needs.baseline.outputs.<class> form names no class, so a skip is red). A
class is selected unless the classifier's output for it is exactly \"false\":
a missing output is selected (fail closed), and run_all == \"true\" selects
every class. If the classifier job is absent from needs, or did not succeed,
every class counts as selected, so any class-gated skip is red. The baseline
job keeps its own rule: it must be present and success. A repo without sieve
needs no classifier job; it simply has no class-gated jobs.

--delegated <job>=<aggregator> (repeatable; comma or space separated pairs,
REQ-015): the delegated job's own result is not judged (not failure, not
skip, not completeness; it may be absent from needs; an unknown result string
is still red). Red instead if: the aggregator is absent from needs or not
success; the aggregator is not also in --never-skip; the aggregator's job in
the workflow does not list the delegated job in its needs: (scalar or
sequence); the delegated job is not a workflow job, is the baseline or self
job, or is delegated to itself; the aggregator is itself delegated. A
malformed pair is exit 2.

--record <file> (REQ-023) also writes one JSON record (schema jig.record.v1):
the verdict, exit code and reasons, the needs JSON as given (the classifier
outputs and every job's result), the options, and the GitHub context from
GITHUB_REPOSITORY, GITHUB_WORKFLOW_REF, GITHUB_EVENT_NAME, GITHUB_REF,
GITHUB_SHA, JIG_HEAD_SHA, GITHUB_RUN_ID and GITHUB_RUN_ATTEMPT (null when
unset). A record that cannot be written is exit 2, even after a green
verdict line. required.yml uploads it as the artifact jig-record-<attempt>,
kept 90 days, readable by anyone who can read the repository's Actions runs.
Risk: the record keeps every job output the caller passes in needs for 90
days. GitHub drops an output that contains a registered secret, but not
other sensitive values; a caller must not put such values in job outputs.

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
    nightly-YYYY-MM-DD (stable, beta, nightly are refused), except
    toolchain: stable in an advisory canary: a job with continue-on-error:
    true (literally) that no other job needs, except the job calling
    jig's required.yml. Each canary is printed as
    'advisory canary: <file> job <id>' and must be in the verdict's
    --advisory (REQ-021);
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
An advisory canary's own steps are not limited: one that publishes, or
whose upload another workflow consumes, is not seen; review canary jobs.

Exit: 0 passed, 1 failed or nothing compared, 2 could not run.
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
    SHAs differ;
  - the workflow has no top-level permissions:, has one other than read-all
    (write-all, an expression), or a scope other than read or none (REQ-022).

--baseline-job \"\" (REQ-024): this workflow hosts no baseline, because its
verdict is one of several (one per workflow) and another required verdict
judges the baseline. jig caller then prints a note saying so, and is red if
any job here calls baseline.yml. It cannot see the other workflows: whether
some required verdict hosts the baseline is checked by temper (temper#106)
over the repository's required contexts.

Blind to: job-level permissions: a job may still widen its own token.
Blind to: the classify job's checkout. It needs fetch-depth: 0 and no
filter:, or sieve falls back to every class (fail closed, slow).

Exit: 0 passed, 1 failed, 2 could not run (unreadable or unparseable
workflow, YAML merge key, no jobs mapping).
";

const LOCKFILES: &str = "\
jig lockfiles: every Cargo workspace has a committed lock (REQ-025)

  jig lockfiles [DIR] [--census lockfile-census.toml] [--format text|json]

DIR is a git work tree (default .). Every tracked Cargo.toml outside a
target/ directory is resolved to its workspace root with cargo
locate-project --workspace. Red (exit 1), one reason line each, if:
  - a workspace has no Cargo.lock that is both on disk and tracked, and the
    census does not list its root manifest;
  - cargo cannot resolve a manifest's workspace;
  - a census listing is malformed: not exactly manifest, issue and reason
    (non-empty strings), a wildcard (* ? [ ] { }), a path not as git
    ls-files prints it, an issue other than #N or owner/repo#N, an unknown
    key, or a top-level key other than [[lockfree]];
  - a census listing is stale: the file is not tracked, a Cargo.toml that
    is not a workspace root, or a workspace that has a committed lock;
  - a manifest is listed more than once.
A malformed listing exempts nothing. The census format is gale's
tools/xtask/lockfile-census.toml:

  [[lockfree]]
  manifest = \"crates/demo/Cargo.toml\"
  issue = \"#534\"
  reason = \"Built only by a local script CI does not run.\"

A listing that is not a Cargo.toml (MODULE.bazel) must be tracked and well
formed but is not judged further: Bazel locks are the bazel input's.
baseline.yml's supply runs this with the lockfile-census input, then checks
every tracked Cargo.lock under --locked and audits it.

Blind to: whether a listed workspace really is never built; that is the
reason's claim, reviewed in the census change.

Exit: 0 passed, 1 failed or no tracked Cargo.toml (nothing compared),
2 could not run (not a git work tree, census unreadable or not TOML).
";

const NAMES: &str = "\
jig names: no tracked file contains a private name (REQ-026)

  jig names [DIR] --patterns FILE [--allow FILE] [--format text|json]

DIR is a git work tree (default .). Every file in its index (git ls-files)
that is not allowed is read as git will publish it, from the index blob
(git cat-file), not from the work tree, and matched line by line as bytes;
a filter or working-tree-encoding cannot change what is scanned. Stage a
change (git add) before checking it. Binary and non-UTF-8 files are
scanned too: skipping them would let one NUL byte hide a name. Red (exit 1)
with one line per finding:

  docs/setup.md:12: matches pattern #2

A finding names the file, the line and the pattern's index, never the
matched text: CI logs of public repositories are public, and printing the
name would leak it again. Patterns match anywhere in a line; anchors
(^ $) anchor to the line, and a CRLF line is matched without its CR.
Paths are matched too: as a whole, per component, and per dot-separated
piece and dot-ended prefix of a component (host-7.tar.gz tries host-7).
A path that matches is a finding (\"<path>: its path matches pattern
#n\"), and wherever it would be printed it reads <tracked path N> instead,
N being its line in git ls-files output. A symlink is scanned as its blob,
the target text; a submodule has no blob here and is skipped.

Patterns file: one Rust regex per line; blank lines and lines starting with
# are skipped; patterns are numbered from 1 in file order. Use (?i) for
case-insensitive matching. A pattern that matches the empty string is
refused. A patterns file with no patterns is red (nothing compared).

Allow file: one repository-relative path per line; a path ending in / allows
everything under that directory. Wildcards, absolute paths and .. are
refused. Each allowed path is printed as skipped.

Keep exact private names out of public repositories: a tracked list of them
is itself the leak. Shape patterns that name nothing (for example
[a-z]+-ci-[0-9]+) can be tracked.

Blind to: git history (it stops new leaks, it does not scrub old ones);
names split across lines, encoded (base64, URL escapes) or compressed
(inside an archive or image format); untracked and generated files; text
outside the repository (issues, pull request text, release notes).

Exit: 0 passed, 1 a finding or no patterns, 2 could not run (not a git work
tree, a tracked path that is not UTF-8, a file that exists but cannot be
read, a bad pattern or allow line).
";

pub const TOPICS: [(&str, &str, &str); 5] = [
    (
        "verdict",
        "the required / verdict decision and its self-test",
        VERDICT,
    ),
    ("pins", "action, toolchain and pip pin check", PINS),
    ("caller", "caller workflow shape check", CALLER),
    (
        "lockfiles",
        "every Cargo workspace has a committed lock",
        LOCKFILES,
    ),
    ("names", "no tracked file contains a private name", NAMES),
];

pub fn list() -> String {
    let mut s = String::from("jig docs topics:\n");
    for (name, about, _) in TOPICS {
        s += &format!("  {name:<10} {about}\n");
    }
    s += "Run `jig docs <topic>` to print one.\n";
    s
}

pub fn topic(name: &str) -> Option<&'static str> {
    TOPICS.iter().find(|(n, _, _)| *n == name).map(|t| t.2)
}
