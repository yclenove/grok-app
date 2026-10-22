//! Pure selected-route projection. No filesystem, process, environment or auth IO.
use super::{invalid, write, ConfigSnapshot, Table, Value};
use std::collections::BTreeSet;

fn unavailable() -> String {
    "selected route references an unavailable configuration".into()
}

fn reference(table: &Table, field: &str) -> Result<Option<String>, String> {
    match table.fields.get(field) {
        None => Ok(None),
        Some(Value::String(id)) if !id.is_empty() && id.trim() == id => Ok(Some(id.clone())),
        _ => Err(unavailable()),
    }
}

fn take_namespace(root: &mut Table, name: &str, ids: &BTreeSet<String>) -> Result<Table, String> {
    let mut selected = Table::default();
    let namespace = root.fields.remove(name);
    if ids.is_empty() {
        return Ok(selected);
    }
    let Some(Value::Table(mut namespace)) = namespace else {
        return Err(unavailable());
    };
    for id in ids {
        let Some(value @ Value::Table(_)) = namespace.fields.remove(id) else {
            return Err(unavailable());
        };
        selected.fields.insert(id.clone(), value);
    }
    Ok(selected)
}

fn put_namespace(root: &mut Table, name: &str, namespace: Table) {
    if !namespace.fields.is_empty() {
        root.fields.insert(name.into(), Value::Table(namespace));
    }
}

impl ConfigSnapshot {
    /// The returned text contains credentials and is exclusively for a new,
    /// App-owned execution generation. Never return it to UI or rewrite the
    /// source/another running generation. Main-profile spawning is wired later.
    pub(in crate::providers) fn project_custom(
        mut self,
        provider: &str,
        requested: &str,
    ) -> Result<String, String> {
        if provider.trim() != provider
            || provider.is_empty()
            || super::super::is_reserved_custom_provider_id(provider)
            || requested.trim().is_empty()
            || requested.trim() != requested
        {
            return Err("selected provider or request model is invalid".into());
        }
        let selected = super::super::session_selection::custom_model(&self, provider, requested)?;
        let Some(Value::Table(mut namespace)) = self.root.fields.remove("model") else {
            return Err(unavailable());
        };
        let Some(Value::Table(mut model)) = namespace.fields.remove(provider) else {
            return Err(unavailable());
        };
        let mut provider_ids = BTreeSet::new();
        if let Some(id) = reference(&model, "model_provider")? {
            provider_ids.insert(id);
        }
        let inherited = take_namespace(&mut self.root, "model_providers", &provider_ids)?;
        let mut auth_ids = BTreeSet::new();
        if let Some(id) = reference(&model, "auth_provider")? {
            auth_ids.insert(id);
        }
        for value in inherited.fields.values() {
            if let Value::Table(table) = value {
                if let Some(id) = reference(table, "auth_provider")? {
                    auth_ids.insert(id);
                }
            }
        }
        let auth = take_namespace(&mut self.root, "auth_provider", &auth_ids)?;
        put_namespace(&mut self.root, "model_providers", inherited);
        put_namespace(&mut self.root, "auth_provider", auth);

        model
            .fields
            .insert("model".into(), Value::String(selected.id));
        if let Some(window) = selected.context_window {
            model
                .fields
                .insert("context_window".into(), Value::Atom(window.to_string()));
        }
        if let Some(vision) = selected.supports_vision {
            model.fields.insert(
                super::super::APP_SUPPORTS_VISION_KEY.into(),
                Value::Atom(vision.to_string()),
            );
        }
        if !selected.efforts.is_empty() {
            model.fields.insert(
                super::super::APP_EFFORTS_KEY.into(),
                Value::String(super::super::encode_app_efforts(&selected.efforts)),
            );
        }
        let native = model.fields.get(super::super::APP_PROVIDER_MODE_KEY).is_some_and(
            |value| matches!(value, Value::String(mode) if mode == super::super::PROVIDER_MODE_GROK_BUILD_PROXY),
        );
        let mut only_model = Table::default();
        only_model
            .fields
            .insert(provider.into(), Value::Table(model));
        self.root
            .fields
            .insert("model".into(), Value::Table(only_model));

        let models = self
            .root
            .fields
            .entry("models".into())
            .or_insert_with(|| Value::Table(Table::default()));
        let Value::Table(models) = models else {
            return Err(invalid());
        };
        models
            .fields
            .insert("default".into(), Value::String(provider.into()));
        // Existing auxiliary pins must not keep selecting a removed, foreign
        // credential namespace. Native relays resolve their real catalog id;
        // generic channels use the same projected provider alias as the main.
        // Preserve absent pins and all model restrictions/global defaults.
        for role in [
            "web_search",
            "session_summary",
            "image_description",
            "prompt_suggestion",
        ] {
            if let Some(value) = models.fields.get_mut(role) {
                if !matches!(value, Value::String(_)) {
                    return Err(invalid());
                }
                *value = Value::String(if native { requested } else { provider }.into());
            }
        }
        self.project_custom_auth()?;
        write::to_text(&self.root)
    }
}
