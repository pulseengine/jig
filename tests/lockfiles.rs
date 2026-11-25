// rivet: verifies REQ-025
//! The lockfile judgement (`jig lockfiles`), on synthetic trees: tracked
//! paths, each manifest's workspace root, and the files on disk.

use std::collections::{BTreeMap, BTreeSet};

use jig::lockfiles::{Report, check, manifests};

struct Tree {
    tracked: BTreeSet<String>,
    roots: BTreeMap<String, Result<String, String>>,
    on_disk: BTreeSet<String>,
}

/// `ws` is (manifest, root) pairs; `locks` are tracked and on disk.
fn tree(ws: &[(&str, &str)], locks: &[&str], extra: &[&str]) -> Tree {
    let mut tracked = BTreeSet::new();
    let mut roots = BTreeMap::new();
    for (m, r) in ws {
        tracked.insert(m.to_string());
        roots.insert(m.to_string(), Ok(r.to_string()));
    }
    for l in locks.iter().chain(extra) {
        tracked.insert(l.to_string());
    }
    let on_disk = locks.iter().chain(extra).map(|s| s.to_string()).collect();
    Tree {
        tracked,
        roots,
        on_disk,
    }
}

fn run(t: &Tree, census: Option<&str>) -> Report {
    let exists = |p: &str| t.on_disk.contains(p);
    check(
        &t.tracked,
        &t.roots,
        &exists,
        census.map(|c| ("census.toml", c)),
    )
    .unwrap()
}

fn has(rep: &Report, s: &str) -> bool {
    rep.reasons.iter().any(|r| r.contains(s))
}

const NESTED: &[(&str, &str)] = &[
    ("Cargo.toml", "Cargo.toml"),
    ("tools/Cargo.toml", "tools/Cargo.toml"),
];

fn listing(m: &str, issue: &str, reason: &str) -> String {
    format!("[[lockfree]]\nmanifest = \"{m}\"\nissue = \"{issue}\"\nreason = \"{reason}\"\n")
}

#[test]
fn every_workspace_locked_is_green() {
    let t = tree(NESTED, &["Cargo.lock", "tools/Cargo.lock"], &[]);
    let rep = run(&t, None);
    assert!(rep.reasons.is_empty(), "{:?}", rep.reasons);
    assert_eq!(rep.workspaces, 2);
}

#[test]
fn a_member_shares_its_roots_lock() {
    let t = tree(
        &[
            ("Cargo.toml", "Cargo.toml"),
            ("crates/a/Cargo.toml", "Cargo.toml"),
        ],
        &["Cargo.lock"],
        &[],
    );
    let rep = run(&t, None);
    assert!(rep.reasons.is_empty(), "{:?}", rep.reasons);
    assert_eq!(rep.workspaces, 1);
}

#[test]
fn a_nested_workspace_without_a_lock_is_red() {
    let t = tree(NESTED, &["Cargo.lock"], &[]);
    let rep = run(&t, None);
    assert!(has(
        &rep,
        "tools/Cargo.lock: no committed Cargo.lock for the workspace tools/Cargo.toml"
    ));
}

#[test]
fn a_lock_on_disk_but_untracked_or_tracked_but_deleted_is_red() {
    let mut t = tree(NESTED, &["Cargo.lock"], &[]);
    t.on_disk.insert("tools/Cargo.lock".into());
    assert!(has(&run(&t, None), "tools/Cargo.lock: no committed"));
    let mut t = tree(NESTED, &["Cargo.lock"], &[]);
    t.tracked.insert("tools/Cargo.lock".into());
    assert!(has(&run(&t, None), "tools/Cargo.lock: no committed"));
}

