//! `jig pins`: each rule has a planted bad input that turns red and a matching
//! good input that stays green (REQ-008).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jig::pins::{check_text, uses_problem};

const SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";

fn wf(steps: &str) -> String {
    format!("on: push\njobs:\n  build:\n    runs-on: ubuntu-24.04\n    steps:\n{steps}")
}

fn bad(text: &str) -> Vec<String> {
    check_text("w.yml", text)
        .violations
        .iter()
        .map(|v| v.render())
        .collect()
}

fn assert_green(text: &str) {
    let v = bad(text);
    assert!(v.is_empty(), "expected green, got {v:?}");
}

fn assert_red(text: &str, needle: &str) {
    let v = bad(text);
    assert!(
        v.iter().any(|l| l.contains(needle)),
        "expected a violation containing {needle:?}, got {v:?}"
    );
}

// rivet: verifies REQ-008
#[test]
fn a_sha_pinned_use_is_green() {
    assert_green(&wf(&format!("      - uses: actions/checkout@{SHA} # v7\n")));
    assert_green(&wf(&format!("      - uses: a/b/sub/path@{SHA}\n")));
}

// rivet: verifies REQ-008
#[test]
fn tags_branches_short_and_uppercase_shas_are_red() {
    for r in [
        "v4",
        "main",
        "3d3c42e",
        &SHA.to_uppercase(),
        &SHA[..39],
        &format!("{SHA}0"),
    ] {
        assert_red(
            &wf(&format!("      - uses: actions/checkout@{r}\n")),
            "uses:",
        );
    }
    assert_red(&wf("      - uses: actions/checkout\n"), "no @");
}

// rivet: verifies REQ-008
#[test]
fn expressions_in_uses_are_red() {
    assert_red(
        &wf("      - uses: actions/checkout@${{ inputs.ref }}\n"),
        "uses:",
    );
    assert_red(&wf("      - uses: ${{ inputs.action }}\n"), "uses:");
}

// rivet: verifies REQ-008
#[test]
fn local_and_docker_uses_follow_their_rules() {
    assert_green(&wf("      - uses: ./.github/actions/local\n"));
    let d = "a".repeat(64);
    assert_green(&wf(&format!("      - uses: docker://alpine@sha256:{d}\n")));
    assert_red(&wf("      - uses: docker://alpine:3.20\n"), "docker");
    assert_red(&wf("      - uses: docker://alpine@sha256:abc\n"), "docker");
    assert_red(
        &wf(&format!(
            "      - uses: docker://alpine@sha256:{}\n",
            "A".repeat(64)
        )),
        "docker",
    );
    assert!(uses_problem("../escape").is_some());
}

// rivet: verifies REQ-008
#[test]
fn a_job_level_reusable_workflow_use_is_checked_too() {
    let bad_job = "on: push\njobs:\n  call:\n    uses: o/r/.github/workflows/x.yml@v1\n";
    assert_red(bad_job, "uses:");
    let good_job =
        format!("on: push\njobs:\n  call:\n    uses: o/r/.github/workflows/x.yml@{SHA}\n");
    assert_green(&good_job);
}

// rivet: verifies REQ-008
#[test]
fn rust_toolchain_must_be_a_version() {
    let step = |t: &str| {
        wf(&format!(
            "      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: {t}\n"
        ))
    };
    for good in ["1.92.0", "1.92", "nightly-2026-11-01"] {
        assert_green(&step(good));
    }
    for badv in [
        "stable",
        "beta",
        "nightly",
        "nightly-2026",
        "1",
        "${{ matrix.tc }}",
        "1.92.x",
    ] {
        assert_red(&step(badv), "toolchain");
    }
    assert_red(
        &wf(&format!("      - uses: dtolnay/rust-toolchain@{SHA}\n")),
        "without with.toolchain",
    );
    assert_red(
        &wf(&format!(
            "      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          components: clippy\n"
        )),
        "without with.toolchain",
    );
}

// rivet: verifies REQ-008
#[test]
fn cargo_nightly_needs_a_date() {
    let run = |c: &str| wf(&format!("      - run: {c}\n"));
    assert_green(&run("cargo +nightly-2026-11-01 fmt"));
    assert_green(&run("cargo build"));
    assert_red(&run("cargo +nightly fmt"), "cargo +nightly");
    assert_red(&run("cargo +nightly-2026-1-01 fmt"), "cargo +nightly");
    assert_red(&run("cargo   +nightly test"), "cargo +nightly");
    assert_red(
        &wf("      - run: |\n          echo hi\n          cargo +nightly miri test\n"),
        "cargo +nightly",
    );
}

