//! `jig caller` through the binary (REQ-017).

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("jig-caller-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Dir(d)
}

fn caller(d: &Dir, wf: &str, extra: &[&str]) -> Output {
    fs::write(d.0.join("ci.yml"), wf).unwrap();
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["caller", "--workflow"])
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

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA2: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn wf(on: &str, req_if: &str, req_needs: &str, base_uses: &str, req_uses: &str) -> String {
    format!(
        "\
name: CI
{on}
permissions:
  contents: read
jobs:
  baseline:
    uses: {base_uses}
  build:
    needs: baseline
    runs-on: x
  lint:
    runs-on: x
  required:
    needs: {req_needs}
    if: {req_if}
    uses: {req_uses}
"
    )
}

fn base_uses() -> String {
    format!("pulseengine/jig/.github/workflows/baseline.yml@{SHA}")
}

fn req_uses() -> String {
    format!("pulseengine/jig/.github/workflows/required.yml@{SHA}")
}

fn good_on() -> &'static str {
    "on:\n  pull_request:\n  merge_group:\n    types: [checks_requested]"
}

fn good() -> String {
    wf(
        good_on(),
        "always()",
        "[baseline, build, lint]",
        &base_uses(),
        &req_uses(),
    )
}

fn assert_red(d: &Dir, w: &str, want: &str) {
    let o = caller(d, w, &[]);
    assert_eq!(code(&o), 1, "{w}\n{}", stdout(&o));
    assert!(stdout(&o).contains(want), "want '{want}' in {}", stdout(&o));
}

