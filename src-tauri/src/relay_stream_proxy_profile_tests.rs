//! Synthetic profile capture through the production HTTP listener, never a provider.
use super::*;
use crate::providers::{ProcessConfigBinding, ProcessModelBinding};
use crate::session_execution::ExecutionSource;
use crate::session_migration::test_home::IsolatedAppHome;

fn profile(upstream: &str, mode: &str) -> (std::path::PathBuf, String) {
    let owner = uuid::Uuid::new_v4().to_string();
    let generation = uuid::Uuid::new_v4().to_string();
    let home = crate::paths::app_data_root()
        .join("execution-profiles")
        .join(&owner)
        .join(&generation);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("execution-profile.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1, "ownerSessionId": owner, "generation": generation,
        }))
        .unwrap(),
    )
    .unwrap();
    let port = ensure_started_blocking().unwrap();
    let base = format!("http://127.0.0.1:{port}/p/{owner}/{generation}/synthetic-relay/v1");
    let text = format!(
        "[models]\ndefault = \"synthetic-relay\"\n[model.synthetic-relay]\n\
         model = \"grok-4.6\"\nbase_url = {base:?}\napi_key = \"synthetic-not-a-credential\"\n\
         app_upstream_base_url = {upstream:?}\napi_backend = \"responses\"\n\
         app_provider_mode = {mode:?}\n"
    );
    std::fs::write(home.join("config.toml"), text).unwrap();
    (home, base)
}

fn capture(home: &std::path::Path) -> ProcessConfigBinding {
    ProcessModelBinding::capture_for_spawn(
        true,
        false,
        false,
        "grok-4.6",
        home,
        &crate::store::AppSettings::default(),
    )
    .unwrap()
    .2
    .unwrap()
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

fn transport(
    home: &std::path::Path,
) -> (
    std::sync::Arc<crate::acp_client::AcpClient>,
    tokio::io::DuplexStream,
    crate::acp_client::test_transport::EventReceiver,
) {
    let (model, _, identity) = ProcessModelBinding::capture_for_spawn(
        true,
        false,
        false,
        "grok-4.6",
        home,
        &crate::store::AppSettings::default(),
    )
    .unwrap();
    crate::acp_client::test_transport::client_with_binding(
        true,
        crate::session_migration::mcp_stdio::SessionMcpPolicy::default(),
        None,
        model,
        identity,
    )
}

async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{address}"), task)
}

#[test]
fn profile_capture_rejects_mutable_legacy_proxy_addresses() {
    let _root = IsolatedAppHome::acquire("profile-mutable-proxy");
    let (home, base) = profile("https://synthetic.invalid/v1", "generic");
    let text = std::fs::read_to_string(home.join("config.toml")).unwrap();
    for host in ["127.0.0.1", "127.1", "localhost.", "[::1]"] {
        let mutable = format!("http://{host}:54321/r/synthetic-relay/v1");
        std::fs::write(home.join("config.toml"), text.replace(&base, &mutable)).unwrap();
        let error = ProcessModelBinding::capture_for_spawn(
            true,
            false,
            false,
            "grok-4.6",
            &home,
            &crate::store::AppSettings::default(),
        )
        .err()
        .expect("an AppProfile must not consult a mutable global relay route");
        assert!(!error.contains("synthetic-not-a-credential"));
        assert!(!error.contains("synthetic.invalid"));
    }
}

#[test]
fn profile_http_route_keeps_its_upstream_after_global_edit_and_delete() {
    let _root = IsolatedAppHome::acquire("profile-frozen-http");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let upstream = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(any(|| async { "synthetic-upstream-a" })),
            )
            .await
            .unwrap();
        });
        let client = http_client();
        let (home, base) = profile(&format!("http://{address}/v1"), "generic");
        let lease = capture(&home);
        assert!(matches!(lease.execution_source(), ExecutionSource::AppProfile { .. }));
        let request = || client.get(format!("{base}/models"));
        let first = request().send().await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(first.text().await.unwrap(), "synthetic-upstream-a");
        std::fs::write(
            crate::paths::agent_config_toml(),
            "[models]\ndefault='synthetic-relay'\n[model.synthetic-relay]\nbase_url='http://127.0.0.1:1/decoy'\n",
        )
        .unwrap();
        assert_eq!(request().send().await.unwrap().text().await.unwrap(), "synthetic-upstream-a");
        std::fs::rename(
            crate::paths::agent_config_toml(),
            crate::paths::agent_config_toml().with_extension("preserved"),
        )
        .unwrap();
        assert_eq!(request().send().await.unwrap().text().await.unwrap(), "synthetic-upstream-a");
        let retained = lease.clone();
        drop(lease);
        assert_eq!(request().send().await.unwrap().text().await.unwrap(), "synthetic-upstream-a");
        drop(retained);
        assert_eq!(request().send().await.unwrap().status(), StatusCode::GONE);
        upstream.abort();
        let _ = upstream.await;
    });
}

