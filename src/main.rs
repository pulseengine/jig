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
) -> u8 {
    // The table runs first, always, before the input is looked at.
    let rows = match verdict::self_test() {
        Ok(n) => n,
        Err(bad) => {
            let lines: Vec<String> = bad
                .iter()
                .map(|n| format!("self-test row {n} disagrees"))
                .collect();
            report(f, "red", &lines);
            return FAIL;
        }
    };
    if self_test_only {
        if f == Format::Json {
            println!("{}", json!({"verdict": "green", "self_test_rows": rows}));
        } else {
            println!("self-test: {rows} rows agree");
        }
        return PASS;
    }
    let (Some(needs), Some(workflow)) = (needs, workflow) else {
        return could_not_run(
            f,
            "--needs and --workflow are required (or use --self-test)",
        );
    };
    let parsed = read(needs)
        .and_then(|t| verdict::parse_needs(&t))
        .and_then(|n| {
            let wf = read(workflow).and_then(|t| verdict::parse_workflow(&t))?;
            Ok((n, wf))
        });
    let (n, wf) = match parsed {
        Ok(x) => x,
        Err(e) => return could_not_run(f, &e),
    };
    let reasons = verdict::judge(&n, &wf, opts);
    if reasons.is_empty() {
        report(f, "green", &[]);
        PASS
    } else {
        report(f, "red", &reasons);
        FAIL
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
