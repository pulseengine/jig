use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use jig::{caller, docs, pins, verdict};

/// Exit codes: 0 check passed, 1 check failed or nothing compared, 2 could not run.
#[derive(Parser)]
#[command(name = "jig", version, about = "pulseengine shared CI baseline checks")]
struct Cli {
    /// Output format.
    #[arg(long, value_enum, default_value = "text", global = true)]
    format: Format,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum Format {
    Text,
    Json,
}

#[derive(Subcommand)]
enum Cmd {
    /// Judge the caller's `needs` (see `jig docs verdict`).
    Verdict {
        /// JSON of `${{ toJSON(needs) }}`.
        #[arg(long)]
        needs: Option<PathBuf>,
        /// The caller workflow file.
        #[arg(long)]
        workflow: Option<PathBuf>,
        /// The aggregator job's own id.
        #[arg(long, default_value = "required")]
        self_job: String,
        /// The baseline job's id.
        #[arg(long, default_value = "baseline")]
        baseline_job: String,
        /// The caller's classify job: its outputs are the class names plus `run_all`.
        #[arg(long, default_value = "classify")]
        classifier_job: String,
        /// Job ids that may never be skipped (space or comma separated; repeatable).
        #[arg(long)]
        never_skip: Vec<String>,
        /// Job ids allowed to fail (space or comma separated; repeatable).
        #[arg(long)]
        advisory: Vec<String>,
        /// `<job>=<aggregator>` pairs: the aggregator judges the job (space or comma separated; repeatable).
        #[arg(long)]
        delegated: Vec<String>,
        /// Also write a JSON record of this run (needs, verdict, reasons, options,
        /// GitHub context) to this file; a record that cannot be written is exit 2.
        #[arg(long)]
        record: Option<PathBuf>,
        /// Run only the built-in decision table.
        #[arg(long)]
        self_test: bool,
    },
    /// Check that every action and toolchain is pinned (see `jig docs pins`).
    Pins {
        /// Directory to scan.
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Check a caller workflow's shape (see `jig docs caller`).
    Caller {
        /// The caller workflow file.
        #[arg(long)]
        workflow: PathBuf,
        /// The required job's id.
        #[arg(long, default_value = "required")]
        required_job: String,
        /// The baseline job's id.
        #[arg(long, default_value = "baseline")]
        baseline_job: String,
        /// Accept local ./.github/workflows/{baseline,required}.yml calls (pulseengine/jig only).
        #[arg(long)]
        allow_local: bool,
    },
    /// List or print the embedded docs.
    Docs {
        /// Topic to print; omit to list.
        topic: Option<String>,
    },
}

const PASS: u8 = 0;
const FAIL: u8 = 1;
const COULD_NOT_RUN: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Verdict {
            needs,
            workflow,
            self_job,
            baseline_job,
            classifier_job,
            never_skip,
            advisory,
            delegated,
            record,
            self_test,
        } => {
            let delegated = match verdict::parse_delegated(&delegated) {
                Ok(d) => d,
                Err(e) => return ExitCode::from(could_not_run(cli.format, &e)),
            };
            let opts = verdict::Opts {
                self_job,
                baseline_job,
                classifier_job,
                never_skip: verdict::split_ids(&never_skip),
                advisory: verdict::split_ids(&advisory),
                delegated,
            };
            run_verdict(
                cli.format,
                needs.as_deref(),
                workflow.as_deref(),
                &opts,
                self_test,
                record.as_deref(),
            )
        }
        Cmd::Pins { dir } => run_pins(cli.format, &dir),
        Cmd::Caller {
            workflow,
            required_job,
            baseline_job,
            allow_local,
        } => run_caller(
            cli.format,
            &workflow,
            &caller::Opts {
                required_job,
                baseline_job,
                allow_local,
            },
        ),
        Cmd::Docs { topic } => run_docs(topic.as_deref()),
    };
    ExitCode::from(code)
}

fn could_not_run(f: Format, msg: &str) -> u8 {
    if f == Format::Json {
        println!("{}", json!({"verdict": "error", "error": msg}));
    } else {
        eprintln!("jig: could not run: {msg}");
    }
    COULD_NOT_RUN
}

