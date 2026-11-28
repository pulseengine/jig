//! `jig cli-help`: judges a binary's `-h` / `--help` output against the
//! fleet CLI help rules (see `jig docs cli-help`). The judgement here is pure
//! text analysis; spawning the binary lives in [`run`] (unix only).

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

/// The org-wide thresholds, embedded so that every repo gets them through
/// its pinned jig and they change only by a reviewed jig release.
pub const ORG_DEFAULTS: &str = include_str!("../org/cli-help.toml");

/// Thresholds. Only numbers are configurable: no rule can be switched off
/// and no finding can be exempted.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub width_cap: usize,
    pub widths: Vec<usize>,
    pub desc_max: usize,
    pub per_heading_max: usize,
    pub depth: usize,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    width_cap: Option<usize>,
    widths: Option<Vec<usize>>,
    desc_max: Option<usize>,
    per_heading_max: Option<usize>,
    depth: Option<usize>,
}

/// One value a repo override changed: (key, org value, repo value).
pub type Changed = (String, String, String);

impl Config {
    pub fn org() -> Result<Config, String> {
        let c: Config =
            toml::from_str(ORG_DEFAULTS).map_err(|e| format!("org cli-help defaults: {e}"))?;
        c.validate()?;
        Ok(c)
    }

    /// Apply a repo override file's text; returns the merged config and
    /// every value it changed. An unknown key is an error (exit 2).
    pub fn with_override(&self, text: &str) -> Result<(Config, Vec<Changed>), String> {
        let o: Override = toml::from_str(text).map_err(|e| {
            format!("cli-help config: {e} (only width_cap, widths, desc_max, per_heading_max and depth can be set)")
        })?;
        let mut c = self.clone();
        let mut changed = Vec::new();
        let mut set = |k: &str, org: String, new: String| {
            if org != new {
                changed.push((k.to_string(), org, new));
            }
        };
        if let Some(v) = o.width_cap {
            set("width_cap", c.width_cap.to_string(), v.to_string());
            c.width_cap = v;
        }
        if let Some(v) = o.widths {
            set("widths", format!("{:?}", c.widths), format!("{v:?}"));
            c.widths = v;
        }
        if let Some(v) = o.desc_max {
            set("desc_max", c.desc_max.to_string(), v.to_string());
            c.desc_max = v;
        }
        if let Some(v) = o.per_heading_max {
            set(
                "per_heading_max",
                c.per_heading_max.to_string(),
                v.to_string(),
            );
            c.per_heading_max = v;
        }
        if let Some(v) = o.depth {
            set("depth", c.depth.to_string(), v.to_string());
            c.depth = v;
        }
        c.validate()?;
        Ok((c, changed))
    }

    fn validate(&self) -> Result<(), String> {
        if self.width_cap < 40 {
            return Err(format!(
                "cli-help config: width_cap {} is below 40",
                self.width_cap
            ));
        }
        if self.widths.is_empty() || self.widths.iter().any(|w| *w < 20) {
            return Err("cli-help config: widths must be a non-empty list of values >= 20".into());
        }
        if self.desc_max == 0 || self.per_heading_max == 0 {
            return Err("cli-help config: desc_max and per_heading_max must be above 0".into());
        }
        if !(1..=5).contains(&self.depth) {
            return Err(format!(
                "cli-help config: depth {} is not in 1..=5",
                self.depth
            ));
        }
        Ok(())
    }
}

/// How one help call was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// No terminal, COLUMNS unset.
    Plain,
    /// No terminal, COLUMNS set.
    Columns(usize),
    /// A pseudo-terminal of this width, COLUMNS unset.
    Tty(usize),
}

impl Mode {
    pub fn label(&self) -> String {
        match self {
            Mode::Plain => "no tty".into(),
            Mode::Columns(w) => format!("COLUMNS={w}"),
            Mode::Tty(w) => format!("tty {w}"),
        }
    }

    /// The widest line this run may print.
    pub fn allowed(&self, cap: usize) -> usize {
        match self {
            Mode::Plain => cap,
            Mode::Columns(w) | Mode::Tty(w) => (*w).min(cap),
        }
    }

