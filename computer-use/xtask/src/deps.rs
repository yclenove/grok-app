//! Table-driven enforcement of architecture §2, including the external App Host.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{fs, path::Path, process::Command};

#[derive(Deserialize)]
struct Metadata {
    workspace_members: Vec<String>,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    // Cargo supplies the canonical package name here, even for renamed edges.
    name: String,
    #[serde(default)]
    kind: Option<String>,
}

enum CrateName {
    Exact(&'static str),
    Prefix(&'static str),
}

enum Allowed {
    Only(&'static [&'static str]),
    Driver,
    AppProtocols,
    Any,
}

struct Rule {
    owner: CrateName,
    allowed: Allowed,
}

const CORE_DEV: &[&str] = &["insta", "criterion", "gcu-driver-fake", "proptest"];
const MCP_DEV: &[&str] = &["gcu-driver-fake", "png"];

// Keep this table aligned with docs/plans/computer-use-v2/03-architecture.md §2.
// Inspect every declared edge: optional, target-specific, dev, and build. Using
// --no-deps keeps external packages out without filtering their declared edges.
const RULES: &[Rule] = &[
    Rule {
        owner: CrateName::Exact("gcu-protocol"),
        allowed: Allowed::Only(&["serde", "serde_json", "schemars", "thiserror"]),
    },
    Rule {
        owner: CrateName::Exact("gcu-host-api"),
        allowed: Allowed::Only(&["gcu-protocol", "async-trait"]),
    },
    Rule {
        owner: CrateName::Exact("gcu-core"),
        allowed: Allowed::Only(&[
            "gcu-protocol",
            "gcu-host-api",
            "tokio",
            "parking_lot",
            "similar",
            "unicode-segmentation",
            "image",
            "tracing",
            "smol_str",
            "smallvec",
            "bitflags",
            "thiserror",
            "async-trait",
            "serde",
            "serde_json",
            "toml",
            "unicode-normalization",
            "futures-core",
        ]),
    },
    Rule {
        owner: CrateName::Prefix("gcu-driver-"),
        allowed: Allowed::Driver,
    },
    Rule {
        owner: CrateName::Exact("gcu-mcp"),
        allowed: Allowed::Only(&[
            "gcu-protocol",
            "gcu-core",
            "gcu-host-api",
            "gcu-host-protocol",
            "rmcp",
            "tokio",
            "axum",
            "serde",
            "serde_json",
            "thiserror",
            "async-trait",
            "base64",
            "tempfile",
        ]),
    },
    Rule {
        owner: CrateName::Exact("gcu-cli"),
        allowed: Allowed::Any,
    },
    Rule {
        owner: CrateName::Exact("gcu-host-protocol"),
        allowed: Allowed::Only(&[
            "gcu-protocol",
            "serde",
            "serde_json",
            "schemars",
            "thiserror",
            "tokio",
            "interprocess",
            "hmac",
            "sha2",
            "rand",
            "libc",
            "windows",
        ]),
    },
    Rule {
        owner: CrateName::Exact("src-tauri"),
        allowed: Allowed::AppProtocols,
    },
    // Script dependencies are explicitly constrained by architecture §2.
    Rule {
        owner: CrateName::Exact("gcu-script"),
        allowed: Allowed::Only(&["gcu-core", "gcu-protocol", "rquickjs"]),
    },
    // P9-01 fixed the YAML/CLI/report set. Task YAML is parsed in-tree so the
    // crate does not take the unmaintained serde_yaml crate.
    Rule {
        owner: CrateName::Exact("gcu-eval"),
        allowed: Allowed::Only(&[
            "gcu-protocol",
            "gcu-core",
            "gcu-driver-fake",
            "serde",
            "serde_json",
            "tokio",
            "clap",
            "anyhow",
            "thiserror",
            "tracing",
            // Unix only: kill the client's process group and open the auth seed with O_NOFOLLOW.
            "libc",
        ]),
    },
    Rule {
        owner: CrateName::Exact("gcu-overlay"),
        allowed: Allowed::Any,
    },
    Rule {
        owner: CrateName::Exact("gcu-fixture"),
        allowed: Allowed::Any,
    },
    Rule {
        owner: CrateName::Exact("xtask"),
        allowed: Allowed::Any,
    },
];

fn rule_for(owner: &str) -> Result<&'static Rule> {
    RULES
        .iter()
        .find(|rule| match rule.owner {
            CrateName::Exact(name) => owner == name,
            CrateName::Prefix(prefix) => owner.starts_with(prefix),
        })
        .with_context(|| format!("no architecture dependency rule for workspace crate {owner}"))
}

