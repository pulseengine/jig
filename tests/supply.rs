// rivet: verifies REQ-007
//! baseline.yml's "npm lockfiles" step and ci-controls/npm-lockfiles.sh must
//! be the same text: the supply-npm-advisory control runs the script, so a
//! drift would leave the workflow's check unproven.

use std::fs;

fn step_body(workflow: &str, step: &str) -> String {
    let lines: Vec<&str> = workflow.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim() == format!("- name: {step}"))
        .unwrap_or_else(|| panic!("step '{step}' not found"));
    // The step's own keys (if:, env:, ...) sit at 8 spaces before run: |.
    let run = start
        + 1
        + lines[start + 1..]
            .iter()
            .take_while(|l| l.starts_with("        ") && !l.trim_start().starts_with("- "))
            .position(|l| l.trim() == "run: |")
            .unwrap_or_else(|| panic!("step '{step}' has no run block"));
    let body: Vec<&str> = lines[run + 1..]
        .iter()
        .take_while(|l| l.is_empty() || l.starts_with("          "))
        .map(|l| l.strip_prefix("          ").unwrap_or(l))
        .collect();
    body.join("\n").trim_end().to_string()
}

#[test]
fn npm_step_matches_control_script() {
    let wf = fs::read_to_string(".github/workflows/baseline.yml").unwrap();
    let script = fs::read_to_string("ci-controls/npm-lockfiles.sh").unwrap();
    let mut it = script.lines();
    assert_eq!(it.next(), Some("#!/usr/bin/env bash"));
    assert_eq!(it.next(), Some("set -eo pipefail"));
    let script_body = it.collect::<Vec<_>>().join("\n").trim_end().to_string();
    assert_eq!(step_body(&wf, "npm lockfiles"), script_body);
}

fn script_body(path: &str) -> String {
    let script = fs::read_to_string(path).unwrap();
    let mut it = script.lines();
    assert_eq!(it.next(), Some("#!/usr/bin/env bash"));
    assert_eq!(it.next(), Some("set -eo pipefail"));
    it.collect::<Vec<_>>().join("\n").trim_end().to_string()
}

// rivet: verifies REQ-025
#[test]
fn cargo_lockfiles_step_matches_control_script() {
    let wf = fs::read_to_string(".github/workflows/baseline.yml").unwrap();
    assert_eq!(
        step_body(&wf, "cargo lockfiles are fresh"),
        script_body("ci-controls/cargo-lockfiles.sh")
    );
}

// The audit step installs its tool first; the control runs what follows.
// rivet: verifies REQ-025
#[test]
fn cargo_audit_step_matches_control_script() {
    let wf = fs::read_to_string(".github/workflows/baseline.yml").unwrap();
    let body = step_body(&wf, "cargo audit");
    let (_, after) = body
        .split_once("\nesac\n")
        .expect("cargo audit step has no install case");
    assert_eq!(after, script_body("ci-controls/cargo-audit.sh"));
}

// rivet: verifies REQ-026
#[test]
fn private_names_step_matches_control_script() {
    let wf = fs::read_to_string(".github/workflows/baseline.yml").unwrap();
    assert_eq!(
        step_body(&wf, "private names"),
        script_body("ci-controls/names.sh")
    );
}

// rivet: verifies REQ-027
#[test]
fn confined_probe_step_matches_control_script() {
    let wf = fs::read_to_string(".github/workflows/baseline.yml").unwrap();
    assert_eq!(
        step_body(&wf, "confined probe"),
        script_body("ci-controls/confined.sh")
    );
    // The probe binary is always the pinned release, checked by sha256, never
    // built from the code under test in the job it judges.
    let fetch = step_body(&wf, "fetch pinned jig for the confined probe");
    assert!(!fetch.contains("cargo"), "{fetch}");
    assert!(!fetch.contains("FROM_SOURCE"), "{fetch}");
    assert!(fetch.contains("sha256sum -c -"), "{fetch}");
    assert!(!fetch.contains("GITHUB_PATH"), "{fetch}");
    // Called by absolute path with the dynamic loader variables cleared.
    let lines: Vec<&str> = wf.lines().collect();
    let probe = lines
        .iter()
        .position(|l| l.trim() == "- name: confined probe")
        .unwrap();
    let keys: Vec<&str> = lines[probe + 1..]
        .iter()
        .take_while(|l| l.trim() != "run: |")
        .map(|l| l.trim())
        .collect();
    for want in [
        "JIG_PROBE: ${{ runner.temp }}/jig-probe/jig",
        "LD_PRELOAD: \"\"",
        "LD_LIBRARY_PATH: \"\"",
    ] {
        assert!(keys.contains(&want), "missing {want}: {keys:?}");
    }
}

/// The probe and the label move together: "confined" in the guard job's
/// runs-on if and only if JIG_CONFINED is "true". The probe skips only on a
/// GitHub-hosted runner (runner.environment), never on a missing ZWINGER_*
/// label, so an unconfined self-hosted run still goes red.
// rivet: verifies REQ-027
#[test]
fn confined_label_and_probe_flag_are_paired() {
    let wf = fs::read_to_string(".github/workflows/baseline.yml").unwrap();
    let flag = wf
        .lines()
        .find_map(|l| l.trim().strip_prefix("JIG_CONFINED: "))
        .expect("JIG_CONFINED is set at workflow level");
    let on = match flag {
        "\"true\"" => true,
        "\"false\"" => false,
        other => panic!("JIG_CONFINED must be \"true\" or \"false\", not {other}"),
    };
    let lines: Vec<&str> = wf.lines().collect();
    let guard = lines.iter().position(|l| *l == "  guard:").unwrap();
    let runs_on = lines[guard..]
        .iter()
        .find(|l| l.trim_start().starts_with("runs-on:"))
        .unwrap();
    assert_eq!(runs_on.contains("\"confined\""), on, "{runs_on}");
    let probe = lines
        .iter()
        .position(|l| l.trim() == "- name: confined probe")
        .unwrap();
    let cond = lines[probe + 1].trim();
    assert_eq!(
        cond,
        "if: env.JIG_CONFINED == 'true' && runner.environment == 'self-hosted'"
    );
    assert!(
        !wf.contains("if: env.ZWINGER"),
        "a ZWINGER_* label must never gate the probe"
    );
}