    /// Every mode a help call runs in, for these widths.
    pub fn all(widths: &[usize]) -> Vec<Mode> {
        let mut v = vec![Mode::Plain];
        v.extend(widths.iter().map(|w| Mode::Columns(*w)));
        v.extend(widths.iter().map(|w| Mode::Tty(*w)));
        v
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub rule: &'static str,
    /// The command path, e.g. "jig verdict".
    pub command: String,
    /// "-h", "--help" or the probe flag.
    pub flag: String,
    pub mode: String,
    /// 1-based line in that output; 0 when the finding is about the call.
    pub line: usize,
    pub detail: String,
    /// Findings with the same rule, command, flag and group are one finding
    /// seen in several modes (the group is the allowed width for CH1).
    pub group: usize,
}

/// One finding and every mode it was seen in.
pub struct Grouped {
    pub first: Finding,
    pub modes: Vec<String>,
}

/// Merge findings that differ only by mode, keeping the first (plain run first).
pub fn group(findings: impl IntoIterator<Item = Finding>) -> Vec<Grouped> {
    let mut v: Vec<Grouped> = Vec::new();
    for f in findings {
        match v.iter_mut().find(|g| {
            g.first.rule == f.rule
                && g.first.command == f.command
                && g.first.flag == f.flag
                && g.first.group == f.group
                && (g.first.line == f.line || f.rule.starts_with("CH1"))
        }) {
            Some(g) => {
                if !g.modes.contains(&f.mode) {
                    g.modes.push(f.mode);
                }
            }
            None => {
                let mode = f.mode.clone();
                v.push(Grouped {
                    first: f,
                    modes: vec![mode],
                });
            }
        }
    }
    v
}

impl Grouped {
    pub fn render(&self) -> String {
        let f = &self.first;
        let at = if f.line == 0 {
            String::new()
        } else {
            format!(" line {}", f.line)
        };
        format!(
            "{} {} {}{at}: {} [{}]",
            f.rule,
            f.command,
            f.flag,
            f.detail,
            self.modes.join(", ")
        )
    }
}

impl Finding {
    pub fn render(&self) -> String {
        let at = if self.line == 0 {
            String::new()
        } else {
            format!(" line {}", self.line)
        };
        format!(
            "{} {} {} ({}){at}: {}",
            self.rule, self.command, self.flag, self.mode, self.detail
        )
    }
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)").unwrap()
});

/// Output bytes as the text a reader sees: ANSI escapes and carriage returns
/// removed, tabs expanded to 8.
pub fn normalize(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let s = ANSI.replace_all(&s, "");
    let mut out = String::with_capacity(s.len());
    for line in s.split('\n') {
        let line = line.trim_end_matches('\r').replace('\r', "");
        let mut col = 0;
        for ch in line.chars() {
            if ch == '\t' {
                let n = 8 - col % 8;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            } else {
                out.push(ch);
                col += 1;
            }
        }
        out.push('\n');
    }
    out.pop();
    out
}

fn width(line: &str) -> usize {
    line.chars().count()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// A column-0 heading such as "Commands:" or "Options:".
fn heading(line: &str) -> Option<&str> {
    static H: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z0-9 /_()-]*):$").unwrap());
    H.captures(line).map(|c| c.get(1).unwrap().as_str())
}

fn is_examples(h: &str) -> bool {
    let h = h.to_ascii_lowercase();
    h == "example" || h == "examples"
}

#[derive(Debug, Default)]
pub struct Row {
    pub line: usize,
    pub name: String,
    pub desc: String,
}

#[derive(Debug, Default)]
pub struct Section {
    pub heading: String,
    pub rows: Vec<Row>,
}

impl Section {
    /// A command list: every row name is one lowercase command token.
    pub fn is_commands(&self) -> bool {
        static CMD: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9_.-]*$").unwrap());
        !self.rows.is_empty() && self.rows.iter().all(|r| CMD.is_match(&r.name))
    }
}

/// Lists found in one help output, plus layout faults (CH1b) as (line, detail).
#[derive(Debug, Default)]
pub struct Layout {
    pub sections: Vec<Section>,
    pub faults: Vec<(usize, String)>,
}

/// Split a row line at its first run of two or more spaces.
fn split_row(body: &str) -> (&str, Option<(usize, &str)>) {
    match body.find("  ") {
        Some(i) => {
            let rest = &body[i..];
            let d = rest.len() - rest.trim_start().len();
            let desc = rest.trim_start();
            if desc.is_empty() {
                (&body[..i], None)
            } else {
                (&body[..i], Some((i + d, desc)))
            }
        }
        None => (body, None),
    }
}

