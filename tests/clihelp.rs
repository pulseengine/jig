// rivet: verifies REQ-031
//! jig cli-help: the text judgement is tested on the library; spawning, the
//! pty runs and the exit codes on the binary against the fixture CLI
//! (ci-controls/fixtures/cli-help/cli.sh; its planted variants are the
//! cli-help-* controls in ci-controls.toml).

use std::process::Command;

use jig::clihelp::{Config, Mode, Output, judge, judge_pair, layout, refs, subcommands};

fn ok(text: &str) -> Output {
    Output {
        code: Some(0),
        stdout: text.to_string(),
    }
}

const CLAP_TOP: &str = "\
demo: things

Usage: demo [OPTIONS] <COMMAND>

Commands:
  check   Check the thing
  report  Print a report

Options:
      --format <FORMAT>  Output format [default: text] [possible values: text, json]
  -h, --help             Print help
  -V, --version          Print version
";

#[test]
fn clap_option_indents_are_rows_not_faults() {
    let l = layout(CLAP_TOP);
    assert!(l.faults.is_empty(), "{:?}", l.faults);
    assert_eq!(subcommands(CLAP_TOP), vec!["check", "report"]);
}

#[test]
fn clap_next_line_layout_is_separable() {
    let t = "Options:\n      --format <FORMAT>\n          Output format\n\n  -h, --help\n          Print help\n";
    assert!(layout(t).faults.is_empty());
}

#[test]
fn a_description_wrapped_under_the_names_is_a_fault() {
    let t = "Commands:\n  check   Check the thing and say\n  what was found\n  report  Print a report\n";
    let l = layout(t);
    assert_eq!(l.faults.len(), 1, "{:?}", l.faults);
    assert_eq!(l.faults[0].0, 3);
}

#[test]
fn a_continuation_left_of_the_description_column_is_a_fault() {
    let t = "Commands:\n  check   Check the thing and say\n      what was found\n";
    assert_eq!(layout(t).faults.len(), 1);
    let t = "Commands:\n  check   Check the thing and say\n          what was found\n";
    assert!(layout(t).faults.is_empty());
}

#[test]
fn free_text_sections_are_not_judged_as_lists() {
    let t = "Notes:\n  demo has two commands.\n  Read the docs.\n\nExamples:\n  demo check --all\n";
    assert!(layout(t).faults.is_empty());
}

#[test]
fn refs_skip_usage_examples_and_value_brackets() {
    let t = "\
Usage: demo v1.2.3 REQ-1

Options:
  --pin <V>  Pin a version [default: v1.2] [possible values: v1.2, v2.0]
  --rust     Needs Rust 1.100 and 2 GB

Examples:
  demo --pin v1.2 # see #12
";
    assert!(refs(t).is_empty(), "{:?}", refs(t));
}

#[test]
fn refs_find_ids_issues_and_versions() {
    for (line, what) in [
        ("  check  Check it (REQ-12)", "a requirement id"),
        ("  check  Check it (DD-3)", "a requirement id"),
        ("  check  Fixes #41", "an issue reference"),
        ("  check  Since v0.9", "a version tag"),
        ("  check  Since 0.9.1", "a version number"),
    ] {
        let t = format!("Commands:\n{line}\n");
        let r = refs(&t);
        assert_eq!(r, vec![(2, what)], "{line}");
    }
    // A URL fragment or a two-part number is not a reference.
    assert!(refs("Commands:\n  check  See a/b#12 and 1.2 GB\n").is_empty());
}

#[test]
fn widths_are_judged_against_each_run() {
    let cfg = Config::org().unwrap();
    let line = format!("Commands:\n  check   {}\n", "x".repeat(70));
    let f = judge(&cfg, "demo", "-h", Mode::Tty(60), &ok(&line), false);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].rule, "CH1 width");
    assert!(f[0].detail.contains("over the 60"), "{}", f[0].detail);
    // The same text fits a 100-column run.
    let f = judge(&cfg, "demo", "-h", Mode::Columns(120), &ok(&line), false);
    assert!(f.iter().all(|x| x.rule != "CH1 width"), "{f:?}");
    assert_eq!(Mode::Columns(120).allowed(100), 100);
    assert_eq!(Mode::Plain.allowed(100), 100);
}

