//! Task YAML subset used by the evaluation suite.
//!
//! The subset is block maps, block sequences, and one-line flow collections.
//! It is intentionally smaller than YAML 1.2 so the runner does not depend on
//! the unmaintained `serde_yaml` crate. Parsing does no IO beyond the file the
//! caller passes. Invalid documents return [`crate::Error`] and do not panic.

use crate::oracle::{EffectCheck, OracleSpec, PathCheck};
use crate::yaml::{
    expect_map, expect_seq, map_get, parse_yaml, require, require_str, require_str_list,
    require_str_list_value, require_u64, yaml_to_f64, yaml_to_json, Yaml,
};
use crate::Error;
use std::path::{Path, PathBuf};

/// One evaluation task loaded from `eval/tasks`.
#[derive(Debug, Clone)]
pub struct Task {
    /// Stable id, `[a-z0-9][a-z0-9-]{0,63}`.
    pub id: String,
    /// `macos`, `windows`, and/or `linux`.
    pub platforms: Vec<String>,
    /// `native`, `browser`, `dialog`, `cross_app`, `text`, or `red_team`.
    pub category: String,
    /// Chinese instruction. Reports must not inline this text.
    pub prompt_zh: String,
    /// English instruction. Reports must not inline this text.
    pub prompt_en: String,
    /// Fixture to launch. Only `fixture` is accepted in this card.
    pub launch: String,
    /// Fake-driver scene name. Unknown scenes make the task invalid at run time.
    pub scene: String,
    /// External oracle. The agent does not grade itself.
    pub oracle: OracleSpec,
    /// Script that must be judged pass. Relative paths are resolved at load.
    pub pass_script: PathBuf,
    /// Script that must be judged fail.
    pub fail_script: PathBuf,
    /// Script that must be judged partial.
    pub partial_script: PathBuf,
    /// Suite names. Missing means `core`.
    pub suites: Vec<String>,
    /// Maximum model turns passed to the client.
    pub max_turns: u64,
    /// Wall-clock budget. The runner kills the client at this limit.
    pub max_seconds: u64,
}

/// Load every `*.yaml` task under `tasks_dir`.
///
/// Blocking: reads the directory tree and each file. Timeout: none. Cancellation:
/// none. A malformed file fails the load; the caller does not get a partial list.
pub fn load_tasks(tasks_dir: &Path) -> Result<Vec<Task>, Error> {
    let eval_root = tasks_dir
        .parent()
        .ok_or_else(|| Error::msg("tasks dir has no parent"))?
        .to_path_buf();
    let mut files = Vec::new();
    collect_yaml(tasks_dir, &mut files)?;
    files.sort();
    let mut tasks = Vec::with_capacity(files.len());
    for file in files {
        let text = std::fs::read_to_string(&file)
            .map_err(|err| Error::msg(format!("read {}: {err}", file.display())))?;
        if text.len() > 65_536 {
            return Err(Error::msg(format!("{} exceeds 64KiB", file.display())));
        }
        tasks.push(parse_task(&text, &eval_root, &file)?);
    }
    Ok(tasks)
}

fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    let entries = std::fs::read_dir(dir)
        .map_err(|err| Error::msg(format!("read {}: {err}", dir.display())))?;
    for entry in entries {
        let path = entry.map_err(|err| Error::msg(err.to_string()))?.path();
        if path.is_dir() {
            collect_yaml(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("yaml") {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_task(text: &str, eval_root: &Path, file: &Path) -> Result<Task, Error> {
    let yaml = parse_yaml(text).map_err(|err| Error::msg(format!("{}: {err}", file.display())))?;
    let map = expect_map(&yaml)?;
    let id = require_str(map, "id")?;
    if !valid_id(&id) {
        return Err(Error::msg(format!("bad task id {id}")));
    }
    let platforms = require_str_list(map, "platforms")?;
    for platform in &platforms {
        if !matches!(platform.as_str(), "macos" | "windows" | "linux") {
            return Err(Error::msg(format!("{id}: unknown platform {platform}")));
        }
    }
    let category = require_str(map, "category")?;
    if !matches!(
        category.as_str(),
        "native" | "browser" | "dialog" | "cross_app" | "text" | "red_team"
    ) {
        return Err(Error::msg(format!("{id}: unknown category {category}")));
    }
    let setup = expect_map(require(map, "setup")?)?;
    let launch = require_str(setup, "launch")?;
    if launch != "fixture" {
        return Err(Error::msg(format!("{id}: only the fixture app is allowed")));
    }
    let oracle_map = expect_map(require(map, "oracle")?)?;
    let checks = parse_checks(require(oracle_map, "all")?)?;
    if checks.is_empty() {
        return Err(Error::msg(format!("{id}: oracle.all is empty")));
    }
    let forbidden = match map_get(map, "forbidden") {
        Some(value) => parse_checks(value)?,
        None => Vec::new(),
    };
    let effects = match map_get(map, "effects") {
        Some(value) => parse_effects(value)?,
        None => Vec::new(),
    };
    let selftest = expect_map(require(map, "oracle_selftest")?)?;
    let budget = expect_map(require(map, "budget")?)?;
    let max_turns = require_u64(budget, "max_turns")?;
    let max_seconds = require_u64(budget, "max_seconds")?;
    if max_turns == 0 || max_seconds == 0 {
        return Err(Error::msg(format!("{id}: budget must be positive")));
    }
    let suites = match map_get(map, "suites") {
        Some(value) => require_str_list_value(value)?,
        None => vec!["core".to_string()],
    };
    Ok(Task {
        id,
        platforms,
        category,
        prompt_zh: require_str(map, "prompt_zh")?,
        prompt_en: require_str(map, "prompt_en")?,
        launch,
        scene: require_str(setup, "scene")?,
        oracle: OracleSpec {
            all: checks,
            forbidden,
            effects,
        },
        pass_script: resolve(eval_root, &require_str(selftest, "pass_script")?),
        fail_script: resolve(eval_root, &require_str(selftest, "fail_script")?),
        partial_script: resolve(eval_root, &require_str(selftest, "partial_script")?),
        suites,
        max_turns,
        max_seconds,
    })
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    id.len() <= 64 && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn resolve(root: &Path, relative: &str) -> PathBuf {
    root.join(relative)
}

fn parse_checks(value: &Yaml) -> Result<Vec<PathCheck>, Error> {
    let mut checks = Vec::new();
    for item in expect_seq(value)? {
        checks.push(parse_check(item)?);
    }
    Ok(checks)
}

fn parse_check(value: &Yaml) -> Result<PathCheck, Error> {
    let map = expect_map(value)?;
    let path = require_str(map, "path")?;
    let cmp = if let Some(expected) = map_get(map, "equals") {
        crate::oracle::Cmp::Equals(yaml_to_json(expected)?)
    } else if let Some(expected) = map_get(map, "greater_than") {
        crate::oracle::Cmp::GreaterThan(yaml_to_f64(expected)?)
    } else if let Some(expected) = map_get(map, "less_than") {
        crate::oracle::Cmp::LessThan(yaml_to_f64(expected)?)
    } else if map_get(map, "exists").is_some() {
        crate::oracle::Cmp::Exists
    } else {
        return Err(Error::msg(format!("check {path} has no comparator")));
    };
    Ok(PathCheck { path, cmp })
}

fn parse_effects(value: &Yaml) -> Result<Vec<EffectCheck>, Error> {
    let map = expect_map(value)?;
    let mut effects = Vec::new();
    if matches!(map_get(map, "forbid_network"), Some(Yaml::Bool(true))) {
        effects.push(EffectCheck::NoNetwork);
    }
    if let Some(list) = map_get(map, "forbidden_processes") {
        for name in require_str_list_value(list)? {
            effects.push(EffectCheck::ProcessAbsent { name });
        }
    }
    if let Some(list) = map_get(map, "unchanged_files") {
        for path in require_str_list_value(list)? {
            effects.push(EffectCheck::FileUnchanged { path });
        }
    }
    if let Some(list) = map_get(map, "absent_files") {
        for path in require_str_list_value(list)? {
            effects.push(EffectCheck::FileAbsent { path });
        }
    }
    Ok(effects)
}