/// Read the lists of one help output. Rows sit at the list's first indent;
/// a row's description, and every continuation line, must stay right of the
/// name column: at or after the column where the description began, or for
/// a description on its own line, at least 4 columns deeper than the names.
pub fn layout(text: &str) -> Layout {
    let mut out = Layout::default();
    let mut cur: Option<Section> = None;
    let mut free = true; // outside any list (or in an examples section)
    let mut row_indent = 0usize;
    let mut desc_col: Option<usize> = None;
    let mut cont_min = 0usize;
    // Layout faults count only in a section that is a list: one with at
    // least one "name  description" row. Free text (notes, after_help) is
    // not judged for columns.
    let mut pending: Vec<(usize, String)> = Vec::new();
    let mut listy = false;
    let close = |s: Section, pending: &mut Vec<(usize, String)>, listy: bool, out: &mut Layout| {
        if listy {
            out.faults.append(pending);
            out.sections.push(s);
        } else {
            pending.clear();
        }
    };
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.trim().is_empty() {
            continue;
        }
        let ind = indent(line);
        if ind == 0 {
            if let Some(s) = cur.take() {
                close(s, &mut pending, listy, &mut out);
            }
            listy = false;
            match heading(line) {
                Some(h) if !is_examples(h) => {
                    cur = Some(Section {
                        heading: h.to_string(),
                        rows: Vec::new(),
                    });
                    free = false;
                    row_indent = 0;
                }
                _ => free = true,
            }
            continue;
        }
        if free {
            continue;
        }
        let Some(sec) = cur.as_mut() else { continue };
        let body = &line[ind..];
        // clap indents an option without a short flag deeper ("      --format")
        // than one with ("  -h, --help"), so an option-shaped line is a row at
        // any indent left of the current description column.
        let option_row = body.starts_with('-') && (sec.rows.is_empty() || ind < cont_min);
        if row_indent == 0 || (option_row && ind < row_indent) {
            row_indent = ind;
        }
        if ind == row_indent || option_row {
            let (name, desc) = split_row(body);
            let option_like =
                name.starts_with('-') || name.starts_with('<') || name.starts_with('[');
            if !option_like && name.contains(' ') {
                pending.push((
                    n,
                    format!("text at column {} sits in the name column (a description wrapped under the names?)", ind + 1),
                ));
                continue;
            }
            match desc {
                Some((off, d)) => {
                    listy = true;
                    desc_col = Some(ind + off);
                    cont_min = ind + off;
                    sec.rows.push(Row {
                        line: n,
                        name: name.trim().to_string(),
                        desc: d.trim().to_string(),
                    });
                }
                None => {
                    desc_col = None;
                    cont_min = ind + 4;
                    sec.rows.push(Row {
                        line: n,
                        name: name.trim().to_string(),
                        desc: String::new(),
                    });
                }
            }
        } else if ind < row_indent {
            pending.push((
                n,
                format!(
                    "line starts at column {}, left of the list's names (column {})",
                    ind + 1,
                    row_indent + 1
                ),
            ));
        } else if sec.rows.is_empty() {
            continue;
        } else if ind < cont_min {
            let what = match desc_col {
                Some(d) => format!("left of its description column ({})", d + 1),
                None => format!(
                    "not deeper than the names (needs column {} or more)",
                    cont_min + 1
                ),
            };
            pending.push((n, format!("continuation at column {} is {what}", ind + 1)));
        } else if let Some(r) = sec.rows.last_mut() {
            if !r.desc.is_empty() {
                r.desc.push(' ');
            }
            r.desc.push_str(body.trim());
        }
    }
    if let Some(s) = cur.take() {
        close(s, &mut pending, listy, &mut out);
    }
    out
}

/// Lines wider than `allowed`: (line, width).
pub fn too_wide(text: &str, allowed: usize) -> Vec<(usize, usize)> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let w = width(l);
            (w > allowed).then_some((i + 1, w))
        })
        .collect()
}

static REFS: LazyLock<[(Regex, &'static str); 4]> = LazyLock::new(|| {
    [
        (
            Regex::new(r"\b(REQ|DD|SR|VER)-\d+").unwrap(),
            "a requirement id",
        ),
        (
            Regex::new(r"(^|[^\w/&#])#\d+\b").unwrap(),
            "an issue reference",
        ),
        (
            Regex::new(r"\bv\d+\.\d+(\.\d+)*\b").unwrap(),
            "a version tag",
        ),
        (
            Regex::new(r"(^|[^\w.])\d+\.\d+\.\d+\b").unwrap(),
            "a version number",
        ),
    ]
});

static BRACKETS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(default|possible values|aliases|env|short aliases|alias): [^\]]*\]").unwrap()
});

/// References that do not belong in help text (CH4): (line, what). The
/// usage block, examples sections and clap's value brackets are skipped.
pub fn refs(text: &str) -> Vec<(usize, &'static str)> {
    let mut v = Vec::new();
    let mut skip = false;
    for (i, line) in text.lines().enumerate() {
        if indent(line) == 0 && !line.trim().is_empty() {
            skip = line.starts_with("Usage:") || heading(line).is_some_and(is_examples);
        }
        if skip {
            continue;
        }
        let clean = BRACKETS.replace_all(line, "");
        for (re, what) in REFS.iter() {
            if re.is_match(&clean) {
                v.push((i + 1, *what));
                break;
            }
        }
    }
    v
}

