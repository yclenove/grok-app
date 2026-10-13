#![allow(dead_code)]
use gcu_driver_fake::FakeDriver;
use gcu_host_api::{
    testing::{FakeUserActivity, MemoryPolicyStore, RecordingEventSink, ScriptedApprovals},
    ActionSummary, AppInfo, ApprovalDecision, ApprovalProvider, Cancel, ConfirmationLevel,
    ForegroundPolicy, HandoffReason, ImageRef, ModelReason, PolicyConfig, Risk, SessionCtx,
};
use gcu_mcp::{serve_io, Host, ServerConfig};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

pub struct Harness {
    pub driver: Arc<FakeDriver>,
    pub approvals: Arc<ScriptedApprovals>,
    pub policy: Arc<MemoryPolicyStore>,
    pub activity: Arc<FakeUserActivity>,
    pub reader: BufReader<ReadHalf<DuplexStream>>,
    pub writer: WriteHalf<DuplexStream>,
    pub task: tokio::task::JoinHandle<Result<(), gcu_mcp::ServeError>>,
    next: u64,
}
impl Harness {
    pub fn new(scene: &str, decisions: &[ApprovalDecision]) -> Self {
        Self::driver(FakeDriver::from_scene(scene).unwrap(), decisions)
    }
    pub fn driver(driver: FakeDriver, decisions: &[ApprovalDecision]) -> Self {
        Self::configured(driver, decisions, ServerConfig::default())
    }
    pub fn configured(
        driver: FakeDriver,
        decisions: &[ApprovalDecision],
        config: ServerConfig,
    ) -> Self {
        let driver = Arc::new(driver.with_image_digest([0x5a; 32]).unwrap());
        let approvals = Arc::new(ScriptedApprovals::new());
        for decision in decisions {
            approvals.push_decision(*decision);
        }
        let policy = Arc::new(MemoryPolicyStore::new(PolicyConfig {
            sensitive_action_confirmation: ConfirmationLevel::Off,
            allow_foreground: ForegroundPolicy::Always,
            ..PolicyConfig::default()
        }));
        let activity = Arc::new(FakeUserActivity::new());
        let host = Host {
            approvals: Arc::new(PersistingApprovals {
                scripted: approvals.clone(),
                policy: policy.clone(),
            }),
            policy: policy.clone(),
            events: Arc::new(RecordingEventSink::new()),
            activity: activity.clone(),
        };
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (reader, writer) = tokio::io::split(client);
        let (input, output) = tokio::io::split(server);
        let native = driver.clone();
        let task = tokio::spawn(serve_io(native, host, config, input, output));
        Self {
            driver,
            approvals,
            policy,
            activity,
            reader: BufReader::new(reader),
            writer,
            task,
            next: 1,
        }
    }
    pub async fn send(&mut self, value: Value) {
        let mut line = serde_json::to_vec(&value).unwrap();
        line.push(b'\n');
        self.writer.write_all(&line).await.unwrap();
    }
    pub async fn read(&mut self) -> Value {
        let mut line = String::new();
        let n = tokio::time::timeout(Duration::from_secs(20), self.reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        assert!(n > 0, "unexpected server EOF");
        serde_json::from_str(&line).unwrap()
    }
    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let result = self.read().await;
        assert_eq!(result["id"], id, "{result}");
        result
    }
    pub async fn initialize(&mut self, elicitation: bool) -> Value {
        let capabilities = if elicitation {
            json!({"elicitation":{"form":{}}})
        } else {
            json!({})
        };
        let reply = self
            .request(
                "initialize",
                json!({
                    "protocolVersion":"2025-11-25", "capabilities":capabilities,
                    "clientInfo":{"name":"gcu-e2e","version":"1"}
                }),
            )
            .await;
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        reply["result"].clone()
    }
    pub async fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
            .await["result"]
            .clone()
    }
    pub async fn observe(&mut self) -> Value {
        self.call(
            "get_app_state",
            json!({"app":"fake.form","include_screenshot":false}),
        )
        .await
    }
    pub async fn close(mut self) {
        self.writer.shutdown().await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .is_ok());
    }
}
pub fn text(result: &Value) -> String {
    result["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

// This test Host owns persistence, exactly as a trusted approval UI would.
struct PersistingApprovals {
    scripted: Arc<ScriptedApprovals>,
    policy: Arc<MemoryPolicyStore>,
}
#[async_trait::async_trait]
impl ApprovalProvider for PersistingApprovals {
    async fn request_app(
        &self,
        s: &SessionCtx,
        a: &AppInfo,
        r: Option<&ModelReason>,
        risk: Risk,
        c: &Cancel,
    ) -> ApprovalDecision {
        let decision = self.scripted.request_app(s, a, r, risk, c).await;
        if decision == ApprovalDecision::Always {
            self.policy.insert_always(a.identity.clone());
        }
        decision
    }
    async fn confirm_action(
        &self,
        s: &SessionCtx,
        a: &AppInfo,
        summary: &ActionSummary,
        crop: Option<&ImageRef>,
        c: &Cancel,
    ) -> bool {
        self.scripted.confirm_action(s, a, summary, crop, c).await
    }
    async fn handoff(&self, s: &SessionCtx, a: &AppInfo, r: HandoffReason, c: &Cancel) {
        self.scripted.handoff(s, a, r, c).await;
    }
}