// rivet: verifies REQ-008
#[test]
fn pip_install_needs_require_hashes() {
    let run = |c: &str| wf(&format!("      - run: |\n          {c}\n"));
    assert_green(&run("pip install --require-hashes -r req.txt"));
    assert_green(&run("python -m pip install -r req.txt --require-hashes"));
    assert_green(&run("pip freeze"));
    assert_red(&run("pip install requests"), "--require-hashes");
    assert_red(&run("pip3 install requests"), "--require-hashes");
    assert_red(&run("python -m pip install requests"), "--require-hashes");
    assert_red(
        &run("python3 -m pip install -U pip && pip install --require-hashes -r r.txt"),
        "--require-hashes",
    );
    assert_red(
        &run("pip install \\\n            requests"),
        "--require-hashes",
    );
    assert_green(&run(
        "pip install \\\n            --require-hashes -r r.txt",
    ));
}

// rivet: verifies REQ-008
#[test]
fn a_file_that_does_not_parse_is_red() {
    assert_red("jobs: [oops\n", "not checked, so not passed");
    assert_red("on: push\nbogus: *missing\n", "not checked");
    assert_red("a: 1\na: 2\n", "duplicate");
}

// rivet: verifies REQ-008
#[test]
fn a_merge_key_is_red() {
    let t = format!(
        "on: push\njobs:\n  a: &a\n    runs-on: x\n  b:\n    <<: *a\n    steps:\n      - uses: actions/checkout@{SHA}\n"
    );
    assert_red(&t, "merge key");
    // The same file without the merge key is fine.
    assert_green(&format!(
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@{SHA}\n"
    ));
}

// rivet: verifies REQ-008
#[test]
fn violations_carry_file_line_and_path() {
    let t = wf(&format!(
        "      - uses: actions/checkout@{SHA}\n      - uses: actions/setup-node@v4\n      - run: |\n          echo\n          cargo +nightly fmt\n"
    ));
    let v = check_text("x.yml", &t).violations;
    assert_eq!(v.len(), 2, "{v:?}");
    assert_eq!(v[0].line, Some(7));
    assert!(v[0].path.contains("jobs.build.steps[1]"), "{}", v[0].path);
    assert_eq!(v[1].line, Some(10), "{v:?}");
    assert!(v[1].render().starts_with("x.yml:10:"));
}

// rivet: verifies REQ-008
#[test]
fn anchored_uses_are_still_checked() {
    let t = "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - &s\n        uses: actions/checkout@v4\n      - *s\n";
    assert_red(t, "uses:");
}

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("jig-pins-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Dir(d)
}

fn put(d: &Dir, rel: &str, text: &str) {
    let p = d.0.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn pins(dir: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .arg("pins")
        .arg(dir)
        .args(extra)
        .output()
        .unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

// rivet: verifies REQ-008
#[test]
fn jig_passes_its_own_pin_check() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(root.join(".github/workflows").is_dir());
    let o = pins(root, &[]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stdout));
}

// rivet: verifies REQ-008
#[test]
fn zero_files_compared_is_exit_1_and_a_missing_dir_is_exit_2() {
    let d = dir("none");
    assert_eq!(code(&pins(&d.0, &[])), 1);
    let o = pins(&d.0, &["--format", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["compared_nothing"], true);
    assert_eq!(code(&pins(&d.0.join("absent"), &[])), 2);
}

// rivet: verifies REQ-008
#[test]
fn directory_scan_finds_workflows_and_composite_actions_but_skips_target_and_git() {
    let good = format!(
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@{SHA}\n"
    );
    let badw =
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n";
    let d = dir("scan");
    put(&d, ".github/workflows/ci.yml", &good);
    put(&d, ".github/workflows/other.yaml", &good);
    put(
        &d,
        "tools/x/action.yml",
        "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
    );
    assert_eq!(code(&pins(&d.0, &[])), 0);
    // Bad inputs in each scanned place turn it red.
    for rel in [
        ".github/workflows/bad.yml",
        ".github/workflows/bad.yaml",
        "tools/y/action.yaml",
        "action.yml",
    ] {
        let d = dir("scan-bad");
        put(&d, ".github/workflows/ci.yml", &good);
        let body = if rel.contains("action") {
            "runs:\n  using: composite\n  steps:\n    - uses: actions/checkout@v4\n"
        } else {
            badw
        };
        put(&d, rel, body);
        let o = pins(&d.0, &[]);
        assert_eq!(code(&o), 1, "{rel}");
        assert!(String::from_utf8_lossy(&o.stdout).contains(rel), "{rel}");
    }
    // Skipped places: not scanned even when bad.
    let d = dir("scan-skip");
    put(&d, ".github/workflows/ci.yml", &good);
    put(
        &d,
        "target/x/action.yml",
        "runs:\n  steps:\n    - uses: a/b@v1\n",
    );
    put(
        &d,
        ".git/x/action.yml",
        "runs:\n  steps:\n    - uses: a/b@v1\n",
    );
    put(&d, "sub/.github/workflows/nested.yml", badw);
    assert_eq!(code(&pins(&d.0, &[])), 0);
}

// rivet: verifies REQ-008
#[test]
fn a_file_that_does_not_parse_makes_the_scan_red_not_empty() {
    let d = dir("unparse");
    put(&d, ".github/workflows/ci.yml", "jobs: [oops\n");
    let o = pins(&d.0, &[]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stdout).contains("not checked"));
}

