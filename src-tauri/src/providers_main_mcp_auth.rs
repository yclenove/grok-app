//! Private MCP authentication captured from the same source as each definition.
//! A connect must not consult later global homes or server-name credentials.
use super::ConfigSnapshot;
use crate::extensions::{McpCredentialEntry, McpServerDef};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct MainMcpSources<'a> {
    pub user_config: Option<&'a str>,
    pub main_credentials: Option<&'a [u8]>,
    pub user_credentials: Option<&'a [u8]>,
}

/// May contain authentication headers/tokens: deliberately no Debug or serde.
pub(crate) struct CapturedMcpServer {
    pub(crate) definition: McpServerDef,
    credential: Option<McpCredentialEntry>,
    unbound_auth: bool,
}

impl CapturedMcpServer {
    fn capture(definition: McpServerDef, root: Option<&Value>) -> Self {
        if definition
            .url
            .as_deref()
            .is_none_or(|url| url.trim().is_empty())
        {
            return Self {
                definition,
                credential: None,
                unbound_auth: false,
            };
        }
        let stored =
            root.and_then(|root| crate::mcp_oauth::parse_stored_mcp_oauth(root, &definition.name));
        let header = crate::extensions::mcp_def_authorization_header(&definition);
        let bound = stored.as_ref().is_some_and(|stored| {
            let entry = root.and_then(|root| {
                root.get("servers")
                    .and_then(|servers| servers.get(&definition.name))
                    .or_else(|| root.get(&definition.name))
            });
            // Newer stores can pin the actual MCP endpoint independently of
            // the OAuth audience. An explicit endpoint never falls back.
            if let Some(endpoint) = entry.and_then(|entry| entry.get("mcp_url")) {
                return endpoint.as_str().is_some_and(|endpoint| {
                    same_endpoint(definition.url.as_deref(), Some(endpoint))
                });
            }
            same_endpoint(definition.url.as_deref(), stored.resource.as_deref())
                || header.is_some_and(|header| {
                    let token = header
                        .strip_prefix("Bearer ")
                        .or_else(|| header.strip_prefix("bearer "))
                        .map(str::trim);
                    token == Some(stored.access_token.trim())
                })
        });
        let unbound_auth = stored.is_some() && !bound && header.is_none();
        let credential = stored.filter(|_| bound).map(|stored| McpCredentialEntry {
            access_token: stored.access_token,
            expires_at: stored.expires_at,
        });
        Self {
            definition,
            credential,
            unbound_auth,
        }
    }

    pub(crate) fn to_acp(&self, now_secs: u64) -> Option<Value> {
        if self.unbound_auth {
            return None;
        }
        crate::extensions::mcp_def_to_acp_with_credential(
            &self.definition,
            self.credential.as_ref(),
            now_secs,
            false,
        )
    }
}

