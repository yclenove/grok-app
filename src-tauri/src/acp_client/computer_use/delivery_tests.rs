//! Fixed localhost wire, synthetic Host decisions. No real CLI/native consent.
use super::*;
use grok_computer_use_core::{broker::BrokerOptions, fake::FakeAdapter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

struct Fixture {
    client: Arc<AcpClient>,
    broker: Arc<crate::computer_use::ComputerUseBroker>,
    transport: Arc<RequestTransport>,
    lines: tokio::io::Lines<BufReader<OwnedReadHalf>>,
    write: OwnedWriteHalf,
    request: Value,
    adapter: Arc<FakeAdapter>,
}

impl Fixture {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, _events) = AcpClient::connect_tcp(
            &listener.local_addr().unwrap().to_string(),
            std::env::temp_dir(),
        )
        .await
        .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let (read, write) = stream.into_split();
        let adapter = Arc::new(FakeAdapter::new());
        let broker = Arc::new(crate::computer_use::ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: false,
                lease_path: std::env::temp_dir()
                    .join(format!("cu-delivery-{}.lease", uuid::Uuid::new_v4())),
                ..BrokerOptions::default()
            },
        ));
        let issued = broker.reserve_transport_connection("delivery-app").unwrap();
        broker
            .bind_transport_agent(issued.token(), "delivery-agent")
            .unwrap();
        let transport = Arc::new(RequestTransport {
            history_path: None,
            picker: ParkingMutex::new(None),
            broker: broker.clone(),
            token: issued.token().into(),
            entry: json!({"name":crate::computer_use::MCP_SERVER_NAME}),
            deliveries: ParkingMutex::new(DeliveryLedger::default()),
        });
        client.computer_use_connections.lock().insert(
            "delivery-app".into(),
            RequestConnection {
                transport: Some(transport.clone()),
                error: None,
            },
        );
        let request = broker.submit_access(issued.token(), json!({"action":"request","purpose":"read owned fixture","scopeIntent":"fixture","surfaces":["desktop"],"capabilities":["observe"]})).unwrap();
        Self {
            client,
            broker,
            transport,
            lines: BufReader::new(read).lines(),
            write,
            request,
            adapter,
        }
    }

    fn approve(&self) -> DecisionDelivery {
        let id = self.request["requestId"].as_str().unwrap();
        self.broker
            .bind_synthetic_scope(id, &self.adapter.fixture_id())
            .unwrap();
        self.broker
            .commit_access(
                &self.transport.token,
                id,
                self.request["revision"].as_u64().unwrap(),
            )
            .unwrap();
        self.client
            .computer_use_decision("delivery-app", "delivery-agent")
            .unwrap()
    }

    fn deny(&self) -> DecisionDelivery {
        self.broker
            .deny_access(
                &self.transport.token,
                self.request["requestId"].as_str().unwrap(),
                self.request["revision"].as_u64().unwrap(),
            )
            .unwrap();
        self.client
            .computer_use_decision("delivery-app", "delivery-agent")
            .unwrap()
    }

    async fn next(&mut self) -> Value {
        serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(2), self.lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }

    async fn reply(&mut self, request: &Value) {
        let line = json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"end_turn"}})
            .to_string()
            + "\n";
        self.write.write_all(line.as_bytes()).await.unwrap();
    }

    async fn none(&mut self) {
        assert!(
            tokio::time::timeout(Duration::from_millis(80), self.lines.next_line())
                .await
                .is_err()
        );
    }

    async fn finish(self) {
        self.client.kill().await;
        self.broker.fence_task_consent("delivery-app");
        for ticket in self.broker.take_cleanup_tickets() {
            self.broker.finish_stop_cleanup(&ticket).unwrap();
        }
    }
}

async fn pending(client: &AcpClient) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if !client.pending.lock().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn queued_claim_is_not_consumed_and_not_started_can_retry_once() {
    let mut fixture = Fixture::new().await;
    let delivery = fixture.approve();
    let lock_client = fixture.client.clone();
    let writer = lock_client.stdin.lock().await;
    let rejected = {
        let client = fixture.client.clone();
        let delivery = delivery.clone();
        tokio::spawn(async move {
            client
                .prompt_computer_use(&delivery, "turn-denied-before-write", &|| {
                    Err("turn busy".into())
                })
                .await
        })
    };
    pending(&fixture.client).await;
    assert!(!delivery.was_admitted());
    if let Decision::Continue(claim) = &delivery.decision {
        fixture.broker.validate_continuation(claim).unwrap();
    }
    drop(writer);
    assert!(matches!(
        rejected.await.unwrap(),
        DeliveryAttempt::NotStarted
    ));
    assert!(fixture.client.pending.lock().is_empty());
    fixture.none().await;
    let success = {
        let client = fixture.client.clone();
        let delivery = delivery.clone();
        tokio::spawn(async move {
            client
                .prompt_computer_use(&delivery, "turn-accepted", &|| Ok(()))
                .await
        })
    };
    let wire = fixture.next().await;
    assert_eq!(wire["method"], "session/prompt");
    assert_eq!(wire["params"]["sessionId"], "delivery-agent");
    assert!(delivery.was_admitted());
    fixture.reply(&wire).await;
    assert!(matches!(success.await.unwrap(), DeliveryAttempt::Completed));
    assert!(matches!(
        fixture
            .client
            .prompt_computer_use(&delivery, "duplicate", &|| Ok(()))
            .await,
        DeliveryAttempt::NotStarted
    ));
    assert!(fixture
        .client
        .computer_use_decision("delivery-app", "delivery-agent")
        .is_none());
    fixture.none().await;
    fixture.finish().await;
}

