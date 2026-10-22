//! Authentication for a new execution profile, never a global rewrite.
use super::{invalid, ConfigSnapshot, Table, Value};
use std::collections::BTreeSet;

fn team_set(value: &Value) -> Result<BTreeSet<String>, String> {
    match value {
        Value::String(team) => Ok(BTreeSet::from([team.clone()])),
        Value::Array(teams) => teams
            .iter()
            .map(|team| match team {
                Value::String(team) => Ok(team.clone()),
                _ => Err(invalid()),
            })
            .collect(),
        _ => Err(invalid()),
    }
}

fn auth_table<'a>(root: &'a Table, name: &str) -> Result<Option<&'a Table>, String> {
    let table = match root.fields.get(name) {
        None => return Ok(None),
        Some(Value::Table(table)) => table,
        _ => return Err(invalid()),
    };
    match table.fields.get("preferred_method") {
        None => {}
        Some(Value::String(method)) if matches!(method.as_str(), "api_key" | "oidc") => {}
        _ => return Err(invalid()),
    }
    match table.fields.get("disable_api_key_auth") {
        None => {}
        Some(Value::Atom(value)) if matches!(value.as_str(), "true" | "false") => {}
        _ => return Err(invalid()),
    }
    if let Some(team) = table.fields.get("force_login_team_uuid") {
        team_set(team)?;
    }
    Ok(Some(table))
}

fn effective_auth<'a>(root: &'a Table, field: &str) -> Result<Option<&'a Value>, String> {
    let explicit = auth_table(root, "grok_com_config")?;
    let alias = auth_table(root, "auth")?;
    Ok(explicit
        .and_then(|table| table.fields.get(field))
        .or_else(|| alias.and_then(|table| table.fields.get(field))))
}

fn official_restriction(root: &mut Table, field: &str, value: Value) -> Result<(), String> {
    let explicit = root
        .fields
        .entry("grok_com_config".into())
        .or_insert_with(|| Value::Table(Table::default()));
    let Value::Table(explicit) = explicit else {
        return Err(invalid());
    };
    explicit.fields.insert(field.into(), value);
    Ok(())
}

impl ConfigSnapshot {
    pub(super) fn replace_official_auth(
        &mut self,
        mut official: Self,
        api_key: Option<&str>,
    ) -> Result<(), String> {
        // The official source owns login definitions/method selection; effective
        // restrictions from either applicable input must still be enforced.
        for root in [&self.root, &official.root] {
            for name in ["auth", "grok_com_config"] {
                auth_table(root, name)?;
            }
        }
        let runtime_ban = matches!(effective_auth(&self.root, "disable_api_key_auth")?, Some(Value::Atom(value)) if value == "true");
        let runtime_teams = effective_auth(&self.root, "force_login_team_uuid")?
            .map(team_set)
            .transpose()?;
        let official_teams = effective_auth(&official.root, "force_login_team_uuid")?
            .map(team_set)
            .transpose()?;
        for name in ["auth", "grok_com_config"] {
            self.root.fields.remove(name);
            if let Some(value) = official.root.fields.remove(name) {
                self.root.fields.insert(name.into(), value);
            }
        }
        if runtime_ban
            && !matches!(effective_auth(&self.root, "disable_api_key_auth")?, Some(Value::Atom(value)) if value == "true")
        {
            official_restriction(
                &mut self.root,
                "disable_api_key_auth",
                Value::Atom("true".into()),
            )?;
        }
        if let Some(runtime_teams) = runtime_teams {
            let teams = official_teams
                .as_ref()
                .map(|official| runtime_teams.intersection(official).cloned().collect())
                .unwrap_or(runtime_teams);
            if official_teams.as_ref() != Some(&teams) {
                official_restriction(
                    &mut self.root,
                    "force_login_team_uuid",
                    Value::Array(teams.into_iter().map(Value::String).collect()),
                )?;
            }
        }
        if let Some(key) = api_key {
            if key.trim().is_empty() {
                return Err("official API key is empty".into());
            }
            let auth = auth_table(&self.root, "auth")?;
            let explicit = auth_table(&self.root, "grok_com_config")?;
            let effective = |field: &str| {
                explicit
                    .and_then(|table| table.fields.get(field))
                    .or_else(|| auth.and_then(|table| table.fields.get(field)))
            };
            if matches!(effective("disable_api_key_auth"), Some(Value::Atom(value)) if value == "true")
                || effective("force_login_team_uuid").is_some()
                || matches!(effective("preferred_method"), Some(Value::String(value)) if value == "oidc")
            {
                return Err("selected route conflicts with authentication policy".into());
            }
            self.root
                .fields
                .entry("auth".into())
                .or_insert_with(|| Value::Table(Table::default()));
            for name in ["auth", "grok_com_config"] {
                if let Some(Value::Table(table)) = self.root.fields.get_mut(name) {
                    table
                        .fields
                        .insert("preferred_method".into(), Value::String("api_key".into()));
                }
            }
        }
        Ok(())
    }

    pub(super) fn project_custom_auth(&mut self) -> Result<(), String> {
        let auth = auth_table(&self.root, "auth")?;
        let explicit = auth_table(&self.root, "grok_com_config")?;
        // Build expands [auth] as an alias, then gives each explicit field in
        // [grok_com_config] precedence. Preserve effective restrictions rather
        // than changing a ban to false or removing a login-team requirement.
        let effective = |field: &str| {
            explicit
                .and_then(|table| table.fields.get(field))
                .or_else(|| auth.and_then(|table| table.fields.get(field)))
        };
        if matches!(effective("disable_api_key_auth"), Some(Value::Atom(value)) if value == "true")
            || effective("force_login_team_uuid").is_some()
        {
            return Err("selected route conflicts with authentication policy".into());
        }
        self.root
            .fields
            .entry("auth".into())
            .or_insert_with(|| Value::Table(Table::default()));
        for name in ["auth", "grok_com_config"] {
            let Some(Value::Table(table)) = self.root.fields.get_mut(name) else {
                continue;
            };
            table
                .fields
                .insert("preferred_method".into(), Value::String("api_key".into()));
            // These are global official login definitions. Selected model's
            // named auth_provider helpers remain in their own projected table.
            for field in ["oidc", "oauth2", "auth_provider_command"] {
                table.fields.remove(field);
            }
        }
        Ok(())
    }
}