fn run_docs(topic: Option<&str>) -> u8 {
    match topic {
        None => {
            print!("{}", docs::list());
            PASS
        }
        Some(t) => match docs::topic(t) {
            Some(text) => {
                print!("{text}");
                PASS
            }
            None => {
                eprintln!("jig: unknown docs topic '{t}'\n{}", docs::list());
                COULD_NOT_RUN
            }
        },
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn run_verdict(
    f: Format,
    needs: Option<&Path>,
    workflow: Option<&Path>,
    opts: &verdict::Opts,
    self_test_only: bool,
    record: Option<&Path>,
) -> u8 {
    let code = judge_verdict(f, needs, workflow, opts, self_test_only, record);
    let Some(path) = record else {
        return code.0;
    };
    let needs_text = needs.and_then(|p| fs::read_to_string(p).ok());
    match write_record(path, &code, needs_text.as_deref(), opts) {
        Ok(()) => code.0,
        Err(e) => could_not_run(f, &e),
    }
}

/// Exit code, verdict word and reasons, for the record.
type Judged = (u8, &'static str, Vec<String>);

/// One JSON record of a verdict run (REQ-023): what sieve selected, every
/// job's result, the verdict and the GitHub context, for the shadow-phase
/// evidence. Absent context variables are recorded as null.
fn write_record(
    path: &Path,
    judged: &Judged,
    needs_text: Option<&str>,
    opts: &verdict::Opts,
) -> Result<(), String> {
    let env = |k: &str| std::env::var(k).ok();
    let needs = needs_text.map(|t| {
        serde_json::from_str::<serde_json::Value>(t)
            .unwrap_or_else(|_| serde_json::Value::String(t.to_string()))
    });
    let rec = json!({
        "schema": "jig.record.v1",
        "jig": env!("CARGO_PKG_VERSION"),
        "verdict": judged.1,
        "exit": judged.0,
        "reasons": judged.2,
        "needs": needs,
        "options": {
            "self_job": opts.self_job,
            "baseline_job": opts.baseline_job,
            "classifier_job": opts.classifier_job,
            "never_skip": opts.never_skip,
            "advisory": opts.advisory,
            "delegated": opts.delegated,
        },
        "github": {
            "repository": env("GITHUB_REPOSITORY"),
            "workflow_ref": env("GITHUB_WORKFLOW_REF"),
            "event": env("GITHUB_EVENT_NAME"),
            "ref": env("GITHUB_REF"),
            "sha": env("GITHUB_SHA"),
            "head_sha": env("JIG_HEAD_SHA"),
            "run_id": env("GITHUB_RUN_ID"),
            "run_attempt": env("GITHUB_RUN_ATTEMPT"),
        },
    });
    let text = serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())? + "\n";
    fs::write(path, text).map_err(|e| format!("cannot write record {}: {e}", path.display()))
}

fn judge_verdict(
    f: Format,
    needs: Option<&Path>,
    workflow: Option<&Path>,
    opts: &verdict::Opts,
    self_test_only: bool,
    record: Option<&Path>,
) -> Judged {
    // The table runs first, always, before the input is looked at.
    let rows = match verdict::self_test() {
        Ok(n) => n,
        Err(bad) => {
            let lines: Vec<String> = bad
                .iter()
                .map(|n| format!("self-test row {n} disagrees"))
                .collect();
            report(f, "red", &lines);
            return (FAIL, "red", lines);
        }
    };
    if self_test_only && record.is_some() {
        let e = "--record needs --needs and --workflow, not --self-test".to_string();
        return (could_not_run(f, &e), "error", vec![e]);
    }
    if self_test_only {
        if f == Format::Json {
            println!("{}", json!({"verdict": "green", "self_test_rows": rows}));
        } else {
            println!("self-test: {rows} rows agree");
        }
        return (PASS, "green", Vec::new());
    }
    let (Some(needs), Some(workflow)) = (needs, workflow) else {
        let e = "--needs and --workflow are required (or use --self-test)".to_string();
        return (could_not_run(f, &e), "error", vec![e]);
    };
    let parsed = read(needs)
        .and_then(|t| verdict::parse_needs(&t))
        .and_then(|n| {
            let wf = read(workflow).and_then(|t| verdict::parse_workflow(&t))?;
            Ok((n, wf))
        });
    let (n, wf) = match parsed {
        Ok(x) => x,
        Err(e) => return (could_not_run(f, &e), "error", vec![e]),
    };
    let reasons = verdict::judge(&n, &wf, opts);
    if reasons.is_empty() {
        report(f, "green", &[]);
        (PASS, "green", reasons)
    } else {
        report(f, "red", &reasons);
        (FAIL, "red", reasons)
    }
}

fn report(f: Format, verdict: &str, reasons: &[String]) {
    report_as(f, "verdict", verdict, reasons);
}

/// Text output starts with `<label>: green|red`; JSON keeps the `verdict` key.
fn report_as(f: Format, label: &str, verdict: &str, reasons: &[String]) {
    if f == Format::Json {
        println!("{}", json!({"verdict": verdict, "reasons": reasons}));
    } else if reasons.is_empty() {
        println!("{label}: green");
    } else {
        println!("{label}: red");
        for r in reasons {
            println!("  {r}");
        }
    }
}

fn run_caller(f: Format, workflow: &Path, opts: &caller::Opts) -> u8 {
    let reasons = match read(workflow).and_then(|t| caller::check(&t, opts)) {
        Ok(r) => r,
        Err(e) => return could_not_run(f, &e),
    };
    if reasons.is_empty() {
        report_as(f, "caller", "green", &[]);
        PASS
    } else {
        report_as(f, "caller", "red", &reasons);
        FAIL
    }
}

fn run_pins(f: Format, dir: &Path) -> u8 {
    let rep = match pins::check_dir(dir) {
        Ok(r) => r,
        Err(e) => return could_not_run(f, &e),
    };
    let nothing = rep.files == 0;
    let code = if nothing || !rep.violations.is_empty() {
        FAIL
    } else {
        PASS
    };
    if f == Format::Json {
        println!(
            "{}",
            json!({
                "verdict": if code == PASS { "green" } else { "red" },
                "files": rep.files,
                "uses": rep.uses,
                "compared_nothing": nothing,
                "violations": rep.violations,
                "advisory_canaries": rep.canaries.iter()
                    .map(|(f, j)| json!({"file": f, "job": j}))
                    .collect::<Vec<_>>(),
            })
        );
    } else if nothing {
        println!(
            "pins: no workflow or action files found under {}; compared nothing",
            dir.display()
        );
    } else {
        for v in &rep.violations {
            println!("{}", v.render());
        }
        for (f, j) in &rep.canaries {
            println!(
                "advisory canary: {f} job '{j}' (toolchain stable, continue-on-error: true; list it in the verdict's advisory)"
            );
        }
        println!(
            "pins: {} files, {} uses checked, {} violations",
            rep.files,
            rep.uses,
            rep.violations.len()
        );
    }
    code
}