#[tokio::test]
async fn denial_waits_for_real_prompt_rpc_and_dispatches_once_without_authority() {
    let mut fixture = Fixture::new().await;
    let busy = {
        let client = fixture.client.clone();
        tokio::spawn(async move {
            client
                .prompt_for("delivery-agent", "fixed existing prompt")
                .await
        })
    };
    let first = fixture.next().await;
    let denial = fixture.deny();
    assert!(matches!(
        fixture
            .client
            .prompt_computer_use(&denial, "busy-notice", &|| Ok(()))
            .await,
        DeliveryAttempt::NotStarted
    ));
    assert!(!denial.was_admitted());
    fixture.reply(&first).await;
    busy.await.unwrap().unwrap();
    let send = {
        let client = fixture.client.clone();
        let denial = denial.clone();
        tokio::spawn(async move {
            client
                .prompt_computer_use(&denial, "notice-turn", &|| Ok(()))
                .await
        })
    };
    let wire = fixture.next().await;
    assert!(wire["params"]["prompt"][0]["text"]
        .as_str()
        .unwrap()
        .contains("\"authority\":\"none\""));
    fixture.reply(&wire).await;
    assert!(matches!(send.await.unwrap(), DeliveryAttempt::Completed));
    assert_eq!(fixture.broker.executable_grant_count("delivery-app"), 0);
    assert_eq!(fixture.adapter.observe_calls(), 0);
    assert!(fixture.adapter.executions().is_empty());
    assert!(matches!(
        fixture
            .client
            .prompt_computer_use(&denial, "duplicate-denial", &|| Ok(()))
            .await,
        DeliveryAttempt::NotStarted
    ));
    fixture.none().await;
    fixture.finish().await;
}

#[tokio::test]
async fn stop_fences_a_queued_notice_and_never_restarts_the_stopped_chat() {
    let mut fixture = Fixture::new().await;
    let notice = fixture.deny();
    let lock_client = fixture.client.clone();
    let writer = lock_client.stdin.lock().await;
    let send = {
        let client = fixture.client.clone();
        let notice = notice.clone();
        tokio::spawn(async move {
            client
                .prompt_computer_use(&notice, "stale", &|| Ok(()))
                .await
        })
    };
    pending(&fixture.client).await;
    fixture.client.computer_use_chat_stopped("delivery-app");
    drop(writer);
    assert!(matches!(send.await.unwrap(), DeliveryAttempt::NotStarted));
    assert!(fixture
        .client
        .computer_use_decision("delivery-app", "delivery-agent")
        .is_none());
    fixture.client.computer_use_chat_started("delivery-app");
    assert!(fixture
        .client
        .computer_use_decision("delivery-app", "delivery-agent")
        .is_none());
    assert!(!notice.was_admitted());
    fixture.none().await;
    fixture.finish().await;
}

#[tokio::test]
async fn disconnect_after_real_wire_admission_is_unknown_and_never_replayed() {
    let mut fixture = Fixture::new().await;
    let delivery = fixture.approve();
    let send = {
        let client = fixture.client.clone();
        let delivery = delivery.clone();
        tokio::spawn(async move {
            client
                .prompt_computer_use(&delivery, "unknown-turn", &|| Ok(()))
                .await
        })
    };
    let _wire = fixture.next().await;
    assert!(delivery.was_admitted());
    fixture.write.shutdown().await.unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), send)
            .await
            .unwrap()
            .unwrap(),
        DeliveryAttempt::Unknown(_)
    ));
    assert!(matches!(
        fixture
            .client
            .prompt_computer_use(&delivery, "retry-forbidden", &|| Ok(()))
            .await,
        DeliveryAttempt::NotStarted
    ));
    assert!(fixture.client.pending.lock().is_empty());
    fixture.finish().await;
}