#[test]
fn content_rules_run_on_the_plain_output_only() {
    let cfg = Config::org().unwrap();
    let mut t = String::from("Commands:\n");
    for i in 0..9 {
        t += &format!("  c{i}  Command {i} (REQ-{i})\n");
    }
    let plain = judge(&cfg, "demo", "-h", Mode::Plain, &ok(&t), true);
    assert!(plain.iter().any(|f| f.rule == "CH3 ungrouped"));
    assert!(plain.iter().any(|f| f.rule == "CH4 refs"));
    let tty = judge(&cfg, "demo", "-h", Mode::Tty(80), &ok(&t), false);
    assert!(tty.is_empty(), "{tty:?}");
}

#[test]
fn a_failing_help_call_is_ch5() {
    let cfg = Config::org().unwrap();
    let bad = Output {
        code: Some(1),
        stdout: CLAP_TOP.into(),
    };
    let f = judge(&cfg, "demo", "--help", Mode::Plain, &bad, true);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].rule, "CH5 exit");
    let empty = judge(&cfg, "demo", "--help", Mode::Plain, &ok("  \n"), true);
    assert_eq!(empty[0].detail, "printed nothing to stdout");
}

#[test]
fn ch7_wants_examples_on_top_and_a_short_h() {
    let long = format!("{CLAP_TOP}\nExamples:\n  demo check\n");
    assert!(judge_pair("demo", true, &ok(CLAP_TOP), &ok(&long)).is_empty());
    let f = judge_pair("demo", true, &ok(CLAP_TOP), &ok(CLAP_TOP));
    assert_eq!(f.len(), 1, "{f:?}");
    // A subcommand needs no examples, but -h must not be the longer one.
    assert!(judge_pair("demo check", false, &ok(CLAP_TOP), &ok(CLAP_TOP)).is_empty());
    let f = judge_pair("demo check", false, &ok(&long), &ok(CLAP_TOP));
    assert!(f[0].detail.contains("more than --help"), "{f:?}");
}

#[test]
fn a_repo_override_changes_numbers_only() {
    let org = Config::org().unwrap();
    let (c, changed) = org.with_override("desc_max = 70\n").unwrap();
    assert_eq!(c.desc_max, 70);
    assert_eq!(changed, vec![("desc_max".into(), "60".into(), "70".into())]);
    let e = org.with_override("skip = [\"CH3\"]\n").err().unwrap();
    assert!(e.contains("only width_cap"), "{e}");
    assert!(org.with_override("widths = []\n").is_err());
    assert!(org.with_override("depth = 9\n").is_err());
}

#[test]
fn org_defaults_match_the_documented_numbers() {
    let c = Config::org().unwrap();
    assert_eq!(
        (
            c.width_cap,
            c.widths.clone(),
            c.desc_max,
            c.per_heading_max,
            c.depth
        ),
        (100, vec![60, 80, 120], 60, 8, 3)
    );
}

fn jig(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

#[cfg(unix)]
#[test]
fn the_fixture_cli_is_green_in_every_mode() {
    let (code, out) = jig(&[
        "cli-help",
        "--",
        "sh",
        "ci-controls/fixtures/cli-help/cli.sh",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("3 command(s), 45 run(s), 0 finding(s)"),
        "{out}"
    );
}

#[cfg(unix)]
#[test]
fn jig_passes_its_own_check() {
    let bin = env!("CARGO_BIN_EXE_jig");
    let (code, out) = jig(&["cli-help", "--", bin]);
    assert_eq!(code, 0, "{out}");
}

#[cfg(unix)]
#[test]
fn a_binary_that_does_not_start_is_exit_2() {
    let (code, out) = jig(&["cli-help", "--", "./no-such-binary-for-jig"]);
    assert_eq!(code, 2, "{out}");
}

#[cfg(unix)]
#[test]
fn an_unknown_config_key_is_exit_2() {
    let dir = std::env::temp_dir().join(format!("jig-clihelp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("cfg.toml");
    std::fs::write(&cfg, "allow = [\"CH1\"]\n").unwrap();
    let (code, out) = jig(&[
        "cli-help",
        "--config",
        cfg.to_str().unwrap(),
        "--",
        "sh",
        "ci-controls/fixtures/cli-help/cli.sh",
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 2, "{out}");
}

#[cfg(unix)]
#[test]
fn an_override_is_printed_in_every_run() {
    let dir = std::env::temp_dir().join(format!("jig-clihelp-ov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("cfg.toml");
    std::fs::write(&cfg, "desc_max = 70\n").unwrap();
    let (code, out) = jig(&[
        "cli-help",
        "--config",
        cfg.to_str().unwrap(),
        "--",
        "sh",
        "ci-controls/fixtures/cli-help/cli.sh",
    ]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("repo override: desc_max = 70 (org default 60)"),
        "{out}"
    );
}
