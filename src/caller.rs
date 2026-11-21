//! `jig caller`: the shape of a caller workflow (REQ-017).
//!
//! A required check that can be skipped, filtered away or run from a fork
//! trigger is not a gate. Every finding is red; unreadable input is exit 2.

use crate::yaml::{self, Node};

const FILTERS: [&str; 5] = [
    "paths",
    "paths-ignore",
    "branches",
    "branches-ignore",
    "types",
];
const TRIGGERS: [&str; 2] = ["pull_request", "merge_group"];

#[derive(Debug, Clone)]
pub struct Opts {
    pub required_job: String,
    pub baseline_job: String,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            required_job: "required".into(),
            baseline_job: "baseline".into(),
        }
    }
}

type Trigger<'a> = (String, Option<&'a Node>);

/// The triggers of `on:` with each trigger's value, if the form has one.
fn triggers(on: &Node) -> Result<Vec<Trigger<'_>>, String> {
    match on {
        Node::Scalar { value, .. } => Ok(vec![(value.clone(), None)]),
        Node::Seq { items, .. } => items
            .iter()
            .map(|i| match i {
                Node::Scalar { value, .. } => Ok((value.clone(), None)),
                _ => Err("on: has a non-scalar item".to_string()),
            })
            .collect(),
        Node::Map { entries, .. } => Ok(entries
            .iter()
            .map(|e| (e.key.clone(), Some(&e.value)))
            .collect()),
    }
}

/// Scalar or sequence of scalars as a list of strings.
fn string_list(n: &Node, what: &str) -> Result<Vec<String>, String> {
    match n {
        Node::Scalar { value, .. } => Ok(vec![value.clone()]),
        Node::Seq { items, .. } => items
            .iter()
            .map(|i| {
                i.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{what} has a non-scalar item"))
            })
            .collect(),
        Node::Map { .. } => Err(format!("{what} is a mapping")),
    }
}

fn is_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `Some(sha)` when `uses` is `<path>/.github/workflows/<file>@<40 hex>`.
fn pinned_sha<'a>(uses: &'a str, file: &str) -> Option<&'a str> {
    let (path, sha) = uses.rsplit_once('@')?;
    let want = format!("/.github/workflows/{file}");
    // The part before the workflow path must name a repository (`owner/repo`),
    // not a local `./` path.
    let repo = path.strip_suffix(&want)?;
    (repo.contains('/') && !repo.starts_with('.') && is_sha(sha)).then_some(sha)
}

fn check_on(on: &Node, r: &mut Vec<String>) -> Result<(), String> {
    let trig = triggers(on)?;
    for t in TRIGGERS {
        if !trig.iter().any(|(n, _)| n == t) {
            r.push(format!("on: lacks {t}"));
        }
    }
    if trig.iter().any(|(n, _)| n == "pull_request_target") {
        r.push("on: has pull_request_target; jig's controls must never run there".into());
    }
    for (name, val) in &trig {
        if !TRIGGERS.contains(&name.as_str()) {
            continue;
        }
        match val {
            None | Some(Node::Scalar { .. }) => {}
            Some(Node::Seq { .. }) => r.push(format!("on.{name} is a sequence, not a mapping")),
            Some(Node::Map { entries, .. }) => {
                for e in entries.iter().filter(|e| FILTERS.contains(&e.key.as_str())) {
                    let allowed = name == "merge_group"
                        && e.key == "types"
                        && string_list(&e.value, "types").is_ok_and(|v| v == ["checks_requested"]);
                    if !allowed {
                        r.push(format!(
                            "on.{name} has a {} filter (line {})",
                            e.key, e.line
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Every reason the workflow is red; `Err` means it could not be read.
pub fn check(text: &str, o: &Opts) -> Result<Vec<String>, String> {
    let root = yaml::parse(text).map_err(|e| format!("workflow {e}"))?;
    if !matches!(root, Node::Map { .. }) {
        return Err("workflow is not a mapping".into());
    }
    let mut r = Vec::new();

    match root.get("on") {
        None => r.push("workflow has no on: trigger".to_string()),
        Some(on) => check_on(on, &mut r)?,
    }

    let Some(Node::Map { entries: jobs, .. }) = root.get("jobs") else {
        return Err("workflow has no jobs mapping".into());
    };
    let (req, base) = (&o.required_job, &o.baseline_job);
    let mut shas: Vec<(&str, &str)> = Vec::new();

    match jobs.iter().find(|e| e.key == *req) {
        None => r.push(format!("required job '{req}' is missing")),
        Some(e) => {
            let cond = match e.value.get("if") {
                None => None,
                Some(n) => Some(
                    n.as_str()
                        .ok_or_else(|| format!("job '{req}' has a non-scalar if:"))?,
                ),
            };
            let norm: Option<String> =
                cond.map(|c| c.chars().filter(|c| !c.is_whitespace()).collect());
            if !matches!(norm.as_deref(), Some("always()" | "${{always()}}")) {
                r.push(format!(
                    "required job '{req}' if: is {}, not always()",
                    cond.map_or("absent".to_string(), |c| format!("'{}'", c.trim()))
                ));
            }
            let needs = match e.value.get("needs") {
                None => Vec::new(),
                Some(n) => string_list(n, &format!("job '{req}' needs:"))?,
            };
            for j in jobs.iter().filter(|j| j.key != *req) {
                if !needs.contains(&j.key) {
                    r.push(format!(
                        "required job '{req}' needs: misses job '{}'",
                        j.key
                    ));
                }
            }
            match e.value.get("uses").and_then(Node::as_str) {
                Some(u) => match pinned_sha(u, "required.yml") {
                    Some(sha) => shas.push((req, sha)),
                    None => r.push(format!(
                        "required job '{req}' uses '{u}', not .../.github/workflows/required.yml@<40 hex>"
                    )),
                },
                None => r.push(format!("required job '{req}' has no uses:")),
            }
        }
    }

    match jobs.iter().find(|e| e.key == *base) {
        None => r.push(format!("baseline job '{base}' is missing")),
        Some(e) => match e.value.get("uses").and_then(Node::as_str) {
            Some(u) => match pinned_sha(u, "baseline.yml") {
                Some(sha) => shas.push((base, sha)),
                None => r.push(format!(
                    "baseline job '{base}' uses '{u}', not .../.github/workflows/baseline.yml@<40 hex>"
                )),
            },
            None => r.push(format!("baseline job '{base}' has no uses:")),
        },
    }
    if let [(a, sa), (b, sb)] = shas[..]
        && sa != sb
    {
        r.push(format!("jobs '{a}' and '{b}' are pinned to different SHAs"));
    }
    Ok(r)
}