#[test]
fn profile_generation_rejects_replacement_while_new_generation_can_choose_another_upstream() {
    let _root = IsolatedAppHome::acquire("profile-relay-generations");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let router = Router::new().fallback(any(|request: Request| async move {
            if request.uri().path().starts_with("/a/") {
                "upstream-a"
            } else {
                "upstream-b"
            }
        }));
        let (upstream, server) = serve(router).await;
        let (home_a, base_a) = profile(&format!("{upstream}/a/v1"), "generic");
        let lease_a = capture(&home_a);
        let text_a = std::fs::read_to_string(home_a.join("config.toml")).unwrap();
        std::fs::write(home_a.join("config.toml"), text_a.replace("/a/v1", "/b/v1")).unwrap();
        assert!(ProcessModelBinding::capture_for_spawn(
            true,
            false,
            false,
            "grok-4.6",
            &home_a,
            &crate::store::AppSettings::default()
        )
        .is_err());
        let (home_b, base_b) = profile(&format!("{upstream}/b/v1"), "generic");
        let lease_b = capture(&home_b);
        let client = http_client();
        let legacy = local_proxy_base_url("synthetic-relay", ensure_started_blocking().unwrap());
        let global = format!("[model.synthetic-relay]\nbase_url = {legacy:?}\napp_upstream_base_url = \"{upstream}/a/v1\"\n");
        std::fs::write(crate::paths::agent_config_toml(), &global).unwrap();
        assert_eq!(client.get(format!("{legacy}/models")).send().await.unwrap().text().await.unwrap(), "upstream-a");
        std::fs::write(crate::paths::agent_config_toml(), global.replace("/a/v1", "/b/v1")).unwrap();
        assert_eq!(client.get(format!("{legacy}/models")).send().await.unwrap().text().await.unwrap(), "upstream-b");
        assert_eq!(
            client
                .get(format!("{base_a}/models"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "upstream-a"
        );
        assert_eq!(
            client
                .get(format!("{base_b}/models"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "upstream-b"
        );
        drop(lease_a);
        assert_eq!(
            client
                .get(format!("{base_a}/models"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::GONE
        );
        assert_eq!(
            client
                .get(format!("{base_b}/models"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "upstream-b"
        );
        drop(lease_b);
        server.abort();
        let _ = server.await;
    });
}

#[test]
fn acp_kill_cancels_header_wait_even_when_a_config_clone_survives() {
    let _root = IsolatedAppHome::acquire("profile-relay-header-cancel");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (started, mut signal) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().fallback(any(move || {
            let started = started.clone();
            async move {
                started.send(()).unwrap();
                std::future::pending::<Response>().await
            }
        }));
        let (upstream, server) = serve(router).await;
        let (home, base) = profile(&format!("{upstream}/v1"), "generic");
        let retained = capture(&home);
        let (acp, _peer, _events) = transport(&home);
        let client = http_client();
        let request_client = client.clone();
        let target = format!("{base}/models");
        let pending = tokio::spawn(async move { request_client.get(target).send().await.unwrap() });
        tokio::time::timeout(std::time::Duration::from_secs(5), signal.recv())
            .await
            .unwrap()
            .unwrap();
        acp.kill().await;
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let error = response.text().await.unwrap();
        assert!(error.contains("execution relay cancelled"));
        assert!(!error.contains(&upstream));
        assert_eq!(
            client
                .get(format!("{base}/models"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::GONE
        );
        assert!(ProcessModelBinding::capture_for_spawn(
            true,
            false,
            false,
            "grok-4.6",
            &home,
            &crate::store::AppSettings::default()
        )
        .is_err());
        drop(retained);
        server.abort();
        let _ = server.await;
    });
}

#[test]
fn acp_eof_revokes_the_frozen_http_route_before_external_config_clones_drop() {
    let _root = IsolatedAppHome::acquire("profile-relay-eof");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (home, base) = profile("http://127.0.0.1:1/v1", "generic");
        let retained = capture(&home);
        let (acp, peer, mut events) = transport(&home);
        drop(peer);
        let (_, event) = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            crate::acp_client::AcpEvent::ProcessExited { .. }
        ));
        assert!(!acp.is_alive());
        assert_eq!(
            http_client()
                .get(format!("{base}/models"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::GONE
        );
        drop(retained);
    });
}

#[test]
fn cancellation_interrupts_non_sse_body_collection() {
    let _root = IsolatedAppHome::acquire("profile-relay-body-cancel");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (started, mut signal) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().fallback(any(move || {
            let started = started.clone();
            async move {
                let body = async_stream::stream! {
                    started.send(()).unwrap();
                    yield Ok::<Bytes, Infallible>(Bytes::from_static(b"partial"));
                    std::future::pending::<()>().await;
                };
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from_stream(body))
                    .unwrap()
            }
        }));
        let (upstream, server) = serve(router).await;
        let (home, base) = profile(&format!("{upstream}/v1"), "generic");
        let lease = capture(&home);
        let pending = tokio::spawn(async move {
            http_client()
                .get(format!("{base}/models"))
                .send()
                .await
                .unwrap()
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), signal.recv())
            .await
            .unwrap()
            .unwrap();
        drop(lease);
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert!(response
            .text()
            .await
            .unwrap()
            .contains("execution relay cancelled"));
        server.abort();
        let _ = server.await;
    });
}

#[test]
fn frozen_sse_filters_frames_preserves_chinese_and_stops_on_acp_kill() {
    let _root = IsolatedAppHome::acquire("profile-relay-sse-cancel");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let router = Router::new().fallback(any(|| async {
            let body = async_stream::stream! {
                let raw = "data: {\"type\":\"ping\"}\n\ndata: {\"id\":\"synthetic\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"中文会话导入\"}}]}\n\n";
                for byte in raw.as_bytes() {
                    yield Ok::<Bytes, Infallible>(Bytes::copy_from_slice(&[*byte]));
                }
                yield Ok::<Bytes, Infallible>(Bytes::from_static(b"data: partial-event"));
                std::future::pending::<()>().await;
            };
            Response::builder().header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(body)).unwrap()
        }));
        let (upstream, server) = serve(router).await;
        let (home, base) = profile(&format!("{upstream}/v1"), "generic");
        let retained = capture(&home);
        let (acp, _peer, _events) = transport(&home);
        let response = http_client().get(format!("{base}/responses")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.bytes_stream();
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\n\n") {
                bytes.extend_from_slice(&stream.next().await.unwrap().unwrap());
            }
            String::from_utf8(bytes).unwrap()
        }).await.unwrap();
        assert!(first.contains("中文会话导入"));
        assert!(!first.contains("ping"));
        assert!(!first.contains('\u{fffd}'));
        acp.kill().await;
        assert!(tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await.unwrap().is_none());
        drop(retained);
        server.abort();
        let _ = server.await;
    });
}

