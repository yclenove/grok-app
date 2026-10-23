//! Public scoped stream/HTTP compositions; no control watcher is polled.
use super::*;
use gcu_core::scope::Clock;
use gcu_mcp::http::HttpEndpoint;
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Default)]
struct HeldClock(AtomicU64);
impl Clock for HeldClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

fn scope(generation: u64) -> FixtureScope {
    FixtureScope::try_new("run", generation, "engine", "desktop", "control", vec![]).unwrap()
}

async fn connection(lease: ScopeLease) -> Connection {
    Connection::open_scoped(Spy::new(), lease, host(0), ServerConfig::default())
        .await
        .unwrap()
}

fn lose_authority(loss: &str, lease: &ScopeLease, clock: &HeldClock) {
    match loss {
        "expiry" => clock.0.store(4, Ordering::SeqCst),
        "revoke" => lease.revoke(),
        "rebind" => lease.rebind(scope(2)).unwrap(),
        _ => unreachable!(),
    }
}

fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},
        "clientInfo":{"name":"scope-transport-test","version":"1"}
    }})
}

#[tokio::test]
async fn scoped_http_is_rejected_before_binding_through_both_public_starts() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for supplied_secret in [false, true] {
            let connection = connection(ScopeLease::install(scope(1))).await;
            assert!(connection.is_scoped());
            let reserved = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let address = reserved.local_addr().unwrap();
            // The occupied-port case also proves refusal precedes bind errors.
            if !supplied_secret {
                drop(reserved);
                assert!(matches!(
                    HttpEndpoint::start(connection, address.port()).await,
                    Err(ServeError::ScopedHttp)
                ));
                assert!(TcpStream::connect(address).await.is_err());
                let _still_free = TcpListener::bind(address).await.unwrap();
            } else {
                assert!(matches!(
                    HttpEndpoint::start_with_secret(
                        connection,
                        address.port(),
                        gcu_host_protocol::auth::Secret::random(),
                    )
                    .await,
                    Err(ServeError::ScopedHttp)
                ));
                drop(reserved);
                assert!(TcpStream::connect(address).await.is_err());
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn first_scoped_frame_refuses_lost_authority_before_sdk_bootstrap() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let frames = [
            json!({"jsonrpc":"2.0","id":1,"method":"ping","params":{}}).to_string(),
            // rmcp normally emits its pre-initialize metadata error directly.
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}).to_string(),
            // The transport's own envelope and parse errors must be gated too.
            json!({"jsonrpc":"invalid","id":1,"method":"ping"}).to_string(),
            "{malformed".to_owned(),
        ];
        for loss in ["expiry", "revoke", "rebind"] {
            for frame in &frames {
                let clock = Arc::new(HeldClock::default());
                let lease = ScopeLease::install_with_deadline(
                    scope(1),
                    clock.clone(),
                    Duration::from_secs(3),
                );
                let connection = connection(lease.clone()).await;
                let (client, wire) = tokio::io::duplex(64 * 1024);
                let (input, output) = tokio::io::split(wire);
                let serve = connection.serve_stream(input, output);
                tokio::pin!(serve);
                // Start the public serve path while Active and hold it in the
                // initial read. No initialize or other frame has been sent.
                std::future::poll_fn(|cx| {
                    assert!(serve.as_mut().poll(cx).is_pending());
                    std::task::Poll::Ready(())
                })
                .await;
                lose_authority(loss, &lease, &clock);
                let (read, mut write) = tokio::io::split(client);
                write
                    .write_all(format!("{frame}\n").as_bytes())
                    .await
                    .unwrap();
                write.shutdown().await.unwrap();
                let responses = async move {
                    let mut read = BufReader::new(read);
                    let mut line = String::new();
                    let count = read.read_line(&mut line).await.unwrap();
                    if frame == "{malformed" {
                        assert_eq!(count, 0, "{loss}: {line}");
                    } else {
                        assert!(count > 0, "{loss}: expected fixed refusal");
                        let reply: Value = serde_json::from_str(&line).unwrap();
                        assert_eq!(
                            reply,
                            json!({
                                "jsonrpc":"2.0","id":1,"error":{
                                    "code":-32600,"message":SCOPE_DENIED_MESSAGE,
                                    "data":{"code":"app_denied","message":SCOPE_DENIED_MESSAGE}
                                }
                            }),
                            "{loss}: {frame}"
                        );
                    }
                    line.clear();
                    assert_eq!(read.read_line(&mut line).await.unwrap(), 0, "{line}");
                };
                let (_, result) = tokio::join!(responses, &mut serve);
                assert!(matches!(result, Err(ServeError::Protocol)));
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn active_scoped_and_unscoped_bootstrap_preserve_wire_behavior() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for scoped in [true, false] {
            let connection = if scoped {
                connection(ScopeLease::install_with_deadline(
                    scope(1),
                    Arc::new(HeldClock::default()),
                    Duration::from_secs(3),
                ))
                .await
            } else {
                Connection::open_host_only(Arc::new(Spy::new()), host(0), ServerConfig::default())
                    .await
                    .unwrap()
            };
            assert_eq!(connection.is_scoped(), scoped);
            let mut rpc = Rpc::from_connection(connection).await;
            rpc.send(json!({"jsonrpc":"2.0","id":1,"method":"ping","params":{}}))
                .await;
            let mut ping = String::new();
            rpc.reader.read_line(&mut ping).await.unwrap();
            assert_eq!(ping, "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
            // Malformed JSON is still the same recovering NDJSON error.
            rpc.writer.write_all(b"{malformed\n").await.unwrap();
            let mut malformed = String::new();
            rpc.reader.read_line(&mut malformed).await.unwrap();
            // Match the existing encoder with or without the workspace's
            // serde_json/preserve_order feature unification.
            assert_eq!(
                malformed,
                format!(
                    "{}\n",
                    json!({"jsonrpc":"2.0","id":null,"error":{
                        "code":-32700,"message":"Parse error"
                    }})
                )
            );
            let metadata = rpc.request("tools/list", json!({})).await;
            assert!(metadata.get("result").is_none(), "{metadata}");
            assert!(
                metadata["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("_meta"),
                "{metadata}"
            );
            rpc.send(initialize()).await;
            let initialized = rpc.read().await;
            assert_eq!(initialized["id"], 2);
            assert!(
                initialized["result"]["serverInfo"].is_object(),
                "{initialized}"
            );
            rpc.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await;
            rpc.close().await;
        }
    })
    .await
    .unwrap();
}