// rivet: verifies REQ-008
#[test]
fn json_output_lists_violations() {
    let d = dir("json");
    put(
        &d,
        ".github/workflows/ci.yml",
        &wf("      - uses: actions/checkout@v4\n"),
    );
    let o = pins(&d.0, &["--format", "json"]);
    assert_eq!(code(&o), 1);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["verdict"], "red");
    assert_eq!(v["violations"][0]["file"], ".github/workflows/ci.yml");
    assert_eq!(v["violations"][0]["line"], 6);
}

#[test]
fn docs_lists_and_prints_topics() {
    let bin = env!("CARGO_BIN_EXE_jig");
    let o = Command::new(bin).arg("docs").output().unwrap();
    assert_eq!(code(&o), 0);
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(s.contains("verdict") && s.contains("pins"));
    for t in ["verdict", "pins", "caller", "lockfiles", "names"] {
        let o = Command::new(bin).args(["docs", t]).output().unwrap();
        assert_eq!(code(&o), 0);
        assert!(String::from_utf8_lossy(&o.stdout).starts_with(&format!("jig {t}:")));
    }
    assert_eq!(
        code(&Command::new(bin).args(["docs", "nope"]).output().unwrap()),
        2
    );
}

// rivet: verifies REQ-008
#[test]
fn pip_hash_check_is_not_fooled_by_comments_or_other_commands() {
    let run = |c: &str| wf(&format!("      - run: |\n          {c}\n"));
    assert_red(
        &run("pip install requests # --require-hashes"),
        "--require-hashes",
    );
    assert_red(
        &run("pip install requests & echo --require-hashes"),
        "--require-hashes",
    );
    assert_red(
        &run("echo $(pip install requests) --require-hashes"),
        "--require-hashes",
    );
    assert_red(
        &run("echo `pip install requests` --require-hashes"),
        "--require-hashes",
    );
    assert_red(&run("pip3.12 install requests"), "--require-hashes");
    assert_red(
        &run("python3.12 -m pip install requests"),
        "--require-hashes",
    );
    assert_red(
        &run("pip install requests --require-hashes-x"),
        "--require-hashes",
    );
    assert_red(
        &run("pip --require-hashes freeze; pip install requests"),
        "--require-hashes",
    );
    assert_green(&run("pip install --require-hashes -r r.txt # pinned"));
    assert_green(&run(
        "echo '# not a comment' && pip install --require-hashes -r r.txt",
    ));
}

// rivet: verifies REQ-008
#[test]
fn every_local_action_a_workflow_uses_must_have_been_scanned() {
    let call =
        |p: &str| format!("on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: {p}\n");
    let bad_action = "runs:\n  using: composite\n  steps:\n    - uses: actions/checkout@v4\n";
    let good_action =
        "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n";

    // A nested directory named target is scanned, so its bad action is found.
    let d = dir("local-nested-target");
    put(&d, ".github/workflows/ci.yml", &call("./tools/target"));
    put(&d, "tools/target/action.yml", bad_action);
    let o = pins(&d.0, &[]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stdout).contains("tools/target/action.yml"));

    // A used action that is missing (or was skipped) is red, not silently passed.
    let d = dir("local-missing");
    put(&d, ".github/workflows/ci.yml", &call("./tools/gone"));
    let o = pins(&d.0, &[]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stdout).contains("./tools/gone"));

    // An action under the skipped root target/ is red when a workflow uses it.
    let d = dir("local-root-target");
    put(&d, ".github/workflows/ci.yml", &call("./target/x"));
    put(&d, "target/x/action.yml", good_action);
    assert_eq!(code(&pins(&d.0, &[])), 1);

    // Escaping the repository is red.
    let d = dir("local-escape");
    put(&d, ".github/workflows/ci.yml", &call("./../elsewhere"));
    assert_eq!(code(&pins(&d.0, &[])), 1);

    // A local reusable workflow must be one of the scanned workflow files.
    let d = dir("local-reusable");
    put(
        &d,
        ".github/workflows/ci.yml",
        "on: push\njobs:\n  a:\n    uses: ./.github/workflows/called.yml\n",
    );
    put(
        &d,
        ".github/workflows/called.yml",
        "on: workflow_call\njobs:\n  b:\n    runs-on: x\n    steps:\n      - run: echo\n",
    );
    assert_eq!(code(&pins(&d.0, &[])), 0);

    // The resolved, scanned good action stays green.
    let d = dir("local-good");
    put(&d, ".github/workflows/ci.yml", &call("./tools/ok"));
    put(&d, "tools/ok/action.yaml", good_action);
    assert_eq!(code(&pins(&d.0, &[])), 0);
}

