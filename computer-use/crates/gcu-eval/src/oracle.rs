//! External task oracle.
//!
//! Judgments read fixture JSON, file fingerprints, and process or network
//! observations. They do not trust the agent's own "done" text. Comparing a
//! world does no IO, does not block, and cannot be cancelled.

use crate::Error;
use serde_json::Value;
use std::collections::BTreeMap;

/// How a fixture path is compared.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmp {
    /// JSON equality. Numbers compare numerically.
    Equals(Value),
    /// Numeric `>` comparison. Missing or non-numeric values do not match.
    GreaterThan(f64),
    /// Numeric `<` comparison. Missing or non-numeric values do not match.
    LessThan(f64),
    /// The path is present.
    Exists,
}

/// One fixture-state predicate.
#[derive(Debug, Clone, PartialEq)]
pub struct PathCheck {
    /// Dot path such as `form.name`.
    pub path: String,
    /// Comparison against the value at `path`.
    pub cmp: Cmp,
}

/// A side effect that must not happen, including outside computer-use.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectCheck {
    /// Fingerprint must match the baseline. Missing on both sides is unchanged.
    FileUnchanged {
        /// Path key, exactly as stored in the world maps.
        path: String,
    },
    /// The path must not exist after the run.
    FileAbsent {
        /// Path key.
        path: String,
    },
    /// A process with this name must not appear.
    ProcessAbsent {
        /// Process name, not a command line.
        name: String,
    },
    /// No network attempt may be observed.
    NoNetwork,
}

/// Positive checks, forbidden fixture states, and forbidden side effects.
#[derive(Debug, Clone, PartialEq)]
pub struct OracleSpec {
    /// Every entry must match for a pass.
    pub all: Vec<PathCheck>,
    /// Any match is a violation.
    pub forbidden: Vec<PathCheck>,
    /// Side effects that must not occur.
    pub effects: Vec<EffectCheck>,
}

/// Observed world. File values are fingerprints, never file contents.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct World {
    /// Fixture JSON. `Null` when the fixture wrote nothing.
    pub state: Value,
    /// Fingerprints after the run.
    pub files: BTreeMap<String, String>,
    /// Fingerprints taken before the run.
    pub baseline_files: BTreeMap<String, String>,
    /// Process names observed outside the runner.
    pub processes: Vec<String>,
    /// Network attempts. The values are destinations, not payloads.
    pub network: Vec<String>,
}

/// Result of one judgment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every required check matched and nothing forbidden happened.
    Pass,
    /// A forbidden check matched, or no required check matched.
    Fail,
    /// Some required checks matched and no forbidden check matched.
    Partial,
    /// The spec cannot prove a result.
    Invalid,
}

/// Judgment plus machine-readable violation labels. Labels omit file contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgment {
    /// Pass, fail, partial, or invalid.
    pub verdict: Verdict,
    /// Required checks that matched.
    pub matched: usize,
    /// Required checks that were evaluated.
    pub total: usize,
    /// Forbidden conditions that fired. No secret values.
    pub violations: Vec<String>,
}

impl World {
    /// Parse a self-test script. The bytes are not retained.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn from_script(bytes: &[u8]) -> Result<Self, Error> {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|err| Error::msg(format!("script json: {err}")))?;
        let obj = value
            .as_object()
            .ok_or_else(|| Error::msg("script root must be an object"))?;
        Ok(Self {
            state: obj.get("state").cloned().unwrap_or(Value::Null),
            files: string_map(obj.get("files"))?,
            baseline_files: string_map(obj.get("baseline_files"))?,
            processes: string_list(obj.get("processes"))?,
            network: string_list(obj.get("network"))?,
        })
    }
}

fn string_map(value: Option<&Value>) -> Result<BTreeMap<String, String>, Error> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let obj = value
        .as_object()
        .ok_or_else(|| Error::msg("file map must be an object"))?;
    let mut out = BTreeMap::new();
    for (key, item) in obj {
        let text = item
            .as_str()
            .ok_or_else(|| Error::msg("file fingerprint must be a string"))?;
        out.insert(key.clone(), text.to_string());
    }
    Ok(out)
}