// rivet: verifies REQ-017
#[test]
fn a_well_formed_caller_is_green() {
    let d = dir("good");
    let o = caller(&d, &good(), &[]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    for cond in [
        "${{ always() }}",
        "${{always()}}",
        "always( )",
        "'always()'",
    ] {
        let w = wf(
            good_on(),
            cond,
            "[baseline, build, lint]",
            &base_uses(),
            &req_uses(),
        );
        assert_eq!(code(&caller(&d, &w, &[])), 0, "{cond}");
    }
}

// rivet: verifies REQ-017
#[test]
fn on_forms_scalar_sequence_and_mapping_are_all_read() {
    let d = dir("onforms");
    let mk = |on: &str| {
        wf(
            on,
            "always()",
            "[baseline, build, lint]",
            &base_uses(),
            &req_uses(),
        )
    };
    assert_eq!(
        code(&caller(&d, &mk("on: [pull_request, merge_group]"), &[])),
        0
    );
    assert_eq!(
        code(&caller(
            &d,
            &mk("on:\n  - pull_request\n  - merge_group"),
            &[]
        )),
        0
    );
    // a scalar `on` has one trigger, so the other is lacking
    assert_red(&d, &mk("on: pull_request"), "on: lacks merge_group");
    assert_red(&d, &mk("on: merge_group"), "on: lacks pull_request");
    assert_red(&d, &mk("on: [push]"), "on: lacks pull_request");
    assert_red(
        &d,
        &mk("on:\n  push:\n  merge_group:"),
        "on: lacks pull_request",
    );
    assert_red(
        &d,
        "jobs:\n  required:\n    if: always()\n",
        "no on: trigger",
    );
}

// rivet: verifies REQ-017
#[test]
fn filters_on_pull_request_and_merge_group_are_red() {
    let d = dir("filters");
    for f in [
        "paths: ['src/**']",
        "paths-ignore: ['docs/**']",
        "branches: [main]",
        "branches-ignore: [wip]",
        "types: [opened]",
    ] {
        let key = f.split(':').next().unwrap();
        let on = format!("on:\n  pull_request:\n    {f}\n  merge_group:");
        let w = wf(
            &on,
            "always()",
            "[baseline, build, lint]",
            &base_uses(),
            &req_uses(),
        );
        assert_red(&d, &w, &format!("on.pull_request has a {key} filter"));
        let on = format!("on:\n  pull_request:\n  merge_group:\n    {f}");
        let w = wf(
            &on,
            "always()",
            "[baseline, build, lint]",
            &base_uses(),
            &req_uses(),
        );
        assert_red(&d, &w, &format!("on.merge_group has a {key} filter"));
    }
    // only merge_group types: [checks_requested] is allowed
    let on = "on:\n  pull_request:\n  merge_group:\n    types: [checks_requested, other]";
    let w = wf(
        on,
        "always()",
        "[baseline, build, lint]",
        &base_uses(),
        &req_uses(),
    );
    assert_red(&d, &w, "on.merge_group has a types filter");
    let on = "on:\n  pull_request:\n    types: [checks_requested]\n  merge_group:";
    let w = wf(
        on,
        "always()",
        "[baseline, build, lint]",
        &base_uses(),
        &req_uses(),
    );
    assert_red(&d, &w, "on.pull_request has a types filter");
    let on = "on:\n  pull_request:\n  merge_group:\n    types: checks_requested";
    let w = wf(
        on,
        "always()",
        "[baseline, build, lint]",
        &base_uses(),
        &req_uses(),
    );
    assert_eq!(code(&caller(&d, &w, &[])), 0);
}

// rivet: verifies REQ-017
#[test]
fn the_required_job_must_be_always_and_need_everything() {
    let d = dir("required");
    let mk = |c: &str, n: &str| wf(good_on(), c, n, &base_uses(), &req_uses());
    assert_red(
        &d,
        &mk("success()", "[baseline, build, lint]"),
        "not always()",
    );
    assert_red(
        &d,
        &mk("${{ !cancelled() }}", "[baseline, build, lint]"),
        "not always()",
    );
    assert_red(
        &d,
        &mk("always() && true", "[baseline, build, lint]"),
        "not always()",
    );
    assert_red(
        &d,
        &mk("always()", "[baseline, build]"),
        "misses job 'lint'",
    );
    assert_red(&d, &mk("always()", "baseline"), "misses job 'build'");
    // no if at all
    let w = good().replace("    if: always()\n", "");
    assert_red(&d, &w, "if: is absent");
    // no needs at all
    let w = good().replace("    needs: [baseline, build, lint]\n", "");
    assert_red(&d, &w, "misses job 'baseline'");
    // missing required job
    let w = "on: [pull_request, merge_group]\njobs:\n  baseline:\n    uses: x\n";
    assert_red(&d, w, "required job 'required' is missing");
}

// rivet: verifies REQ-017
#[test]
fn baseline_and_required_must_use_the_same_pinned_sha() {
    let d = dir("shas");
    let mk = |b: &str, r: &str| wf(good_on(), "always()", "[baseline, build, lint]", b, r);
    let b = |s: &str| format!("pulseengine/jig/.github/workflows/baseline.yml@{s}");
    let r = |s: &str| format!("pulseengine/jig/.github/workflows/required.yml@{s}");
    assert_red(&d, &mk(&b(SHA), &r(SHA2)), "different SHAs");
    assert_red(&d, &mk(&b("main"), &r(SHA)), "baseline.yml@<40 hex>");
    assert_red(
        &d,
        &mk(&b(&SHA.to_uppercase()), &r(SHA)),
        "baseline.yml@<40 hex>",
    );
    assert_red(&d, &mk(&b(&SHA[..39]), &r(SHA)), "baseline.yml@<40 hex>");
    assert_red(&d, &mk(&b(SHA), &r("v1")), "required.yml@<40 hex>");
    assert_red(
        &d,
        &mk(&format!("./.github/workflows/baseline.yml@{SHA}"), &r(SHA)),
        "baseline.yml@<40 hex>",
    );
    assert_red(
        &d,
        &mk(
            &format!("pulseengine/jig/.github/workflows/other.yml@{SHA}"),
            &r(SHA),
        ),
        "baseline.yml@<40 hex>",
    );
    // a fork or another org is fine: the path is only required to end right
    assert_eq!(
        code(&caller(
            &d,
            &mk(
                &format!("acme/ci/.github/workflows/baseline.yml@{SHA}"),
                &format!("acme/ci/.github/workflows/required.yml@{SHA}"),
            ),
            &[]
        )),
        0
    );
    // missing baseline job
    let w = good().replace("  baseline:\n    uses:", "  other:\n    uses:");
    assert_red(&d, &w, "baseline job 'baseline' is missing");
}

// rivet: verifies REQ-017
#[test]
fn pull_request_target_is_red_in_every_form() {
    let d = dir("prt");
    let mk = |on: &str| {
        wf(
            on,
            "always()",
            "[baseline, build, lint]",
            &base_uses(),
            &req_uses(),
        )
    };
    for on in [
        "on:\n  pull_request:\n  merge_group:\n  pull_request_target:",
        "on: [pull_request, merge_group, pull_request_target]",
        "on:\n  - pull_request\n  - merge_group\n  - pull_request_target",
    ] {
        assert_red(&d, &mk(on), "pull_request_target");
    }
    assert_red(&d, &mk("on: pull_request_target"), "pull_request_target");
}

// rivet: verifies REQ-017
#[test]
fn custom_job_ids_are_honoured() {
    let d = dir("ids");
    let w = good()
        .replace("required:", "gate:")
        .replace("baseline:", "base:")
        .replace("[baseline, build, lint]", "[base, build, lint]")
        .replace("needs: baseline", "needs: base");
    let o = caller(
        &d,
        &w,
        &["--required-job", "gate", "--baseline-job", "base"],
    );
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    assert_eq!(code(&caller(&d, &w, &[])), 1);
}

// rivet: verifies REQ-017
#[test]
fn unreadable_or_unparseable_workflow_is_exit_2() {
    let d = dir("exit2");
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["caller", "--workflow"])
        .arg(d.0.join("missing.yml"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
    assert_eq!(code(&caller(&d, "jobs: [unclosed\n", &[])), 2);
    assert_eq!(code(&caller(&d, "on: x\n", &[])), 2);
    let merged = good().replace("  lint:\n", "  lint: &l\n") + "  other:\n    <<: *l\n";
    assert_eq!(code(&caller(&d, &merged, &[])), 2);
    assert_eq!(code(&caller(&d, "- a\n- b\n", &[])), 2);
}

// rivet: verifies REQ-017
#[test]
fn json_output_and_docs_topic() {
    let d = dir("json");
    let w = good().replace("always()", "success()");
    let o = caller(&d, &w, &["--format", "json"]);
    assert_eq!(code(&o), 1);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["verdict"], "red");
    assert!(v["reasons"][0].as_str().unwrap().contains("always()"));
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["docs", "caller"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 0);
    assert!(stdout(&o).starts_with("jig caller:"));
}

// rivet: verifies REQ-017
#[test]
fn local_calls_pass_only_with_allow_local_and_never_mixed() {
    let lb = "./.github/workflows/baseline.yml";
    let lr = "./.github/workflows/required.yml";
    let needs = "[baseline, build, lint]";
    let d = dir("local-calls");
    // Without the flag a local call is red.
    let w = wf(good_on(), "always()", needs, lb, lr);
    assert_eq!(code(&caller(&d, &w, &[])), 1);
    // With it, both local is green.
    let o = caller(&d, &w, &["--allow-local"]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    // One local and one pinned remote is red: two jig versions in one verdict.
    let mixed = wf(good_on(), "always()", needs, lb, &req_uses());
    let o = caller(&d, &mixed, &["--allow-local"]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("different SHAs"), "{}", stdout(&o));
    // A local path with a ref, or another local file, stays red.
    let refd = wf(
        good_on(),
        "always()",
        needs,
        "./.github/workflows/baseline.yml@main",
        lr,
    );
    assert_eq!(code(&caller(&d, &refd, &["--allow-local"])), 1);
    let other = wf(
        good_on(),
        "always()",
        needs,
        "./.github/workflows/other.yml",
        lr,
    );
    assert_eq!(code(&caller(&d, &other, &["--allow-local"])), 1);
}

/// good() with its `permissions:` block replaced (`None` drops it).
fn with_perms(perms: Option<&str>) -> String {
    let g = good();
    let block = "permissions:\n  contents: read\n";
    assert!(g.contains(block));
    g.replace(block, &perms.map_or(String::new(), |p| format!("{p}\n")))
}

// rivet: verifies REQ-022
#[test]
fn workflow_permissions_must_be_read_only() {
    let d = dir("perms");
    for p in [
        "permissions: read-all",
        "permissions: {}",
        "permissions:\n  contents: read\n  pull-requests: read",
        "permissions:\n  contents: read\n  id-token: none",
    ] {
        let o = caller(&d, &with_perms(Some(p)), &[]);
        assert_eq!(code(&o), 0, "{p}\n{}", stdout(&o));
    }
    assert_red(&d, &with_perms(None), "no top-level permissions:");
    assert_red(
        &d,
        &with_perms(Some("permissions: write-all")),
        "permissions: is 'write-all'",
    );
    assert_red(
        &d,
        &with_perms(Some("permissions: ${{ inputs.p }}")),
        "permissions: is '${{ inputs.p }}'",
    );
    assert_red(
        &d,
        &with_perms(Some("permissions:\n  contents: write")),
        "permissions.contents is 'write'",
    );
    assert_red(
        &d,
        &with_perms(Some("permissions:\n  contents: read\n  id-token: write")),
        "permissions.id-token is 'write'",
    );
    assert_red(
        &d,
        &with_perms(Some("permissions:\n  - contents")),
        "permissions: is a sequence",
    );
}

// rivet: verifies REQ-022
#[test]
fn job_level_permissions_are_not_judged() {
    // Documented blind spot: a job may still widen its own token.
    let d = dir("jobperms");
    let w = good().replace(
        "  lint:\n    runs-on: x\n",
        "  lint:\n    runs-on: x\n    permissions:\n      contents: write\n",
    );
    assert_ne!(w, good());
    assert_eq!(code(&caller(&d, &w, &[])), 0);
}
