// rivet: verifies REQ-027
//! jig confined: confinement is judged by what the kernel refuses. A probe
//! that succeeds is red, a refusal Unix permissions already explain is not
//! evidence, and a broken positive control cannot judge. Real kernel denials
//! are tested where a sandbox is available (macOS sandbox-exec); on Linux CI
//! the plant controls cover the unconfined path.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jig::confined::{Probe, Report};

fn jig() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jig"))
}

/// A scratch tree with a granted dir and a probe dir holding a readable secret.
/// Canonical, because macOS sandbox rules match /private/var, not /var.
fn tree(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("jig-confined-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("g")).unwrap();
    std::fs::create_dir_all(base.join("p")).unwrap();
    std::fs::write(base.join("p/secret"), "s").unwrap();
    std::fs::canonicalize(&base).unwrap()
}

fn run(cmd: &mut Command, t: &Path, json: bool) -> Output {
    if json {
        cmd.args(["--format", "json"]);
    }
    cmd.arg("confined")
        .arg("--granted")
        .arg(t.join("g"))
        .arg("--write")
        .arg(t.join("p/write-target"))
        .arg("--read")
        .arg(t.join("p/secret"))
        .output()
        .unwrap()
}

fn is_root(t: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(t).unwrap().uid() == 0
}

#[test]
fn exit_codes_follow_the_probes() {
    let r = |w, r| Report { write: w, read: r }.exit();
    assert_eq!(r(Probe::Denied("EACCES"), Probe::Denied("EPERM")), 0);
    assert_eq!(r(Probe::Allowed, Probe::Denied("EACCES")), 1);
    assert_eq!(r(Probe::Denied("EACCES"), Probe::Allowed), 1);
    assert_eq!(r(Probe::Unjudged("x".into()), Probe::Allowed), 1);
    assert_eq!(r(Probe::Denied("EACCES"), Probe::Unjudged("x".into())), 2);
    assert_eq!(
        r(Probe::Unjudged("x".into()), Probe::Unjudged("y".into())),
        2
    );
}

#[test]
fn an_unconfined_run_is_red_and_leaves_nothing_behind() {
    let t = tree("open");
    let o = run(&mut jig(), &t, false);
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(
        out.contains("write probe: succeeded: NOT confined"),
        "{out}"
    );
    assert!(out.contains("read probe: succeeded: NOT confined"), "{out}");
    assert!(out.ends_with("confined: not confined\n"), "{out}");
    let left: Vec<_> = std::fs::read_dir(t.join("p")).unwrap().collect();
    assert_eq!(left.len(), 1, "probe files left behind");
    assert_eq!(std::fs::read_dir(t.join("g")).unwrap().count(), 0);
}

#[test]
fn forged_labels_do_not_make_a_run_confined_and_values_are_not_printed() {
    let t = tree("labels");
    let o = run(
        jig()
            .env("ZWINGER_PROFILE", "forged-profile-value")
            .env("ZWINGER_GRANTS", "/forged/grant"),
        &t,
        true,
    );
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("\"verdict\":\"not confined\""), "{out}");
    assert!(out.contains("\"ZWINGER_PROFILE_set\":true"), "{out}");
    assert!(!out.contains("forged"), "label value printed: {out}");
}

#[test]
fn a_refusal_unix_permissions_explain_is_not_evidence() {
    use std::os::unix::fs::PermissionsExt;
    let t = tree("dac");
    // Runs as root too: root gets no bypass in the judgement either.
    std::fs::set_permissions(t.join("p"), std::fs::Permissions::from_mode(0o555)).unwrap();
    std::fs::set_permissions(t.join("p/secret"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let o = run(&mut jig(), &t, false);
    std::fs::set_permissions(t.join("p"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(2), "{out}");
    assert!(
        out.contains("write probe: cannot judge: Unix permissions"),
        "{out}"
    );
    assert!(
        out.contains("read probe: cannot judge: Unix permissions"),
        "{out}"
    );
    assert!(!out.contains(": confined\n"), "{out}");
}

/// An ancestor without search permission refuses the stat with EACCES;
/// that is Unix permissions, not confinement, so it cannot judge.
#[test]
fn an_unsearchable_ancestor_is_not_evidence() {
    use std::os::unix::fs::PermissionsExt;
    let t = tree("ancestor");
    if is_root(&t) {
        return;
    }
    std::fs::set_permissions(t.join("p"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let o = run(&mut jig(), &t, false);
    std::fs::set_permissions(t.join("p"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(2), "{out}");
    assert!(
        out.contains("read probe: cannot judge: read target"),
        "{out}"
    );
    assert!(!out.contains("refused by the kernel"), "{out}");
}

/// A target another user owns is never judged: group bits, ACL entries or
/// supplementary groups could refuse it without any confinement.
#[test]
fn a_target_owned_by_another_user_is_not_judged() {
    use std::os::unix::fs::MetadataExt;
    let t = tree("owner");
    let other = Path::new("/etc/hosts");
    if is_root(&t)
        || std::fs::metadata(other).map(|m| m.uid()).ok()
            == std::fs::metadata(&t).map(|m| m.uid()).ok()
    {
        return;
    }
    let o = jig()
        .arg("confined")
        .arg("--granted")
        .arg(t.join("g"))
        .arg("--write")
        .arg(t.join("p/write-target"))
        .arg("--read")
        .arg(other)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    // The write probe succeeds here (unconfined), so the run is red either
    // way; what matters is that the read probe did not judge.
    assert!(
        out.contains("read probe: cannot judge: Unix permissions"),
        "{out}"
    );
}

#[test]
fn missing_targets_cannot_judge_and_print_no_path() {
    let t = tree("missing");
    let o = jig()
        .arg("confined")
        .arg("--granted")
        .arg(t.join("g"))
        .arg("--write")
        .arg(t.join("absent/write-target"))
        .arg("--read")
        .arg(t.join("absent/secret"))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(2), "{out}");
    assert!(!out.contains(&*t.to_string_lossy()), "path printed: {out}");
}

#[test]
fn a_broken_positive_control_cannot_run() {
    let t = tree("control");
    let o = jig()
        .arg("confined")
        .arg("--granted")
        .arg(t.join("no-such-dir"))
        .arg("--write")
        .arg(t.join("p/write-target"))
        .arg("--read")
        .arg(t.join("p/secret"))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(2), "{err}");
    assert!(err.contains("positive control"), "{err}");
}

/// Kernel denials through the macOS sandbox (EPERM). Skipped where
/// sandbox-exec is absent.
#[test]
fn kernel_denials_are_judged() {
    if !Path::new("/usr/bin/sandbox-exec").exists() {
        return;
    }
    let t = tree("sandbox");
    let p = t.join("p");
    let both = format!(
        "(version 1)(allow default)(deny file-write* (subpath \"{0}\"))\
         (deny file-read-data (literal \"{0}/secret\"))",
        p.display()
    );
    let write_only = format!(
        "(version 1)(allow default)(deny file-write* (subpath \"{}\"))",
        p.display()
    );
    let sandboxed = |profile: &str| {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(profile).arg(env!("CARGO_BIN_EXE_jig"));
        run(&mut c, &t, false)
    };
    let o = sandboxed(&both);
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(
        out.contains("write probe: refused by the kernel (EPERM)"),
        "{out}"
    );
    assert!(out.ends_with("confined: confined\n"), "{out}");

    let o = sandboxed(&write_only);
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(
        o.status.code(),
        Some(1),
        "a half-confined run must be red: {out}"
    );
}
