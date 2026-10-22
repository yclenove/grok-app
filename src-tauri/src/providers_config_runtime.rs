//! Structural runtime overlays, preserving strings and unknown CLI configuration.
use super::{invalid, write, ConfigSnapshot, Table, Value};
use crate::{extensions::ExtensionsPrefs, store::AppSettings};

fn table<'a>(root: &'a mut Table, path: &[&str]) -> Result<&'a mut Table, String> {
    let mut current = root;
    for name in path {
        let value = current
            .fields
            .entry((*name).into())
            .or_insert_with(|| Value::Table(Table::default()));
        let Value::Table(next) = value else {
            return Err(invalid());
        };
        current = next;
    }
    Ok(current)
}

fn boolean(root: &mut Table, path: &[&str], key: &str, value: bool) -> Result<(), String> {
    table(root, path)?
        .fields
        .insert(key.into(), Value::Atom(value.to_string()));
    Ok(())
}

impl ConfigSnapshot {
    fn overlay_permission(&mut self, permission: &str) -> Result<(), String> {
        table(&mut self.root, &["ui"])?.fields.insert(
            "permission_mode".into(),
            Value::String(crate::agent_prefs::ui_permission_mode(permission).into()),
        );
        boolean(
            &mut self.root,
            &["ui"],
            "yolo",
            matches!(
                crate::permission::PermissionPolicy::parse(permission),
                crate::permission::PermissionPolicy::AlwaysApprove
            ),
        )
    }

    fn overlay_compat(&mut self) -> Result<(), String> {
        for name in ["claude", "cursor"] {
            boolean(&mut self.root, &["compat", name], "mcps", false)?;
        }
        Ok(())
    }

    pub(in crate::providers) fn permission_text(
        mut self,
        permission: &str,
    ) -> Result<String, String> {
        self.overlay_permission(permission)?;
        write::to_text(&self.root)
    }

    pub(in crate::providers) fn compat_text(mut self) -> Result<String, String> {
        self.overlay_compat()?;
        write::to_text(&self.root)
    }

    pub(in crate::providers) fn runtime_text(
        mut self,
        settings: &AppSettings,
        permission: &str,
        inject: bool,
        prefs: &ExtensionsPrefs,
    ) -> Result<String, String> {
        self.overlay_permission(permission)?;
        self.overlay_compat()?;
        boolean(
            &mut self.root,
            &["subagents"],
            "enabled",
            settings.subagents_enabled,
        )?;
        boolean(
            &mut self.root,
            &["memory"],
            "enabled",
            settings.experimental_memory,
        )?;
        for (key, enabled) in [
            ("todo_gate_enabled", settings.todo_gate_enabled),
            (
                "subagent_worktree_snapshot_enabled",
                settings.subagent_worktree_snapshot_enabled,
            ),
            (
                "two_pass_compaction_enabled",
                settings.two_pass_compaction_enabled,
            ),
            ("auto_wake_enabled", settings.auto_wake_enabled),
        ] {
            boolean(&mut self.root, &[], key, enabled)?;
        }
        self.root.fields.insert(
            "todo_gate_max_fires_per_prompt".into(),
            Value::Atom(
                crate::agent_todo_gate::normalize_todo_gate_max_fires(Some(
                    settings.todo_gate_max_fires_per_prompt,
                ))
                .to_string(),
            ),
        );
        boolean(
            &mut self.root,
            &["features"],
            "auto_wake",
            settings.auto_wake_enabled,
        )?;
        let solo = inject && !settings.official_aux_with_user_mcp;
        if let Some(servers) = self.root.fields.get_mut("mcp_servers") {
            let Value::Table(servers) = servers else {
                return Err(invalid());
            };
            for (name, server) in &mut servers.fields {
                let Value::Table(server) = server else {
                    return Err(invalid());
                };
                // Never synthesize a server definition or reinterpret dots as a
                // nested path. Official main/inject-off cannot launch this MCP.
                let enabled = if name == "official-aux" {
                    inject
                } else {
                    !solo && crate::extensions::is_enabled(&prefs.mcp, name)
                };
                boolean(server, &[], "enabled", enabled)?;
            }
        }
        write::to_text(&self.root)
    }
}

#[cfg(test)]
#[path = "providers_config_runtime_tests.rs"]
mod tests;