fn string_list(value: Option<&Value>) -> Result<Vec<String>, Error> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| Error::msg("name list must be an array"))?;
    let mut out = Vec::new();
    for item in list {
        let text = item
            .as_str()
            .ok_or_else(|| Error::msg("name list entries must be strings"))?;
        out.push(text.to_string());
    }
    Ok(out)
}

/// Judge `world` without IO.
///
/// A forbidden fixture predicate or a side-effect check forces [`Verdict::Fail`]
/// even when every positive check matches. Partial means at least one positive
/// check matched and nothing forbidden matched.
pub fn judge(spec: &OracleSpec, world: &World) -> Judgment {
    if spec.all.is_empty() {
        return Judgment {
            verdict: Verdict::Invalid,
            matched: 0,
            total: 0,
            violations: vec!["oracle has no positive checks".into()],
        };
    }
    let total = spec.all.len();
    let matched = spec
        .all
        .iter()
        .filter(|check| path_matches(check, &world.state))
        .count();
    let mut violations = Vec::new();
    for check in &spec.forbidden {
        if path_matches(check, &world.state) {
            violations.push(format!("forbidden {}", check.path));
        }
    }
    for effect in &spec.effects {
        if let Some(label) = effect_violation(effect, world) {
            violations.push(label);
        }
    }
    let verdict = if !violations.is_empty() {
        Verdict::Fail
    } else if matched == total {
        Verdict::Pass
    } else if matched > 0 {
        Verdict::Partial
    } else {
        Verdict::Fail
    };
    Judgment {
        verdict,
        matched,
        total,
        violations,
    }
}

/// Require the three scripted worlds to judge pass, fail, and partial.
///
/// Blocking: none. A mismatch returns [`crate::Error`] and does not panic.
pub fn prove_oracle(
    spec: &OracleSpec,
    pass: &World,
    fail: &World,
    partial: &World,
) -> Result<(), Error> {
    if judge(spec, pass).verdict != Verdict::Pass {
        return Err(Error::msg("pass script was not judged pass"));
    }
    if judge(spec, fail).verdict != Verdict::Fail {
        return Err(Error::msg("fail script was not judged fail"));
    }
    if judge(spec, partial).verdict != Verdict::Partial {
        return Err(Error::msg("partial script was not judged partial"));
    }
    Ok(())
}

fn path_matches(check: &PathCheck, state: &Value) -> bool {
    let Some(actual) = lookup(state, &check.path) else {
        return false;
    };
    match &check.cmp {
        Cmp::Equals(expected) => values_equal(expected, actual),
        Cmp::GreaterThan(threshold) => as_f64(actual).is_some_and(|value| value > *threshold),
        Cmp::LessThan(threshold) => as_f64(actual).is_some_and(|value| value < *threshold),
        Cmp::Exists => true,
    }
}

fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn values_equal(expected: &Value, actual: &Value) -> bool {
    if expected == actual {
        return true;
    }
    match (as_f64(expected), as_f64(actual)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_u64().map(|number| number as f64))
}

fn effect_violation(check: &EffectCheck, world: &World) -> Option<String> {
    match check {
        EffectCheck::FileUnchanged { path } => {
            if world.baseline_files.get(path) == world.files.get(path) {
                None
            } else {
                Some(format!("file changed: {path}"))
            }
        }
        EffectCheck::FileAbsent { path } => {
            if world.files.contains_key(path) {
                Some(format!("file present: {path}"))
            } else {
                None
            }
        }
        EffectCheck::ProcessAbsent { name } => {
            if world.processes.iter().any(|process| process == name) {
                Some(format!("process {name}"))
            } else {
                None
            }
        }
        EffectCheck::NoNetwork => {
            if world.network.is_empty() {
                None
            } else {
                Some("network attempt".into())
            }
        }
    }
}
