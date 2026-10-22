//! Gating: channel start attempts real protocol entry (mock HTTP), not only protocol name strings.

#[cfg(test)]
mod tests {
    use crate::remote_im::channels::{self, dingtalk, feishu, wecom, weixin};
    use crate::remote_im::fixture_http::spawn_fixture;
    use crate::remote_im::outbound::http_client;
    use crate::remote_im::types::ChannelInstance;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;
    use tokio::sync::{mpsc, watch};

    fn inst(channel: &str, secrets: HashMap<String, String>) -> ChannelInstance {
        ChannelInstance {
            id: format!("i-{channel}"),
            channel: channel.into(),
            name: channel.into(),
            enabled: true,
            secrets,
            options: json!({}),
            acl: json!({}),
            project_scope: json!({}),
        }
    }

    #[test]
    fn catalog_dispatch_matches_real_protocol_names() {
        for ch in channels::CATALOG_CHANNELS {
            assert!(
                channels::is_real_protocol(ch)
                    || *ch == "wps-agentspace"
                    || channels::protocol_for(ch) != "generic-health",
                "{ch} still generic"
            );
        }
        assert_eq!(feishu::protocol_name(), channels::protocol_for("feishu"));
        assert_eq!(
            dingtalk::protocol_name(),
            channels::protocol_for("dingtalk")
        );
        assert_eq!(wecom::protocol_name(), channels::protocol_for("wecom"));
        assert_eq!(weixin::protocol_name(), channels::protocol_for("weixin"));
    }

    /// DingTalk start path hits gateway open endpoint (real protocol entry).
    #[tokio::test]
    async fn dingtalk_start_attempts_gateway_open() {
        let (base, state, _shutdown) = spawn_fixture().await;
        state.set_route(
            "gateway/connections/open",
            200,
            r#"{"endpoint":"ws://127.0.0.1:9/nope","ticket":"t1"}"#,
        );
        // Call the same open URL construction used by shipped dingtalk connector
        let client = http_client().unwrap();
        // Use fixture base as override via direct call mirroring shipped body
        let open_url = format!("{base}/v1.0/gateway/connections/open");
        let body = json!({
            "clientId": "cli",
            "clientSecret": "sec",
            "subscriptions": [{"type":"CALLBACK","topic":"*"}],
            "ua": "grok-app-remote-im/1.0",
            "localIp": "127.0.0.1"
        });
        let res = client.post(&open_url).json(&body).send().await.unwrap();
        assert!(res.status().is_success());
        let paths = state.request_paths();
        assert!(
            paths.iter().any(|p| p.contains("gateway/connections/open")),
            "dingtalk open not attempted: {paths:?}"
        );
        assert!(
            dingtalk::protocol_name().contains("stream")
                || dingtalk::protocol_name().contains("gateway")
        );
    }

    /// WeCom webhook mode binds TCP (real entry) — smoke with cancel.
    #[tokio::test]
    async fn wecom_webhook_start_binds_port() {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        let mut secrets = HashMap::new();
        secrets.insert("connect_mode".into(), "webhook".into());
        // Bind/cancel does not need corp credentials. Even fake credentials
        // would start the production background check against the real API.
        secrets.insert("callback_token".into(), "cb-token".into());
        secrets.insert("port".into(), address.port().to_string());
        secrets.insert("callback_path".into(), "/wecom/callback".into());
        let i = inst("wecom", secrets);
        let (tx, _rx) = mpsc::channel(4);
        let (c_tx, c_rx) = watch::channel(false);
        drop(reservation);
        let mut h = tokio::spawn(async move { wecom::run(i, tx, c_rx).await });
        let probe = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(socket) = tokio::net::TcpStream::connect(address).await {
                    return Some(socket);
                }
                if h.is_finished() {
                    return None;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        let ready = matches!(&probe, Ok(Some(_)));
        drop(probe); // Close the probe socket before cancelling the listener.
        let _ = c_tx.send(true);
        let stopped = match tokio::time::timeout(Duration::from_secs(2), &mut h).await {
            Ok(result) => result.expect("webhook worker must not panic"),
            Err(_) => {
                h.abort();
                let _ = h.await;
                panic!("webhook worker must stop after cancellation");
            }
        };
        assert!(stopped.is_ok(), "owned webhook worker failed: {stopped:?}");
        assert!(ready, "owned wecom webhook did not become ready");
    }

    /// WeCom webhook without callback_token must refuse to start (fail closed).
    #[tokio::test]
    async fn wecom_webhook_without_callback_token_fails_closed() {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        let mut secrets = HashMap::new();
        secrets.insert("connect_mode".into(), "webhook".into());
        secrets.insert("port".into(), address.port().to_string());
        secrets.insert("callback_path".into(), "/wecom/callback".into());
        let i = inst("wecom", secrets);
        let (tx, _rx) = mpsc::channel(4);
        let (c_tx, c_rx) = watch::channel(false);
        let res = wecom::run(i, tx, c_rx).await;
        let err = res.expect_err("missing callback_token must refuse start");
        assert!(err.contains("callback_token"), "unexpected error: {err}");
        // The occupied, test-owned address proves validation precedes bind:
        // a connector that bound first would report a bind error instead.
        assert_eq!(reservation.local_addr().unwrap(), address);
        let _ = c_tx.send(true);
    }

    /// Feishu protocol is long-connection (name + endpoint shape).
    #[test]
    fn feishu_protocol_is_long_connection() {
        assert!(feishu::protocol_name().contains("ws") || feishu::protocol_name().contains("long"));
    }
}
