use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use jig::{caller, docs, lockfiles, names, pins, verdict};

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
    /// Check that every Cargo workspace has a committed lock (see `jig docs lockfiles`).
    Lockfiles {
        /// Repository root (a git work tree).
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Lockfile census: workspaces that ship without a lock, each with an issue and a reason.
        #[arg(long)]
        census: Option<PathBuf>,
    },
    /// Check that no tracked file contains a private name (see `jig docs names`).
    Names {
        /// Repository root (a git work tree).
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Patterns file: one regex per line, `#` comments.
        #[arg(long)]
        patterns: PathBuf,
        /// Allow file: repository-relative paths (a trailing `/` allows a directory).
        #[arg(long)]
        allow: Option<PathBuf>,
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
        Cmd::Lockfiles { dir, census } => run_lockfiles(cli.format, &dir, census.as_deref()),
        Cmd::Names {
            dir,
            patterns,
            allow,
        } => run_names(cli.format, &dir, &patterns, allow.as_deref()),
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
    if opts.baseline_job.is_empty() && f == Format::Text {
        println!("{}", caller::no_baseline_note(&opts.self_job));
    }
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
    if opts.baseline_job.is_empty() && f == Format::Text {
        println!("{}", caller::no_baseline_note(&opts.required_job));
    }
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

fn run_lockfiles(f: Format, dir: &Path, census: Option<&Path>) -> u8 {
    use std::collections::{BTreeMap, BTreeSet};
    use std::process::Command;

    let top = match fs::canonicalize(dir) {
        Ok(p) => p,
        Err(e) => return could_not_run(f, &format!("{}: {e}", dir.display())),
    };
    let tracked: BTreeSet<String> = match tracked_files(&top) {
        Ok(t) => t,
        Err(e) => return could_not_run(f, &e),
    };
    let manifests = lockfiles::manifests(&tracked);
    if manifests.is_empty() {
        if f == Format::Json {
            println!("{}", json!({"verdict": "red", "compared_nothing": true}));
        } else {
            println!("lockfiles: no tracked Cargo.toml; compared nothing");
        }
        return FAIL;
    }
    let mut roots: BTreeMap<String, Result<String, String>> = BTreeMap::new();
    for m in &manifests {
        let r = Command::new("cargo")
            .args(["locate-project", "--workspace", "--message-format", "plain"])
            .arg("--manifest-path")
            .arg(top.join(m))
            .output();
        let root = match r {
            Ok(o) if o.status.success() => {
                let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                fs::canonicalize(&p)
                    .map_err(|e| format!("{p}: {e}"))
                    .and_then(|abs| {
                        abs.strip_prefix(&top)
                            .map(|rel| rel.to_string_lossy().into_owned())
                            .map_err(|_| format!("workspace root {p} is outside the repository"))
                    })
            }
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
            Err(e) => return could_not_run(f, &format!("cargo: {e}")),
        };
        roots.insert(m.clone(), root);
    }
    let text;
    let census_arg = match census {
        Some(p) => {
            text = match read(p) {
                Ok(t) => t,
                Err(e) => return could_not_run(f, &e),
            };
            Some((p.to_str().unwrap_or("census"), text.as_str()))
        }
        None => None,
    };
    let exists = |rel: &str| top.join(rel).is_file();
    let rep = match lockfiles::check(&tracked, &roots, &exists, census_arg) {
        Ok(r) => r,
        Err(e) => return could_not_run(f, &e),
    };
    let code = if rep.reasons.is_empty() { PASS } else { FAIL };
    if f == Format::Json {
        println!(
            "{}",
            json!({
                "verdict": if code == PASS { "green" } else { "red" },
                "workspaces": rep.workspaces,
                "exempt": rep.exempt,
                "not_judged": rep.not_judged,
                "reasons": rep.reasons,
            })
        );
    } else {
        for r in &rep.reasons {
            println!("{r}");
        }
        for m in &rep.exempt {
            println!("lock-free by census: {m}");
        }
        for m in &rep.not_judged {
            println!("census listing not judged by jig (not a Cargo.toml): {m}");
        }
        println!(
            "lockfiles: {} workspace(s), {} lock-free by census, {} violation(s)",
            rep.workspaces,
            rep.exempt.len(),
            rep.reasons.len()
        );
    }
    code
}

/// Tracked files of the work tree at `top`, as `git ls-files` prints them.
fn tracked_files(top: &Path) -> Result<std::collections::BTreeSet<String>, String> {
    Ok(tracked_raw(top)?
        .iter()
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect())
}

/// Tracked paths as raw bytes, in git ls-files order.
fn tracked_raw(top: &Path) -> Result<Vec<Vec<u8>>, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(top)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git ls-files: {}", msg.trim()));
    }
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_vec())
        .collect())
}

