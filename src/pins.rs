//! `jig pins`: every action and toolchain is pinned (REQ-008).
//!
//! Workflows and composite actions are parsed as YAML and walked; no rule is a
//! line regex over the file. A file that does not parse, or uses a merge key,
//! is a violation, and a scan that finds no file compared nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::yaml::{self, Node};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Violation {
    pub file: String,
    pub line: Option<usize>,
    /// Where in the document: `jobs.build.steps[1]`.
    pub path: String,
    pub reason: String,
}

impl Violation {
    pub fn render(&self) -> String {
        let at = match self.line {
            Some(l) => format!("{}:{l}", self.file),
            None => self.file.clone(),
        };
        if self.path.is_empty() {
            format!("{at}: {}", self.reason)
        } else {
            format!("{at}: {} ({})", self.reason, self.path)
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// `uses:` values examined.
    pub uses: usize,
    pub violations: Vec<Violation>,
    /// Local `uses: ./path` references: (line, document path, value).
    pub local: Vec<(usize, String, String)>,
}

fn hex_lower(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Why a `uses:` value is not an accepted pin, or None when it is.
pub fn uses_problem(u: &str) -> Option<String> {
    if let Some(rest) = u.strip_prefix("./") {
        // `./` alone is the action at the repository root.
        return if rest.split('/').any(|p| p == "..") {
            Some(format!("local path '{u}' leaves the repository"))
        } else {
            None
        };
    }
    if let Some(rest) = u.strip_prefix("docker://") {
        return match rest.rsplit_once("@sha256:") {
            Some((img, d))
                if !img.is_empty() && !img.contains(char::is_whitespace) && hex_lower(d, 64) =>
            {
                None
            }
            _ => Some(format!(
                "'{u}' is a docker image not pinned by @sha256:<64 lowercase hex>"
            )),
        };
    }
    let Some((repo, rf)) = u.split_once('@') else {
        return Some(format!("'{u}' has no @<40-hex SHA> pin"));
    };
    let name_ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() < 2 || !parts.iter().all(|p| name_ok(p)) {
        return Some(format!(
            "'{u}' is not owner/repo[/path]@<sha>, ./local or docker://image@sha256:<digest>"
        ));
    }
    if !hex_lower(rf, 40) {
        return Some(format!(
            "'{u}' is not pinned to a full 40-char lowercase hex commit SHA (tags, branches, short or uppercase SHAs and expressions are refused)"
        ));
    }
    None
}

/// `1.92`, `1.92.0` or `nightly-YYYY-MM-DD`.
pub fn is_toolchain_version(t: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some(d) = t.strip_prefix("nightly-") {
        let p: Vec<&str> = d.split('-').collect();
        return p.len() == 3
            && p[0].len() == 4
            && p[1].len() == 2
            && p[2].len() == 2
            && p.iter().all(|x| digits(x));
    }
    let p: Vec<&str> = t.split('.').collect();
    (p.len() == 2 || p.len() == 3) && p.iter().all(|x| digits(x))
}

/// Does `after` (text following `+nightly`) start with `-YYYY-MM-DD`?
fn dated(after: &str) -> bool {
    let b = after.as_bytes();
    b.len() >= 11
        && b[0] == b'-'
        && b[1..5].iter().all(u8::is_ascii_digit)
        && b[5] == b'-'
        && b[6..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'-'
        && b[9..11].iter().all(u8::is_ascii_digit)
}

/// Offsets (as 0-based line numbers) of `cargo +nightly` without a date.
fn undated_nightly(run: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (ln, line) in run.lines().enumerate() {
        let mut from = 0;
        while let Some(i) = line[from..].find("+nightly") {
            let at = from + i;
            let before = line[..at].trim_end();
            let is_cargo = before.ends_with("cargo")
                && before[..before.len() - 5]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'));
            if is_cargo && !dated(&line[at + 8..]) {
                out.push(ln);
            }
            from = at + 8;
        }
    }
    out
}

/// Drop a shell comment: `#` at the start or after whitespace, outside quotes.
///
/// A backslash outside single quotes escapes the next character. When the
/// quote state is unbalanced at the end of the line it cannot be trusted, so
/// the line is cut at the first `#` after whitespace regardless of quotes:
/// dropping more text can only hide a `--require-hashes`, never invent one.
fn strip_comment(line: &str) -> &str {
    let (mut single, mut double, mut prev_ws, mut escaped) = (false, false, true, false);
    let mut cut = None;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            prev_ws = false;
            continue;
        }
        match c {
            '\\' if !single => escaped = true,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '#' if !single && !double && prev_ws => {
                cut = Some(i);
                break;
            }
            _ => {}
        }
        prev_ws = c.is_whitespace();
    }
    if let Some(i) = cut {
        return &line[..i];
    }
    if single || double || escaped {
        let mut prev_ws = true;
        for (i, c) in line.char_indices() {
            if c == '#' && prev_ws {
                return &line[..i];
            }
            prev_ws = c.is_whitespace();
        }
    }
    line
}

/// `pip`, `pip3`, `pip3.12` (any path prefix).
fn is_pip(tok: &str) -> bool {
    let name = tok.rsplit('/').next().unwrap_or(tok);
    name.strip_prefix("pip")
        .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// 0-based line numbers of `pip install` commands without `--require-hashes`.
///
/// Comments are dropped first, and a command is split at every shell
/// separator (`;`, `&`, `|`, newlines) and at `$(`, backticks and parentheses,
/// so `--require-hashes` counts only as an argument after `install` in the
/// same pip command.
fn unhashed_pip(run: &str) -> Vec<usize> {
    // Join backslash continuations, remembering the line each logical line began on.
    let mut logical: Vec<(usize, String)> = Vec::new();
    let mut cont = false;
    for (ln, line) in run.lines().enumerate() {
        let line = strip_comment(line);
        let (body, next_cont) = match line.trim_end().strip_suffix('\\') {
            Some(b) => (b, true),
            None => (line, false),
        };
        match logical.last_mut() {
            Some((_, s)) if cont => {
                s.push(' ');
                s.push_str(body);
            }
            _ => logical.push((ln, body.to_string())),
        }
        cont = next_cont;
    }
    let mut out = Vec::new();
    for (ln, text) in logical {
        let seps = |c: char| matches!(c, ';' | '&' | '|' | '`' | '(' | ')' | '$' | '\n');
        for seg in text.split(seps) {
            let toks: Vec<&str> = seg
                .split_whitespace()
                .map(|t| t.trim_matches(|c| matches!(c, '"' | '\'')))
                .collect();
            // The first `install` anywhere after a pip invocation. Tokens in
            // between may be options or their values, so none of them can
            // hide it (fail closed).
            // A bare `pip` token also covers `python -m pip`.
            let invoked = |j: usize| is_pip(toks[j]);
            let install = toks
                .iter()
                .enumerate()
                .position(|(i, t)| *t == "install" && (0..i).any(invoked));
            if let Some(i) = install
                && !toks[i + 1..].contains(&"--require-hashes")
            {
                out.push(ln);
            }
        }
    }
    out
}

struct Walker<'a> {
    file: &'a str,
    report: Report,
    /// Every literal value each `env:` key is given anywhere in the document.
    env: BTreeMap<String, Vec<String>>,
    /// The repository pins its toolchain in rust-toolchain(.toml), which
    /// rustup obeys whatever the action installed.
    toolchain_file: bool,
}

impl Walker<'_> {
    fn add(&mut self, line: usize, path: &str, reason: String) {
        self.report.violations.push(Violation {
            file: self.file.to_string(),
            line: Some(line),
            path: path.to_string(),
            reason,
        });
    }

    fn run_text(&mut self, node: &Node, path: &str) {
        let Node::Scalar { value, line } = node else {
            return;
        };
        // yaml-rust2 puts a block scalar's marker on its first content line.
        let base = *line;
        for ln in undated_nightly(value) {
            self.add(
                base + ln,
                path,
                "`cargo +nightly` without a pinned date (use +nightly-YYYY-MM-DD)".into(),
            );
        }
        for ln in unhashed_pip(value) {
            self.add(
                base + ln,
                path,
                "pip install without --require-hashes".into(),
            );
        }
    }

    fn walk(&mut self, node: &Node, path: &str) {
        match node {
            Node::Scalar { .. } => {}
            Node::Seq { items, .. } => {
                for (i, n) in items.iter().enumerate() {
                    self.walk(n, &format!("{path}[{i}]"));
                }
            }
            Node::Map { entries, .. } => {
                for e in entries {
                    let p = if path.is_empty() {
                        e.key.clone()
                    } else {
                        format!("{path}.{}", e.key)
                    };
                    match (e.key.as_str(), &e.value) {
                        ("uses", Node::Scalar { value, line }) => {
                            self.report.uses += 1;
                            if let Some(why) = uses_problem(value) {
                                self.add(*line, path, format!("uses: {why}"));
                            }
                            if value.starts_with("./") {
                                self.report
                                    .local
                                    .push((*line, path.to_string(), value.clone()));
                            }
                            let lower = value.to_ascii_lowercase();
                            if lower == "dtolnay/rust-toolchain"
                                || lower.starts_with("dtolnay/rust-toolchain@")
                            {
                                self.toolchain(node, *line, path);
                            }
                        }
                        ("uses", other) => {
                            self.report.uses += 1;
                            self.add(other.line(), path, "uses: is not a plain string".into());
                        }
                        ("run", n @ Node::Scalar { .. }) => self.run_text(n, path),
                        _ => {}
                    }
                    self.walk(&e.value, &p);
                }
            }
        }
    }

    /// A version, or `${{ env.NAME }}` whose every literal definition is one.
    fn pinned_toolchain(&self, value: &str) -> bool {
        if is_toolchain_version(value) {
            return true;
        }
        let inner = value
            .strip_prefix("${{")
            .and_then(|v| v.strip_suffix("}}"))
            .map(str::trim);
        match inner.and_then(|v| v.strip_prefix("env.")) {
            Some(name) => self.env.get(name).is_some_and(|defs| {
                !defs.is_empty() && defs.iter().all(|d| is_toolchain_version(d))
            }),
            None => false,
        }
    }

    fn toolchain(&mut self, step: &Node, uses_line: usize, path: &str) {
        if self.toolchain_file {
            return;
        }
        match step.get("with").and_then(|w| w.get("toolchain")) {
            None => self.add(
                uses_line,
                path,
                "dtolnay/rust-toolchain without with.toolchain (a version like 1.92.0 is required)"
                    .into(),
            ),
            Some(Node::Scalar { value, line }) => {
                if !self.pinned_toolchain(value) {
                    self.add(
                        *line,
                        path,
                        format!("dtolnay/rust-toolchain toolchain '{value}' is not a version (N.N[.N] or nightly-YYYY-MM-DD)"),
                    );
                }
            }
            Some(other) => self.add(other.line(), path, "toolchain is not a plain string".into()),
        }
    }
}

/// Check one workflow or action file's text.
pub fn check_text(file: &str, text: &str) -> Report {
    check_text_with(file, text, false)
}

/// As [`check_text`], told whether the repository pins its toolchain in a
/// rust-toolchain file.
pub fn check_text_with(file: &str, text: &str, toolchain_file: bool) -> Report {
    match yaml::parse(text) {
        Err(e) => Report {
            uses: 0,
            local: Vec::new(),
            violations: vec![Violation {
                file: file.to_string(),
                line: None,
                path: String::new(),
                reason: format!("{e}; not checked, so not passed"),
            }],
        },
        Ok(root) => {
            let mut env = BTreeMap::new();
            env_defs(&root, &mut env);
            let mut w = Walker {
                file,
                report: Report::default(),
                env,
                toolchain_file,
            };
            w.walk(&root, "");
            w.report
        }
    }
}

fn is_yaml(name: &str, stems: &[&str]) -> bool {
    stems
        .iter()
        .any(|s| name == format!("{s}.yml") || name == format!("{s}.yaml"))
}

fn collect(dir: &Path, root: bool, out: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for ent in rd {
        let ent = ent.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let name = ent.file_name().to_string_lossy().into_owned();
        let ty = ent
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", ent.path().display()))?;
        if ty.is_dir() {
            // Only the root target/ is skipped; anything a workflow uses from a
            // skipped place is caught by the local-use check in check_dir.
            if name != ".git" && !(root && name == "target") {
                collect(&ent.path(), false, out)?;
            }
        } else if ty.is_file() && is_yaml(&name, &["action"]) {
            out.insert(ent.path());
        }
    }
    Ok(())
}

/// Files the scan covers: `.github/workflows/*.yml|yaml` and every composite action.
pub fn scan_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let mut set = BTreeSet::new();
    let wf = root.join(".github").join("workflows");
    if wf.is_dir() {
        let rd = fs::read_dir(&wf).map_err(|e| format!("cannot read {}: {e}", wf.display()))?;
        for ent in rd {
            let ent = ent.map_err(|e| format!("cannot read {}: {e}", wf.display()))?;
            let name = ent.file_name().to_string_lossy().into_owned();
            let is_file = ent.file_type().map(|t| t.is_file()).unwrap_or(false);
            if is_file && (name.ends_with(".yml") || name.ends_with(".yaml")) {
                set.insert(ent.path());
            }
        }
    }
    collect(root, true, &mut set)?;
    Ok(set.into_iter().collect())
}