#[test]
fn manifests_skip_target_dirs_and_other_names() {
    let tracked: BTreeSet<String> = [
        "Cargo.toml",
        "target/x/Cargo.toml",
        "a/target/Cargo.toml",
        "a/Cargo.toml.planted",
        "b/Cargo.toml",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(manifests(&tracked), vec!["Cargo.toml", "b/Cargo.toml"]);
}

#[test]
fn a_manifest_cargo_cannot_resolve_is_red() {
    let mut t = tree(NESTED, &["Cargo.lock", "tools/Cargo.lock"], &[]);
    t.roots.insert(
        "bad/Cargo.toml".into(),
        Err("error: failed to parse".into()),
    );
    t.tracked.insert("bad/Cargo.toml".into());
    assert!(has(
        &run(&t, None),
        "bad/Cargo.toml: cargo cannot resolve its workspace"
    ));
}

#[test]
fn a_census_listing_exempts_a_lock_free_workspace() {
    let t = tree(NESTED, &["Cargo.lock"], &[]);
    let rep = run(
        &t,
        Some(&listing("tools/Cargo.toml", "#534", "never built")),
    );
    assert!(rep.reasons.is_empty(), "{:?}", rep.reasons);
    assert_eq!(rep.exempt, vec!["tools/Cargo.toml"]);
    let rep = run(
        &t,
        Some(&listing(
            "tools/Cargo.toml",
            "pulseengine/gale#534",
            "never built",
        )),
    );
    assert!(rep.reasons.is_empty(), "{:?}", rep.reasons);
}

#[test]
fn an_unlisted_lock_free_workspace_names_the_census() {
    let t = tree(NESTED, &["Cargo.lock"], &[]);
    let rep = run(&t, Some(""));
    assert!(has(&rep, "or list tools/Cargo.toml in census.toml"));
}

#[test]
fn a_listing_whose_workspace_has_a_lock_is_stale() {
    let t = tree(NESTED, &["Cargo.lock", "tools/Cargo.lock"], &[]);
    let rep = run(&t, Some(&listing("tools/Cargo.toml", "#1", "x")));
    assert!(has(
        &rep,
        "'tools/Cargo.toml' is stale: its workspace has a committed tools/Cargo.lock"
    ));
}

#[test]
fn a_listing_of_an_untracked_file_is_stale() {
    let t = tree(NESTED, &["Cargo.lock", "tools/Cargo.lock"], &[]);
    let rep = run(&t, Some(&listing("gone/Cargo.toml", "#1", "x")));
    assert!(has(
        &rep,
        "'gone/Cargo.toml' is stale: it is not a tracked file"
    ));
}

#[test]
fn a_listing_of_a_member_is_stale() {
    let t = tree(
        &[
            ("Cargo.toml", "Cargo.toml"),
            ("crates/a/Cargo.toml", "Cargo.toml"),
        ],
        &[],
        &[],
    );
    let census = listing("Cargo.toml", "#1", "x") + &listing("crates/a/Cargo.toml", "#1", "x");
    let rep = run(&t, Some(&census));
    assert!(has(
        &rep,
        "'crates/a/Cargo.toml' is stale: it is not a workspace root (its workspace is Cargo.toml)"
    ));
    assert_eq!(rep.exempt, vec!["Cargo.toml"]);
}

#[test]
fn malformed_listings_are_red_one_reason_each() {
    let t = tree(NESTED, &["Cargo.lock"], &[]);
    let cases = [
        (listing("tools/*/Cargo.toml", "#1", "x"), "is a wildcard"),
        (
            listing("./tools/Cargo.toml", "#1", "x"),
            "must be the path git ls-files prints",
        ),
        (
            listing("tools/Cargo.toml", "1", "x"),
            "is not #N or owner/repo#N",
        ),
        (
            listing("tools/Cargo.toml", "gale#1", "x"),
            "is not #N or owner/repo#N",
        ),
        (
            listing("tools/Cargo.toml", "#1", " "),
            "empty manifest or reason",
        ),
        (
            "[[lockfree]]\nmanifest = \"tools/Cargo.toml\"\nissue = \"#1\"\n".into(),
            "manifest, issue and reason are required",
        ),
        (
            listing("tools/Cargo.toml", "#1", "x") + "owner = \"me\"\n",
            "unknown key 'owner'",
        ),
        ("version = 1\n".into(), "unknown top-level key 'version'"),
        (
            "lockfree = 3\n".into(),
            "lockfree is not an array of tables",
        ),
    ];
    for (census, want) in cases {
        let rep = run(&t, Some(&census));
        assert!(
            has(&rep, want),
            "{census:?}: want {want:?}, got {:?}",
            rep.reasons
        );
    }
}

#[test]
fn a_malformed_listing_exempts_nothing() {
    let t = tree(NESTED, &["Cargo.lock"], &[]);
    let rep = run(&t, Some(&listing("tools/Cargo.toml", "1", "x")));
    assert!(has(&rep, "tools/Cargo.lock: no committed"));
    assert!(rep.exempt.is_empty());
}

#[test]
fn a_duplicate_listing_is_red() {
    let t = tree(NESTED, &["Cargo.lock"], &[]);
    let one = listing("tools/Cargo.toml", "#1", "x");
    let rep = run(&t, Some(&format!("{one}\n{one}")));
    assert!(has(&rep, "'tools/Cargo.toml' is listed 2 times"));
}

#[test]
fn a_census_that_is_not_toml_cannot_run() {
    let t = tree(NESTED, &["Cargo.lock"], &[]);
    let exists = |p: &str| t.on_disk.contains(p);
    let r = check(
        &t.tracked,
        &t.roots,
        &exists,
        Some(("census.toml", "[[lockfree")),
    );
    assert!(r.unwrap_err().contains("census.toml: not TOML"));
}

#[test]
fn a_bazel_listing_is_tracked_but_not_judged() {
    let t = tree(
        NESTED,
        &["Cargo.lock", "tools/Cargo.lock"],
        &["os/MODULE.bazel"],
    );
    let rep = run(&t, Some(&listing("os/MODULE.bazel", "#534", "bazel")));
    assert!(rep.reasons.is_empty(), "{:?}", rep.reasons);
    assert_eq!(rep.not_judged, vec!["os/MODULE.bazel"]);
    let rep = run(&t, Some(&listing("os/gone/MODULE.bazel", "#534", "bazel")));
    assert!(has(&rep, "is stale: it is not a tracked file"));
}
