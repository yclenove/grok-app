//! Fixed ACP wire fixtures. No live CLI, credentials or native acceptance.
use super::*;

#[test]
fn eligibility_uses_owned_native_transport_not_child_presence() {
    assert!(local_transport_allowed(true, false, false, true));
    for args in [
        (false, false, false, true),
        (false, false, false, false),
        (false, false, true, true),
        (true, true, false, true),
    ] {
        assert!(!local_transport_allowed(args.0, args.1, args.2, args.3));
    }
}

#[test]
fn request_entry_survives_base_failure_and_replaces_same_name_only() {
    let request = json!({"name": crate::computer_use::MCP_SERVER_NAME, "command":"synthetic-request-runtime"});
    let base = json!([{"name":"other-mcp"}, {"name":crate::computer_use::MCP_SERVER_NAME,"command":"user-defined-collision"}]);
    let merged = merge_request_entry(base, Some(&request));
    assert_eq!(merged, json!([{"name":"other-mcp"},request]));
    assert_eq!(
        merge_request_entry(Value::Null, Some(&request)),
        json!([request])
    );
    assert_eq!(
        merge_request_entry(json!([]), Some(&request)),
        json!([request])
    );
}

#[tokio::test]
async fn tcp_localhost_is_not_owned_native_and_registers_no_request_owner() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (client, _events) = AcpClient::connect_tcp(
        &listener.local_addr().unwrap().to_string(),
        std::env::temp_dir(),
    )
    .await
    .unwrap();
    let (_peer, _) = listener.accept().await.unwrap();
    assert!(!client.supports_local_computer_use().await);
    assert!(client.computer_use_connections.lock().is_empty());
    client.kill().await;
}

#[tokio::test]
async fn new_load_and_standard_fork_transmit_request_only_catalog() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for (resume, fork, expected) in [
        (None, false, "session/new"),
        (Some("prior-agent"), false, "session/load"),
        (Some("prior-agent"), true, "session/fork"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            let request: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["method"], expected);
            let catalog = request["params"]["mcpServers"].as_array().unwrap();
            assert_eq!(catalog.len(), 2);
            assert_eq!(catalog[0]["name"], "other-mcp");
            assert_eq!(catalog[1]["name"], crate::computer_use::MCP_SERVER_NAME);
            let reply = json!({"jsonrpc":"2.0", "id":request["id"], "result":{"sessionId":"returned-agent"}}).to_string() + "\n";
            write.write_all(reply.as_bytes()).await.unwrap();
            // Keep the transport alive until the client explicitly closes it.
            assert!(lines.next_line().await.unwrap().is_none());
        });
        // Use the production wire path with a fixed catalog so the test never
        // reads user MCP credentials. This is not eligibility/native evidence.
        let (client, _events) = AcpClient::connect_tcp(&address, std::env::temp_dir())
            .await
            .unwrap();
        let catalog = merge_request_entry(
            json!([{"name":"other-mcp"}]),
            Some(
                &json!({"name":crate::computer_use::MCP_SERVER_NAME,"command":"synthetic-request-runtime"}),
            ),
        );
        let opened = client
            .open_session_with_catalog(
                resume,
                fork,
                &std::env::temp_dir().to_string_lossy(),
                catalog,
            )
            .await
            .unwrap();
        assert_eq!(opened.0, "returned-agent");
        client.kill().await;
        peer.await.unwrap();
    }
}