/// True when the output has an "Examples:" section.
pub fn has_examples(text: &str) -> bool {
    text.lines().any(|l| heading(l).is_some_and(is_examples))
}

/// Subcommands listed in a help output, without clap's `help`.
pub fn subcommands(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut v = Vec::new();
    for s in layout(text).sections.iter().filter(|s| s.is_commands()) {
        for r in &s.rows {
            if r.name != "help" && seen.insert(r.name.clone()) {
                v.push(r.name.clone());
            }
        }
    }
    v
}

/// The result of one help call.
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
}

/// Judge one help output. `content` is false for the extra width runs: the
/// content rules (CH2, CH3, CH4, CH5) are judged once, on the plain run.
pub fn judge(
    cfg: &Config,
    command: &str,
    flag: &str,
    mode: Mode,
    out: &Output,
    content: bool,
) -> Vec<Finding> {
    let mut v = Vec::new();
    let f = |rule: &'static str, line: usize, detail: String| Finding {
        rule,
        command: command.to_string(),
        flag: flag.to_string(),
        mode: mode.label(),
        line,
        detail,
        group: 0,
    };
    if out.code != Some(0) || out.stdout.trim().is_empty() {
        if content {
            let what = match out.code {
                Some(0) => "printed nothing to stdout".to_string(),
                Some(c) => format!("exited {c}"),
                None => "was killed by a signal".to_string(),
            };
            v.push(f("CH5 exit", 0, what));
        }
        return v;
    }
    let text = &out.stdout;
    let allowed = mode.allowed(cfg.width_cap);
    let wide = too_wide(text, allowed);
    if let Some((line, w)) = wide.first() {
        let more = if wide.len() > 1 {
            format!(" ({} lines over)", wide.len())
        } else {
            String::new()
        };
        let mut x = f(
            "CH1 width",
            *line,
            format!("{w} columns, over the {allowed} this run allows{more}"),
        );
        x.group = allowed;
        v.push(x);
    }
    let lay = layout(text);
    if let Some((line, d)) = lay.faults.first() {
        v.push(f("CH1b separable", *line, d.clone()));
    }
    if !content {
        return v;
    }
    for s in lay.sections.iter().filter(|s| s.is_commands()) {
        for r in &s.rows {
            let n = r.desc.chars().count();
            if n > cfg.desc_max {
                v.push(f(
                    "CH2 desc",
                    r.line,
                    format!(
                        "'{}' description is {n} characters, over {}",
                        r.name, cfg.desc_max
                    ),
                ));
            }
        }
        if s.rows.len() > cfg.per_heading_max {
            v.push(f(
                "CH3 ungrouped",
                s.rows.first().map_or(0, |r| r.line),
                format!(
                    "{} commands under '{}:', over {}; group them under headings",
                    s.rows.len(),
                    s.heading,
                    cfg.per_heading_max
                ),
            ));
        }
    }
    for (line, what) in refs(text) {
        v.push(f("CH4 refs", line, format!("{what} in help text")));
    }
    v
}

/// CH7: `-h` is the short form, `--help` the long one; the top-level
/// `--help` carries examples.
pub fn judge_pair(command: &str, top: bool, short: &Output, long: &Output) -> Vec<Finding> {
    let mut v = Vec::new();
    if short.code != Some(0) || long.code != Some(0) {
        return v;
    }
    let f = |flag: &str, detail: String| Finding {
        rule: "CH7 short-long",
        command: command.to_string(),
        flag: flag.to_string(),
        mode: Mode::Plain.label(),
        line: 0,
        detail,
        group: 0,
    };
    let (s, l) = (short.stdout.lines().count(), long.stdout.lines().count());
    if s > l {
        v.push(f("-h", format!("-h has {s} lines, more than --help's {l}")));
    }
    if top && !has_examples(&long.stdout) {
        v.push(f(
            "--help",
            "the top-level --help has no 'Examples:' section".into(),
        ));
    }
    v
}

/// CH6: an unknown flag must exit 2.
pub fn judge_unknown(command: &str, flag: &str, code: Option<i32>) -> Option<Finding> {
    (code != Some(2)).then(|| Finding {
        rule: "CH6 unknown",
        command: command.to_string(),
        flag: flag.to_string(),
        mode: Mode::Plain.label(),
        line: 0,
        detail: match code {
            Some(c) => format!("an unknown flag exited {c}, not 2"),
            None => "an unknown flag was killed by a signal, not exit 2".into(),
        },
        group: 0,
    })
}

#[cfg(unix)]
pub mod run;
