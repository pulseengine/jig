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
    assert_eq!(
        lines[start + 1].trim(),
        "run: |",
        "step '{step}' has no run block"
    );
    let body: Vec<&str> = lines[start + 2..]
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
