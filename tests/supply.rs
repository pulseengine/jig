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
