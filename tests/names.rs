// rivet: verifies REQ-026
//! jig names: the private-name guard. The judgement is tested on the library;
//! the no-echo property is tested on the binary in a real git work tree.
//! The names below are synthetic; the pattern used for jig's own controls
//! (ci-controls/fixtures/names-patterns.txt) must not match this file.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use jig::names::{Finding, check, is_binary, parse_allow, parse_patterns};

const NAME: &str = "build-host-7";

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|s| s.to_string()).collect()
}

#[test]
fn patterns_are_numbered_skipping_comments_and_blanks() {
    let p = parse_patterns("# shapes\n\nbuild-host-[0-9]+\n  # more\n(?i)LAB-[a-z]+\n").unwrap();
    assert_eq!(p.iter().map(|x| x.index).collect::<Vec<_>>(), vec![1, 2]);
    assert!(p[1].re.is_match(b"lab-x"));
}

#[test]
fn a_bad_pattern_names_its_index_not_its_text() {
    let e = parse_patterns("ok-[0-9]\nsecret-(\n").err().unwrap();
    assert!(e.contains("pattern #2 (line 2)"), "{e}");
    assert!(!e.contains("secret"), "{e}");
}

#[test]
fn a_pattern_matching_the_empty_string_is_refused() {
    let e = parse_patterns("x*\n").err().unwrap();
    assert!(e.contains("matches the empty string"), "{e}");
}

#[test]
fn allow_lines_are_paths_only() {
    assert_eq!(
        parse_allow("# c\ndocs/a.md\nplants/\n").unwrap(),
        vec!["docs/a.md", "plants/"]
    );
    for bad in ["docs/*.md", "/etc/hosts", "./a", "a/../b", "a?"] {
        assert!(parse_allow(bad).is_err(), "{bad}");
    }
}

#[test]
fn findings_give_file_line_and_pattern_index() {
    let p = parse_patterns("build-host-[0-9]+\nnever-[0-9]{30}\n").unwrap();
    let files = |rel: &str| -> Option<Vec<u8>> {
        match rel {
            "a.md" => Some(format!("one\nssh {NAME}\n").into_bytes()),
            "b.md" => Some(b"clean\n".to_vec()),
            _ => None,
        }
    };
    let rep = check(&set(&["a.md", "b.md", "gone.md"]), &p, &[], &files);
    assert_eq!(
        rep.findings,
        vec![Finding {
            file: "a.md".into(),
            line: 2,
            pattern: 1
        }]
    );
    assert_eq!(rep.scanned, 2);
}

#[test]
fn allowed_paths_and_binary_files_are_skipped_and_counted() {
    let p = parse_patterns("build-host-[0-9]+\n").unwrap();
    let files = |rel: &str| -> Option<Vec<u8>> {
        match rel {
            "img.png" => Some([b"\0PNG".as_slice(), NAME.as_bytes()].concat()),
            _ => Some(NAME.as_bytes().to_vec()),
        }
    };
    let allow = vec!["plants/".to_string(), "docs/x.md".to_string()];
    let rep = check(
        &set(&["img.png", "plants/a.md", "docs/x.md", "docs/xy.md"]),
        &p,
        &allow,
        &files,
    );
    assert_eq!(rep.binary, 1);
    assert_eq!(rep.allowed, vec!["docs/x.md", "plants/a.md"]);
    let files: Vec<&str> = rep.findings.iter().map(|f| f.file.as_str()).collect();
    assert_eq!(files, vec!["docs/xy.md", "img.png"]);
    assert!(is_binary(b"a\0b") && !is_binary(b"abc"));
}

