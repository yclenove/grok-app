//! Bind selected model routes to the newly allocated immutable generation.
use super::{invalid, write, ConfigSnapshot, Value};

impl ConfigSnapshot {
    pub(in crate::providers) fn freeze_profile_routes(
        mut self,
        owner: &str,
        generation: &str,
    ) -> Result<String, String> {
        use crate::relay_stream_proxy as relay;
        for provider in self.model_ids()? {
            let fields = self.model_fields(&provider)?.ok_or_else(invalid)?;
            let base = fields.get("base_url").map(String::as_str).unwrap_or("");
            let upstream = relay::effective_upstream_base(&fields);
            if !relay::is_legacy_proxy_url(base)
                && !relay::is_profile_url(base)
                && !relay::host_needs_stream_sanitize(&upstream)
            {
                continue;
            }
            let url = reqwest::Url::parse(&upstream).map_err(|_| invalid())?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || relay::is_legacy_proxy_url(&upstream)
                || relay::is_profile_url(&upstream)
            {
                return Err("execution relay upstream is unavailable or invalid".into());
            }
            let port = relay::ensure_started_blocking()?;
            let Some(Value::Table(models)) = self.root.fields.get_mut("model") else {
                return Err(invalid());
            };
            let Some(Value::Table(model)) = models.fields.get_mut(&provider) else {
                return Err(invalid());
            };
            model.fields.insert(
                "base_url".into(),
                Value::String(relay::profile_proxy_base_url(
                    owner, generation, &provider, port,
                )),
            );
            model.fields.insert(
                relay::APP_UPSTREAM_BASE_URL_KEY.into(),
                Value::String(upstream),
            );
        }
        write::to_text(&self.root)
    }
}
