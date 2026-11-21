//! `jig verdict` through the binary and the table API (REQ-001, REQ-002, REQ-005).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use jig::verdict::{self, Expect};

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("jig-verdict-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Dir(d)
}

const WF: &str = "\
on: pull_request
jobs:
  baseline:
    uses: o/r/.github/workflows/b.yml@0123456789abcdef0123456789abcdef01234567
  build:
    needs: baseline
    if: ${{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.build != 'false') }}
    runs-on: x
  lint:
    runs-on: x
  fuzz:
    runs-on: x
  required:
    needs: [baseline, build, lint, fuzz]
    if: always()
    runs-on: x
";

fn needs(base: &str, outs: &str, rest: &[(&str, &str)]) -> String {
    let mut s = format!(r#"{{"baseline":{{"result":"{base}","outputs":{outs}}}"#);
    for (id, r) in rest {
        s += &format!(r#","{id}":{{"result":"{r}"}}"#);
    }
    s + "}"
}

fn run(d: &Dir, needs_json: &str, wf: &str, extra: &[&str]) -> Output {
    fs::write(d.0.join("needs.json"), needs_json).unwrap();
    fs::write(d.0.join("ci.yml"), wf).unwrap();
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["verdict", "--needs"])
        .arg(d.0.join("needs.json"))
        .arg("--workflow")
        .arg(d.0.join("ci.yml"))
        .args(extra)
        .output()
        .unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

const F: &str = r#"{"build":"false","run_all":"false"}"#;
const T: &str = r#"{"build":"true","run_all":"false"}"#;
const ALL: [(&str, &str); 3] = [
    ("build", "success"),
    ("lint", "success"),
    ("fuzz", "success"),
];

// rivet: verifies REQ-001
#[test]
fn all_green_passes() {
    let d = dir("green");
    let o = run(&d, &needs("success", F, &ALL), WF, &[]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
}

// rivet: verifies REQ-001
#[test]
fn a_failure_is_red_and_a_cancel_is_red() {
    let d = dir("fail");
    let o = run(
        &d,
        &needs(
            "success",
            F,
            &[
                ("build", "failure"),
                ("lint", "success"),
                ("fuzz", "success"),
            ],
        ),
        WF,
        &[],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("job 'build' result is failure"));
    let o = run(
        &d,
        &needs(
            "success",
            F,
            &[
                ("build", "success"),
                ("lint", "cancelled"),
                ("fuzz", "success"),
            ],
        ),
        WF,
        &[],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("job 'lint' result is cancelled"));
}

// rivet: verifies REQ-001
#[test]
fn empty_needs_is_red_not_green() {
    let d = dir("empty");
    let o = run(&d, "{}", WF, &[]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("needs is empty"));
}

// rivet: verifies REQ-001
#[test]
fn baseline_not_success_is_red() {
    let d = dir("base");
    for r in ["failure", "skipped", "cancelled"] {
        let o = run(&d, &needs(r, F, &ALL), WF, &[]);
        assert_eq!(code(&o), 1, "baseline {r}");
        assert!(stdout(&o).contains("baseline job 'baseline'"));
    }
    let o = run(
        &d,
        r#"{"build":{"result":"success"},"lint":{"result":"success"},"fuzz":{"result":"success"}}"#,
        WF,
        &[],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("absent"));
}

// rivet: verifies REQ-001
#[test]
fn skip_of_unselected_class_is_green_but_selected_is_red() {
    let d = dir("skip");
    let skipped = [
        ("build", "skipped"),
        ("lint", "success"),
        ("fuzz", "success"),
    ];
    let o = run(&d, &needs("success", F, &skipped), WF, &[]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    let o = run(&d, &needs("success", T, &skipped), WF, &[]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("class 'build' was selected"));
}

// rivet: verifies REQ-001
#[test]
fn run_all_never_skip_and_classless_skips_are_red() {
    let d = dir("skip2");
    let b = [
        ("build", "skipped"),
        ("lint", "success"),
        ("fuzz", "success"),
    ];
    let o = run(
        &d,
        &needs("success", r#"{"build":"false","run_all":"true"}"#, &b),
        WF,
        &[],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("run_all is true"));
    let o = run(&d, &needs("success", F, &b), WF, &["--never-skip", "build"]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("never-skip"));
    // lint has no if:, so a skip has no class: fail closed.
    let l = [
        ("build", "success"),
        ("lint", "skipped"),
        ("fuzz", "success"),
    ];
    let o = run(&d, &needs("success", F, &l), WF, &[]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("cannot be determined"));
}

// rivet: verifies REQ-001
#[test]
fn unknown_result_is_red() {
    let d = dir("unknown");
    let o = run(
        &d,
        &needs(
            "success",
            F,
            &[
                ("build", "success"),
                ("lint", "passed"),
                ("fuzz", "success"),
            ],
        ),
        WF,
        &[],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("unknown result 'passed'"));
}

// rivet: verifies REQ-001
#[test]
fn advisory_failure_is_green_and_lists_split_on_space_and_comma() {
    let d = dir("adv");
    let r = [
        ("build", "success"),
        ("lint", "failure"),
        ("fuzz", "failure"),
    ];
    let o = run(
        &d,
        &needs("success", F, &r),
        WF,
        &["--advisory", "lint fuzz"],
    );
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    let o = run(
        &d,
        &needs("success", F, &r),
        WF,
        &["--advisory", "lint", "--advisory", "fuzz"],
    );
    assert_eq!(code(&o), 0);
    let o = run(
        &d,
        &needs("success", F, &r),
        WF,
        &["--advisory", "lint,fuzz"],
    );
    assert_eq!(code(&o), 0);
    // Not advisory: red.
    let o = run(&d, &needs("success", F, &r), WF, &["--advisory", "fuzz"]);
    assert_eq!(code(&o), 1);
}

// rivet: verifies REQ-002
#[test]
fn a_job_missing_from_needs_is_red_unless_advisory_or_self() {
    let d = dir("missing");
    let r = [("build", "success"), ("lint", "success")];
    let o = run(&d, &needs("success", F, &r), WF, &[]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("job 'fuzz' is in the workflow but missing from needs"));
    let o = run(&d, &needs("success", F, &r), WF, &["--advisory", "fuzz"]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    // The self job is never required in needs; --self-job renames it.
    let wf = WF.replace("required:", "gate:");
    let o = run(&d, &needs("success", F, &ALL), &wf, &["--self-job", "gate"]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    let o = run(&d, &needs("success", F, &ALL), &wf, &[]);
    assert_eq!(code(&o), 1);
}

// rivet: verifies REQ-002
#[test]
fn a_needs_key_that_is_not_a_job_is_red() {
    let d = dir("extra");
    let r = [
        ("build", "success"),
        ("lint", "success"),
        ("fuzz", "success"),
        ("ghost", "success"),
    ];
    let o = run(&d, &needs("success", F, &r), WF, &[]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("needs key 'ghost' is not a job in the workflow"));
}

// rivet: verifies REQ-002
#[test]
fn an_unparseable_workflow_is_exit_2() {
    let d = dir("badwf");
    let n = needs("success", F, &ALL);
    for (what, wf) in [
        (
            "merge key",
            "jobs:\n  a: &a\n    runs-on: x\n  baseline:\n    <<: *a\n",
        ),
        ("no jobs", "on: push\n"),
        ("empty jobs", "jobs: {}\n"),
        ("broken yaml", "jobs: [oops\n"),
    ] {
        let o = run(&d, &n, wf, &[]);
        assert_eq!(
            code(&o),
            2,
            "{what}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

// rivet: verifies REQ-002
#[test]
fn unreadable_or_invalid_input_and_bad_args_are_exit_2() {
    let d = dir("exit2");
    let o = run(&d, "not json", WF, &[]);
    assert_eq!(code(&o), 2);
    let o = run(&d, "[]", WF, &[]);
    assert_eq!(code(&o), 2);
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args([
            "verdict",
            "--needs",
            "/nonexistent/needs.json",
            "--workflow",
        ])
        .arg(d.0.join("ci.yml"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["verdict"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["verdict", "--bogus"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
}

// rivet: verifies REQ-001
#[test]
fn json_format_reports_verdict_and_reasons() {
    let d = dir("json");
    let o = run(&d, "{}", WF, &["--format", "json"]);
    assert_eq!(code(&o), 1);
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["verdict"], "red");
    assert!(v["reasons"][0].as_str().unwrap().contains("needs is empty"));
}

// rivet: verifies REQ-005
#[test]
fn self_test_flag_exits_0() {
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["verdict", "--self-test"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    assert!(stdout(&o).contains("rows agree"));
}

// rivet: verifies REQ-005
#[test]
fn the_table_has_the_required_cases_and_all_agree() {
    let rows = verdict::table();
    assert!(rows.len() >= 15);
    for want in [
        "all green",
        "one failure",
        "one cancelled",
        "empty needs",
        "baseline failed",
        "baseline skipped",
        "selected-class skip",
        "unselected-class skip",
        "never-skip job skipped",
        "run_all with skip",
        "skip with no class",
        "unknown result",
        "advisory failure",
        "job missing from needs",
        "extra needs key",
    ] {
        assert!(rows.iter().any(|r| r.name == want), "no row named {want}");
    }
    assert_eq!(verdict::disagreeing(&rows), Vec::<usize>::new());
}

// rivet: verifies REQ-005
#[test]
fn a_row_with_a_flipped_expectation_is_caught() {
    let mut rows = verdict::table();
    let n_green = rows.iter().position(|r| r.expect == Expect::Green).unwrap();
    let n_red = rows
        .iter()
        .position(|r| matches!(r.expect, Expect::Red(_)))
        .unwrap();
    rows[n_green].expect = Expect::Red("anything");
    rows[n_red].expect = Expect::Green;
    let mut want = vec![n_green + 1, n_red + 1];
    want.sort();
    assert_eq!(verdict::disagreeing(&rows), want);
    // A wrong reason text on a red row is also a disagreement.
    let mut rows = verdict::table();
    rows[n_red].expect = Expect::Red("text that is never a reason");
    assert_eq!(verdict::disagreeing(&rows), vec![n_red + 1]);
}

// ---- delegation (REQ-015) ----

fn wf_d(proofs_needs: &str) -> String {
    format!(
        "\
on: pull_request
jobs:
  baseline:
    uses: o/r/.github/workflows/b.yml@0123456789abcdef0123456789abcdef01234567
  build:
    needs: baseline
    if: ${{{{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.build != 'false') }}}}
    runs-on: x
  kani:
    runs-on: x
  proofs:
{proofs_needs}    if: always()
    runs-on: x
  required:
    needs: [baseline, build, proofs]
    if: always()
    runs-on: x
"
    )
}

const DEL_OK: [(&str, &str); 2] = [("build", "success"), ("proofs", "success")];

fn del(d: &Dir, rest: &[(&str, &str)], wf: &str, extra: &[&str]) -> Output {
    run(d, &needs("success", F, rest), wf, extra)
}

// rivet: verifies REQ-015
#[test]
fn a_delegated_job_is_judged_by_its_aggregator_alone() {
    let d = dir("del-green");
    let wf = wf_d("    needs: [kani]\n");
    let args = ["--never-skip", "proofs", "--delegated", "kani=proofs"];
    // Absent, failed, skipped and cancelled delegated results are not judged.
    assert_eq!(code(&del(&d, &DEL_OK, &wf, &args)), 0);
    for res in ["failure", "skipped", "cancelled", "success"] {
        let mut rest = DEL_OK.to_vec();
        rest.push(("kani", res));
        let o = del(&d, &rest, &wf, &args);
        assert_eq!(code(&o), 0, "{res}: {}", stdout(&o));
    }
    // An unknown result string is still red.
    let mut rest = DEL_OK.to_vec();
    rest.push(("kani", "weird"));
    let o = del(&d, &rest, &wf, &args);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("unknown result 'weird'"));
    // Without the flag the job is missing from needs.
    let o = del(&d, &DEL_OK, &wf, &["--never-skip", "proofs"]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("job 'kani' is in the workflow but missing from needs"));
}

// rivet: verifies REQ-015
#[test]
fn pairs_may_be_repeated_comma_or_space_separated() {
    let d = dir("del-forms");
    let wf = wf_d("    needs: kani\n");
    for args in [
        &["--never-skip", "proofs", "--delegated", "kani=proofs"][..],
        &["--never-skip=proofs", "--delegated=kani=proofs,kani=proofs"][..],
        &[
            "--never-skip",
            "proofs",
            "--delegated",
            "kani=proofs kani=proofs",
        ][..],
        &[
            "--never-skip",
            "proofs",
            "--delegated",
            "kani=proofs",
            "--delegated",
            "kani=proofs",
        ][..],
    ] {
        let o = del(&d, &DEL_OK, &wf, args);
        assert_eq!(code(&o), 0, "{args:?}: {}", stdout(&o));
    }
}

// rivet: verifies REQ-015
#[test]
fn an_aggregator_that_does_not_list_the_delegated_job_is_red() {
    let d = dir("del-unlisted");
    // The delegated job was skipped by something other than sieve, the
    // aggregator succeeded without it: red.
    let wf = wf_d("");
    let mut rest = DEL_OK.to_vec();
    rest.push(("kani", "skipped"));
    let o = del(
        &d,
        &rest,
        &wf,
        &["--never-skip", "proofs", "--delegated", "kani=proofs"],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("does not list delegated job 'kani'"));
}

// rivet: verifies REQ-015
#[test]
fn aggregator_must_be_present_successful_and_never_skip() {
    let d = dir("del-agg");
    let wf = wf_d("    needs: [kani]\n");
    let args = ["--never-skip", "proofs", "--delegated", "kani=proofs"];
    let o = del(&d, &[("build", "success")], &wf, &args);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("is absent from needs"));
    for res in ["failure", "skipped", "cancelled"] {
        let o = del(&d, &[("build", "success"), ("proofs", res)], &wf, &args);
        assert_eq!(code(&o), 1, "{res}");
        assert!(stdout(&o).contains("not success"), "{res}: {}", stdout(&o));
    }
    let o = del(&d, &DEL_OK, &wf, &["--delegated", "kani=proofs"]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("is not in never-skip"));
}

// rivet: verifies REQ-015
#[test]
fn delegation_to_self_unknown_jobs_and_chains_are_red() {
    let d = dir("del-shape");
    let wf = wf_d("    needs: [kani]\n");
    let o = del(
        &d,
        &DEL_OK,
        &wf,
        &["--never-skip", "kani proofs", "--delegated", "kani=kani"],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("delegated to itself"));
    let o = del(
        &d,
        &DEL_OK,
        &wf,
        &["--never-skip", "proofs", "--delegated", "ghost=proofs"],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("'ghost' is not a job in the workflow"));
    let o = del(
        &d,
        &DEL_OK,
        &wf,
        &[
            "--never-skip",
            "proofs",
            "--delegated",
            "kani=proofs,proofs=build",
        ],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("is itself delegated"));
    let o = del(
        &d,
        &DEL_OK,
        &wf,
        &["--never-skip", "proofs", "--delegated", "baseline=proofs"],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("cannot be delegated"));
}

// rivet: verifies REQ-015
#[test]
fn a_malformed_delegation_is_exit_2() {
    let d = dir("del-bad");
    let wf = wf_d("    needs: [kani]\n");
    for bad in [
        "kani",
        "kani=",
        "=proofs",
        "kani=a=b",
        "kani=proofs,kani=build",
    ] {
        let o = del(
            &d,
            &DEL_OK,
            &wf,
            &["--never-skip", "proofs", "--delegated", bad],
        );
        assert_eq!(code(&o), 2, "{bad}");
    }
}

// rivet: verifies REQ-015
#[test]
fn self_test_covers_delegation() {
    let rows = verdict::table();
    for want in [
        "delegated skipped elsewhere, aggregator does not list it",
        "aggregator absent from needs",
        "aggregator failed",
        "aggregator not in never-skip",
        "job delegated to itself",
        "aggregator is itself delegated",
    ] {
        assert!(rows.iter().any(|r| r.name == want), "no row named {want}");
    }
    assert_eq!(verdict::disagreeing(&rows), Vec::<usize>::new());
}