fn same_endpoint(left: Option<&str>, right: Option<&str>) -> bool {
    let parse = |value: Option<&str>| {
        let mut url = reqwest::Url::parse(value?.trim()).ok()?;
        if !matches!(url.scheme(), "https" | "http") {
            return None;
        }
        url.set_fragment(None);
        Some(url)
    };
    match (parse(left), parse(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

impl MainMcpSources<'_> {
    pub(super) fn capture(&self, main: &ConfigSnapshot) -> Result<Vec<CapturedMcpServer>, String> {
        let main_credentials = self
            .main_credentials
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
        let user_credentials = self
            .user_credentials
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
        let mut by_name = BTreeMap::new();
        if let Some(user) = self.user_config {
            for definition in ConfigSnapshot::parse(user)?.mcp_definitions()? {
                by_name.insert(
                    definition.name.clone(),
                    CapturedMcpServer::capture(definition, user_credentials.as_ref()),
                );
            }
        }
        for definition in main.mcp_definitions()? {
            by_name.insert(
                definition.name.clone(),
                CapturedMcpServer::capture(definition, main_credentials.as_ref()),
            );
        }
        let definitions = crate::plugin_mcp::merge_plugin_mcp_defs(
            by_name
                .values()
                .map(|server| server.definition.clone())
                .collect(),
        );
        Ok(definitions
            .into_iter()
            .map(|definition| {
                if let Some(server) = by_name.remove(&definition.name) {
                    return server;
                }
                // Plugin definitions have no config-table origin. Only accept a
                // token with an endpoint/header binding; freeze the chosen source.
                let main =
                    CapturedMcpServer::capture(definition.clone(), main_credentials.as_ref());
                if main.credential.is_some() {
                    main
                } else {
                    CapturedMcpServer::capture(definition, user_credentials.as_ref())
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn remote(url: &str, header: Option<&str>) -> McpServerDef {
        serde_json::from_value(json!({
            "name": "bound", "url": url,
            "headers": header.map(|header| json!({"Authorization": header}))
        }))
        .unwrap()
    }

    #[test]
    fn same_name_credentials_cannot_cross_endpoint_path_scheme_port_or_query() {
        let credentials = json!({"bound": {
            "access_token": "synthetic-only", "resource": "https://intended.invalid/mcp"
        }});
        for url in [
            "https://foreign.invalid/mcp",
            "https://intended.invalid/other",
            "https://intended.invalid:444/mcp",
            "http://intended.invalid/mcp",
            "https://intended.invalid/mcp?other=1",
        ] {
            let server = CapturedMcpServer::capture(remote(url, None), Some(&credentials));
            assert!(
                server.to_acp(0).is_none(),
                "unbound endpoint was injected: {url}"
            );
        }
        let server = CapturedMcpServer::capture(
            remote("https://INTENDED.invalid:443/a/../mcp#documentation", None),
            Some(&credentials),
        );
        assert!(server.to_acp(0).is_some());
    }

    #[test]
    fn explicit_endpoint_is_separate_from_audience_and_never_falls_back() {
        let mut credentials = json!({"servers": {"bound": {
            "access_token": "synthetic-only",
            "mcp_url": "https://intended.invalid/mcp",
            "resource": "https://audience.invalid/resource"
        }}});
        assert!(CapturedMcpServer::capture(
            remote("https://intended.invalid/mcp", None),
            Some(&credentials)
        )
        .to_acp(0)
        .is_some());
        credentials["servers"]["bound"]["mcp_url"] = json!("https://foreign.invalid/mcp");
        credentials["servers"]["bound"]["resource"] = json!("https://intended.invalid/mcp");
        assert!(CapturedMcpServer::capture(
            remote("https://intended.invalid/mcp", None),
            Some(&credentials)
        )
        .to_acp(0)
        .is_none());
    }

    #[test]
    fn legacy_nested_credentials_require_endpoint_or_explicit_header_and_keep_expiry_gate() {
        let credentials = json!({"bound": {
            "token_response": {"access_token": "synthetic-only", "expires_in": 100},
            "obtained_at": 2000
        }});
        let server = CapturedMcpServer::capture(
            remote(
                "https://intended.invalid/mcp",
                Some("Bearer synthetic-only"),
            ),
            Some(&credentials),
        );
        assert!(server.to_acp(2010).is_some());
        assert!(server.to_acp(2050).is_none());
        assert!(CapturedMcpServer::capture(
            remote("https://intended.invalid/mcp", None),
            Some(&credentials)
        )
        .to_acp(2010)
        .is_none());
    }

    #[test]
    fn unrelated_same_name_credentials_never_disable_stdio_or_override_explicit_headers() {
        let credentials = json!({"bound": {"access_token": "synthetic-foreign", "resource": "https://foreign.invalid/mcp"}});
        let stdio =
            serde_json::from_value(json!({"name": "bound", "command": "unexecuted-synthetic"}))
                .unwrap();
        assert!(CapturedMcpServer::capture(stdio, Some(&credentials))
            .to_acp(0)
            .is_some());
        let server = CapturedMcpServer::capture(
            remote(
                "https://intended.invalid/mcp",
                Some("Bearer synthetic-explicit"),
            ),
            Some(&credentials),
        );
        let entry = server.to_acp(0).unwrap();
        assert_eq!(entry["headers"][0]["value"], "Bearer synthetic-explicit");
    }
}
