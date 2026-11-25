//! `jig lockfiles`: every Cargo workspace has a committed lock, or is listed
//! in the repository's lockfile census with an issue and a reason (REQ-025).
//!
//! The judgement is pure: the caller (main.rs) supplies the tracked paths,
//! each tracked manifest's workspace root as cargo resolved it, and which
//! paths exist on disk. The census format is gale's
//! `tools/xtask/lockfile-census.toml`: one `[[lockfree]]` table per manifest
//! with `manifest`, `issue` and `reason`, no wildcards, and a listing that
//! goes stale fails.

use std::collections::{BTreeMap, BTreeSet};

/// One census listing, as read.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub manifest: String,
    pub issue: String,
    pub reason: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// Distinct workspace roots found.
    pub workspaces: usize,
    /// Workspace manifests accepted as lock-free through the census.
    pub exempt: Vec<String>,
    /// Census listings jig does not judge (not a Cargo.toml).
    pub not_judged: Vec<String>,
    /// One line per violation; empty means green.
    pub reasons: Vec<String>,
}

/// Tracked `Cargo.toml` files outside any `target/` directory.
pub fn manifests(tracked: &BTreeSet<String>) -> Vec<String> {
    tracked
        .iter()
        .filter(|p| p.rsplit('/').next() == Some("Cargo.toml"))
        .filter(|p| !p.split('/').any(|c| c == "target"))
        .cloned()
        .collect()
}

fn issue_ok(s: &str) -> bool {
    let num = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    let name = |n: &str| {
        !n.is_empty()
            && n.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if let Some(n) = s.strip_prefix('#') {
        return num(n);
    }
    match s.split_once('#') {
        Some((repo, n)) => match repo.split_once('/') {
            Some((owner, r)) => name(owner) && name(r) && num(n),
            None => false,
        },
        None => false,
    }
}

/// Parse the census. A file that is not TOML is `Err` (could not run); a
/// listing that is malformed is returned in the second vector as a reason.
pub fn parse_census(path: &str, text: &str) -> Result<(Vec<Listing>, Vec<String>), String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("{path}: not TOML: {e}"))?;
    let mut out = Vec::new();
    let mut bad = Vec::new();
    for key in doc.keys().filter(|k| *k != "lockfree") {
        bad.push(format!(
            "{path}: unknown top-level key '{key}' (only [[lockfree]] tables)"
        ));
    }
    let Some(list) = doc.get("lockfree") else {
        return Ok((out, bad));
    };
    let Some(list) = list.as_array() else {
        bad.push(format!("{path}: lockfree is not an array of tables"));
        return Ok((out, bad));
    };
    for (i, item) in list.iter().enumerate() {
        let n = i + 1;
        let Some(t) = item.as_table() else {
            bad.push(format!("{path}: lockfree entry {n} is not a table"));
            continue;
        };
        for k in t.keys() {
            if !["manifest", "issue", "reason"].contains(&k.as_str()) {
                bad.push(format!("{path}: lockfree entry {n}: unknown key '{k}'"));
            }
        }
        let field = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::trim);
        let (Some(m), Some(is), Some(r)) = (field("manifest"), field("issue"), field("reason"))
        else {
            bad.push(format!(
                "{path}: lockfree entry {n}: manifest, issue and reason are required strings"
            ));
            continue;
        };
        let mut ok = true;
        if m.is_empty() || r.is_empty() {
            bad.push(format!(
                "{path}: lockfree entry {n}: empty manifest or reason"
            ));
            ok = false;
        }
        if m.chars().any(|c| "*?[]{}".contains(c)) {
            bad.push(format!(
                "{path}: lockfree entry {n}: '{m}' is a wildcard; list each manifest"
            ));
            ok = false;
        }
        if m.starts_with('/') || m.starts_with("./") {
            bad.push(format!(
                "{path}: lockfree entry {n}: '{m}' must be the path git ls-files prints"
            ));
            ok = false;
        }
        if !issue_ok(is) {
            bad.push(format!(
                "{path}: lockfree entry {n}: issue '{is}' is not #N or owner/repo#N"
            ));
            ok = false;
        }
        if ok {
            out.push(Listing {
                manifest: m.to_string(),
                issue: is.to_string(),
                reason: r.to_string(),
            });
        }
    }
    Ok((out, bad))
}

fn dir_of(manifest: &str) -> &str {
    manifest.rsplit_once('/').map_or("", |(d, _)| d)
}

fn lock_of(root: &str) -> String {
    match dir_of(root) {
        "" => "Cargo.lock".to_string(),
        d => format!("{d}/Cargo.lock"),
    }
}

/// Judge the tree.
///
/// `roots` maps each tracked manifest to the repository-relative path of its
/// workspace root manifest, or to cargo's error. `exists` says whether a
/// repository-relative path is a file on disk.
pub fn check(
    tracked: &BTreeSet<String>,
    roots: &BTreeMap<String, Result<String, String>>,
    exists: &dyn Fn(&str) -> bool,
    census: Option<(&str, &str)>,
) -> Result<Report, String> {
    let mut rep = Report::default();
    let mut ws: BTreeSet<String> = BTreeSet::new();
    for (m, r) in roots {
        match r {
            Ok(root) => {
                ws.insert(root.clone());
            }
            Err(e) => rep.reasons.push(format!(
                "{m}: cargo cannot resolve its workspace: {}",
                e.trim()
            )),
        }
    }
    rep.workspaces = ws.len();
    let committed = |lock: &str| tracked.contains(lock) && exists(lock);

    let (listings, bad, cpath) = match census {
        Some((p, text)) => {
            let (l, b) = parse_census(p, text)?;
            (l, b, p)
        }
        None => (Vec::new(), Vec::new(), ""),
    };
    rep.reasons.extend(bad);

    let mut listed: BTreeMap<&str, usize> = BTreeMap::new();
    for l in &listings {
        *listed.entry(l.manifest.as_str()).or_default() += 1;
    }
    for (m, n) in &listed {
        if *n > 1 {
            rep.reasons
                .push(format!("{cpath}: '{m}' is listed {n} times; list it once"));
        }
    }
    for m in listed.keys() {
        let m = *m;
        if !tracked.contains(m) {
            rep.reasons
                .push(format!("{cpath}: '{m}' is stale: it is not a tracked file"));
            continue;
        }
        if m.rsplit('/').next() != Some("Cargo.toml") {
            rep.not_judged.push(m.to_string());
            continue;
        }
        if !ws.contains(m) {
            let owner = roots
                .get(m)
                .and_then(|r| r.as_ref().ok())
                .map_or("unknown", String::as_str);
            rep.reasons.push(format!(
                "{cpath}: '{m}' is stale: it is not a workspace root (its workspace is {owner})"
            ));
            continue;
        }
        if committed(&lock_of(m)) {
            rep.reasons.push(format!(
                "{cpath}: '{m}' is stale: its workspace has a committed {}",
                lock_of(m)
            ));
        }
    }

    for root in &ws {
        let lock = lock_of(root);
        if committed(&lock) {
            continue;
        }
        if listed.contains_key(root.as_str()) {
            rep.exempt.push(root.clone());
        } else if census.is_some() {
            rep.reasons.push(format!(
                "{lock}: no committed Cargo.lock for the workspace {root} (commit it, or list {root} in {cpath} with an issue and a reason)"
            ));
        } else {
            rep.reasons.push(format!(
                "{lock}: no committed Cargo.lock for the workspace {root}"
            ));
        }
    }
    Ok(rep)
}
