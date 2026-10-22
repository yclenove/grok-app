//! Official main configuration, with an explicit, separate authentication source.
//! Pure projection: never read credentials, activate a route or write either input.
use super::{invalid, write, ConfigSnapshot, Table, Value};

impl ConfigSnapshot {
    pub(in crate::providers) fn project_official(
        mut self,
        requested: &str,
        official: Self,
        api_key: Option<&str>,
    ) -> Result<String, String> {
        if requested.is_empty() || requested.trim() != requested {
            return Err("selected request model is invalid".into());
        }
        let removed_models = self.model_ids()?;
        // A namespace with a wrong type is not permission to silently repair
        // the source. Reject before replacing it in this private projection.
        for name in ["model", "model_providers", "auth_provider"] {
            if self
                .root
                .fields
                .get(name)
                .is_some_and(|value| !matches!(value, Value::Table(_)))
            {
                return Err(invalid());
            }
            self.root.fields.remove(name);
        }
        self.replace_official_auth(official, api_key)?;
        if let Some(endpoints) = self.root.fields.get_mut("endpoints") {
            let Value::Table(endpoints) = endpoints else {
                return Err(invalid());
            };
            // Use CLI built-in official inference/catalog endpoints. Keep
            // unrelated observability and future fields, never a relay URL.
            for field in [
                "cli_chat_proxy_base_url",
                "xai_api_base_url",
                "models_base_url",
                "models_list_url",
                "models_endpoint",
            ] {
                endpoints.fields.remove(field);
            }
        }
        let models = self
            .root
            .fields
            .entry("models".into())
            .or_insert_with(|| Value::Table(Table::default()));
        let Value::Table(models) = models else {
            return Err(invalid());
        };
        models.fields.insert(
            "default".into(),
            Value::String(super::super::OFFICIAL_DEFAULT_MODEL.into()),
        );
        // A pin into a deleted alias cannot retain that alias's credentials.
        // Otherwise keep official role pins, effort, restrictions and unknowns.
        for role in [
            "web_search",
            "session_summary",
            "image_description",
            "prompt_suggestion",
        ] {
            if let Some(value) = models.fields.get(role) {
                let Value::String(id) = value else {
                    return Err(invalid());
                };
                if removed_models.contains(id) {
                    models.fields.remove(role);
                }
            }
        }
        if let Some(key) = api_key {
            // This key is explicitly supplied by the caller's official source,
            // never inherited from a model table (even a colliding catalog id).
            // With no endpoint/body override, Build merges into its catalog.
            let mut model = Table::default();
            model
                .fields
                .insert("api_key".into(), Value::String(key.into()));
            let mut namespace = Table::default();
            let key_model = if requested == super::super::OFFICIAL_DEFAULT_MODEL {
                super::super::OFFICIAL_CATALOG_MODEL
            } else {
                requested
            };
            namespace
                .fields
                .insert(key_model.into(), Value::Table(model));
            self.root
                .fields
                .insert("model".into(), Value::Table(namespace));
        }
        write::to_text(&self.root)
    }
}