fn run_names(f: Format, dir: &Path, patterns: &Path, allow: Option<&Path>) -> u8 {
    let top = match fs::canonicalize(dir) {
        Ok(p) => p,
        Err(e) => return could_not_run(f, &format!("{}: {e}", dir.display())),
    };
    let pats = match read(patterns).and_then(|t| names::parse_patterns(&t)) {
        Ok(p) => p,
        Err(e) => return could_not_run(f, &e),
    };
    let allow = match allow {
        Some(p) => match read(p).and_then(|t| names::parse_allow(&t)) {
            Ok(a) => a,
            Err(e) => return could_not_run(f, &e),
        },
        None => Vec::new(),
    };
    if pats.is_empty() {
        if f == Format::Json {
            println!("{}", json!({"verdict": "red", "compared_nothing": true}));
        } else {
            println!("names: the patterns file has no patterns; compared nothing");
        }
        return FAIL;
    }
    // What a commit of this tree publishes is the index, so contents come from
    // the index blobs (git ls-files --stage, git cat-file --batch), not the
    // work tree: a filter or working-tree-encoding cannot make the scanned
    // bytes differ from the published ones, and a symlink's blob is its
    // target text. A path that is not UTF-8 is refused rather than skipped.
    let staged = match staged_blobs(&top) {
        Ok(s) => s,
        Err(e) => return could_not_run(f, &e),
    };
    let mut tracked = std::collections::BTreeSet::new();
    let mut objects: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for (i, (mode, sha, raw)) in staged.into_iter().enumerate() {
        let Ok(path) = String::from_utf8(raw) else {
            let msg = format!(
                "tracked path {} is not UTF-8; jig names cannot judge it",
                i + 1
            );
            return could_not_run(f, &msg);
        };
        // A gitlink (submodule) has no blob in this repository.
        let obj = (mode != "160000").then_some(sha);
        objects.entry(path.clone()).or_insert(obj);
        tracked.insert(path);
    }
    let mut batch = match BlobReader::start(&top) {
        Ok(b) => b,
        Err(e) => return could_not_run(f, &e),
    };
    let unreadable = std::cell::RefCell::new(None::<String>);
    let batch_cell = std::cell::RefCell::new(&mut batch);
    let read_file = |rel: &str| -> Option<Vec<u8>> {
        let sha = objects.get(rel)?.as_deref()?;
        match batch_cell.borrow_mut().get(sha) {
            Ok(b) => Some(b),
            Err(e) => {
                let n = tracked.iter().position(|t| t == rel).map_or(0, |i| i + 1);
                let shown = names::label(rel, n, &pats);
                unreadable
                    .borrow_mut()
                    .get_or_insert(format!("{shown}: {e}"));
                None
            }
        }
    };
    let rep = names::check(&tracked, &pats, &allow, &read_file);
    if let Some(e) = unreadable.into_inner() {
        return could_not_run(f, &e);
    }
    let code = if rep.findings.is_empty() { PASS } else { FAIL };
    if f == Format::Json {
        let findings: Vec<_> = rep
            .findings
            .iter()
            .map(|x| json!({"file": x.file, "line": x.line, "pattern": x.pattern}))
            .collect();
        println!(
            "{}",
            json!({
                "verdict": if code == PASS { "green" } else { "red" },
                "scanned": rep.scanned,
                "binary": rep.binary,
                "allowed": rep.allowed,
                "findings": findings,
            })
        );
    } else {
        for x in &rep.findings {
            if x.line == 0 {
                println!("{}: its path matches pattern #{}", x.file, x.pattern);
            } else {
                println!("{}:{}: matches pattern #{}", x.file, x.line, x.pattern);
            }
        }
        for a in &rep.allowed {
            println!("skipped (allow file): {a}");
        }
        println!(
            "names: {} file(s) scanned, {} binary, {} allowed, {} finding(s) for {} pattern(s)",
            rep.scanned,
            rep.binary,
            rep.allowed.len(),
            rep.findings.len(),
            pats.len()
        );
    }
    code
}

/// The index entries as (mode, object id, raw path), in git ls-files order.
fn staged_blobs(top: &Path) -> Result<Vec<(String, String, Vec<u8>)>, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(top)
        .args(["ls-files", "--stage", "-z"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git ls-files: {}", msg.trim()));
    }
    let mut v = Vec::new();
    for rec in out.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        // "<mode> <object> <stage>\t<path>"
        let tab = rec
            .iter()
            .position(|b| *b == b'\t')
            .ok_or("git ls-files --stage: a record without a tab")?;
        let head = String::from_utf8_lossy(&rec[..tab]).into_owned();
        let mut it = head.split(' ');
        let (Some(mode), Some(sha)) = (it.next(), it.next()) else {
            return Err("git ls-files --stage: a record without mode and object".into());
        };
        v.push((mode.to_string(), sha.to_string(), rec[tab + 1..].to_vec()));
    }
    Ok(v)
}

/// One `git cat-file --batch` process, asked for one object at a time.
struct BlobReader {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    out: std::io::BufReader<std::process::ChildStdout>,
}

impl BlobReader {
    fn start(top: &Path) -> Result<Self, String> {
        use std::process::{Command, Stdio};
        let mut child = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("git cat-file: {e}"))?;
        let stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
        let out = std::io::BufReader::new(child.stdout.take().ok_or("git cat-file: no stdout")?);
        Ok(BlobReader { child, stdin, out })
    }

    fn get(&mut self, sha: &str) -> Result<Vec<u8>, String> {
        use std::io::{BufRead, Read, Write};
        writeln!(self.stdin, "{sha}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("git cat-file: {e}"))?;
        let mut head = String::new();
        self.out
            .read_line(&mut head)
            .map_err(|e| format!("git cat-file: {e}"))?;
        // "<object> blob <size>", or "<object> missing"
        let parts: Vec<&str> = head.trim_end().split(' ').collect();
        let [_, "blob", size] = parts.as_slice() else {
            return Err(format!(
                "git cat-file: object is not a readable blob ({})",
                head.trim_end()
            ));
        };
        let size: usize = size
            .parse()
            .map_err(|_| "git cat-file: bad size".to_string())?;
        let mut buf = vec![0u8; size + 1];
        self.out
            .read_exact(&mut buf)
            .map_err(|e| format!("git cat-file: {e}"))?;
        buf.pop();
        Ok(buf)
    }
}

impl Drop for BlobReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
