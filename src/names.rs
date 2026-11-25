//! Private-name guard (jig#17): tracked files must not contain a match of a
//! caller's private-name patterns. Pure judgement; the binary lists the
//! tracked files and reads them.
//!
//! A finding names the file, the line and the pattern's index, never the
//! matched text: baseline logs are public, and echoing a hostname would leak
//! it again.

use regex::bytes::Regex;
use std::collections::BTreeSet;

/// One pattern, numbered from 1 in file order (comments and blanks skipped).
pub struct Pattern {
    pub index: usize,
    pub re: Regex,
}

/// `file` is the path as git prints it, or `<tracked path N>` when the path
/// itself matches a pattern (see [`label`]). `line` 0 means the path matched.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub pattern: usize,
}

#[derive(Debug, Default)]
pub struct Report {
    pub scanned: usize,
    pub binary: usize,
    pub allowed: Vec<String>,
    pub findings: Vec<Finding>,
}

/// Parse a patterns file: one Rust regex per line, `#` starts a comment line,
/// blank lines are skipped. A bad pattern is an error that names its index
/// and line number but not its text.
pub fn parse_patterns(text: &str) -> Result<Vec<Pattern>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let index = out.len() + 1;
        let re = Regex::new(line)
            .map_err(|_| format!("pattern #{index} (line {}) is not a valid regex", n + 1))?;
        if re.is_match(b"") {
            return Err(format!(
                "pattern #{index} (line {}) matches the empty string, so it would match every line",
                n + 1
            ));
        }
        out.push(Pattern { index, re });
    }
    Ok(out)
}

/// Parse an allow file: one repository-relative path per line; a path ending
/// in `/` allows everything under that directory. `#` starts a comment line.
/// Wildcards, absolute paths and `..` are refused.
pub fn parse_allow(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains(['*', '?', '[', ']', '{', '}']) {
            return Err(format!(
                "allow line {}: wildcards are not accepted; list paths",
                n + 1
            ));
        }
        if line.starts_with('/') || line.starts_with("./") || line.split('/').any(|c| c == "..") {
            return Err(format!(
                "allow line {}: give a path relative to the repository root",
                n + 1
            ));
        }
        out.push(line.to_string());
    }
    Ok(out)
}

fn allowed(path: &str, allow: &[String]) -> bool {
    allow.iter().any(|a| {
        if a.ends_with('/') {
            path.starts_with(a.as_str())
        } else {
            path == a
        }
    })
}

/// Binary: a NUL byte in the first 8000 bytes (git's own heuristic). Counted
/// for the summary only: binary files are scanned too.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

/// Scan one file's bytes line by line (split on `\n`). The patterns match
/// bytes, so a file that is not UTF-8 or holds a NUL is scanned like any
/// other: skipping it would let one NUL byte hide a name.
pub fn scan_file(path: &str, bytes: &[u8], patterns: &[Pattern]) -> Vec<Finding> {
    let mut out = Vec::new();
    for (n, line) in bytes.split(|b| *b == b'\n').enumerate() {
        // A CRLF line is matched without its \r, so `$` anchors as in LF.
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        for p in patterns {
            if p.re.is_match(line) {
                out.push(Finding {
                    file: path.to_string(),
                    line: n + 1,
                    pattern: p.index,
                });
            }
        }
    }
    out
}

/// A path matches when the pattern matches the whole path, any component, or
/// each dot-separated piece or dot-ended prefix of one, so an anchored pattern
/// (`^host-[0-9]+$`) still finds `docs/host-7.md`.
pub fn path_matches(path: &str, p: &Pattern) -> bool {
    p.re.is_match(path.as_bytes())
        || path.split('/').any(|c| {
            // The component, each dot-separated piece of it, and each prefix
            // that ends before a dot: host-7.tar.gz tries host-7.tar.gz,
            // host-7, tar, gz, host-7 and host-7.tar.
            let dots = c.match_indices('.').map(|(i, _)| &c[..i]);
            std::iter::once(c)
                .chain(c.split('.'))
                .chain(dots)
                .any(|s| p.re.is_match(s.as_bytes()))
        })
}

/// How a tracked path may be printed: the path itself, or `<tracked path N>`
/// (N counts from 1 in `git ls-files` order) when the path matches a
/// pattern, so a private name in a file name is never printed either.
pub fn label(path: &str, n: usize, patterns: &[Pattern]) -> String {
    if patterns.iter().any(|p| path_matches(path, p)) {
        format!("<tracked path {n}>")
    } else {
        path.to_string()
    }
}

/// Judge every tracked file: its path, then its content. `read` returns a
/// file's bytes (a symlink's target text, as git stores it), or None when it
/// is tracked but absent from the tree (deleted, not staged): such a file is
/// skipped, since nothing of it is published from this tree.
pub fn check(
    tracked: &BTreeSet<String>,
    patterns: &[Pattern],
    allow: &[String],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Report {
    let mut rep = Report::default();
    for (i, path) in tracked.iter().enumerate() {
        let shown = label(path, i + 1, patterns);
        if allowed(path, allow) {
            rep.allowed.push(shown);
            continue;
        }
        for p in patterns {
            if path_matches(path, p) {
                rep.findings.push(Finding {
                    file: shown.clone(),
                    line: 0,
                    pattern: p.index,
                });
            }
        }
        let Some(bytes) = read(path) else { continue };
        if is_binary(&bytes) {
            rep.binary += 1;
        }
        rep.scanned += 1;
        rep.findings.extend(scan_file(&shown, &bytes, patterns));
    }
    rep
}
