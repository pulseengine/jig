//! `jig verdict`: the aggregator decision (REQ-001, REQ-002, REQ-005).
//!
//! Inputs are the JSON of `${{ toJSON(needs) }}` and the caller workflow. The
//! decision is red on every finding listed in docs.rs (`jig docs verdict`);
//! anything unreadable is "could not run", never green.

use std::collections::{BTreeMap, BTreeSet};

use crate::yaml::{self, Node};

/// Results GitHub reports for a needed job.
const KNOWN: [&str; 4] = ["success", "failure", "cancelled", "skipped"];
/// Baseline outputs that are not job classes.
const NON_CLASS: [&str; 3] = ["run_all", "reason", "classes"];

#[derive(Debug, Clone)]
pub struct Opts {
    pub self_job: String,
    pub baseline_job: String,
    pub never_skip: BTreeSet<String>,
    pub advisory: BTreeSet<String>,
    /// Delegated job -> the caller aggregator that judges it.
    pub delegated: BTreeMap<String, String>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            self_job: "required".into(),
            baseline_job: "baseline".into(),
            never_skip: BTreeSet::new(),
            advisory: BTreeSet::new(),
            delegated: BTreeMap::new(),
        }
    }
}

/// Split `a b,c` style lists (repeatable flags pass several) into ids.
pub fn split_ids(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .flat_map(|v| v.split(|c: char| c == ',' || c.is_whitespace()))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parse `job=aggregator` pairs (comma or space separated; repeatable). A
/// malformed pair, or a job given two different aggregators, is an error.
pub fn parse_delegated(values: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for pair in split_ids(values) {
        let Some((job, agg)) = pair.split_once('=') else {
            return Err(format!("--delegated '{pair}' is not <job>=<aggregator>"));
        };
        if job.is_empty() || agg.is_empty() || agg.contains('=') {
            return Err(format!("--delegated '{pair}' is not <job>=<aggregator>"));
        }
        if let Some(prev) = out.insert(job.to_string(), agg.to_string())
            && prev != agg
        {
            return Err(format!(
                "--delegated gives job '{job}' two aggregators ('{prev}' and '{agg}')"
            ));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct NeededJob {
    pub result: String,
    pub outputs: BTreeMap<String, String>,
}

pub type Needs = BTreeMap<String, NeededJob>;

/// Parse the JSON of `toJSON(needs)`. A missing or non-string `result` is kept
/// as an unknown value so the judge turns it red.
pub fn parse_needs(text: &str) -> Result<Needs, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("needs is not JSON: {e}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "needs is not a JSON object".to_string())?;
    let mut out = Needs::new();
    for (id, job) in obj {
        let job = job
            .as_object()
            .ok_or_else(|| format!("needs.{id} is not a JSON object"))?;
        let result = match job.get("result") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => "<missing>".to_string(),
        };
        let outputs = match job.get("outputs") {
            Some(serde_json::Value::Object(o)) => o
                .iter()
                .map(|(k, v)| {
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (k.clone(), s)
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        out.insert(id.clone(), NeededJob { result, outputs });
    }
    Ok(out)
}

/// The jobs of the caller workflow, each job's `if:` text and `needs:` list.
#[derive(Debug, Clone)]
pub struct Workflow {
    pub jobs: BTreeMap<String, Option<String>>,
    pub needs: BTreeMap<String, Vec<String>>,
}

pub fn parse_workflow(text: &str) -> Result<Workflow, String> {
    let root = yaml::parse(text).map_err(|e| format!("workflow {e}"))?;
    let Some(Node::Map { entries, .. }) = root.get("jobs") else {
        return Err("workflow has no jobs mapping".into());
    };
    if entries.is_empty() {
        return Err("workflow has an empty jobs mapping".into());
    }
    let mut jobs = BTreeMap::new();
    let mut needs = BTreeMap::new();
    for e in entries {
        let Node::Map { .. } = &e.value else {
            return Err(format!("workflow job '{}' is not a mapping", e.key));
        };
        let cond = match e.value.get("if") {
            None => None,
            Some(Node::Scalar { value, .. }) => Some(value.clone()),
            Some(_) => return Err(format!("workflow job '{}' has a non-scalar if:", e.key)),
        };
        let job_needs = match e.value.get("needs") {
            None => Vec::new(),
            Some(Node::Scalar { value, .. }) => vec![value.clone()],
            Some(Node::Seq { items, .. }) => items
                .iter()
                .map(|i| {
                    i.as_str().map(str::to_string).ok_or_else(|| {
                        format!("workflow job '{}' has a non-scalar needs: item", e.key)
                    })
                })
                .collect::<Result<_, _>>()?,
            Some(Node::Map { .. }) => {
                return Err(format!("workflow job '{}' has a mapping needs:", e.key));
            }
        };
        jobs.insert(e.key.clone(), cond);
        needs.insert(e.key.clone(), job_needs);
    }
    Ok(Workflow { jobs, needs })
}

/// Class names a job's `if:` reads as `needs.<baseline>.outputs.<class>`.
pub fn classes_of(cond: &str, baseline: &str) -> Vec<String> {
    let pat = format!("needs.{baseline}.outputs.");
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = cond[from..].find(&pat) {
        let start = from + i;
        let before_ok = cond[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !(ident(c) || c == '.'));
        let after = start + pat.len();
        let name: String = cond[after..].chars().take_while(|c| ident(*c)).collect();
        if before_ok && !name.is_empty() && !NON_CLASS.contains(&name.as_str()) {
            out.push(name);
        }
        from = after;
    }
    out
}

/// Every reason the verdict is red; empty means green.
pub fn judge(needs: &Needs, wf: &Workflow, o: &Opts) -> Vec<String> {
    let mut r = Vec::new();
    let b = &o.baseline_job;
    if needs.is_empty() {
        r.push("needs is empty: nothing was compared".to_string());
    }
    let baseline = needs.get(b);
    match baseline {
        None => r.push(format!("baseline job '{b}' is absent from needs")),
        Some(j) if j.result != "success" => r.push(format!(
            "baseline job '{b}' result is '{}', not success",
            j.result
        )),
        Some(_) => {}
    }
    let out = |name: &str| {
        baseline
            .and_then(|j| j.outputs.get(name))
            .map(String::as_str)
    };
    let run_all = out("run_all") == Some("true");

    for (id, j) in needs {
        if o.delegated.contains_key(id) {
            // The aggregator judges it; only an unreadable result is red here.
            if !KNOWN.contains(&j.result.as_str()) {
                r.push(format!("job '{id}' has unknown result '{}'", j.result));
            }
            continue;
        }
        let advisory = o.advisory.contains(id);
        match j.result.as_str() {
            "success" => {}
            "failure" | "cancelled" | "skipped" if id == b => {} // reported above
            "failure" | "cancelled" => {
                if !advisory {
                    r.push(format!("job '{id}' result is {}", j.result));
                }
            }
            "skipped" => {
                if advisory {
                    continue;
                }
                if o.never_skip.contains(id) {
                    r.push(format!("job '{id}' was skipped but is never-skip"));
                } else if run_all {
                    r.push(format!("job '{id}' was skipped while run_all is true"));
                } else {
                    let classes = match wf.jobs.get(id) {
                        Some(Some(cond)) => classes_of(cond, b),
                        _ => Vec::new(),
                    };
                    if classes.is_empty() {
                        r.push(format!(
                            "job '{id}' was skipped but its class cannot be determined from its if:"
                        ));
                    }
                    // Anything but an explicit "false" runs the job, so a
                    // missing output counts as selected.
                    for c in classes.iter().filter(|c| out(c) != Some("false")) {
                        r.push(format!(
                            "job '{id}' was skipped but class '{c}' was selected"
                        ));
                    }
                }
            }
            other => {
                if !KNOWN.contains(&other) {
                    r.push(format!("job '{id}' has unknown result '{other}'"));
                }
            }
        }
    }
    // The baseline's own result may be unknown too.
    if let Some(j) = baseline
        && !KNOWN.contains(&j.result.as_str())
    {
        r.push(format!("job '{b}' has unknown result '{}'", j.result));
    }

    for id in wf.jobs.keys() {
        if *id != o.self_job
            && !o.advisory.contains(id)
            && !o.delegated.contains_key(id)
            && !needs.contains_key(id)
        {
            r.push(format!(
                "job '{id}' is in the workflow but missing from needs"
            ));
        }
    }
    for id in needs.keys() {
        if !wf.jobs.contains_key(id) {
            r.push(format!("needs key '{id}' is not a job in the workflow"));
        }
    }
    judge_delegated(needs, wf, o, &mut r);
    r
}

/// The delegation rules (REQ-015): a delegated job is judged by its
/// aggregator, which must itself be held to a standard the verdict can see.
fn judge_delegated(needs: &Needs, wf: &Workflow, o: &Opts, r: &mut Vec<String>) {
    for (job, agg) in &o.delegated {
        if job == agg {
            r.push(format!("job '{job}' is delegated to itself"));
            continue;
        }
        if *job == o.baseline_job || *job == o.self_job {
            r.push(format!(
                "job '{job}' is the {} job and cannot be delegated",
                if *job == o.baseline_job {
                    "baseline"
                } else {
                    "required"
                }
            ));
        }
        if !wf.jobs.contains_key(job) {
            r.push(format!(
                "delegated job '{job}' is not a job in the workflow"
            ));
        }
        if o.delegated.contains_key(agg) {
            r.push(format!("aggregator '{agg}' of '{job}' is itself delegated"));
        }
        match needs.get(agg) {
            None => r.push(format!(
                "aggregator '{agg}' of delegated job '{job}' is absent from needs"
            )),
            Some(a) if a.result != "success" => r.push(format!(
                "aggregator '{agg}' of delegated job '{job}' result is '{}', not success",
                a.result
            )),
            Some(_) => {}
        }
        if !o.never_skip.contains(agg) {
            r.push(format!(
                "aggregator '{agg}' of delegated job '{job}' is not in never-skip"
            ));
        }
        match wf.needs.get(agg) {
            None => r.push(format!(
                "aggregator '{agg}' of delegated job '{job}' is not a job in the workflow"
            )),
            Some(n) if !n.contains(job) => r.push(format!(
                "aggregator '{agg}' does not list delegated job '{job}' in its needs:"
            )),
            Some(_) => {}
        }
    }
}

// ---- self-test (REQ-005) ----

#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    Green,
    /// Red, with some reason containing this text.
    Red(&'static str),
    /// Exit 2: the input cannot be read.
    CouldNotRun,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub name: &'static str,
    pub needs: String,
    pub workflow: String,
    pub never_skip: Vec<&'static str>,
    pub advisory: Vec<&'static str>,
    /// `job=aggregator` pairs, as given to --delegated.
    pub delegated: Vec<&'static str>,
    pub expect: Expect,
}

/// What a row actually produces.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Green,
    Red(Vec<String>),
    CouldNotRun(String),
}

pub fn run_row(row: &Row) -> Outcome {
    let opts = Opts {
        never_skip: row.never_skip.iter().map(|s| s.to_string()).collect(),
        advisory: row.advisory.iter().map(|s| s.to_string()).collect(),
        ..Opts::default()
    };
    let delegated: Vec<String> = row.delegated.iter().map(|s| s.to_string()).collect();
    let Ok(delegated) = parse_delegated(&delegated) else {
        return Outcome::CouldNotRun("bad --delegated".to_string());
    };
    let opts = Opts { delegated, ..opts };
    let needs = match parse_needs(&row.needs) {
        Ok(n) => n,
        Err(e) => return Outcome::CouldNotRun(e),
    };
    let wf = match parse_workflow(&row.workflow) {
        Ok(w) => w,
        Err(e) => return Outcome::CouldNotRun(e),
    };
    let reasons = judge(&needs, &wf, &opts);
    if reasons.is_empty() {
        Outcome::Green
    } else {
        Outcome::Red(reasons)
    }
}

pub fn row_agrees(row: &Row) -> bool {
    match (&row.expect, run_row(row)) {
        (Expect::Green, Outcome::Green) => true,
        (Expect::Red(want), Outcome::Red(reasons)) => reasons.iter().any(|r| r.contains(want)),
        (Expect::CouldNotRun, Outcome::CouldNotRun(_)) => true,
        _ => false,
    }
}

/// 1-based numbers of the rows whose outcome differs from the expectation.
pub fn disagreeing(rows: &[Row]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| !row_agrees(r))
        .map(|(i, _)| i + 1)
        .collect()
}

const OUTS_NONE: &str = r#"{"build":"false","unit_test":"false","fuzz":"false","run_all":"false"}"#;

const WORKFLOW: &str = "\
name: CI
on: pull_request
jobs:
  baseline:
    uses: pulseengine/jig/.github/workflows/baseline.yml@0123456789abcdef0123456789abcdef01234567
  build:
    needs: baseline
    if: ${{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.build != 'false') }}
    runs-on: ubuntu-24.04
  test:
    needs: baseline
    if: ${{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.unit_test != 'false') }}
    runs-on: ubuntu-24.04
  fuzz:
    needs: baseline
    if: ${{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.fuzz != 'false') }}
    runs-on: ubuntu-24.04
  clippy:
    runs-on: ubuntu-24.04
  required:
    needs: [baseline, build, test, fuzz, clippy]
    if: always()
    runs-on: ubuntu-24.04
";

/// `toJSON(needs)` for the fixture: baseline result and outputs, then the rest.
fn nj(base: &str, outs: &str, rest: &[(&str, &str)]) -> String {
    let mut s = format!(r#"{{"baseline":{{"result":"{base}","outputs":{outs}}}"#);
    for (id, res) in rest {
        s += &format!(r#","{id}":{{"result":"{res}","outputs":{{}}}}"#);
    }
    s.push('}');
    s
}

fn row(
    name: &'static str,
    needs: String,
    never_skip: &[&'static str],
    advisory: &[&'static str],
    expect: Expect,
) -> Row {
    Row {
        name,
        needs,
        workflow: WORKFLOW.to_string(),
        never_skip: never_skip.to_vec(),
        advisory: advisory.to_vec(),
        delegated: vec![],
        expect,
    }
}

const OK: [(&str, &str); 4] = [
    ("build", "success"),
    ("test", "success"),
    ("fuzz", "success"),
    ("clippy", "success"),
];

fn with(id: &str, res: &'static str) -> Vec<(&'static str, &'static str)> {
    OK.iter()
        .map(|(i, r)| if *i == id { (*i, res) } else { (*i, *r) })
        .collect()
}

/// The built-in decision table: planted needs JSON and workflows with the
/// verdict each must produce.
pub fn table() -> Vec<Row> {
    let all = |s: &'static str| -> Vec<(&'static str, &'static str)> {
        OK.iter().map(|(i, _)| (*i, s)).collect()
    };
    let outs_build = r#"{"build":"true","unit_test":"false","fuzz":"false","run_all":"false"}"#;
    let outs_all = r#"{"build":"false","unit_test":"false","fuzz":"false","run_all":"true"}"#;
    let outs_missing = r#"{"unit_test":"false","fuzz":"false","run_all":"false"}"#;
    let mut missing = with("fuzz", "success");
    missing.retain(|(i, _)| *i != "test");
    let mut extra = OK.to_vec();
    extra.push(("ghost", "success"));
    let mut rows = vec![
        row(
            "all green",
            nj("success", OUTS_NONE, &OK),
            &[],
            &[],
            Expect::Green,
        ),
        row(
            "one failure",
            nj("success", OUTS_NONE, &with("test", "failure")),
            &[],
            &[],
            Expect::Red("job 'test' result is failure"),
        ),
        row(
            "one cancelled",
            nj("success", OUTS_NONE, &with("build", "cancelled")),
            &[],
            &[],
            Expect::Red("job 'build' result is cancelled"),
        ),
        row(
            "empty needs",
            "{}".to_string(),
            &[],
            &[],
            Expect::Red("needs is empty"),
        ),
        row(
            "baseline failed",
            nj("failure", OUTS_NONE, &OK),
            &[],
            &[],
            Expect::Red("baseline job 'baseline' result is 'failure'"),
        ),
        row(
            "baseline skipped",
            nj("skipped", OUTS_NONE, &OK),
            &[],
            &[],
            Expect::Red("baseline job 'baseline' result is 'skipped'"),
        ),
        row(
            "baseline absent",
            r#"{"build":{"result":"success"}}"#.to_string(),
            &[],
            &[],
            Expect::Red("baseline job 'baseline' is absent"),
        ),
        row(
            "selected-class skip",
            nj("success", outs_build, &with("build", "skipped")),
            &[],
            &[],
            Expect::Red("class 'build' was selected"),
        ),
        row(
            "unselected-class skip",
            nj("success", OUTS_NONE, &with("build", "skipped")),
            &[],
            &[],
            Expect::Green,
        ),
        row(
            "class output missing counts as selected",
            nj("success", outs_missing, &with("build", "skipped")),
            &[],
            &[],
            Expect::Red("class 'build' was selected"),
        ),
        row(
            "never-skip job skipped",
            nj("success", OUTS_NONE, &with("test", "skipped")),
            &["test"],
            &[],
            Expect::Red("never-skip"),
        ),
        row(
            "run_all with skip",
            nj("success", outs_all, &with("build", "skipped")),
            &[],
            &[],
            Expect::Red("run_all is true"),
        ),
        row(
            "skip with no class",
            nj("success", OUTS_NONE, &with("clippy", "skipped")),
            &[],
            &[],
            Expect::Red("class cannot be determined"),
        ),
        row(
            "unknown result",
            nj("success", OUTS_NONE, &with("test", "weird")),
            &[],
            &[],
            Expect::Red("unknown result 'weird'"),
        ),
        row(
            "advisory failure",
            nj("success", OUTS_NONE, &with("fuzz", "failure")),
            &[],
            &["fuzz"],
            Expect::Green,
        ),
        row(
            "advisory skipped despite selected class",
            nj("success", outs_build, &all("skipped")),
            &[],
            &["build", "test", "fuzz", "clippy"],
            Expect::Green,
        ),
        row(
            "advisory job missing from needs",
            nj("success", OUTS_NONE, &with("fuzz", "success")[..3]),
            &[],
            &["clippy"],
            Expect::Green,
        ),
        row(
            "job missing from needs",
            nj("success", OUTS_NONE, &missing),
            &[],
            &[],
            Expect::Red("job 'test' is in the workflow but missing from needs"),
        ),
        row(
            "extra needs key",
            nj("success", OUTS_NONE, &extra),
            &[],
            &[],
            Expect::Red("needs key 'ghost' is not a job in the workflow"),
        ),
    ];
    // The advisory exemption must not hide a failed non-advisory job.
    rows.push(row(
        "failure outside the advisory list",
        nj("success", OUTS_NONE, &with("build", "failure")),
        &[],
        &["fuzz"],
        Expect::Red("job 'build' result is failure"),
    ));
    let bad_wf = |name, wf: &str| Row {
        name,
        needs: nj("success", OUTS_NONE, &OK),
        workflow: wf.to_string(),
        never_skip: vec![],
        advisory: vec![],
        delegated: vec![],
        expect: Expect::CouldNotRun,
    };
    rows.push(bad_wf(
        "workflow with a merge key",
        "jobs:\n  a: &a\n    runs-on: x\n  baseline:\n    <<: *a\n",
    ));
    rows.push(bad_wf("workflow without jobs", "name: CI\non: push\n"));
    rows.push(bad_wf("workflow that is not YAML", "jobs: [unclosed\n"));
    rows.extend(delegation_rows());
    rows
}

/// Caller workflow with a delegated `kani` job covered by the `proofs`
/// aggregator; `proofs_needs` is the aggregator's own `needs:` line.
fn workflow_d(proofs_needs: &str) -> String {
    format!(
        "\
name: CI
on: pull_request
jobs:
  baseline:
    uses: pulseengine/jig/.github/workflows/baseline.yml@0123456789abcdef0123456789abcdef01234567
  build:
    needs: baseline
    if: ${{{{ !cancelled() && (needs.baseline.result != 'success' || needs.baseline.outputs.build != 'false') }}}}
    runs-on: ubuntu-24.04
  kani:
    runs-on: ubuntu-24.04
  proofs:
{proofs_needs}    if: always()
    runs-on: ubuntu-24.04
  required:
    needs: [baseline, build, proofs]
    if: always()
    runs-on: ubuntu-24.04
"
    )
}

/// Delegation rows (REQ-015).
fn delegation_rows() -> Vec<Row> {
    let seq = workflow_d("    needs: [kani]\n");
    let scalar = workflow_d("    needs: kani\n");
    let unlisted = workflow_d("");
    let d =
        |name, needs: String, wf: &str, never: &[&'static str], del: &[&'static str], expect| Row {
            name,
            needs,
            workflow: wf.to_string(),
            never_skip: never.to_vec(),
            advisory: vec![],
            delegated: del.to_vec(),
            expect,
        };
    let base = [("build", "success"), ("proofs", "success")];
    let with_kani = |res: &'static str| {
        let mut v = base.to_vec();
        v.push(("kani", res));
        v
    };
    let kd = &["kani=proofs"];
    let np = &["proofs"];
    vec![
        d(
            "delegated job absent from needs, aggregator ok",
            nj("success", OUTS_NONE, &base),
            &seq,
            np,
            kd,
            Expect::Green,
        ),
        d(
            "delegated job's failure is not judged",
            nj("success", OUTS_NONE, &with_kani("failure")),
            &seq,
            np,
            kd,
            Expect::Green,
        ),
        d(
            "delegated job's skip is not judged",
            nj("success", OUTS_NONE, &with_kani("skipped")),
            &scalar,
            np,
            kd,
            Expect::Green,
        ),
        d(
            "delegated job with an unknown result",
            nj("success", OUTS_NONE, &with_kani("weird")),
            &seq,
            np,
            kd,
            Expect::Red("job 'kani' has unknown result 'weird'"),
        ),
        d(
            "delegated skipped elsewhere, aggregator does not list it",
            nj("success", OUTS_NONE, &with_kani("skipped")),
            &unlisted,
            np,
            kd,
            Expect::Red("does not list delegated job 'kani'"),
        ),
        d(
            "aggregator absent from needs",
            nj("success", OUTS_NONE, &[("build", "success")]),
            &seq,
            np,
            kd,
            Expect::Red("aggregator 'proofs' of delegated job 'kani' is absent from needs"),
        ),
        d(
            "aggregator failed",
            nj(
                "success",
                OUTS_NONE,
                &[("build", "success"), ("proofs", "failure")],
            ),
            &seq,
            np,
            kd,
            Expect::Red("aggregator 'proofs' of delegated job 'kani' result is 'failure'"),
        ),
        d(
            "aggregator skipped",
            nj(
                "success",
                OUTS_NONE,
                &[("build", "success"), ("proofs", "skipped")],
            ),
            &seq,
            np,
            kd,
            Expect::Red("result is 'skipped', not success"),
        ),
        d(
            "aggregator not in never-skip",
            nj("success", OUTS_NONE, &base),
            &seq,
            &[],
            kd,
            Expect::Red("is not in never-skip"),
        ),
        d(
            "delegated job not in the workflow",
            nj("success", OUTS_NONE, &base),
            &seq,
            np,
            &["ghost=proofs"],
            Expect::Red("delegated job 'ghost' is not a job in the workflow"),
        ),
        d(
            "job delegated to itself",
            nj("success", OUTS_NONE, &with_kani("success")),
            &seq,
            &["kani", "proofs"],
            &["kani=kani"],
            Expect::Red("job 'kani' is delegated to itself"),
        ),
        d(
            "aggregator is itself delegated",
            nj("success", OUTS_NONE, &base),
            &seq,
            np,
            &["kani=proofs", "proofs=build"],
            Expect::Red("aggregator 'proofs' of 'kani' is itself delegated"),
        ),
        d(
            "malformed delegation pair",
            nj("success", OUTS_NONE, &base),
            &seq,
            np,
            &["kani"],
            Expect::CouldNotRun,
        ),
        d(
            "non-delegated failure still red beside a delegation",
            nj(
                "success",
                OUTS_NONE,
                &[("build", "failure"), ("proofs", "success")],
            ),
            &seq,
            np,
            kd,
            Expect::Red("job 'build' result is failure"),
        ),
    ]
}

/// Run the built-in table; `Err` carries the 1-based numbers that disagree.
pub fn self_test() -> Result<usize, Vec<usize>> {
    let rows = table();
    let bad = disagreeing(&rows);
    if bad.is_empty() {
        Ok(rows.len())
    } else {
        Err(bad)
    }
}