#[test]
fn cancellation_interrupts_a_request_body_before_any_upstream_request() {
    let _root = IsolatedAppHome::acquire("profile-relay-request-cancel");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (home, base) = profile("http://127.0.0.1:1/v1", "generic");
        let lease = capture(&home);
        let (started, signal) = tokio::sync::oneshot::channel();
        let body = async_stream::stream! {
            started.send(()).unwrap();
            yield Ok::<Bytes, Infallible>(Bytes::from_static(b"partial"));
            std::future::pending::<()>().await;
        };
        let uri = reqwest::Url::parse(&format!("{base}/responses"))
            .unwrap()
            .path()
            .to_string();
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .body(Body::from_stream(body))
            .unwrap();
        let pending = tokio::spawn(proxy_request(request));
        tokio::time::timeout(std::time::Duration::from_secs(5), signal)
            .await
            .unwrap()
            .unwrap();
        drop(lease);
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(error, "execution relay cancelled");
    });
}

#[test]
fn profile_upstreams_and_addresses_fail_closed_without_exposing_config_values() {
    let _root = IsolatedAppHome::acquire("profile-relay-invalid");
    let (home, base) = profile("https://synthetic.invalid/v1", "generic");
    let original = std::fs::read_to_string(home.join("config.toml")).unwrap();
    for bad in [
        "http://127.1:54321/r/synthetic/v1",
        "http://localhost.:54321/p/another/generation/model/v1",
        "https://synthetic-user:synthetic-password@synthetic.invalid/v1",
        "https://synthetic.invalid/v1?synthetic-secret=query",
        "https://synthetic.invalid/v1#synthetic-secret",
    ] {
        std::fs::write(
            home.join("config.toml"),
            original.replace("https://synthetic.invalid/v1", bad),
        )
        .unwrap();
        let error = ProcessModelBinding::capture_for_spawn(
            true,
            false,
            false,
            "grok-4.6",
            &home,
            &crate::store::AppSettings::default(),
        )
        .err()
        .unwrap();
        assert!(!error.contains("synthetic"));
        assert!(!error.contains(&base));
    }
    for bad in [
        base.replace("/p/", "/r/"),
        base.replace("127.0.0.1", "localhost"),
        base.replace("synthetic-relay/v1", "other-relay/v1"),
    ] {
        std::fs::write(home.join("config.toml"), original.replace(&base, &bad)).unwrap();
        assert!(ProcessModelBinding::capture_for_spawn(
            true,
            false,
            false,
            "grok-4.6",
            &home,
            &crate::store::AppSettings::default()
        )
        .is_err());
    }
    // A remote hostname containing 'localhost' is not this application's relay.
    let direct = "https://api.localhost.synthetic.invalid/r/model/v1";
    let external = original
        .replace(&base, direct)
        .replace("app_upstream_base_url", "ignored_upstream");
    std::fs::write(home.join("config.toml"), external).unwrap();
    let identity = capture(&home);
    assert!(matches!(
        identity.execution_source(),
        ExecutionSource::AppProfile { .. }
    ));
}

