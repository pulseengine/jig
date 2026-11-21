//! `jig classes`: the adopter's sieve.toml uses only the fleet class
//! vocabulary (REQ-016).

/// The fleet class vocabulary, in output order. Shared with the workflow
/// generator and `jig docs classes`.
pub const VOCABULARY: [&str; 9] = [
    "build",
    "unit_test",
    "integration",
    "emulator",
    "coverage",
    "mutation",
    "fuzz",
    "bench",
    "docs",
];

/// The only accepted `schema` value.
pub const SCHEMA: &str = "sieve.config.v1";

#[derive(Debug, PartialEq)]
pub struct Report {
    /// Vocabulary classes the config defines, in vocabulary order.
    pub defined: Vec<String>,
    /// Defined class names outside the vocabulary, sorted.
    pub unknown: Vec<String>,
}

impl Report {
    /// Every reason the check is red; empty means green.
    pub fn reasons(&self) -> Vec<String> {
        let mut r = Vec::new();
        if self.defined.is_empty() && self.unknown.is_empty() {
            r.push("no [class.*] is defined: nothing was compared".to_string());
        }
        for u in &self.unknown {
            r.push(format!(
                "class '{u}' is not in the fleet vocabulary ({})",
                VOCABULARY.join(", ")
            ));
        }
        r
    }
}

/// Parse a sieve config. `Err` means it could not be read (exit 2).
pub fn check(text: &str) -> Result<Report, String> {
    let root: toml::Table = text
        .parse()
        .map_err(|e| format!("config is not TOML: {e}"))?;
    match root.get("schema") {
        Some(toml::Value::String(s)) if s == SCHEMA => {}
        Some(toml::Value::String(s)) => {
            return Err(format!("schema is '{s}', expected '{SCHEMA}'"));
        }
        Some(_) => return Err(format!("schema is not a string, expected '{SCHEMA}'")),
        None => return Err(format!("schema is missing, expected '{SCHEMA}'")),
    }
    let mut names: Vec<String> = Vec::new();
    match root.get("class") {
        None => {}
        Some(toml::Value::Table(t)) => {
            for (name, v) in t {
                if !v.is_table() {
                    return Err(format!("class.{name} is not a table"));
                }
                names.push(name.clone());
            }
        }
        Some(_) => return Err("class is not a table of tables".to_string()),
    }
    let defined = VOCABULARY
        .iter()
        .filter(|v| names.iter().any(|n| n == *v))
        .map(|s| s.to_string())
        .collect();
    let mut unknown: Vec<String> = names
        .into_iter()
        .filter(|n| !VOCABULARY.contains(&n.as_str()))
        .collect();
    unknown.sort();
    Ok(Report { defined, unknown })
}