#[derive(Debug)]
pub struct DirReport {
    pub files: usize,
    pub uses: usize,
    pub violations: Vec<Violation>,
}

/// Scan a directory. `Err` means the scan could not run (exit 2).
pub fn check_dir(root: &Path) -> Result<DirReport, String> {
    let files = scan_files(root)?;
    let tc_file = toolchain_file_pinned(root);
    let scanned: BTreeSet<String> = files.iter().map(|f| shown(root, f)).collect();
    let mut rep = DirReport {
        files: files.len(),
        uses: 0,
        violations: Vec::new(),
    };
    for f in files {
        let name = shown(root, &f);
        let text = fs::read_to_string(&f).map_err(|e| format!("cannot read {name}: {e}"))?;
        let r = check_text_with(&name, &text, tc_file);
        rep.uses += r.uses;
        rep.violations.extend(r.violations);
        for (line, path, value) in r.local {
            if let Some(why) = unscanned_local(&value, &scanned) {
                rep.violations.push(Violation {
                    file: shown(root, &f),
                    line: Some(line),
                    path,
                    reason: format!("uses: {why}"),
                });
            }
        }
    }
    Ok(rep)
}

/// A scanned file's path relative to the scan root, with `/` separators.
fn shown(root: &Path, f: &Path) -> String {
    f.strip_prefix(root)
        .unwrap_or(f)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Why a local `uses: ./path` was not checked, if it was not: the action
/// (`path/action.yml|yaml`) or reusable workflow (`path.yml|yaml`) it names
/// must be one of the files this scan read. A missing, symlinked or skipped
/// target is red, never passed unseen.
fn unscanned_local(value: &str, scanned: &BTreeSet<String>) -> Option<String> {
    let rel = value.trim_start_matches("./").trim_end_matches('/');
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.contains(&"..") {
        return None; // already reported by uses_problem
    }
    if !rel.is_empty() && parts.iter().any(|p| p.is_empty() || *p == ".") {
        return Some(format!("local path '{value}' is not a plain relative path"));
    }
    // `./` alone is the action at the repository root.
    let candidates = if rel.is_empty() {
        vec!["action.yml".to_string(), "action.yaml".to_string()]
    } else if rel.ends_with(".yml") || rel.ends_with(".yaml") {
        vec![rel.to_string()]
    } else {
        vec![format!("{rel}/action.yml"), format!("{rel}/action.yaml")]
    };
    if candidates.iter().any(|c| scanned.contains(c)) {
        None
    } else {
        Some(format!(
            "local '{value}' names no file this scan read (missing, symlinked, or in a skipped directory), so it was not checked"
        ))
    }
}

/// Collect every `env:` mapping's scalar values, by key, anywhere in the tree.
fn env_defs(node: &Node, out: &mut BTreeMap<String, Vec<String>>) {
    match node {
        Node::Scalar { .. } => {}
        Node::Seq { items, .. } => items.iter().for_each(|n| env_defs(n, out)),
        Node::Map { entries, .. } => {
            for e in entries {
                if let ("env", Node::Map { entries: vars, .. }) = (e.key.as_str(), &e.value) {
                    for v in vars {
                        let val = match &v.value {
                            Node::Scalar { value, .. } => value.clone(),
                            // A non-scalar definition is never a version.
                            _ => String::new(),
                        };
                        out.entry(v.key.clone()).or_default().push(val);
                    }
                }
                env_defs(&e.value, out);
            }
        }
    }
}

/// Does the repository root pin a toolchain version in rust-toolchain(.toml)?
fn toolchain_file_pinned(root: &Path) -> bool {
    if let Ok(t) = fs::read_to_string(root.join("rust-toolchain.toml")) {
        return t.lines().any(|l| {
            let l = l.trim();
            l.strip_prefix("channel")
                .map(str::trim_start)
                .and_then(|r| r.strip_prefix('='))
                .map(|v| v.trim().trim_matches('"'))
                .is_some_and(is_toolchain_version)
        });
    }
    fs::read_to_string(root.join("rust-toolchain")).is_ok_and(|t| is_toolchain_version(t.trim()))
}