// rivet: verifies REQ-008
#[cfg(unix)]
#[test]
fn a_symlinked_local_action_is_red() {
    let d = dir("local-symlink");
    put(
        &d,
        ".github/workflows/ci.yml",
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: ./tools/link\n",
    );
    put(
        &d,
        "target/real/action.yml",
        "runs:\n  using: composite\n  steps:\n    - uses: actions/checkout@v4\n",
    );
    fs::create_dir_all(d.0.join("tools")).unwrap();
    std::os::unix::fs::symlink(d.0.join("target/real"), d.0.join("tools/link")).unwrap();
    assert_eq!(code(&pins(&d.0, &[])), 1);
}

// rivet: verifies REQ-008
#[test]
fn pip_hash_check_survives_escaped_quotes_and_option_values() {
    let run = |c: &str| wf(&format!("      - run: |\n          {c}\n"));
    // `\'` is a literal quote, so the `#` starts a comment.
    assert_red(
        &run("echo \\'; pip install requests # --require-hashes"),
        "--require-hashes",
    );
    assert_red(
        &run("echo \\\"; pip install requests # --require-hashes"),
        "--require-hashes",
    );
    // An unbalanced quote: comment state is unknown, so the # is taken as a comment.
    assert_red(
        &run("echo 'x; pip install requests # --require-hashes"),
        "--require-hashes",
    );
    // Option values between pip and install do not hide the install.
    assert_red(
        &run("pip --cache-dir /tmp/c install requests"),
        "--require-hashes",
    );
    assert_red(
        &run("pip3 --log l.txt install requests"),
        "--require-hashes",
    );
    assert_red(
        &run("python3 -m pip --cache-dir c install requests"),
        "--require-hashes",
    );
    assert_green(&run(
        "pip --cache-dir /tmp/c install --require-hashes -r r.txt",
    ));
    assert_green(&run(
        "echo 'a # b' && pip install --require-hashes -r r.txt",
    ));
}

// rivet: verifies REQ-008
#[test]
fn the_repo_root_action_is_a_local_use() {
    let d = dir("root-action");
    put(
        &d,
        ".github/workflows/ci.yml",
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: ./\n",
    );
    put(
        &d,
        "action.yml",
        "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
    );
    let o = pins(&d.0, &[]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stdout));
    // Without a root action.yml the same use is red, once.
    let d = dir("root-action-missing");
    put(
        &d,
        ".github/workflows/ci.yml",
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: ./\n",
    );
    let o = pins(&d.0, &[]);
    assert_eq!(code(&o), 1);
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).matches("uses:").count(),
        1
    );
}

// rivet: verifies REQ-008
#[test]
fn a_rust_toolchain_file_does_not_excuse_a_floating_toolchain() {
    let w = format!(
        "on: push\njobs:\n  b:\n    runs-on: x\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: stable\n"
    );
    let d = dir("tc-file");
    put(&d, ".github/workflows/ci.yml", &w);
    put(
        &d,
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.92.0\"\n",
    );
    assert_eq!(code(&pins(&d.0, &[])), 1);
}

// rivet: verifies REQ-008
#[test]
fn a_toolchain_must_be_a_literal_version_not_an_expression() {
    // Any earlier step can export a variable (core.exportVariable, GITHUB_ENV),
    // so an env reference is never resolved statically.
    let tc = |val: &str| {
        format!(
            "on: push\nenv:\n  RUST_VERSION: 1.92.0\njobs:\n  b:\n    runs-on: x\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: {val}\n"
        )
    };
    assert_red(&tc("${{ env.RUST_VERSION }}"), "not a version");
    assert_red(&tc("${{ matrix.rust }}"), "not a version");
    assert_green(&tc("1.92.0"));
}