#[test]
fn frozen_http_preserves_request_and_upstream_status_for_encoded_provider_names() {
    let _root = IsolatedAppHome::acquire("profile-relay-request-roundtrip");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let router = Router::new().fallback(any(|request: Request| async move {
            let uri = request.uri().to_string();
            let method = request.method().to_string();
            let authorization = request
                .headers()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let extra = request
                .headers()
                .get("x-synthetic-extra")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let body = axum::body::to_bytes(request.into_body(), 1024)
                .await
                .unwrap();
            (
                StatusCode::IM_A_TEAPOT,
                axum::Json(serde_json::json!({
                    "uri": uri, "method": method, "authorization": authorization, "extra": extra,
                    "body": std::str::from_utf8(&body).unwrap(),
                })),
            )
        }));
        let (upstream, server) = serve(router).await;
        let (home, base) = profile(&format!("{upstream}/custom/root"), "generic");
        let original = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let provider = "中文/模型";
        let encoded = "%E4%B8%AD%E6%96%87%2F%E6%A8%A1%E5%9E%8B";
        let base = base.replace("synthetic-relay/v1", &format!("{encoded}/v1"));
        let text = original
            .replace("synthetic-relay\"", &format!("{provider}\""))
            .replace(
                "[model.synthetic-relay]",
                &format!("[model.\"{provider}\"]"),
            )
            .replace("/synthetic-relay/v1", &format!("/{encoded}/v1"));
        std::fs::write(home.join("config.toml"), text).unwrap();
        let lease = capture(&home);
        let response = http_client()
            .post(format!("{base}/responses?synthetic=one%20two"))
            .header("authorization", "Bearer synthetic-not-a-credential")
            .header("x-synthetic-extra", "synthetic-header")
            .body("{\"input\":\"中文请求\"}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
        let result: serde_json::Value = response.json().await.unwrap();
        assert_eq!(result["uri"], "/custom/root/responses?synthetic=one%20two");
        assert_eq!(result["method"], "POST");
        assert_eq!(result["authorization"], "Bearer synthetic-not-a-credential");
        assert_eq!(result["extra"], "synthetic-header");
        assert_eq!(result["body"], "{\"input\":\"中文请求\"}");
        drop(lease);
        server.abort();
        let _ = server.await;
    });
}