fn repo(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jig-names-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for (rel, text) in files {
        let p = d.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let git = |args: &[&str]| {
        let s = Command::new("git")
            .arg("-C")
            .arg(&d)
            .args(args)
            .output()
            .unwrap();
        assert!(s.status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    d
}

fn jig(dir: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let mut out = String::from_utf8_lossy(&o.stdout).into_owned();
    out += &String::from_utf8_lossy(&o.stderr);
    (o.status.code().unwrap_or(-1), out)
}

#[test]
fn the_binary_never_prints_the_match() {
    let d = repo(
        "echo",
        &[("docs/setup.md", &format!("connect to {NAME}.lan\n"))],
    );
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "build-host-[0-9]+\n").unwrap();
    for fmt in ["text", "json"] {
        let (code, out) = jig(
            &d,
            &[
                "--format",
                fmt,
                "names",
                "--patterns",
                pats.to_str().unwrap(),
            ],
        );
        assert_eq!(code, 1, "{out}");
        assert!(!out.contains(NAME), "{fmt} output echoed the name: {out}");
        if fmt == "text" {
            assert!(out.contains("docs/setup.md:1: matches pattern #1"), "{out}");
        }
    }
}

#[test]
fn untracked_files_are_not_scanned() {
    let d = repo("untracked", &[("a.md", "clean\n")]);
    std::fs::write(d.join("loose.md"), NAME).unwrap();
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "build-host-[0-9]+\n").unwrap();
    let (code, out) = jig(&d, &["names", "--patterns", pats.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn no_patterns_is_red_and_a_bad_pattern_could_not_run() {
    let d = repo("empty", &[("a.md", "clean\n")]);
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "# nothing\n").unwrap();
    let (code, out) = jig(&d, &["names", "--patterns", pats.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("compared nothing"), "{out}");
    std::fs::write(&pats, "a(\n").unwrap();
    let (code, _) = jig(&d, &["names", "--patterns", pats.to_str().unwrap()]);
    assert_eq!(code, 2);
}

#[test]
fn a_nul_byte_does_not_hide_a_name() {
    // A text file with one NUL in its first 8000 bytes looks binary to git;
    // it must still be scanned.
    let d = repo("nul", &[("notes.txt", &format!("\0\nhost {NAME}\n"))]);
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "build-host-[0-9]+\n").unwrap();
    let (code, out) = jig(&d, &["names", "--patterns", pats.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("notes.txt:2: matches pattern #1"), "{out}");
    assert!(!out.contains(NAME), "{out}");
}

#[test]
fn a_name_in_a_path_is_a_finding_and_the_path_is_never_printed() {
    let d = repo(
        "path",
        &[
            ("a.md", "clean\n"),
            (&format!("hosts/{NAME}.md"), &format!("see {NAME}\n")),
        ],
    );
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "build-host-[0-9]+\n").unwrap();
    for fmt in ["text", "json"] {
        let (code, out) = jig(
            &d,
            &[
                "--format",
                fmt,
                "names",
                "--patterns",
                pats.to_str().unwrap(),
            ],
        );
        assert_eq!(code, 1, "{out}");
        assert!(!out.contains(NAME), "{fmt} output printed the name: {out}");
        if fmt == "text" {
            assert!(
                out.contains("<tracked path 2>: its path matches pattern #1"),
                "{out}"
            );
            assert!(
                out.contains("<tracked path 2>:1: matches pattern #1"),
                "{out}"
            );
        }
    }
    // An allowed path is listed as skipped, still without the name.
    let allow = d.join("allow.txt");
    std::fs::write(&allow, "hosts/\n").unwrap();
    let (code, out) = jig(
        &d,
        &[
            "names",
            "--patterns",
            pats.to_str().unwrap(),
            "--allow",
            allow.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("skipped (allow file): <tracked path 2>"),
        "{out}"
    );
    assert!(!out.contains(NAME), "{out}");
}

#[cfg(unix)]
#[test]
fn a_symlink_target_is_scanned() {
    let d = repo("symlink", &[("a.md", "clean\n")]);
    std::os::unix::fs::symlink(format!("/srv/{NAME}/share"), d.join("link")).unwrap();
    let s = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["add", "link"])
        .output()
        .unwrap();
    assert!(s.status.success());
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "build-host-[0-9]+\n").unwrap();
    let (code, out) = jig(&d, &["names", "--patterns", pats.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("link:1: matches pattern #1"), "{out}");
    assert!(!out.contains(NAME), "{out}");
}

#[test]
fn an_anchored_pattern_matches_a_path_component_or_its_stem() {
    let p = parse_patterns("^build-host-[0-9]+$\n").unwrap();
    for path in [
        format!("hosts/{NAME}/x.md"),
        format!("docs/{NAME}.md"),
        NAME.to_string(),
    ] {
        assert!(jig::names::path_matches(&path, &p[0]), "{path}");
        assert_eq!(jig::names::label(&path, 3, &p), "<tracked path 3>");
    }
    assert!(!jig::names::path_matches("docs/other.md", &p[0]));
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_cannot_be_judged() {
    use std::os::unix::ffi::OsStrExt;
    let d = repo("nonutf8", &[("a.md", "clean\n")]);
    let blob = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["hash-object", "-w", "a.md"])
        .output()
        .unwrap();
    let blob = String::from_utf8(blob.stdout).unwrap();
    let mut info = format!("100644,{},", blob.trim()).into_bytes();
    info.extend_from_slice(b"\xffhost.md");
    let s = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(std::ffi::OsStr::from_bytes(&info))
        .output()
        .unwrap();
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "build-host-[0-9]+\n").unwrap();
    let (code, out) = jig(&d, &["names", "--patterns", pats.to_str().unwrap()]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("is not UTF-8"), "{out}");
}

#[test]
fn an_anchored_pattern_finds_a_name_before_several_dots() {
    let p = parse_patterns("^build-host-[0-9]+$\n").unwrap();
    for path in [format!("dist/{NAME}.tar.gz"), format!("x/a.{NAME}.b/y")] {
        assert!(jig::names::path_matches(&path, &p[0]), "{path}");
    }
}

#[test]
fn a_crlf_line_matches_an_anchored_pattern() {
    let p = parse_patterns("^build-host-[0-9]+$\n").unwrap();
    let hits = jig::names::scan_file("a.txt", format!("x\r\n{NAME}\r\n").as_bytes(), &p);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 2);
}

#[test]
fn the_index_is_scanned_not_the_work_tree() {
    // Staged content is what a commit publishes: a name staged and then
    // removed from the work tree is still found; one only in the work tree
    // is not.
    let d = repo(
        "index",
        &[("staged.md", &format!("{NAME}\n")), ("b.md", "clean\n")],
    );
    std::fs::write(d.join("staged.md"), "clean now\n").unwrap();
    std::fs::write(d.join("b.md"), format!("{NAME}\n")).unwrap();
    let pats = d.join("pats.txt");
    std::fs::write(&pats, "build-host-[0-9]+\n").unwrap();
    let (code, out) = jig(&d, &["names", "--patterns", pats.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("staged.md:1: matches pattern #1"), "{out}");
    assert!(!out.contains("b.md:"), "{out}");
}