impl Rule {
    fn allows(&self, dependency: &str) -> bool {
        match self.allowed {
            Allowed::Only(names) => names.contains(&dependency),
            // Native bindings and their supporting external crates are allowed.
            // Within computer-use, a driver may only use protocol and core.
            Allowed::Driver => {
                !dependency.starts_with("gcu-") || matches!(dependency, "gcu-protocol" | "gcu-core")
            }
            // App's existing non-computer-use dependencies remain unrestricted.
            Allowed::AppProtocols => {
                !dependency.starts_with("gcu-")
                    || matches!(dependency, "gcu-protocol" | "gcu-host-protocol")
            }
            Allowed::Any => true,
        }
    }
}

fn check_edge(rule: &Rule, owner: &str, dependency: &str, violations: &mut Vec<String>) {
    if !rule.allows(dependency) {
        violations.push(format!(
            "{owner} -> {dependency}: forbidden by architecture §2"
        ));
    }
}

pub(crate) fn check_metadata(input: &str) -> Result<Vec<String>> {
    let metadata: Metadata = serde_json::from_str(input).context("invalid cargo metadata JSON")?;
    let mut violations = Vec::new();
    for member in &metadata.workspace_members {
        let package = metadata
            .packages
            .iter()
            .find(|package| &package.id == member)
            .with_context(|| format!("workspace member {member} missing from cargo metadata"))?;
        let rule = rule_for(&package.name)?;
        for dependency in &package.dependencies {
            if package.name == "gcu-core"
                && dependency.kind.as_deref() == Some("dev")
                && CORE_DEV.contains(&dependency.name.as_str())
            {
                continue;
            }
            if package.name == "gcu-mcp"
                && dependency.kind.as_deref() == Some("dev")
                && MCP_DEV.contains(&dependency.name.as_str())
            {
                continue;
            }
            check_edge(rule, &package.name, &dependency.name, &mut violations);
        }
    }
    Ok(violations)
}

pub(crate) fn check_tauri_manifest(input: &str) -> Result<Vec<String>> {
    let manifest: toml::Table = toml::from_str(input).context("invalid src-tauri/Cargo.toml")?;
    let inherited = manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table);
    let mut violations = Vec::new();
    check_tauri_tables(&manifest, inherited, &mut violations)?;
    if let Some(targets) = manifest.get("target") {
        for target in targets
            .as_table()
            .context("target must be a table")?
            .values()
        {
            check_tauri_tables(
                target.as_table().context("target entry must be a table")?,
                inherited,
                &mut violations,
            )?;
        }
    }
    Ok(violations)
}

fn check_tauri_tables(
    table: &toml::Table,
    inherited: Option<&toml::Table>,
    violations: &mut Vec<String>,
) -> Result<()> {
    let rule = rule_for("src-tauri")?;
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(dependencies) = table.get(section) {
            for (alias, value) in dependencies
                .as_table()
                .with_context(|| format!("{section} must be a table"))?
            {
                let value = if value.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                    inherited
                        .and_then(|dependencies| dependencies.get(alias))
                        .with_context(|| format!("unresolved inherited dependency {alias}"))?
                } else {
                    value
                };
                let name = match value {
                    toml::Value::String(_) => alias.as_str(),
                    toml::Value::Table(dependency) => match dependency.get("package") {
                        Some(package) => package
                            .as_str()
                            .context("dependency package name must be a string")?,
                        None => alias.as_str(),
                    },
                    _ => bail!("invalid dependency declaration for {alias}"),
                };
                check_edge(rule, "src-tauri", name, violations);
            }
        }
    }
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    // Resolve from the build-time manifest location, independent of the cwd.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live under the computer-use workspace")?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .arg("--manifest-path")
        .arg(workspace.join("Cargo.toml"))
        .output()
        .context("failed to execute cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut violations =
        check_metadata(std::str::from_utf8(&output.stdout).context("metadata is not UTF-8")?)?;
    let tauri_manifest = workspace
        .parent()
        .context("computer-use must live under the repository root")?
        .join("src-tauri/Cargo.toml");
    violations.extend(check_tauri_manifest(
        &fs::read_to_string(&tauri_manifest)
            .with_context(|| format!("failed to read {}", tauri_manifest.display()))?,
    )?);
    if !violations.is_empty() {
        bail!("dependency rules failed:\n{}", violations.join("\n"));
    }
    println!("PASS: architecture §2 dependency rules (workspace + src-tauri)");
    Ok(())
}
