//! `jig classes` through the binary (REQ-016).

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
    let d = std::env::temp_dir().join(format!("jig-classes-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Dir(d)
}

fn classes(d: &Dir, config: &str, extra: &[&str]) -> Output {
    fs::write(d.0.join("sieve.toml"), config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["classes", "--config"])
        .arg(d.0.join("sieve.toml"))
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

const GOOD: &str = r#"
schema = "sieve.config.v1"
[class.core]
cargo = ["core/Cargo.toml"]
"#;

fn cfg(classes: &[&str]) -> String {
    let mut s = String::from("schema = \"sieve.config.v1\"\n");
    for c in classes {
        s += &format!("[class.{c}]\ncargo = [\"{c}/Cargo.toml\"]\n");
    }
    s + "[workflows]\nscope_leaf_edits = true\n"
}

// rivet: verifies REQ-016
#[test]
fn vocabulary_classes_pass_and_are_printed() {
    let d = dir("good");
    let o = classes(&d, &cfg(&["build", "unit_test", "docs"]), &[]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
    assert_eq!(stdout(&o).trim(), "classes: build unit_test docs");
}

// rivet: verifies REQ-016
#[test]
fn every_vocabulary_name_is_accepted() {
    let d = dir("all");
    let o = classes(&d, &cfg(&jig::classes::VOCABULARY), &[]);
    assert_eq!(code(&o), 0, "{}", stdout(&o));
}

// rivet: verifies REQ-016
#[test]
fn the_vocabulary_is_exactly_the_fleet_nine() {
    assert_eq!(
        jig::classes::VOCABULARY,
        [
            "build",
            "unit_test",
            "integration",
            "emulator",
            "coverage",
            "mutation",
            "fuzz",
            "bench",
            "docs"
        ]
    );
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["classes", "--list"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 0);
    let got: Vec<String> = stdout(&o).lines().map(str::to_string).collect();
    assert_eq!(got, jig::classes::VOCABULARY);
}

// rivet: verifies REQ-016
#[test]
fn a_class_outside_the_vocabulary_is_red_and_named() {
    let d = dir("unknown");
    let o = classes(&d, &cfg(&["build", "core", "proofs"]), &[]);
    assert_eq!(code(&o), 1, "{}", stdout(&o));
    let s = stdout(&o);
    assert!(
        s.contains("class 'core'") && s.contains("class 'proofs'"),
        "{s}"
    );
    assert!(!s.contains("class 'build'"), "{s}");
}

// rivet: verifies REQ-016
#[test]
fn near_miss_names_are_not_vocabulary() {
    let d = dir("nearmiss");
    for bad in ["Build", "unit-test", "tests", "doc", "fuzzing"] {
        let o = classes(&d, &cfg(&["build", bad]), &[]);
        assert_eq!(code(&o), 1, "{bad}: {}", stdout(&o));
    }
}

// rivet: verifies REQ-016
#[test]
fn no_class_at_all_compared_nothing() {
    let d = dir("none");
    let o = classes(
        &d,
        "schema = \"sieve.config.v1\"\n[workflows]\nx = 1\n",
        &[],
    );
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("nothing was compared"));
    let o = classes(&d, "schema = \"sieve.config.v1\"\n[class]\n", &[]);
    assert_eq!(code(&o), 1);
}

// rivet: verifies REQ-016
#[test]
fn unreadable_or_malformed_config_is_exit_2() {
    let d = dir("bad");
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["classes", "--config"])
        .arg(d.0.join("missing.toml"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
    assert_eq!(code(&classes(&d, "schema = [unclosed", &[])), 2);
    // class is not a table of tables
    assert_eq!(
        code(&classes(
            &d,
            "schema = \"sieve.config.v1\"\nclass = [\"build\"]\n",
            &[]
        )),
        2
    );
    assert_eq!(
        code(&classes(
            &d,
            "schema = \"sieve.config.v1\"\n[class]\nbuild = 1\n",
            &[]
        )),
        2
    );
    // no --config and no --list
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .arg("classes")
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
}

// rivet: verifies REQ-016
#[test]
fn schema_must_be_sieve_config_v1() {
    let d = dir("schema");
    let body = "[class.build]\ncargo = [\"a\"]\n";
    assert_eq!(
        code(&classes(
            &d,
            &format!("schema = \"sieve.config.v2\"\n{body}"),
            &[]
        )),
        2
    );
    assert_eq!(code(&classes(&d, body, &[])), 2);
    assert_eq!(code(&classes(&d, &format!("schema = 1\n{body}"), &[])), 2);
    assert_eq!(
        code(&classes(
            &d,
            &format!("schema = \"sieve.config.v1\"\n{body}"),
            &[]
        )),
        0
    );
}

// rivet: verifies REQ-016
#[test]
fn json_output_names_defined_and_unknown() {
    let d = dir("json");
    let o = classes(&d, &cfg(&["fuzz", "core"]), &["--format", "json"]);
    assert_eq!(code(&o), 1);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["verdict"], "red");
    assert_eq!(v["defined"][0], "fuzz");
    assert_eq!(v["unknown"][0], "core");
    let o = classes(
        &d,
        GOOD.replace("core", "mutation").as_str(),
        &["--format", "json"],
    );
    assert_eq!(code(&o), 0);
}

// rivet: verifies REQ-016
#[test]
fn docs_has_a_classes_topic() {
    let o = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["docs", "classes"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 0);
    let s = stdout(&o);
    assert!(s.starts_with("jig classes:"));
    for c in jig::classes::VOCABULARY {
        assert!(s.contains(c), "docs omit {c}");
    }
}
