//! Control admission uses real loopback requests; adapters never touch a desktop.
use super::*;
use crate::adapter::{
    AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, TargetInfo,
};
use crate::broker::{BrokerOptions, StopState};
use crate::fake::FakeAdapter;
use crate::protocol::Observation;
use parking_lot::Condvar;
use std::sync::mpsc;

fn fixture() -> (Arc<ComputerUseBroker>, IpcServer, String) {
    let fake = Arc::new(FakeAdapter::new());
    let broker = broker(fake);
    let server = spawn(Arc::clone(&broker)).unwrap();
    let token = server.credential_for_session("owner", "run").unwrap();
    (broker, server, token)
}

fn broker(adapter: Arc<dyn ComputerUseAdapter>) -> Arc<ComputerUseBroker> {
    let broker = Arc::new(ComputerUseBroker::new(
        adapter,
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(format!(
                "cu-control-admission-{}.lock",
                uuid::Uuid::new_v4()
            )),
            ..BrokerOptions::default()
        },
    ));
    broker.open_run("owner", "run").unwrap();
    broker.authorize_target("run", "fake:window:1").unwrap();
    broker
}

fn call(url: &str, token: &str, body: Value) -> (u16, Value) {
    let response = reqwest::blocking::Client::new()
        .post(format!("{url}/cu/tool"))
        .bearer_auth(token)
        .json(&body)
        .timeout(Duration::from_secs(5))
        .send()
        .unwrap();
    (response.status().as_u16(), response.json().unwrap())
}

fn saturate(server: &IpcServer, bucket: String, count: usize) {
    server
        .service
        .rate
        .lock()
        .insert(bucket, vec![Instant::now(); count]);
}

#[test]
fn ordinary_rate_saturation_preserves_stop_and_handoff_admission() {
    let (broker, server, token) = fixture();
    saturate(&server, token.clone(), RATE_MAX);
    let (status, limited) = call(&server.url, &token, json!({"name": "computer_status"}));
    assert_eq!(status, 429);
    assert_eq!(limited["code"], "rate_limited");
    assert_eq!(limited["completion"], "not_started");
    assert!((1..=801).contains(&limited["retryAfterMs"].as_u64().unwrap()));
    for name in ["computer_request_handoff", "computer_stop"] {
        let (status, result) = call(&server.url, &token, json!({"name": name}));
        assert_eq!(status, 200);
        assert_eq!(result["isError"], false);
    }
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
}

#[test]
fn controls_remain_bounded_without_consuming_each_others_budgets() {
    let (_, server, token) = fixture();
    for (name, prefix) in [
        ("computer_stop", "stop:"),
        ("computer_request_handoff", "handoff:"),
    ] {
        saturate(&server, format!("{prefix}{token}"), CONTROL_RATE_MAX);
        let (status, result) = call(&server.url, &token, json!({"name": name}));
        assert_eq!(status, 429);
        assert_eq!(result["code"], "rate_limited");
        assert_eq!(result["completion"], "not_started");
    }
    assert_eq!(
        call(&server.url, &token, json!({"name": "computer_status"})).0,
        200
    );
    // Retain handoff saturation while allowing Stop again.
    server.service.rate.lock().remove(&format!("stop:{token}"));
    assert_eq!(
        call(&server.url, &token, json!({"name": "computer_stop"})).1["isError"],
        false
    );
}

#[test]
fn reserved_control_slots_are_bounded_and_released_by_ownership() {
    let (_, server, token) = fixture();
    let stop_slots = Arc::clone(&server.service.stop_slots)
        .try_acquire_many_owned(CONTROL_SLOTS as u32)
        .unwrap();
    let handoff_slots = Arc::clone(&server.service.handoff_slots)
        .try_acquire_many_owned(CONTROL_SLOTS as u32)
        .unwrap();
    for name in ["computer_stop", "computer_request_handoff"] {
        let (status, result) = call(&server.url, &token, json!({"name": name}));
        assert_eq!(status, 429);
        assert_eq!(result["content"][0]["text"], "too many pending calls");
    }
    drop(stop_slots);
    assert_eq!(
        call(&server.url, &token, json!({"name": "computer_stop"})).1["isError"],
        false
    );
    drop(handoff_slots);
    assert_eq!(server.service.stop_slots.available_permits(), CONTROL_SLOTS);
    assert_eq!(
        server.service.handoff_slots.available_permits(),
        CONTROL_SLOTS
    );
}

