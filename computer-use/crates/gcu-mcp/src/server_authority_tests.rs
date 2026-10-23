//! No control watcher or timer is polled in these request regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use gcu_core::scope::{Clock, FixtureScope, SCOPE_DENIED_MESSAGE};
use rmcp::ServiceExt;
use std::{sync::atomic::AtomicU64, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

#[derive(Debug, Default)]
struct HeldClock(AtomicU64);
impl Clock for HeldClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

fn server(clock: Arc<HeldClock>) -> Server {
    let host = Host {
        approvals: Arc::new(gcu_host_api::testing::ScriptedApprovals::new()),
        policy: Arc::new(gcu_host_api::testing::MemoryPolicyStore::new(
            Default::default(),
        )),
        events: Arc::new(gcu_host_api::testing::RecordingEventSink::new()),
        activity: Arc::new(gcu_host_api::testing::FakeUserActivity::new()),
    };
    let mut server = Server::new_with_approvals(
        Arc::new(gcu_driver_fake::FakeDriver::from_scene("form").unwrap()),
        host,
        ServerConfig::default(),
        SessionFiles::new().unwrap(),
        false,
    )
    .unwrap();
    // Early-handler tests never dispatch a driver. Install the same connection
    // authority/results lease used by new_scoped without adding another driver.
    let lease = ScopeLease::install_with_deadline(
        FixtureScope::try_new("run", 1, "engine", "desktop", "control", vec![]).unwrap(),
        clock,
        Duration::from_secs(3),
    );
    let inner = Arc::get_mut(&mut server.0).unwrap();
    inner.authority = lease.arm_attempt();
    inner.results.scope = Some(lease);
    server
}

struct Rpc {
    input: BufReader<ReadHalf<DuplexStream>>,
    output: WriteHalf<DuplexStream>,
    task: tokio::task::JoinHandle<()>,
}
impl Rpc {
    fn start(server: Server) -> Self {
        let (client, wire) = tokio::io::duplex(64 * 1024);
        let (input, output) = tokio::io::split(client);
        let (read, write) = tokio::io::split(wire);
        let task = tokio::spawn(async move {
            let mut cleanup = Cleanup::new(server.clone());
            let transport =
                crate::stdio::Stdio::new(read, write, server.session(), server.lifecycle());
            if let Ok(running) = crate::server_authority::Authorized(server)
                .serve(transport)
                .await
            {
                let _ = running.waiting().await;
            }
            cleanup.close().await.unwrap();
        });
        Self {
            input: BufReader::new(input),
            output,
            task,
        }
    }
    async fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.output.write_all(&bytes).await.unwrap();
    }
    async fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        loop {
            let mut line = String::new();
            assert!(self.input.read_line(&mut line).await.unwrap() > 0);
            let result: Value = serde_json::from_str(&line).unwrap();
            if result["id"] == id {
                return result;
            }
        }
    }
    async fn initialize(&mut self) -> Value {
        self.request(
            1,
            "initialize",
            json!({"protocolVersion":"2025-11-25",
            "capabilities":{},"clientInfo":{"name":"authority-test","version":"1"}}),
        )
        .await
    }
    async fn close(mut self) {
        self.output.shutdown().await.unwrap();
        self.task.await.unwrap();
    }
}

#[tokio::test]
async fn new_catalog_ping_and_busy_requests_refuse_expiry_without_a_watcher() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for loss in ["expiry", "rebind", "revoke"] {
            for method in ["tools/list", "ping", "tools/call"] {
                let clock = Arc::new(HeldClock::default());
                let server = server(clock.clone());
                let lease = server.0.results.scope.clone().unwrap();
                let _slots = server.0.slots.try_acquire_many(SLOTS as u32).unwrap();
                let mut rpc = Rpc::start(server.clone());
                assert!(rpc.initialize().await["result"].is_object());
                rpc.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                    .await;
                let catalog = rpc.request(2, "tools/list", json!({})).await;
                assert!(!catalog["result"]["tools"].as_array().unwrap().is_empty());
                let busy = rpc
                    .request(3, "tools/call", json!({"name":"list_apps","arguments":{}}))
                    .await;
                assert_eq!(
                    busy["result"]["structuredContent"]["error"]["code"], "busy",
                    "{busy}"
                );
                match loss {
                    "expiry" => clock.0.store(4, Ordering::SeqCst),
                    "rebind" => lease
                        .rebind(
                            FixtureScope::try_new("run", 2, "engine", "desktop", "control", vec![])
                                .unwrap(),
                        )
                        .unwrap(),
                    "revoke" => lease.revoke(),
                    _ => unreachable!(),
                }
                // A fresh lease for each case. No expiry accessor, timer or
                // watcher runs before this NEW request, including Busy.
                let params = if method == "tools/call" {
                    json!({"name":"list_apps","arguments":{}})
                } else {
                    json!({})
                };
                let denied = rpc.request(4, method, params).await;
                if method == "tools/call" {
                    assert_eq!(
                        denied["result"]["structuredContent"]["error"]["code"], "app_denied",
                        "{denied}"
                    );
                    assert_eq!(
                        denied["result"]["structuredContent"]["error"]["message"],
                        SCOPE_DENIED_MESSAGE,
                        "{denied}"
                    );
                    assert!(!denied.to_string().contains("busy"), "{denied}");
                    assert_eq!(
                        denied["result"]["structuredContent"]
                            .as_object()
                            .unwrap()
                            .len(),
                        1
                    );
                } else {
                    assert!(denied.get("result").is_none(), "{denied}");
                    assert_eq!(denied["error"]["message"], SCOPE_DENIED_MESSAGE, "{denied}");
                    assert_eq!(denied["error"]["data"]["code"], "app_denied", "{denied}");
                }
                rpc.close().await;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn initialize_refuses_an_expired_connection_before_a_watcher_runs() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let clock = Arc::new(HeldClock::default());
        let server = server(clock.clone());
        clock.0.store(4, Ordering::SeqCst);
        let mut rpc = Rpc::start(server);
        let denied = rpc.initialize().await;
        assert!(denied.get("result").is_none(), "{denied}");
        assert_eq!(denied["error"]["message"], SCOPE_DENIED_MESSAGE, "{denied}");
        assert_eq!(denied["error"]["data"]["code"], "app_denied", "{denied}");
        rpc.task.await.unwrap();
    })
    .await
    .unwrap();
}
