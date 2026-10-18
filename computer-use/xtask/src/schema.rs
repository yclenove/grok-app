//! Schema golden files and exact UTF-8 catalog byte-budget enforcement.

use crate::budgets::TOOL_CATALOG_BYTES;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{fs, path::Path};

fn sort_keys(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_by_key(|(key, _)| *key);
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), sort_keys(value)))
                    .collect(),
            )
        }
        Value::Array(array) => Value::Array(array.iter().map(sort_keys).collect()),
        _ => value.clone(),
    }
}

pub(crate) fn canonical_json(value: &Value) -> Result<String> {
    let mut text = serde_json::to_string_pretty(&sort_keys(value))?;
    text.push('\n');
    Ok(text)
}

pub(crate) fn golden(directory: &Path, update: bool) -> Result<()> {
    let files = [
        (
            "tools.json",
            canonical_json(&gcu_protocol::tools_list_json())?,
        ),
        (
            "errors.json",
            canonical_json(&gcu_protocol::error::errors_json())?,
        ),
    ];
    if update {
        fs::create_dir_all(directory).context("failed to create golden directory")?;
    }
    for (name, expected) in files {
        let path = directory.join(name);
        if update {
            fs::write(&path, expected)
                .with_context(|| format!("failed to write {}", path.display()))?;
        } else {
            let actual = fs::read_to_string(&path).with_context(|| {
                format!("missing or unreadable golden {name}; run cargo xtask golden --update")
            })?;
            if actual != expected {
                bail!("golden {name} differs; review the protocol change and run cargo xtask golden --update");
            }
        }
    }
    Ok(())
}

pub(crate) struct BudgetReport {
    pub total: usize,
    pub limit: usize,
    pub tools: Vec<(String, usize)>,
}
impl BudgetReport {
    pub fn passed(&self) -> bool {
        self.total <= self.limit
    }
    pub fn render(&self) -> String {
        let mut report = format!("tools/list: {} / {} UTF-8 bytes\n", self.total, self.limit);
        for (name, bytes) in &self.tools {
            report.push_str(&format!("{name}: {bytes} bytes\n"));
        }
        report
    }
}

pub(crate) fn budget_report(tools: &Value, limit: usize) -> Result<BudgetReport> {
    let mut sizes = Vec::new();
    for tool in tools.as_array().context("tools/list must be an array")? {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .context("tool name must be a string")?;
        sizes.push((name.into(), serde_json::to_vec(tool)?.len()));
    }
    Ok(BudgetReport {
        total: serde_json::to_vec(tools)?.len(),
        limit,
        tools: sizes,
    })
}

pub(crate) fn run_golden(update: bool) -> Result<()> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live under computer-use")?;
    golden(&workspace.join("crates/gcu-protocol/golden"), update)?;
    let directory = workspace.join("crates/gcu-host-protocol/golden");
    for (name, schema) in [
        ("messages.json", gcu_host_protocol::messages::schemas()),
        ("handshake.json", gcu_host_protocol::auth::schemas()),
    ] {
        let expected = canonical_json(&schema)?;
        let path = directory.join(name);
        if update {
            fs::create_dir_all(&directory)?;
            fs::write(&path, expected)?;
        } else if fs::read_to_string(&path)
            .context("missing Host golden; run xtask golden --update")?
            != expected
        {
            bail!("Host golden {name} differs; review and run xtask golden --update");
        }
    }
    println!(
        "PASS: protocol golden files {}",
        if update { "updated" } else { "match" }
    );
    Ok(())
}

pub(crate) fn enforce_budget(report: &BudgetReport) -> Result<()> {
    print!("{}", report.render());
    if !report.passed() {
        bail!("default tool catalog exceeds {} bytes", report.limit);
    }
    Ok(())
}

pub(crate) fn run_budget() -> Result<()> {
    let report = budget_report(&gcu_protocol::tools_list_json(), TOOL_CATALOG_BYTES)?;
    enforce_budget(&report)?;
    println!("PASS: default tool catalog byte budget");
    Ok(())
}