#[test]
fn controls_still_require_live_credentials_and_matching_session_identity() {
    let (broker, server, token) = fixture();
    for name in ["computer_stop", "computer_request_handoff"] {
        assert_eq!(
            call(&server.url, "wrong-token", json!({"name": name})).0,
            401
        );
        assert_eq!(
            call(
                &server.url,
                &token,
                json!({"name": name, "session": "other"})
            )
            .0,
            403
        );
        assert_eq!(
            call(
                &server.url,
                &token,
                json!({"name": name, "arguments": {"runId": "other"}})
            )
            .0,
            403
        );
    }
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Running);
    assert!(!broker.is_paused("run").unwrap());
    server.revoke_session("owner");
    for name in ["computer_stop", "computer_request_handoff"] {
        assert_eq!(call(&server.url, &token, json!({"name": name})).0, 401);
    }
}

#[test]
fn credential_rotation_and_revocation_remove_every_admission_bucket() {
    let (_, server, token) = fixture();
    let buckets = [
        token.clone(),
        format!("stop:{token}"),
        format!("handoff:{token}"),
    ];
    for bucket in &buckets {
        saturate(&server, bucket.clone(), 1);
    }
    let next = server.rotate_session_credential("owner", "run").unwrap();
    assert!(buckets
        .iter()
        .all(|bucket| !server.service.rate.lock().contains_key(bucket)));
    for prefix in ["", "stop:", "handoff:"] {
        saturate(&server, format!("{prefix}{next}"), 1);
    }
    server.revoke_session("owner");
    assert!(server.service.rate.lock().is_empty());
}

struct StatusGate {
    entered: mpsc::Sender<()>,
    released: Mutex<bool>,
    changed: Condvar,
}

impl StatusGate {
    fn release(&self) {
        *self.released.lock() = true;
        self.changed.notify_all();
    }
}

struct ReleaseOnDrop(Arc<StatusGate>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct GatedStatusAdapter {
    fake: FakeAdapter,
    gate: Mutex<Option<Arc<StatusGate>>>,
}

impl ComputerUseAdapter for GatedStatusAdapter {
    fn backend_id(&self) -> &'static str {
        "gated-status-fixture"
    }
    fn capabilities(&self) -> Capabilities {
        let gate = self.gate.lock().clone();
        if let Some(gate) = gate {
            gate.entered.send(()).unwrap();
            let mut released = gate.released.lock();
            while !*released {
                assert!(!gate
                    .changed
                    .wait_for(&mut released, Duration::from_secs(5))
                    .timed_out());
            }
        }
        self.fake.capabilities()
    }
    fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
        self.fake.list_targets()
    }
    fn target_alive(&self, id: &str) -> bool {
        self.fake.target_alive(id)
    }
    fn observe(&self, id: &str) -> Result<Observation, String> {
        self.fake.observe(id)
    }
    fn act(&self, request: &DispatchRequest) -> Result<AdapterActResult, String> {
        self.fake.act(request)
    }
    fn abort(&self, run: &str, generation: u64) -> Result<(), String> {
        self.fake.abort(run, generation)
    }
    fn is_idle(&self, run: &str) -> bool {
        self.fake.is_idle(run)
    }
    fn start_periodic_preview(&self, id: &str) {
        self.fake.start_periodic_preview(id);
    }
    fn stop_periodic_preview(&self) {
        self.fake.stop_periodic_preview();
    }
    fn periodic_preview_active(&self) -> bool {
        self.fake.periodic_preview_active()
    }
}

#[test]
fn eight_real_in_flight_http_calls_cannot_starve_stop_or_handoff() {
    let adapter = Arc::new(GatedStatusAdapter {
        fake: FakeAdapter::new(),
        gate: Mutex::new(None),
    });
    let broker = broker(adapter.clone());
    let server = spawn(Arc::clone(&broker)).unwrap();
    let token = server.credential_for_session("owner", "run").unwrap();
    let (entered, waiting) = mpsc::channel();
    let gate = Arc::new(StatusGate {
        entered,
        released: Mutex::new(false),
        changed: Condvar::new(),
    });
    let release = ReleaseOnDrop(Arc::clone(&gate));
    *adapter.gate.lock() = Some(gate);
    let calls = (0..8)
        .map(|_| {
            let url = server.url.clone();
            let token = token.clone();
            std::thread::spawn(move || call(&url, &token, json!({"name": "computer_status"})))
        })
        .collect::<Vec<_>>();
    for _ in 0..8 {
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    assert_eq!(server.service.slots.available_permits(), 0);
    for name in ["computer_request_handoff", "computer_stop"] {
        let (status, result) = call(&server.url, &token, json!({"name": name}));
        assert_eq!(status, 200);
        assert_eq!(result["isError"], false);
        assert_eq!(server.service.slots.available_permits(), 0);
    }
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
    drop(release);
    for task in calls {
        assert_eq!(task.join().unwrap().0, 200);
    }
    assert_eq!(server.service.slots.available_permits(), 8);
}