/// A workflow with a `build` job and a `canary` job on `toolchain: {tc}`;
/// `canary_extra` goes under the canary job, `build_needs` under build.
fn canary_wf(tc: &str, canary_extra: &str, build_needs: &str) -> String {
    format!(
        "on: push\njobs:\n  build:\n    runs-on: x\n{build_needs}    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: 1.100.0\n  canary:\n    runs-on: x\n{canary_extra}    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: {tc}\n"
    )
}

// rivet: verifies REQ-021
#[test]
fn an_advisory_canary_on_stable_is_accepted_and_reported() {
    let w = canary_wf("stable", "    continue-on-error: true\n", "");
    let r = check_text("w.yml", &w);
    assert!(r.violations.is_empty(), "{:?}", r.violations);
    assert_eq!(r.canaries, vec!["canary".to_string()]);
    let d = dir("canary-ok");
    put(&d, ".github/workflows/ci.yml", &w);
    let o = pins(&d.0, &[]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(code(&o), 0, "{out}");
    assert!(
        out.contains("advisory canary: .github/workflows/ci.yml job 'canary'"),
        "{out}"
    );
}

// rivet: verifies REQ-021
#[test]
fn stable_without_continue_on_error_is_red() {
    assert_red(&canary_wf("stable", "", ""), "is not a version");
    assert_red(
        &canary_wf("stable", "    continue-on-error: false\n", ""),
        "is not a version",
    );
    assert_red(
        &canary_wf("stable", "    continue-on-error: ${{ matrix.x }}\n", ""),
        "is not a version",
    );
}

// rivet: verifies REQ-021
#[test]
fn a_canary_that_another_job_needs_is_red() {
    let w = canary_wf(
        "stable",
        "    continue-on-error: true\n",
        "    needs: canary\n",
    );
    assert_red(&w, "is not a version");
    let w = canary_wf(
        "stable",
        "    continue-on-error: true\n",
        "    needs: [canary]\n",
    );
    assert_red(&w, "is not a version");
}

// rivet: verifies REQ-021
#[test]
fn only_stable_qualifies_as_a_canary_toolchain() {
    for tc in ["beta", "nightly", "${{ matrix.tc }}"] {
        assert_red(
            &canary_wf(tc, "    continue-on-error: true\n", ""),
            "is not a version",
        );
    }
}

// rivet: verifies REQ-021
#[test]
fn step_level_continue_on_error_does_not_make_a_canary() {
    let w = format!(
        "on: push\njobs:\n  canary:\n    runs-on: x\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        continue-on-error: true\n        with:\n          toolchain: stable\n"
    );
    assert_red(&w, "is not a version");
}

// rivet: verifies REQ-021
#[test]
fn a_composite_action_has_no_canary() {
    let a = format!(
        "name: a\nruns:\n  using: composite\n  steps:\n    - uses: dtolnay/rust-toolchain@{SHA}\n      continue-on-error: true\n      with:\n        toolchain: stable\n"
    );
    assert_red(&a, "is not a version");
}

// rivet: verifies REQ-021
#[test]
fn only_the_job_calling_jigs_required_may_need_a_canary() {
    let canary = format!(
        "  canary:\n    runs-on: x\n    continue-on-error: true\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: stable\n"
    );
    let with_needer = |needer: &str| format!("on: push\njobs:\n{canary}{needer}");
    // The verdict job reads results only, and REQ-017 makes it need every job.
    for u in [
        format!("pulseengine/jig/.github/workflows/required.yml@{SHA}"),
        "./.github/workflows/required.yml".to_string(),
    ] {
        let w = with_needer(&format!(
            "  required:\n    needs: [canary]\n    if: always()\n    uses: {u}\n"
        ));
        let r = check_text("w.yml", &w);
        assert!(r.violations.is_empty(), "{u}: {:?}", r.violations);
        assert_eq!(r.canaries, vec!["canary".to_string()]);
    }
    // Any other reusable workflow, or a look-alike path, still counts.
    for u in [
        format!("pulseengine/jig/.github/workflows/baseline.yml@{SHA}"),
        format!("other/jig/.github/workflows/required.yml@{SHA}"),
        format!("pulseengine/jig/.github/workflows/required.yml.evil@{SHA}"),
    ] {
        let w = with_needer(&format!(
            "  required:\n    needs: [canary]\n    uses: {u}\n"
        ));
        assert_red(&w, "is not a version");
    }
}
